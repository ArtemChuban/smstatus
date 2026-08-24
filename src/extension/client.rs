use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use extension_protocol::{self as protocol, Request, Response};

const IO_TIMEOUT: Duration = Duration::from_secs(2);

fn set_stream_timeouts(stream: &UnixStream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn connect_and_handshake(socket_path: &Path) -> Result<UnixStream, String> {
    let mut stream = UnixStream::connect(socket_path).map_err(|e| e.to_string())?;
    set_stream_timeouts(&stream)?;
    protocol::perform_handshake_client(&mut stream)?;
    Ok(stream)
}

pub(crate) fn call(stream: &mut UnixStream, method: &str, payload: &str) -> Result<String, String> {
    set_stream_timeouts(stream)?;
    protocol::write_frame(
        stream,
        &Request {
            method: method.to_string(),
            payload: payload.to_string(),
        },
    )
    .map_err(|e| e.to_string())?;

    match protocol::read_frame(stream).map_err(|e| e.to_string())? {
        Response::Ok(value) => Ok(value),
        Response::Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::thread;

    use super::*;

    fn socket_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "smstatus-extension-client-test-{}-{name}.sock",
            std::process::id()
        ))
    }

    fn spawn_server(
        path: std::path::PathBuf,
        handler: impl FnOnce(UnixStream) + Send + 'static,
    ) -> thread::JoinHandle<()> {
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handler(stream);
            let _ = std::fs::remove_file(&path);
        })
    }

    #[test]
    fn call_round_trips_a_request_and_response() {
        let path = socket_path("success");
        let server = spawn_server(path.clone(), |mut stream| {
            protocol::perform_handshake_server(&mut stream).unwrap();
            let request: Request = protocol::read_frame(&mut stream).unwrap();
            assert_eq!(request.method, "ping");
            assert_eq!(request.payload, "hello");
            protocol::write_frame(&mut stream, &Response::Ok("hello".to_string())).unwrap();
        });

        let mut stream = connect_and_handshake(&path).unwrap();
        let response = call(&mut stream, "ping", "hello").unwrap();
        assert_eq!(response, "hello");
        server.join().unwrap();
    }

    #[test]
    fn connect_and_handshake_reports_version_mismatch() {
        let path = socket_path("mismatch");
        let server = spawn_server(path.clone(), |mut stream| {
            let _handshake: protocol::Handshake = protocol::read_frame(&mut stream).unwrap();
            protocol::write_frame(
                &mut stream,
                &protocol::HandshakeResponse::Mismatch {
                    protocol_version: 99,
                },
            )
            .unwrap();
        });

        let err = connect_and_handshake(&path).unwrap_err();
        assert!(err.contains("mismatch"));
        server.join().unwrap();
    }

    #[test]
    fn connect_and_handshake_to_a_missing_socket_returns_a_clean_error() {
        let path = socket_path("missing");
        let err = connect_and_handshake(&path).unwrap_err();
        assert!(!err.is_empty());
    }
}
