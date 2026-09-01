use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 2;
const MAX_FRAME_SIZE: u32 = 8 * 1024 * 1024;

pub const CHECK_METHOD: &str = "check";

pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionEntry {
    pub extension: String,
    pub method: String,
    #[serde(default)]
    pub constraints: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckRequest {
    pub permissions: Vec<PermissionEntry>,
    pub method: String,
    pub payload: String,
}

pub fn is_reserved_method(method: &str) -> bool {
    method == CHECK_METHOD
}

pub fn matching_entries<'a>(
    request: &'a CheckRequest,
    extension: &str,
) -> Vec<&'a PermissionEntry> {
    request
        .permissions
        .iter()
        .filter(|entry| entry.extension == extension && entry.method == request.method)
        .collect()
}

pub fn check_allowlisted(request: &CheckRequest, extension: &str) -> Result<(), String> {
    if matching_entries(request, extension).is_empty() {
        Err(format!(
            "permission denied: no allowlisted entry for extension `{extension}` method `{}`",
            request.method
        ))
    } else {
        Ok(())
    }
}

pub fn constraint_string_list(entry: &PermissionEntry, key: &str) -> Result<Vec<String>, String> {
    let value = entry
        .constraints
        .get(key)
        .ok_or_else(|| format!("missing constraint `{key}`"))?;
    let arr = value
        .as_array()
        .ok_or_else(|| format!("constraint `{key}` must be a string array"))?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("constraint `{key}` must be a string array"))
        })
        .collect()
}

pub fn encode_check_payload(
    permissions: impl IntoIterator<Item = PermissionEntry>,
    method: impl Into<String>,
    payload: impl Into<String>,
) -> Result<String, String> {
    let request = CheckRequest {
        permissions: permissions.into_iter().collect(),
        method: method.into(),
        payload: payload.into(),
    };
    serde_json::to_string(&request).map_err(|e| e.to_string())
}

pub fn decode_check_payload(payload: &str) -> Result<CheckRequest, String> {
    serde_json::from_str(payload).map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize)]
pub struct Handshake {
    pub protocol_version: u32,
}

#[derive(Serialize, Deserialize)]
pub enum HandshakeResponse {
    Ok,
    Mismatch { protocol_version: u32 },
}

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Ok(String),
    Err(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FromExtension {
    Response(Response),
    Event(Event),
}

#[derive(Clone)]
pub struct EventEmit {
    tx: mpsc::Sender<ServerMsg>,
}

impl EventEmit {
    pub fn emit(&self, name: impl Into<String>, payload: impl Into<String>) -> Result<(), String> {
        self.tx
            .send(ServerMsg::Event(Event {
                name: name.into(),
                payload: payload.into(),
            }))
            .map_err(|_| "event channel closed".to_string())
    }
}

enum ServerMsg {
    Request(Request),
    Event(Event),
    Shutdown,
}

pub fn allowlist_check_response(payload: &str, extension: &str) -> Response {
    match decode_check_payload(payload) {
        Ok(check) => match check_allowlisted(&check, extension) {
            Ok(()) => Response::Ok(String::new()),
            Err(msg) => Response::Err(msg),
        },
        Err(msg) => Response::Err(msg),
    }
}

fn accept_and_handshake(socket_path: &str) -> Result<UnixStream, String> {
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path).map_err(|e| e.to_string())?;
    let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
    perform_handshake_server(&mut stream)?;
    Ok(stream)
}

fn drive_loop<S: Read + Write>(mut stream: S, mut respond: impl FnMut(&Request) -> Response) {
    while let Ok(request) = read_frame::<_, Request>(&mut stream) {
        let response = respond(&request);
        if write_frame(&mut stream, &FromExtension::Response(response)).is_err() {
            break;
        }
    }
}

