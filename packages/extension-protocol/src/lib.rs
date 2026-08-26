use std::collections::BTreeMap;
use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_SIZE: u32 = 8 * 1024 * 1024;

/// Reserved wire method for permission checks before a real extension call.
///
/// Every extension must implement this method. Extensions must not expose a
/// user-facing method with the same name.
pub const CHECK_METHOD: &str = "check";

/// One permission entry from a module manifest, with flattened constraints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionEntry {
    pub extension: String,
    pub method: String,
    #[serde(default)]
    pub constraints: BTreeMap<String, serde_json::Value>,
}

/// Payload sent to [`CHECK_METHOD`]: frozen permissions plus the concrete call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckRequest {
    pub permissions: Vec<PermissionEntry>,
    pub method: String,
    pub payload: String,
}

/// Returns true when `method` is the reserved check gate.
pub fn is_reserved_method(method: &str) -> bool {
    method == CHECK_METHOD
}

/// Entries in `request` that match `extension` and `request.method`.
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

/// Allow when at least one matching entry exists (no constraint validation).
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

/// Read a JSON string-array constraint from `entry`.
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

/// Serialize a [`CheckRequest`] for the check RPC payload.
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

/// Deserialize a check RPC payload into a [`CheckRequest`].
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

#[derive(Serialize, Deserialize)]
pub enum Response {
    Ok(String),
    Err(String),
}

/// Decode a check payload and allow when `extension` + method is allowlisted.
///
/// Stock extensions with no extra constraint logic can return this from their
/// reserved [`CHECK_METHOD`] handler.
pub fn allowlist_check_response(payload: &str, extension: &str) -> Response {
    match decode_check_payload(payload) {
        Ok(check) => match check_allowlisted(&check, extension) {
            Ok(()) => Response::Ok(String::new()),
            Err(msg) => Response::Err(msg),
        },
        Err(msg) => Response::Err(msg),
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
}
