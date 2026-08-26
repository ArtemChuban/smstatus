use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::lock;
use crate::reload::{ReloadBatch, ReloadRequest, parse_command_line};

const COALESCE: Duration = Duration::from_millis(200);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotifyOutcome {
    Delivered,
    NotRunning,
}

pub(crate) struct ControlListener {
    listener: UnixListener,
    path: PathBuf,
}

impl ControlListener {
    pub(crate) fn new() -> Result<Self, String> {
        let path = lock::control_socket_path().map_err(|err| err.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|err| err.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;
        Ok(Self { listener, path })
    }

    pub(crate) fn wait_for_reload_or_timeout(&mut self, timeout: Duration) -> Option<ReloadBatch> {
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline - now;
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let mut batch = serve_client(stream);
                    if batch == ReloadBatch::default() {
                        let sleep_for = remaining.min(POLL_INTERVAL);
                        std::thread::sleep(sleep_for);
                        continue;
                    }
                    let coalesce_until = Instant::now() + COALESCE;
                    while Instant::now() < coalesce_until {
                        match self.listener.accept() {
                            Ok((stream, _)) => {
                                let extra = serve_client(stream);
                                if extra != ReloadBatch::default() {
                                    batch.merge_batch(extra);
                                }
                            }
                            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(
                                    POLL_INTERVAL.min(
                                        coalesce_until.saturating_duration_since(Instant::now()),
                                    ),
                                );
                            }
                            Err(err) => {
                                log::error!("control socket accept failed: {err}");
                                break;
                            }
                        }
                    }
                    return Some(batch);
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(remaining.min(POLL_INTERVAL));
                }
                Err(err) => {
                    log::error!("control socket accept failed: {err}");
                    std::thread::sleep(remaining.min(POLL_INTERVAL));
                }
            }
        }
    }
}

impl Drop for ControlListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn read_stream_into_batch(stream: UnixStream, batch: &mut ReloadBatch) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if bytes == 0 {
            break;
        }
        let request = parse_command_line(&line)?;
        batch.merge_request(&request);
    }
    Ok(())
}

fn serve_client(mut stream: UnixStream) -> ReloadBatch {
    let mut batch = ReloadBatch::default();
    let read_err = match stream.try_clone() {
        Ok(clone) => read_stream_into_batch(clone, &mut batch).err(),
        Err(err) => Some(err.to_string()),
    };
    let response = if let Some(err) = &read_err {
        format!("err:{err}\n")
    } else if batch == ReloadBatch::default() {
        "err:empty request\n".to_string()
    } else {
        "ok\n".to_string()
    };
    if let Err(err) = stream.write_all(response.as_bytes()) {
        log::error!("control socket response failed: {err}");
        return ReloadBatch::default();
    }
    if let Some(err) = read_err {
        log::error!("control socket request failed: {err}");
    }
    batch
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

    fn send_lines(path: &PathBuf, lines: &[&str]) -> String {
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

    fn send_request(path: &PathBuf, request: ReloadRequest) -> String {
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
            let batch = serve_client(stream);
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
            let batch = serve_client(stream);
            assert_eq!(batch, ReloadBatch::default());
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
            let batch = serve_client(stream);
            assert_eq!(batch, ReloadBatch::default());
        });

        ready_rx.recv().unwrap();
        let response = send_lines(&path, &["not-a-command"]);
        assert!(response.starts_with("err:"));
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }
}
