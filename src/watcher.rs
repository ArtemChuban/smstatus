use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{Event, EventKind, RecursiveMode, Watcher, event::ModifyKind};

use crate::error::Result;

pub(crate) struct ConfigWatcher {
    _watcher: notify::RecommendedWatcher,
    reload_rx: mpsc::Receiver<()>,
    alive: bool,
}

impl ConfigWatcher {
    pub(crate) fn new(config_dir: &Path, watch_target: PathBuf) -> Result<Self> {
        let (reload_tx, reload_rx) = mpsc::channel::<()>();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<Event>| match res {
                Ok(event) => {
                    let is_content_change = matches!(
                        event.kind,
                        EventKind::Create(_)
                            | EventKind::Remove(_)
                            | EventKind::Modify(ModifyKind::Data(_))
                            | EventKind::Modify(ModifyKind::Name(_))
                    );
                    if is_content_change && event.paths.contains(&watch_target) {
                        let _ = reload_tx.send(());
                    }
                }
                Err(err) => eprintln!("config watcher error: {err}"),
            })?;
        watcher.watch(config_dir, RecursiveMode::NonRecursive)?;
        Ok(Self {
            _watcher: watcher,
            reload_rx,
            alive: true,
        })
    }

    pub(crate) fn wait_for_reload_or_timeout(&mut self, timeout: Duration) -> bool {
        if !self.alive {
            std::thread::sleep(timeout);
            return false;
        }

        match self.reload_rx.recv_timeout(timeout) {
            Ok(()) => {
                while self
                    .reload_rx
                    .recv_timeout(Duration::from_millis(100))
                    .is_ok()
                {}
                true
            }
            Err(mpsc::RecvTimeoutError::Timeout) => false,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!(
                    "config watcher channel disconnected; disabling hot-reload for the rest of this run"
                );
                self.alive = false;
                false
            }
        }
    }
}
