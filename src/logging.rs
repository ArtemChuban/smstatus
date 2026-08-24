use std::fs::{File, OpenOptions};
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use log::{Level, LevelFilter, Log, Metadata, Record};
use nix::fcntl::{Flock, FlockArg};

use crate::error::Result;
use crate::lock;

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

struct FileLogger {
    path: Mutex<PathBuf>,
    retain_days: AtomicU64,
}

fn lock_log_file(path: &Path) -> Result<Flock<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| format!("failed to open {}: {err}", path.display()))?;
    Flock::lock(file, FlockArg::LockExclusive)
        .map_err(|(_, err)| format!("failed to lock {}: {err}", path.display()).into())
}

fn append_locked(path: &Path, line: &str) -> Result<()> {
    let mut file = lock_log_file(path)?;
    file.seek(SeekFrom::End(0))
        .map_err(|err| format!("failed to seek {}: {err}", path.display()))?;
    writeln!(file, "{line}").map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(())
}

impl FileLogger {
    fn effective_path(&self) -> PathBuf {
        #[cfg(test)]
        {
            if let Some(path) = TEST_PATH.with(|cell| cell.borrow().clone()) {
                return path;
            }
            return PathBuf::new();
        }
        #[cfg(not(test))]
        {
            self.path
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_else(|_| PathBuf::new())
        }
    }

    fn write_line(&self, line: &str) {
        let path = self.effective_path();
        if path.as_os_str().is_empty() {
            return;
        }
        let _ = append_locked(&path, line);
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
        let message = record.args().to_string().replace(['\n', '\r'], " ");
        self.write_line(&format!("{ts} {} {message}", record.level()));
    }

    fn flush(&self) {}
}

fn resolve_log_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        if let Some(path) = TEST_PATH.with(|cell| cell.borrow().clone()) {
            return Some(path);
        }
        return None;
    }
    #[cfg(not(test))]
    {
        if let Some(logger) = LOGGER.get() {
            let path = logger.effective_path();
            if !path.as_os_str().is_empty() {
                return Some(path);
            }
        }
        lock::log_file_path().ok()
    }
}

pub(crate) fn append_message(level: Level, message: &str) {
    let Some(path) = resolve_log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let message = message.replace(['\n', '\r'], " ");
    let _ = append_locked(&path, &format!("{ts} {level} {message}"));
}

fn emit(level: Level, message: &str) {
    if LOGGER.get().is_some() {
        match level {
            Level::Error => log::error!("{message}"),
            Level::Warn => log::warn!("{message}"),
            Level::Info => log::info!("{message}"),
            Level::Debug => log::debug!("{message}"),
            Level::Trace => log::trace!("{message}"),
        }
    } else {
        append_message(level, message);
    }
}

pub(crate) fn to_stderr(level: Level, message: &str) {
    emit(level, message);
    eprintln!("{message}");
}

pub(crate) fn to_stdout(level: Level, message: &str) {
    emit(level, message);
    println!("{message}");
}

pub(crate) fn log_message(level: Level, message: &str) {
    emit(level, message);
}

pub(crate) fn init(retain_days: u64) -> Result<()> {
    let path = lock::log_file_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    if let Some(logger) = LOGGER.get() {
        set_retain_days(retain_days);
        let _ = logger.path.lock().map(|mut guard| *guard = path);
        return Ok(());
    }

    prune_old_entries(&path, retain_days)?;

    let logger = FileLogger {
        path: Mutex::new(path),
        retain_days: AtomicU64::new(retain_days),
    };

    let _ = LOGGER.set(logger);
    let Some(logger) = LOGGER.get() else {
        return Err("failed to install logger".into());
    };
    log::set_logger(logger).map_err(|err| format!("failed to install logger: {err}"))?;
    log::set_max_level(LevelFilter::Info);
    Ok(())
}

pub(crate) fn set_retain_days(days: u64) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let previous = logger.retain_days.swap(days, Ordering::Relaxed);
    if previous != days {
        let path = logger.effective_path();
        if !path.as_os_str().is_empty() {
            let _ = prune_old_entries(&path, days);
        }
    }
}

pub(crate) fn current_log_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        if let Some(path) = TEST_PATH.with(|cell| cell.borrow().clone()) {
            return Some(path);
        }
        return None;
    }
    #[cfg(not(test))]
    {
        if let Some(logger) = LOGGER.get() {
            return logger.path.lock().ok().map(|guard| guard.clone());
        }
        lock::log_file_path().ok()
    }
}

