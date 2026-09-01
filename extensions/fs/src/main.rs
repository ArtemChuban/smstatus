use std::path::{Path, PathBuf};

use extension_protocol::{self as protocol, Request, Response, lexical_normalize};

const EXTENSION_NAME: &str = "fs";

fn expand_path(path: &str, home_dir: Option<&Path>) -> Result<PathBuf, String> {
    match path.strip_prefix("~/") {
        Some(rest) => {
            let home = home_dir.ok_or_else(|| "could not determine home directory".to_string())?;
            Ok(home.join(rest))
        }
        None => Ok(PathBuf::from(path)),
    }
}

fn resolve_for_check(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(canon) => canon,
        Err(_) => lexical_normalize(path),
    }
}

fn path_under_prefix(path: &Path, prefix: &Path) -> bool {
    resolve_for_check(path).starts_with(resolve_for_check(prefix))
}

fn read_file(path: &str, home_dir: Option<&Path>) -> Result<String, String> {
    let expanded = expand_path(path, home_dir)?;
    std::fs::read_to_string(&expanded).map_err(|e| format!("read failed: {e}"))
}

fn handle_check(payload: &str, home_dir: Option<&Path>) -> Response {
    let check = match protocol::decode_check_payload(payload) {
        Ok(check) => check,
        Err(msg) => return Response::Err(msg),
    };
    let entries = protocol::matching_entries(&check, EXTENSION_NAME);
    if entries.is_empty() {
        return Response::Err(format!(
            "permission denied: no allowlisted entry for extension `{EXTENSION_NAME}` method `{}`",
            check.method
        ));
    }
    if check.method != "read" {
        return Response::Ok(String::new());
    }
    let expanded_path = match expand_path(&check.payload, home_dir) {
        Ok(path) => path,
        Err(msg) => return Response::Err(msg),
    };
    for entry in entries {
        let prefixes = match protocol::constraint_string_list(entry, "path_prefixes") {
            Ok(prefixes) => prefixes,
            Err(msg) => return Response::Err(msg),
        };
        for prefix in prefixes {
            let expanded_prefix = match expand_path(&prefix, home_dir) {
                Ok(path) => path,
                Err(msg) => return Response::Err(msg),
            };
            if path_under_prefix(&expanded_path, &expanded_prefix) {
                return Response::Ok(String::new());
            }
        }
    }
    Response::Err(format!(
        "permission denied: path `{}` is not under any allowed path_prefixes",
        check.payload
    ))
}

fn handle_request_with(request: &Request, home_dir: Option<&Path>) -> Response {
    if protocol::is_reserved_method(&request.method) {
        return handle_check(&request.payload, home_dir);
    }
    if request.method == "read" {
        match read_file(&request.payload, home_dir) {
            Ok(contents) => Response::Ok(contents),
            Err(msg) => Response::Err(msg),
        }
    } else {
        Response::Err(format!("unknown method: {}", request.method))
    }
}

fn handle_request(request: &Request) -> Response {
    handle_request_with(request, dirs::home_dir().as_deref())
}

fn main() {
    protocol::serve(handle_request);
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn read_check(path: &str, prefixes: &[&str], home_dir: Option<&Path>) -> Response {
        let prefixes_json = prefixes
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(",");
        let encoded = format!(
            r#"{{"permissions":[{{"extension":"fs","method":"read","constraints":{{"path_prefixes":[{prefixes_json}]}}}}],"method":"read","payload":"{path}"}}"#
        );
        handle_request_with(
            &Request {
                method: protocol::CHECK_METHOD.to_string(),
                payload: encoded,
            },
            home_dir,
        )
    }

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

    #[test]
    fn check_allows_path_under_prefix() {
        match read_check("/proc/stat", &["/proc/"], None) {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_path_outside_prefix() {
        match read_check("/etc/passwd", &["/proc/"], None) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn check_denies_dotdot_escape_outside_prefix() {
        match read_check("/proc/../etc/passwd", &["/proc/"], None) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err for .. escape"),
        }
    }

    #[test]
    fn check_denies_symlink_escape_outside_prefix() {
        let root = std::env::temp_dir().join("smstatus-fs-symlink-escape");
        let allowed = root.join("allowed");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&allowed).unwrap();
        let secret = root.join("secret.txt");
        std::fs::write(&secret, "nope\n").unwrap();
        let link = allowed.join("escape");
        symlink(&secret, &link).unwrap();

        let prefix = format!("{}/", allowed.to_string_lossy());
        let path = link.to_string_lossy().into_owned();
        match read_check(&path, &[&prefix], None) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err for symlink escape"),
        }
    }

    #[test]
    fn check_allows_tilde_path_when_prefix_and_path_both_expand() {
        let home = Path::new("/home/testuser");
        match read_check("~/claude/creds.json", &["~/claude/"], Some(home)) {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_when_unexpanded_prefix_would_not_match_expanded_path() {
        let home = Path::new("/home/testuser");
        match read_check(
            "/home/testuser/claude/creds.json",
            &["~/claude/"],
            Some(home),
        ) {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok via expanded prefix, got Err: {msg}"),
        }
        match read_check("/home/testuser/claude/creds.json", &["~/claude/"], None) {
            Response::Err(_) => {}
            Response::Ok(_) => panic!("expected Err when tilde prefix cannot expand"),
        }
    }
}
