use super::*;

fn write_module_pkg(dir: &std::path::Path, name: &str) {
    let pkg = dir.join(name);
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("manifest.toml"), b"").unwrap();
    std::fs::write(pkg.join("module.wasm"), b"").unwrap();
}

#[test]
fn discover_module_kinds_lists_wasm_stems_sorted() {
    let dir = unique_temp_path("discover-sorted-dir");
    std::fs::create_dir(&dir).unwrap();
    write_module_pkg(&dir, "ram");
    write_module_pkg(&dir, "cpu");
    write_module_pkg(&dir, "battery");

    let result = discover_module_kinds(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(result, vec!["battery", "cpu", "ram"]);
}

#[test]
fn discover_module_kinds_ignores_non_wasm_files() {
    let dir = unique_temp_path("discover-ignores-non-wasm");
    std::fs::create_dir(&dir).unwrap();
    write_module_pkg(&dir, "cpu");
    std::fs::write(dir.join("README.md"), b"").unwrap();
    std::fs::write(dir.join("notes.txt"), b"").unwrap();
    let incomplete = dir.join("ram");
    std::fs::create_dir(&incomplete).unwrap();
    std::fs::write(incomplete.join("manifest.toml"), b"").unwrap();

    let result = discover_module_kinds(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(result, vec!["cpu"]);
}

#[test]
fn discover_module_kinds_returns_empty_vec_when_directory_missing() {
    let dir = unique_temp_path("discover-missing-dir");
    let result = discover_module_kinds(&dir);
    assert_eq!(result.unwrap(), Vec::<String>::new());
}

#[test]
fn discover_module_kinds_returns_empty_vec_for_empty_directory() {
    let dir = unique_temp_path("discover-empty-dir");
    std::fs::create_dir(&dir).unwrap();

    let result = discover_module_kinds(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(result, Vec::<String>::new());
}

#[test]
fn discover_module_kinds_propagates_other_io_errors() {
    use std::os::unix::fs::PermissionsExt;

    let dir = unique_temp_path("discover-no-read-perm-dir");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = discover_module_kinds(&dir);

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    if result.is_ok() {
        let _ = std::fs::remove_dir(&dir);
        eprintln!(
            "skipping assertions in discover_module_kinds_propagates_other_io_errors: read_dir unexpectedly succeeded (likely running as root)"
        );
        return;
    }

    let _ = std::fs::remove_dir(&dir);
    assert!(result.is_err());
}