#[cfg(test)]
pub(crate) fn test_path_installed() -> bool {
    TEST_PATH.with(|cell| cell.borrow().is_some())
}

pub(crate) fn prune_old_entries(path: &Path, retain_days: u64) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut file = lock_log_file(path)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|err| format!("failed to seek {}: {err}", path.display()))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let cutoff = Utc::now() - chrono::Duration::days(retain_days as i64);
    let kept: Vec<&str> = contents
        .lines()
        .filter(|line| !line.is_empty())
        .filter(|line| match parse_leading_timestamp(line) {
            Some(ts) => ts >= cutoff,
            None => true,
        })
        .collect();
    let mut rewritten = kept.join("\n");
    if !rewritten.is_empty() {
        rewritten.push('\n');
    }
    file.set_len(0)
        .map_err(|err| format!("failed to truncate {}: {err}", path.display()))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|err| format!("failed to seek {}: {err}", path.display()))?;
    file.write_all(rewritten.as_bytes())
        .map_err(|err| format!("failed to write {}: {err}", path.display()).into())
}

pub(crate) fn count_non_empty_lines(path: &Path) -> usize {
    let Ok(file) = File::open(path) else {
        return 0;
    };
    let mut count = 0usize;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if !line.is_empty() {
            count += 1;
        }
    }
    count
}

/// Non-empty lines in absolute range `[start, start + count)`.
pub(crate) fn lines_in_range(path: &Path, start: usize, count: usize) -> Vec<String> {
    if count == 0 {
        return Vec::new();
    }
    let total = count_non_empty_lines(path);
    if start >= total {
        return Vec::new();
    }
    let want = count.min(total - start);
    // Suffix reads can reuse the backward scanner.
    if start + want == total {
        return tail_lines(path, want);
    }

    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(want);
    let mut idx = 0usize;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.is_empty() {
            continue;
        }
        if idx >= start {
            out.push(line);
            if out.len() == want {
                break;
            }
        }
        idx += 1;
    }
    out
}

pub(crate) fn tail_lines(path: &Path, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(file_len) = file.seek(SeekFrom::End(0)) else {
        return Vec::new();
    };
    if file_len == 0 {
        return Vec::new();
    }

    const CHUNK: u64 = 8 * 1024;
    let mut offset = file_len;
    let mut buf: Vec<u8> = Vec::new();

    loop {
        let read_from = offset.saturating_sub(CHUNK);
        let to_read = (offset - read_from) as usize;
        let mut chunk = vec![0u8; to_read];
        if file.seek(SeekFrom::Start(read_from)).is_err() {
            return Vec::new();
        }
        if file.read_exact(&mut chunk).is_err() {
            return Vec::new();
        }
        buf.splice(0..0, chunk);
        offset = read_from;

        let text = String::from_utf8_lossy(&buf);
        let all: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
        let complete: &[&str] = if offset > 0 && !buf.is_empty() && buf[0] != b'\n' {
            if all.len() > 1 { &all[1..] } else { &[] }
        } else {
            &all
        };

        if complete.len() >= n || offset == 0 {
            let start = complete.len().saturating_sub(n);
            return complete[start..].iter().map(|s| (*s).to_string()).collect();
        }
    }
}

