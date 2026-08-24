use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_SIZE: u32 = 8 * 1024 * 1024;

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
}