fn drive_loop_with_events(
    stream: UnixStream,
    event_rx_bridge: mpsc::Sender<ServerMsg>,
    rx: mpsc::Receiver<ServerMsg>,
    mut respond: impl FnMut(&Request) -> Response,
) {
    let mut reader = match stream.try_clone() {
        Ok(reader) => reader,
        Err(_) => return,
    };
    let req_tx = event_rx_bridge;
    thread::spawn(move || {
        while let Ok(request) = read_frame::<_, Request>(&mut reader) {
            if req_tx.send(ServerMsg::Request(request)).is_err() {
                return;
            }
        }
        let _ = req_tx.send(ServerMsg::Shutdown);
    });

    let mut writer = stream;
    for msg in rx {
        let frame = match msg {
            ServerMsg::Shutdown => break,
            ServerMsg::Request(request) => FromExtension::Response(respond(&request)),
            ServerMsg::Event(event) => FromExtension::Event(event),
        };
        if write_frame(&mut writer, &frame).is_err() {
            break;
        }
    }
}

pub fn serve(handler: impl FnMut(&Request) -> Response) {
    let socket_path = std::env::args()
        .nth(1)
        .expect("missing socket path argument");
    let stream = accept_and_handshake(&socket_path).expect("extension server failed");
    drive_loop(stream, handler);
}

pub fn serve_with_events<F, H>(factory: F)
where
    F: FnOnce(EventEmit) -> H,
    H: FnMut(&Request) -> Response,
{
    let socket_path = std::env::args()
        .nth(1)
        .expect("missing socket path argument");
    serve_with_events_at(&socket_path, factory).expect("extension server failed");
}

pub fn serve_with_events_at<F, H>(socket_path: &str, factory: F) -> Result<(), String>
where
    F: FnOnce(EventEmit) -> H,
    H: FnMut(&Request) -> Response,
{
    let stream = accept_and_handshake(socket_path)?;
    let (tx, rx) = mpsc::channel();
    let emit = EventEmit { tx: tx.clone() };
    let handler = factory(emit);
    drive_loop_with_events(stream, tx, rx, handler);
    Ok(())
}

pub fn write_event<W: Write>(writer: &mut W, name: &str, payload: &str) -> io::Result<()> {
    write_frame(
        writer,
        &FromExtension::Event(Event {
            name: name.to_string(),
            payload: payload.to_string(),
        }),
    )
}

pub fn check_request(
    extension: &str,
    method: &str,
    payload: &str,
    allowed_method: &str,
) -> Request {
    let encoded = encode_check_payload(
        vec![PermissionEntry {
            extension: extension.to_string(),
            method: allowed_method.to_string(),
            constraints: BTreeMap::new(),
        }],
        method,
        payload,
    )
    .unwrap();
    Request {
        method: CHECK_METHOD.to_string(),
        payload: encoded,
    }
}

pub fn write_frame<W: Write>(writer: &mut W, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&bytes)?;
    Ok(())
}

pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> io::Result<T> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_be_bytes(len_bytes);
    if len > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {len} bytes exceeds max frame size of {MAX_FRAME_SIZE} bytes"),
        ));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn perform_handshake_client<S: Read + Write>(stream: &mut S) -> Result<(), String> {
    write_frame(
        stream,
        &Handshake {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .map_err(|e| e.to_string())?;
    match read_frame(stream).map_err(|e| e.to_string())? {
        HandshakeResponse::Ok => Ok(()),
        HandshakeResponse::Mismatch { protocol_version } => Err(format!(
            "protocol version mismatch: client has v{PROTOCOL_VERSION}, extension has v{protocol_version}"
        )),
    }
}

pub fn perform_handshake_server<S: Read + Write>(stream: &mut S) -> Result<(), String> {
    let handshake: Handshake = read_frame(stream).map_err(|e| e.to_string())?;
    if handshake.protocol_version == PROTOCOL_VERSION {
        write_frame(stream, &HandshakeResponse::Ok).map_err(|e| e.to_string())
    } else {
        write_frame(
            stream,
            &HandshakeResponse::Mismatch {
                protocol_version: PROTOCOL_VERSION,
            },
        )
        .map_err(|e| e.to_string())?;
        Err(format!(
            "protocol version mismatch: client has v{}, extension has v{PROTOCOL_VERSION}",
            handshake.protocol_version
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    struct Duplex {
        read_buf: Cursor<Vec<u8>>,
        write_buf: Vec<u8>,
    }

    impl Duplex {
        fn with_read_buf(data: Vec<u8>) -> Self {
            Self {
                read_buf: Cursor::new(data),
                write_buf: Vec::new(),
            }
        }
    }

    impl Read for Duplex {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.read_buf.read(buf)
        }
    }

    impl Write for Duplex {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.write_buf.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.write_buf.flush()
        }
    }

    #[test]
    fn frame_round_trips_through_a_buffer() {
        let mut buf = Vec::new();
        write_frame(
            &mut buf,
            &Request {
                method: "ping".to_string(),
                payload: "hello".to_string(),
            },
        )
        .unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded: Request = read_frame(&mut cursor).unwrap();
        assert_eq!(decoded.method, "ping");
        assert_eq!(decoded.payload, "hello");
    }

    #[test]
    fn read_frame_rejects_oversized_length_prefix() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(MAX_FRAME_SIZE + 1).to_be_bytes());
        let mut cursor = Cursor::new(buf);
        let result: io::Result<Request> = read_frame(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn client_handshake_fails_on_version_mismatch() {
        let mut server_response = Vec::new();
        write_frame(
            &mut server_response,
            &HandshakeResponse::Mismatch {
                protocol_version: 99,
            },
        )
        .unwrap();

        let mut duplex = Duplex::with_read_buf(server_response);
        let err = perform_handshake_client(&mut duplex).unwrap_err();
        assert!(err.contains("mismatch"));
    }

    #[test]
    fn server_handshake_fails_on_version_mismatch() {
        let mut client_handshake = Vec::new();
        write_frame(
            &mut client_handshake,
            &Handshake {
                protocol_version: 99,
            },
        )
        .unwrap();

        let mut duplex = Duplex::with_read_buf(client_handshake);
        let err = perform_handshake_server(&mut duplex).unwrap_err();
        assert!(err.contains("mismatch"));

        let mut written = Cursor::new(duplex.write_buf);
        let response: HandshakeResponse = read_frame(&mut written).unwrap();
        match response {
            HandshakeResponse::Mismatch { protocol_version } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
            }
            HandshakeResponse::Ok => panic!("expected a mismatch response"),
        }
    }

    #[test]
    fn check_method_constant_is_stable() {
        assert_eq!(CHECK_METHOD, "check");
    }

    #[test]
    fn is_reserved_method_detects_check() {
        assert!(is_reserved_method(CHECK_METHOD));
        assert!(!is_reserved_method("read"));
    }

    #[test]
    fn allowlist_check_response_allows_and_denies() {
        let allowed = encode_check_payload(
            vec![PermissionEntry {
                extension: "echo".to_string(),
                method: "ping".to_string(),
                constraints: BTreeMap::new(),
            }],
            "ping",
            "hi",
        )
        .unwrap();
        match allowlist_check_response(&allowed, "echo") {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }

        let denied = encode_check_payload(
            vec![PermissionEntry {
                extension: "echo".to_string(),
                method: "ping".to_string(),
                constraints: BTreeMap::new(),
            }],
            "pong",
            "hi",
        )
        .unwrap();
        match allowlist_check_response(&denied, "echo") {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn check_request_round_trips_through_json() {
        let mut constraints = BTreeMap::new();
        constraints.insert("path_prefixes".to_string(), serde_json::json!(["/proc/"]));
        let original = CheckRequest {
            permissions: vec![PermissionEntry {
                extension: "fs".to_string(),
                method: "read".to_string(),
                constraints,
            }],
            method: "read".to_string(),
            payload: "/proc/stat".to_string(),
        };
        let encoded = encode_check_payload(
            original.permissions.clone(),
            original.method.clone(),
            original.payload.clone(),
        )
        .unwrap();
        let decoded = decode_check_payload(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn check_allowlisted_allows_matching_entry() {
        let request = CheckRequest {
            permissions: vec![PermissionEntry {
                extension: "echo".to_string(),
                method: "ping".to_string(),
                constraints: BTreeMap::new(),
            }],
            method: "ping".to_string(),
            payload: String::new(),
        };
        assert!(check_allowlisted(&request, "echo").is_ok());
    }

    #[test]
    fn check_allowlisted_denies_missing_entry() {
        let request = CheckRequest {
            permissions: vec![PermissionEntry {
                extension: "echo".to_string(),
                method: "ping".to_string(),
                constraints: BTreeMap::new(),
            }],
            method: "pong".to_string(),
            payload: String::new(),
        };
        let err = check_allowlisted(&request, "echo").unwrap_err();
        assert!(err.contains("permission denied"));
    }

    #[test]
    fn constraint_string_list_reads_string_array() {
        let mut constraints = BTreeMap::new();
        constraints.insert(
            "url_prefixes".to_string(),
            serde_json::json!(["https://api.example/"]),
        );
        let entry = PermissionEntry {
            extension: "http".to_string(),
            method: "get".to_string(),
            constraints,
        };
        let list = constraint_string_list(&entry, "url_prefixes").unwrap();
        assert_eq!(list, vec!["https://api.example/".to_string()]);
    }

    #[test]
    fn constraint_string_list_errors_when_key_missing() {
        let entry = PermissionEntry {
            extension: "http".to_string(),
            method: "get".to_string(),
            constraints: BTreeMap::new(),
        };
        let err = constraint_string_list(&entry, "url_prefixes").unwrap_err();
        assert!(err.contains("missing constraint"));
    }

    #[test]
    fn constraint_string_list_errors_on_wrong_type() {
        let mut constraints = BTreeMap::new();
        constraints.insert(
            "url_prefixes".to_string(),
            serde_json::json!("not-an-array"),
        );
        let entry = PermissionEntry {
            extension: "http".to_string(),
            method: "get".to_string(),
            constraints,
        };
        let err = constraint_string_list(&entry, "url_prefixes").unwrap_err();
        assert!(err.contains("must be a string array"));
    }

    #[test]
    fn event_frame_round_trips_through_a_buffer() {
        let mut buf = Vec::new();
        write_event(&mut buf, "state-changed", "{\"group\":1}").unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded: FromExtension = read_frame(&mut cursor).unwrap();
        assert_eq!(
            decoded,
            FromExtension::Event(Event {
                name: "state-changed".to_string(),
                payload: "{\"group\":1}".to_string(),
            })
        );
    }

    #[test]
    fn from_extension_response_round_trips() {
        let mut buf = Vec::new();
        write_frame(
            &mut buf,
            &FromExtension::Response(Response::Ok("body".to_string())),
        )
        .unwrap();
        let mut cursor = Cursor::new(buf);
        let decoded: FromExtension = read_frame(&mut cursor).unwrap();
        assert_eq!(
            decoded,
            FromExtension::Response(Response::Ok("body".to_string()))
        );
    }

    #[test]
    fn serve_with_events_emits_event_and_answers_rpc() {
        use std::os::unix::net::UnixStream;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let socket_path =
            std::env::temp_dir().join(format!("extension-protocol-events-test-{nanos}.sock"));
        let socket_path_str = socket_path.to_string_lossy().into_owned();

        let server_path = socket_path_str.clone();
        let server = thread::spawn(move || {
            serve_with_events_at(&server_path, |emit| {
                emit.emit("ready", "1").unwrap();
                move |request: &Request| Response::Ok(format!("echo:{}", request.payload))
            })
        });

        let mut stream = None;
        for _ in 0..100 {
            match UnixStream::connect(&socket_path_str) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(_) => thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        let mut stream = stream.expect("connect");
        perform_handshake_client(&mut stream).expect("handshake");

        let mut saw_event = false;
        let mut saw_response = false;
        write_frame(
            &mut stream,
            &Request {
                method: "ping".to_string(),
                payload: "hi".to_string(),
            },
        )
        .unwrap();

        while !(saw_event && saw_response) {
            match read_frame::<_, FromExtension>(&mut stream).unwrap() {
                FromExtension::Event(event) => {
                    assert_eq!(event.name, "ready");
                    assert_eq!(event.payload, "1");
                    saw_event = true;
                }
                FromExtension::Response(Response::Ok(body)) => {
                    assert_eq!(body, "echo:hi");
                    saw_response = true;
                }
                FromExtension::Response(Response::Err(msg)) => {
                    panic!("expected Ok, got Err: {msg}")
                }
            }
        }

        drop(stream);
        server.join().unwrap().unwrap();
        let _ = std::fs::remove_file(&socket_path_str);
    }
}
