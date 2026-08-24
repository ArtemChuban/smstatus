use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher, event::ModifyKind};

use crate::error::Result;

#[derive(Debug)]
pub(crate) struct ReloadBatch {
    pub config: bool,
    pub wasm_kinds: Vec<String>,
}

pub(crate) struct ReloadWatcher {
    _watcher: notify::RecommendedWatcher,
    reload_rx: mpsc::Receiver<ReloadEvent>,
    alive: bool,
}

#[derive(Debug)]
enum ReloadEvent {
    Config,
    Wasm(String),
}

mod logic {
    use std::path::Path;

    pub(super) fn wasm_kind_from_path(path: &Path, modules_dir: &Path) -> Option<String> {
        let parent = path.parent()?;
        if parent != modules_dir {
            return None;
        }
        let file_name = path.file_name()?.to_str()?;
        let (stem, ext) = file_name.rsplit_once('.')?;
        if !ext.eq_ignore_ascii_case("wasm") || stem.is_empty() {
            return None;
        }
        Some(stem.to_string())
    }
}

fn canonicalize_or(path: &Path, label: &str) -> PathBuf {
    path.canonicalize().unwrap_or_else(|err| {
        log::error!(
            "reload watcher: failed to canonicalize {label} {}: {err}",
            path.display()
        );
        path.to_path_buf()
    })
}

fn apply_event(event: ReloadEvent, config: &mut bool, kinds: &mut Vec<String>) {
    match event {
        ReloadEvent::Config => *config = true,
        ReloadEvent::Wasm(kind) => {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
    }
}

fn is_content_change(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Modify(ModifyKind::Any)
            | EventKind::Other
            | EventKind::Any
    )
}

fn classify_path(
    path: &Path,
    canonical_config: &Path,
    config_target_fallback: &Path,
    canonical_modules: &Path,
    modules_dir_fallback: &Path,
) -> Option<ReloadEvent> {
    match path.canonicalize() {
        Ok(canonical) => {
            if canonical == canonical_config {
                return Some(ReloadEvent::Config);
            }
            logic::wasm_kind_from_path(&canonical, canonical_modules).map(ReloadEvent::Wasm)
        }
        Err(_) => {
            if path == canonical_config || path == config_target_fallback {
                return Some(ReloadEvent::Config);
            }
            logic::wasm_kind_from_path(path, modules_dir_fallback)
                .or_else(|| logic::wasm_kind_from_path(path, canonical_modules))
                .map(ReloadEvent::Wasm)
        }
    }
}

fn handle_watch_event(
    res: notify::Result<Event>,
    reload_tx: &mpsc::Sender<ReloadEvent>,
    canonical_config: &Path,
    config_target_fallback: &Path,
    canonical_modules: &Path,
    modules_dir_fallback: &Path,
) {
    let event = match res {
        Ok(event) => event,
        Err(err) => {
            log::error!("reload watcher error: {err}");
            return;
        }
    };

    if !is_content_change(&event.kind) {
        return;
    }

    for path in &event.paths {
        if let Some(reload_event) = classify_path(
            path,
            canonical_config,
            config_target_fallback,
            canonical_modules,
            modules_dir_fallback,
        ) {
            let _ = reload_tx.send(reload_event);
        }
    }
}

impl ReloadWatcher {
    pub(crate) fn new(
        config_dir: &Path,
        config_target: PathBuf,
        modules_dir: PathBuf,
    ) -> Result<Self> {
        let (reload_tx, reload_rx) = mpsc::channel::<ReloadEvent>();
        let canonical_config = canonicalize_or(&config_target, "config");
        let canonical_modules = canonicalize_or(&modules_dir, "modules dir");
        let config_target_fallback = config_target.clone();
        let modules_dir_fallback = modules_dir.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            handle_watch_event(
                res,
                &reload_tx,
                &canonical_config,
                &config_target_fallback,
                &canonical_modules,
                &modules_dir_fallback,
            );
        })?;

        watcher.watch(config_dir, RecursiveMode::Recursive)?;

        Ok(Self {
            _watcher: watcher,
            reload_rx,
            alive: true,
        })
    }

    pub(crate) fn wait_for_reload_or_timeout(&mut self, timeout: Duration) -> Option<ReloadBatch> {
        if !self.alive {
            std::thread::sleep(timeout);
            return None;
        }

        match self.reload_rx.recv_timeout(timeout) {
            Ok(first) => {
                let mut config = false;
                let mut kinds = Vec::new();
                apply_event(first, &mut config, &mut kinds);

                while let Ok(event) = self.reload_rx.recv_timeout(Duration::from_millis(200)) {
                    apply_event(event, &mut config, &mut kinds);
                }

                Some(ReloadBatch {
                    config,
                    wasm_kinds: kinds,
                })
            }
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                log::error!(
                    "reload watcher channel disconnected; disabling hot-reload for the rest of this run"
                );
                self.alive = false;
                None
            }
        }
    }

    pub(crate) fn try_reload(&mut self) -> Option<ReloadBatch> {
        if !self.alive {
            return None;
        }
        match self.reload_rx.try_recv() {
            Ok(first) => {
                let mut config = false;
                let mut kinds = Vec::new();
                apply_event(first, &mut config, &mut kinds);
                while let Ok(event) = self.reload_rx.try_recv() {
                    apply_event(event, &mut config, &mut kinds);
                }
                Some(ReloadBatch {
                    config,
                    wasm_kinds: kinds,
                })
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                log::error!(
                    "reload watcher channel disconnected; disabling hot-reload for the rest of this run"
                );
                self.alive = false;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::logic::wasm_kind_from_path;
    use std::path::Path;

    #[test]
    fn wasm_kind_from_path_reads_stem_under_modules_dir() {
        let modules = Path::new("/home/user/.config/smstatus/modules");
        assert_eq!(
            wasm_kind_from_path(&modules.join("cpu.wasm"), modules).as_deref(),
            Some("cpu")
        );
        assert_eq!(
            wasm_kind_from_path(&modules.join("disk.wasm"), modules).as_deref(),
            Some("disk")
        );
    }

    #[test]
    fn wasm_kind_from_path_accepts_case_insensitive_extension() {
        let modules = Path::new("/home/user/.config/smstatus/modules");
        assert_eq!(
            wasm_kind_from_path(&modules.join("cpu.WASM"), modules).as_deref(),
            Some("cpu")
        );
        assert_eq!(
            wasm_kind_from_path(&modules.join("disk.Wasm"), modules).as_deref(),
            Some("disk")
        );
    }

    #[test]
    fn wasm_kind_from_path_rejects_non_wasm_and_wrong_dir() {
        let modules = Path::new("/home/user/.config/smstatus/modules");
        assert_eq!(
            wasm_kind_from_path(&modules.join("cpu.toml"), modules),
            None
        );
        assert_eq!(
            wasm_kind_from_path(Path::new("/tmp/cpu.wasm"), modules),
            None
        );
        assert_eq!(wasm_kind_from_path(&modules.join(".wasm"), modules), None);
    }
}
