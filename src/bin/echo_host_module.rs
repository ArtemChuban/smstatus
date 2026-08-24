use std::os::unix::net::UnixListener;

use smstatus::host_module::protocol;

fn main() {
    let socket_path = std::env::args()
        .nth(1)
        .expect("missing socket path argument");
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("failed to bind socket");
    let (mut stream, _) = listener.accept().expect("failed to accept connection");
    protocol::perform_handshake_server(&mut stream).expect("handshake failed");

    while let Ok(request) = protocol::read_frame::<_, protocol::Request>(&mut stream) {
        let response = protocol::Response::Ok(request.payload);
        if protocol::write_frame(&mut stream, &response).is_err() {
            break;
        }
    }
}
