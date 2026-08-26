use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::lock;
use crate::reload::{ControlLine, ReloadBatch, ReloadRequest, parse_control_line};

const COALESCE: Duration = Duration::from_millis(200);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotifyOutcome {
    Delivered,
    NotRunning,
}

#[derive(Debug, PartialEq, Eq)]
enum ClientOutcome {
    Reload(ReloadBatch),
    Status,
    Ignored,
}

pub(crate) struct ControlListener {
    reload_rx: Receiver<ReloadBatch>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl ControlListener {
    pub(crate) fn new(
        status_provider: Option<Box<dyn Fn() -> String + Send>>,
    ) -> Result<Self, String> {
        let path = lock::control_socket_path().map_err(|err| err.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|err| err.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;

        let (reload_tx, reload_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let thread = thread::spawn(move || {
            control_thread_loop(listener, reload_tx, status_provider, thread_shutdown);
        });

        Ok(Self {
            reload_rx,
            shutdown,
            thread: Some(thread),
            path,
        })
    }

    pub(crate) fn wait_for_reload_or_timeout(&mut self, timeout: Duration) -> Option<ReloadBatch> {
        match self.reload_rx.recv_timeout(timeout) {
            Ok(batch) => Some(batch),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }
}

impl Drop for ControlListener {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn control_thread_loop(
    listener: UnixListener,
    reload_tx: Sender<ReloadBatch>,
    status_provider: Option<Box<dyn Fn() -> String + Send>>,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let provider = status_provider.as_deref();
                match serve_client(stream, provider) {
                    ClientOutcome::Reload(batch) if batch == ReloadBatch::default() => {}
                    ClientOutcome::Reload(mut batch) => {
                        let coalesce_until = Instant::now() + COALESCE;
                        while Instant::now() < coalesce_until && !shutdown.load(Ordering::Relaxed) {
                            match listener.accept() {
                                Ok((stream, _)) => {
                                    if let ClientOutcome::Reload(extra) =
                                        serve_client(stream, provider)
                                        && extra != ReloadBatch::default()
                                    {
                                        batch.merge_batch(extra);
                                    }
                                }
                                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                    thread::sleep(POLL_INTERVAL.min(
                                        coalesce_until.saturating_duration_since(Instant::now()),
                                    ));
                                }
                                Err(_) if shutdown.load(Ordering::Relaxed) => break,
                                Err(err) => {
                                    log::error!("control socket accept failed: {err}");
                                    break;
                                }
                            }
                        }
                        let _ = reload_tx.send(batch);
                    }
                    ClientOutcome::Status | ClientOutcome::Ignored => {}
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(POLL_INTERVAL);
            }
            Err(_) if shutdown.load(Ordering::Relaxed) => break,
            Err(err) => {
                log::error!("control socket accept failed: {err}");
                thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

fn read_control_lines(stream: UnixStream) -> Result<Vec<ControlLine>, String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut lines = Vec::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if bytes == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        lines.push(parse_control_line(&line)?);
    }
    Ok(lines)
}

fn merge_reload_lines(lines: &[ControlLine]) -> Result<ReloadBatch, String> {
    let mut batch = ReloadBatch::default();
    for line in lines {
        if let ControlLine::Reload(request) = line {
            batch.merge_request(request);
        }
    }
    Ok(batch)
}

fn serve_client(
    mut stream: UnixStream,
    status_provider: Option<&(dyn Fn() -> String + Send)>,
) -> ClientOutcome {
    let read_result = match stream.try_clone() {
        Ok(clone) => read_control_lines(clone),
        Err(err) => Err(err.to_string()),
    };

    let (response, outcome) = match read_result {
        Err(err) => (format!("err:{err}\n"), ClientOutcome::Ignored),
        Ok(lines) if lines.is_empty() => {
            ("err:empty request\n".to_string(), ClientOutcome::Ignored)
        }
        Ok(lines) => {
            let has_status = lines.contains(&ControlLine::Status);
            let has_reload = lines
                .iter()
                .any(|line| matches!(line, ControlLine::Reload(request) if *request != ReloadRequest::default()));

            if has_status && has_reload {
                (
                    "err:cannot mix status with reload commands\n".to_string(),
                    ClientOutcome::Ignored,
                )
            } else if has_status {
                let json = match status_provider {
                    Some(provider) => provider(),
                    None => "{\"err\":\"status query is not supported\"}".to_string(),
                };
                (format!("status:{json}\n"), ClientOutcome::Status)
            } else {
                match merge_reload_lines(&lines) {
                    Ok(batch) if batch == ReloadBatch::default() => {
                        ("err:empty request\n".to_string(), ClientOutcome::Ignored)
                    }
                    Ok(batch) => ("ok\n".to_string(), ClientOutcome::Reload(batch)),
                    Err(err) => (format!("err:{err}\n"), ClientOutcome::Ignored),
                }
            }
        }
    };

    if let Err(err) = stream.write_all(response.as_bytes()) {
        log::error!("control socket response failed: {err}");
        return ClientOutcome::Ignored;
    }
    if response.starts_with("err:") {
        log::error!(
            "control socket request failed: {}",
            response.trim().trim_start_matches("err:")
        );
    }
    outcome
}

pub(crate) fn notify_running(request: ReloadRequest) -> Result<NotifyOutcome, String> {
    let lines = request.command_lines();
    if lines.is_empty() {
        return Err("reload request is empty".to_string());
    }
    let path = lock::control_socket_path().map_err(|err| err.to_string())?;
    let mut stream = match UnixStream::connect(&path) {
        Ok(stream) => stream,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(NotifyOutcome::NotRunning);
        }
        Err(err) => return Err(err.to_string()),
    };
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|err| err.to_string())?;
    for line in lines {
        writeln!(stream, "{line}").map_err(|err| err.to_string())?;
    }
    stream
        .shutdown(Shutdown::Write)
        .map_err(|err| err.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader
        .read_line(&mut response)
        .map_err(|err| err.to_string())?;
    if response.starts_with("ok") {
        Ok(NotifyOutcome::Delivered)
    } else if response.starts_with("err:") {
        Err(response.trim().trim_start_matches("err:").to_string())
    } else {
        Err(format!("unexpected control socket response: {response}"))
    }
}

pub(crate) fn query_extension_status() -> Result<String, String> {
    query_extension_status_at(&lock::control_socket_path().map_err(|err| err.to_string())?)
}

fn query_extension_status_at(path: &Path) -> Result<String, String> {
    let mut stream = match UnixStream::connect(path) {
        Ok(stream) => stream,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err("smstatus is not running".to_string());
        }
        Err(err) => return Err(err.to_string()),
    };
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .map_err(|err| err.to_string())?;
    writeln!(stream, "status").map_err(|err| err.to_string())?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|err| err.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader
        .read_line(&mut response)
        .map_err(|err| err.to_string())?;
    if let Some(json) = response.strip_prefix("status:") {
        Ok(json.trim().to_string())
    } else if response.starts_with("err:") {
        Err(response.trim().trim_start_matches("err:").to_string())
    } else {
        Err(format!("unexpected control socket response: {response}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::thread;

    fn temp_socket_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "smstatus-ctl-test-{}-{}-{label}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    fn send_lines(path: &Path, lines: &[&str]) -> String {
        let mut stream = UnixStream::connect(path).unwrap();
        for line in lines {
            writeln!(stream, "{line}").unwrap();
        }
        stream.shutdown(Shutdown::Write).unwrap();
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        response
    }

    fn send_request(path: &Path, request: ReloadRequest) -> String {
        let mut stream = UnixStream::connect(path).unwrap();
        for line in request.command_lines() {
            writeln!(stream, "{line}").unwrap();
        }
        stream.shutdown(Shutdown::Write).unwrap();
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        response
    }

    #[test]
    fn control_socket_round_trip_merges_request_lines() {
        let path = temp_socket_path("round-trip");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();

        let server = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let (stream, _) = listener.accept().unwrap();
            let ClientOutcome::Reload(batch) = serve_client(stream, None) else {
                panic!("expected reload outcome");
            };
            assert!(batch.config);
            assert_eq!(batch.wasm_kinds, vec!["cpu".to_string()]);
            assert_eq!(batch.extension_names, vec!["echo".to_string()]);
        });

        ready_rx.recv().unwrap();
        let request = ReloadRequest {
            config: true,
            modules: vec!["cpu".to_string()],
            extensions: vec!["echo".to_string()],
        };
        let response = send_request(&path, request);
        assert!(response.starts_with("ok"));
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn serve_client_rejects_empty_request() {
        let path = temp_socket_path("empty");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();

        let server = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let (stream, _) = listener.accept().unwrap();
            let outcome = serve_client(stream, None);
            assert_eq!(outcome, ClientOutcome::Ignored);
        });

        ready_rx.recv().unwrap();
        let stream = UnixStream::connect(&path).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        assert!(response.starts_with("err:"));
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn serve_client_responds_on_parse_error() {
        let path = temp_socket_path("parse-error");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();

        let server = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let (stream, _) = listener.accept().unwrap();
            let outcome = serve_client(stream, None);
            assert_eq!(outcome, ClientOutcome::Ignored);
        });

        ready_rx.recv().unwrap();
        let response = send_lines(&path, &["not-a-command"]);
        assert!(response.starts_with("err:"));
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn serve_client_returns_status_json_from_provider() {
        let path = temp_socket_path("status");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let provider = || r#"{"daemon_pid":7,"extensions":[],"recent_calls":[]}"#.to_string();

        let server = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let (stream, _) = listener.accept().unwrap();
            let outcome = serve_client(stream, Some(&provider));
            assert_eq!(outcome, ClientOutcome::Status);
        });

        ready_rx.recv().unwrap();
        let response = send_lines(&path, &["status"]);
        assert!(response.starts_with("status:"));
        let json = response.trim_start_matches("status:").trim();
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(value["daemon_pid"], 7);
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn serve_client_rejects_mixed_status_and_reload_lines() {
        let path = temp_socket_path("mixed");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();

        let server = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let (stream, _) = listener.accept().unwrap();
            let outcome = serve_client(stream, Some(&|| "{}".to_string()));
            assert_eq!(outcome, ClientOutcome::Ignored);
        });

        ready_rx.recv().unwrap();
        let response = send_lines(&path, &["status", "config"]);
        assert!(response.contains("cannot mix status with reload commands"));
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn query_extension_status_reads_status_json_from_listener() {
        let path = temp_socket_path("query-status");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let provider = || r#"{"daemon_pid":9,"extensions":[],"recent_calls":[]}"#.to_string();

        let server = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let (stream, _) = listener.accept().unwrap();
            let outcome = serve_client(stream, Some(&provider));
            assert_eq!(outcome, ClientOutcome::Status);
        });

        ready_rx.recv().unwrap();
        let json = query_extension_status_at(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["daemon_pid"], 9);
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