fn parse_leading_timestamp(line: &str) -> Option<DateTime<Utc>> {
    let token = line.split_whitespace().next()?;
    DateTime::parse_from_rfc3339(token)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

pub(crate) fn parse_log_level(line: &str) -> Option<Level> {
    let mut parts = line.splitn(3, ' ');
    let _ts = parts.next()?;
    let level = parts.next()?;
    match level {
        "ERROR" => Some(Level::Error),
        "WARN" => Some(Level::Warn),
        "INFO" => Some(Level::Info),
        "DEBUG" => Some(Level::Debug),
        "TRACE" => Some(Level::Trace),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogLevelVisibility {
    pub error: bool,
    pub warn: bool,
    pub info: bool,
}

impl Default for LogLevelVisibility {
    fn default() -> Self {
        Self {
            error: true,
            warn: true,
            info: true,
        }
    }
}

impl LogLevelVisibility {
    pub(crate) fn all_enabled(self) -> bool {
        self.error && self.warn && self.info
    }

    pub(crate) fn permits(self, line: &str) -> bool {
        match parse_log_level(line) {
            None | Some(Level::Info) => self.info,
            Some(Level::Error) => self.error,
            Some(Level::Warn) => self.warn,
            Some(Level::Debug) | Some(Level::Trace) => true,
        }
    }
}

fn for_each_matching_line(
    path: &Path,
    visibility: LogLevelVisibility,
    mut f: impl FnMut(&str) -> bool,
) {
    let Ok(file) = File::open(path) else {
        return;
    };
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.is_empty() {
            continue;
        }
        if !visibility.permits(&line) {
            continue;
        }
        if !f(&line) {
            break;
        }
    }
}

pub(crate) fn count_visible_and_file_lines(
    path: &Path,
    visibility: LogLevelVisibility,
) -> (usize, usize) {
    if visibility.all_enabled() {
        let n = count_non_empty_lines(path);
        return (n, n);
    }
    let Ok(file) = File::open(path) else {
        return (0, 0);
    };
    let mut file_total = 0usize;
    let mut visible = 0usize;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.is_empty() {
            continue;
        }
        file_total += 1;
        if visibility.permits(&line) {
            visible += 1;
        }
    }
    (visible, file_total)
}

pub(crate) fn lines_in_range_filtered(
    path: &Path,
    start: usize,
    count: usize,
    visibility: LogLevelVisibility,
) -> Vec<String> {
    if visibility.all_enabled() {
        return lines_in_range(path, start, count);
    }
    if count == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count);
    let mut idx = 0usize;
    for_each_matching_line(path, visibility, |line| {
        if idx >= start {
            out.push(line.to_string());
            if out.len() == count {
                return false;
            }
        }
        idx += 1;
        true
    });
    out
}

#[cfg(test)]
fn message_from_log_line(line: &str) -> String {
    let mut parts = line.splitn(3, ' ');
    let _ts = parts.next();
    let _level = parts.next();
    parts.next().unwrap_or("").to_string()
}

#[cfg(test)]
pub(crate) fn set_path_for_test(path: PathBuf) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::File::create(&path);
    TEST_PATH.with(|cell| *cell.borrow_mut() = Some(path.clone()));
    if let Some(logger) = LOGGER.get() {
        if let Ok(mut guard) = logger.path.lock() {
            *guard = path;
        }
        return;
    }
    let logger = FileLogger {
        path: Mutex::new(path),
        retain_days: AtomicU64::new(7),
    };
    let _ = LOGGER.set(logger);
    if let Some(logger) = LOGGER.get() {
        let _ = log::set_logger(logger);
        log::set_max_level(LevelFilter::Info);
    }
}

#[cfg(test)]
pub(crate) fn clear_for_test() {
    if let Some(path) = current_log_path() {
        let _ = std::fs::write(path, "");
    }
}

