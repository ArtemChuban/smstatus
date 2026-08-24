use std::os::unix::net::UnixListener;

use extension_protocol::{self as protocol, Request, Response};

fn main() {
    let socket_path = std::env::args()
        .nth(1)
        .expect("missing socket path argument");
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("failed to bind socket");
    let (mut stream, _) = listener.accept().expect("failed to accept connection");
    protocol::perform_handshake_server(&mut stream).expect("handshake failed");

    while let Ok(request) = protocol::read_frame::<_, Request>(&mut stream) {
        let response = Response::Ok(request.payload);
        if protocol::write_frame(&mut stream, &response).is_err() {
            break;
        }
    }
}
