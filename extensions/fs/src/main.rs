use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use extension_protocol::{self as protocol, Request, Response};

fn expand_path(path: &str, home_dir: Option<&Path>) -> Result<PathBuf, String> {
    match path.strip_prefix("~/") {
        Some(rest) => {
            let home = home_dir.ok_or_else(|| "could not determine home directory".to_string())?;
            Ok(home.join(rest))
        }
        None => Ok(PathBuf::from(path)),
    }
}

fn read_file(path: &str, home_dir: Option<&Path>) -> Result<String, String> {
    let expanded = expand_path(path, home_dir)?;
    std::fs::read_to_string(&expanded).map_err(|e| format!("read failed: {e}"))
}

fn handle_request(request: &Request) -> Response {
    if request.method == "read" {
        match read_file(&request.payload, dirs::home_dir().as_deref()) {
            Ok(contents) => Response::Ok(contents),
            Err(msg) => Response::Err(msg),
        }
    } else {
        Response::Err(format!("unknown method: {}", request.method))
    }
}

fn main() {
    let socket_path = std::env::args()
        .nth(1)
        .expect("missing socket path argument");
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("failed to bind socket");
    let (mut stream, _) = listener.accept().expect("failed to accept connection");
    protocol::perform_handshake_server(&mut stream).expect("handshake failed");

    while let Ok(request) = protocol::read_frame::<_, Request>(&mut stream) {
        let response = handle_request(&request);
        if protocol::write_frame(&mut stream, &response).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_file_contents_for_existing_path() {
        let dir = std::env::temp_dir().join("smstatus-fs-ok");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("capacity");
        std::fs::write(&path, "87\n").unwrap();

        let response = handle_request(&Request {
            method: "read".to_string(),
            payload: path.to_string_lossy().into_owned(),
        });
        match response {
            Response::Ok(body) => assert_eq!(body, "87\n"),
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn read_returns_err_for_missing_path() {
        let response = handle_request(&Request {
            method: "read".to_string(),
            payload: "/no/such/path/for/smstatus-fs".to_string(),
        });
        match response {
            Response::Err(msg) => assert!(msg.starts_with("read failed:")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn unknown_method_returns_err() {
        let response = handle_request(&Request {
            method: "nope".to_string(),
            payload: String::new(),
        });
        match response {
            Response::Err(msg) => assert!(msg.contains("unknown method")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn expand_path_should_join_home_dir_when_path_starts_with_tilde_slash() {
        let home = Path::new("/home/user");
        let result = expand_path("~/foo/bar", Some(home)).unwrap();
        assert_eq!(result, PathBuf::from("/home/user/foo/bar"));
    }

    #[test]
    fn expand_path_should_return_error_when_tilde_path_given_and_no_home_dir() {
        assert!(expand_path("~/foo", None).is_err());
    }

    #[test]
    fn expand_path_should_return_path_unchanged_when_path_has_no_tilde_prefix() {
        let result = expand_path("/sys/class/power_supply/BAT0", None).unwrap();
        assert_eq!(result, PathBuf::from("/sys/class/power_supply/BAT0"));
    }

    #[test]
    fn read_expands_tilde_slash_against_injected_home() {
        let home = std::env::temp_dir().join("smstatus-fs-home");
        let _ = std::fs::create_dir_all(&home);
        let file = home.join("capacity");
        std::fs::write(&file, "42\n").unwrap();

        let contents = read_file("~/capacity", Some(&home)).unwrap();
        assert_eq!(contents, "42\n");
    }
}
