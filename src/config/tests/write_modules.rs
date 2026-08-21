use super::*;

#[test]
fn write_module_order_swaps_entries_while_preserving_comments_and_other_keys() {
    let path = unique_temp_path("module-order-preserve");
    std::fs::write(
        &path,
        "# a helpful comment\nother_key = \"other_value\"\nmodules = [\"cpu\", \"disk\" # keep disk\n, \"battery\"]\n",
    )
    .unwrap();

    let result = BarConfig::write_module_order(
        &path,
        &["disk".to_string(), "cpu".to_string(), "battery".to_string()],
    );
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(content.contains("# a helpful comment"));
    assert!(content.contains("other_key = \"other_value\""));
    assert!(content.contains("# keep disk"));

    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    let array = doc["modules"].as_array().unwrap();
    let names: Vec<&str> = array.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(names, vec!["disk", "cpu", "battery"]);

    assert!(content.contains("\"disk\" # keep disk"));
    assert!(!content.contains("\"cpu\" # keep disk"));
}

#[test]
fn write_module_order_errors_when_modules_key_missing() {
    let path = unique_temp_path("module-order-missing-key");
    std::fs::write(&path, "separator = \" | \"\n").unwrap();

    let result = BarConfig::write_module_order(&path, &["cpu".to_string()]);
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}

#[test]
fn write_module_order_errors_when_modules_is_not_an_array() {
    let path = unique_temp_path("module-order-not-array");
    std::fs::write(&path, "modules = \"not-a-list\"\n").unwrap();

    let result = BarConfig::write_module_order(&path, &["cpu".to_string()]);
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}

#[test]
fn write_module_order_errors_on_length_mismatch() {
    let path = unique_temp_path("module-order-length-mismatch");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();

    let result = BarConfig::write_module_order(&path, &["cpu".to_string()]);
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}

#[test]
fn write_module_order_errors_when_an_entry_was_renamed_on_disk() {
    let path = unique_temp_path("module-order-renamed-entry");
    std::fs::write(&path, "modules = [\"cpu\", \"ram\"]\n").unwrap();

    let result = BarConfig::write_module_order(&path, &["disk".to_string(), "cpu".to_string()]);
    let _ = std::fs::remove_file(&path);

    assert!(
        result.is_err(),
        "expected an error when the on-disk entry no longer matches the last-loaded list"
    );
}

#[test]
fn write_module_order_errors_when_file_does_not_exist() {
    let path = unique_temp_path("module-order-missing-file");
    let result = BarConfig::write_module_order(&path, &["cpu".to_string()]);
    assert!(result.is_err());
}

#[test]
fn write_module_order_cleans_up_temp_file_when_rename_fails() {
    use std::os::unix::fs::PermissionsExt;

    let dir = unique_temp_path("module-order-rename-fail-dir");
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = BarConfig::write_module_order(&path, &["disk".to_string(), "cpu".to_string()]);

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    let tmp_path = PathBuf::from(format!("{}.tmp", path.display()));
    if result.is_ok() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
        eprintln!(
            "skipping assertions in write_module_order_cleans_up_temp_file_when_rename_fails: rename unexpectedly succeeded (likely running as root)"
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
fn write_module_add_appends_new_entry_while_preserving_comments_and_other_keys() {
    let path = unique_temp_path("module-add-preserve");
    std::fs::write(
        &path,
        "# a helpful comment\nother_key = \"other_value\"\nmodules = [\"cpu\", \"disk\"]\n",
    )
    .unwrap();

    let result =
        BarConfig::write_module_add(&path, &["cpu".to_string(), "disk".to_string()], "battery");
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(content.contains("# a helpful comment"));
    assert!(content.contains("other_key = \"other_value\""));

    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    let names: Vec<&str> = doc["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["cpu", "disk", "battery"]);
}

#[test]
fn write_module_add_errors_when_entry_already_present() {
    let path = unique_temp_path("module-add-already-present");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();

    let result =
        BarConfig::write_module_add(&path, &["cpu".to_string(), "disk".to_string()], "disk");
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}

#[test]
fn write_module_add_errors_when_current_list_does_not_match_expected() {
    let path = unique_temp_path("module-add-mismatch");
    std::fs::write(&path, "modules = [\"cpu\", \"ram\"]\n").unwrap();

    let result =
        BarConfig::write_module_add(&path, &["cpu".to_string(), "disk".to_string()], "battery");
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}

#[test]
fn write_module_add_errors_when_modules_key_missing() {
    let path = unique_temp_path("module-add-missing-key");
    std::fs::write(&path, "separator = \" | \"\n").unwrap();

    let result = BarConfig::write_module_add(&path, &[], "cpu");
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}

#[test]
fn write_module_add_errors_when_file_does_not_exist() {
    let path = unique_temp_path("module-add-missing-file");
    let result = BarConfig::write_module_add(&path, &[], "cpu");
    assert!(result.is_err());
}

#[test]
fn write_module_remove_removes_entry_while_preserving_neighbor_comments() {
    let path = unique_temp_path("module-remove-preserve");
    std::fs::write(
        &path,
        "modules = [\"cpu\", \"disk\" # keep disk\n, \"battery\"]\n",
    )
    .unwrap();

    let result = BarConfig::write_module_remove(
        &path,
        &["cpu".to_string(), "disk".to_string(), "battery".to_string()],
        0,
    );
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(content.contains("# keep disk"));
    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    let names: Vec<&str> = doc["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["disk", "battery"]);
}

#[test]
fn write_module_remove_errors_when_current_list_does_not_match_expected() {
    let path = unique_temp_path("module-remove-mismatch");
    std::fs::write(&path, "modules = [\"cpu\", \"ram\"]\n").unwrap();

    let result = BarConfig::write_module_remove(&path, &["cpu".to_string(), "disk".to_string()], 0);
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}

#[test]
fn write_module_remove_errors_when_index_out_of_range() {
    let path = unique_temp_path("module-remove-out-of-range");
    std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();

    let result = BarConfig::write_module_remove(&path, &["cpu".to_string(), "disk".to_string()], 2);
    let _ = std::fs::remove_file(&path);

    assert!(result.is_err());
}

#[test]
fn write_module_remove_can_shrink_list_to_empty_array() {
    let path = unique_temp_path("module-remove-to-empty");
    std::fs::write(&path, "modules = [\"cpu\"]\n").unwrap();

    let result = BarConfig::write_module_remove(&path, &["cpu".to_string()], 0);
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    let names: Vec<&str> = doc["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(names.is_empty());
}
