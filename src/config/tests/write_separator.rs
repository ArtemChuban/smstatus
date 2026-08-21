use super::*;

#[test]
fn write_separator_updates_value_while_preserving_comments_and_other_keys() {
    let path = unique_temp_path("preserve");
    std::fs::write(
        &path,
        "# a helpful comment\nother_key = \"other_value\"\nseparator = \" | \"\n",
    )
    .unwrap();

    let result = BarConfig::write_separator(&path, " :: ");
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(content.contains("# a helpful comment"));
    assert!(content.contains("other_key = \"other_value\""));
    assert!(content.contains("separator = \" :: \""));
}

#[test]
fn write_separator_inserts_key_when_missing() {
    let path = unique_temp_path("insert");
    std::fs::write(&path, "other_key = \"other_value\"\n").unwrap();

    let result = BarConfig::write_separator(&path, " | ");
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(content.contains("other_key = \"other_value\""));
    assert!(content.contains("separator = \" | \""));
}

#[test]
fn write_separator_accepts_empty_string() {
    let path = unique_temp_path("empty");
    std::fs::write(&path, "separator = \" | \"\n").unwrap();

    let result = BarConfig::write_separator(&path, "");
    assert!(result.is_ok());

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(content.contains("separator = \"\""));
}

#[test]
fn write_separator_errors_when_file_does_not_exist() {
    let path = unique_temp_path("missing");
    let result = BarConfig::write_separator(&path, " | ");
    assert!(result.is_err());
}

#[test]
fn write_separator_cleans_up_temp_file_when_rename_fails() {
    use std::os::unix::fs::PermissionsExt;

    let dir = unique_temp_path("rename-fail-dir");
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "separator = \" | \"\n").unwrap();

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = BarConfig::write_separator(&path, " :: ");

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));
    if result.is_ok() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
        eprintln!(
            "skipping assertions in write_separator_cleans_up_temp_file_when_rename_fails: rename unexpectedly succeeded (likely running as root)"
        );
        return;
    }

    assert!(
        !tmp_path.exists(),
        "temp file should have been cleaned up after rename failure"
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn write_separator_errors_when_content_is_not_valid_toml() {
    let path = unique_temp_path("invalid");
    std::fs::write(&path, "this is not valid toml [[[").unwrap();

    let result = BarConfig::write_separator(&path, " | ");
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}