#[cfg(test)]
pub(crate) fn logged_messages() -> Vec<String> {
    let Some(path) = current_log_path() else {
        return Vec::new();
    };
    let Ok(mut file) = lock_log_file(&path) else {
        return Vec::new();
    };
    let mut contents = String::new();
    if file.seek(SeekFrom::Start(0)).is_err() || file.read_to_string(&mut contents).is_err() {
        return Vec::new();
    }
    contents
        .lines()
        .filter(|line| !line.is_empty())
        .map(message_from_log_line)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_log_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("smstatus-logging-{label}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir.join("smstatus.log")
    }

    fn cleanup(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }

    #[test]
    fn prune_old_entries_drops_lines_older_than_retain_days() {
        let path = temp_log_path("prune");
        let old = (Utc::now() - chrono::Duration::days(10))
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let recent = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        std::fs::write(
            &path,
            format!("{old} INFO old message\n{recent} INFO recent message\n"),
        )
        .unwrap();

        prune_old_entries(&path, 7).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("old message"));
        assert!(contents.contains("recent message"));
        cleanup(&path);
    }

    #[test]
    fn tail_lines_returns_last_n_non_empty_lines() {
        let path = temp_log_path("tail");
        std::fs::write(&path, "one\n\ntwo\nthree\n").unwrap();

        assert_eq!(
            tail_lines(&path, 2),
            vec!["two".to_string(), "three".to_string()]
        );
        assert_eq!(
            tail_lines(&path, 10),
            vec!["one".to_string(), "two".to_string(), "three".to_string()]
        );
        cleanup(&path);
    }

    #[test]
    fn count_and_lines_in_range_cover_full_file() {
        let path = temp_log_path("range");
        std::fs::write(&path, "a\n\nb\nc\nd\n").unwrap();
        assert_eq!(count_non_empty_lines(&path), 4);
        assert_eq!(
            lines_in_range(&path, 1, 2),
            vec!["b".to_string(), "c".to_string()]
        );
        assert_eq!(
            lines_in_range(&path, 2, 10),
            vec!["c".to_string(), "d".to_string()]
        );
        assert_eq!(lines_in_range(&path, 0, 4), tail_lines(&path, 4));
        cleanup(&path);
    }

    #[test]
    fn append_via_write_line_creates_file() {
        let path = temp_log_path("append");
        TEST_PATH.with(|cell| *cell.borrow_mut() = Some(path.clone()));
        let logger = FileLogger {
            path: Mutex::new(path.clone()),
            retain_days: AtomicU64::new(7),
        };
        logger.write_line("2026-08-24T08:41:00.000Z INFO hello");

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "2026-08-24T08:41:00.000Z INFO hello\n");
        TEST_PATH.with(|cell| *cell.borrow_mut() = None);
        cleanup(&path);
    }

    #[test]
    fn parse_log_level_reads_second_token() {
        assert_eq!(
            parse_log_level("2026-08-24T08:41:00.000Z ERROR boom"),
            Some(Level::Error)
        );
        assert_eq!(
            parse_log_level("2026-08-24T08:41:00.000Z WARN careful"),
            Some(Level::Warn)
        );
        assert_eq!(
            parse_log_level("2026-08-24T08:41:00.000Z INFO hello"),
            Some(Level::Info)
        );
        assert_eq!(
            parse_log_level("2026-08-24T08:41:00.000Z DEBUG detail"),
            Some(Level::Debug)
        );
        assert_eq!(
            parse_log_level("2026-08-24T08:41:00.000Z TRACE tiny"),
            Some(Level::Trace)
        );
    }

    #[test]
    fn parse_log_level_rejects_garbage_and_missing_level() {
        assert_eq!(parse_log_level("not a log line"), None);
        assert_eq!(parse_log_level("2026-08-24T08:41:00.000Z"), None);
        assert_eq!(parse_log_level("2026-08-24T08:41:00.000Z NOPE x"), None);
        assert_eq!(parse_log_level(""), None);
    }

    #[test]
    fn filtered_count_and_range_skip_hidden_levels() {
        let path = temp_log_path("filter");
        std::fs::write(
            &path,
            concat!(
                "2026-08-24T08:41:00.000Z ERROR e1
",
                "2026-08-24T08:41:00.000Z INFO i1
",
                "2026-08-24T08:41:00.000Z WARN w1
",
                "2026-08-24T08:41:00.000Z INFO i2
",
                "plain line
",
            ),
        )
        .unwrap();
        let vis = LogLevelVisibility {
            error: true,
            warn: true,
            info: false,
        };
        assert_eq!(count_visible_and_file_lines(&path, vis), (2, 5));
        assert_eq!(
            lines_in_range_filtered(&path, 0, 10, vis),
            vec![
                "2026-08-24T08:41:00.000Z ERROR e1".to_string(),
                "2026-08-24T08:41:00.000Z WARN w1".to_string(),
            ]
        );
        assert_eq!(
            lines_in_range_filtered(&path, 1, 1, vis),
            vec!["2026-08-24T08:41:00.000Z WARN w1".to_string()]
        );
        let all = LogLevelVisibility::default();
        assert_eq!(
            count_visible_and_file_lines(&path, all).0,
            count_non_empty_lines(&path)
        );
        cleanup(&path);
    }

    #[test]
    fn bare_legacy_lines_follow_info_visibility() {
        let path = temp_log_path("bare-legacy");
        std::fs::write(
            &path,
            concat!(
                "root window set to: CPU 1%\n",
                "2026-08-24T08:41:00.000Z ERROR boom\n",
                "root window set to: CPU 2%\n",
            ),
        )
        .unwrap();
        let hide_info = LogLevelVisibility {
            error: true,
            warn: true,
            info: false,
        };
        assert_eq!(count_visible_and_file_lines(&path, hide_info), (1, 3));
        assert_eq!(
            lines_in_range_filtered(&path, 0, 10, hide_info),
            vec!["2026-08-24T08:41:00.000Z ERROR boom".to_string()]
        );
        cleanup(&path);
    }
}
