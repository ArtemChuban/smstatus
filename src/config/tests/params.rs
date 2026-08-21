use super::*;

#[test]
fn write_module_param_set_updates_string_preserving_comments_and_other_keys() {
    let path = unique_temp_path("param-set-string");
    std::fs::write(
        &path,
        "# keep me\nmodules = [\"cpu\"]\n\n[cpu]\n# before format\nformat = \"old\" # trail\nother = 1\n",
    )
    .unwrap();

    let result = BarConfig::write_module_param_set(
        &path,
        "cpu",
        "format",
        "new",
        &ParamWriteExpect::ExistingString("old".to_string()),
    );
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    assert!(content.contains("# keep me"));
    assert!(content.contains("# before format"));
    assert!(content.contains("# trail"));
    assert!(content.contains("other = 1"));
    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(doc["cpu"]["format"].as_str(), Some("new"));
}

#[test]
fn write_module_param_set_errors_when_string_value_drifted() {
    let path = unique_temp_path("param-set-drift");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nformat = \"disk\"\n").unwrap();

    let result = BarConfig::write_module_param_set(
        &path,
        "cpu",
        "format",
        "new",
        &ParamWriteExpect::ExistingString("old".to_string()),
    );
    let _ = std::fs::remove_file(&path);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("changed on disk"),
        "expected drift message, got {err}"
    );
}

#[test]
fn write_module_param_set_errors_when_expected_string_became_non_string() {
    let path = unique_temp_path("param-set-string-became-int");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nformat = 42\n").unwrap();

    let result = BarConfig::write_module_param_set(
        &path,
        "cpu",
        "format",
        "new",
        &ParamWriteExpect::ExistingString("old".to_string()),
    );
    let _ = std::fs::remove_file(&path);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("changed on disk"),
        "non-string drift must use changed-on-disk message, got {err}"
    );
    assert!(
        !err.contains("missing or not a string"),
        "must not use missing message for present non-string, got {err}"
    );
}

#[test]
fn write_module_param_set_converts_non_string_to_string() {
    let path = unique_temp_path("param-set-non-string");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\ncount = 42\n").unwrap();

    let result = BarConfig::write_module_param_set(
        &path,
        "cpu",
        "count",
        "forty-two",
        &ParamWriteExpect::ExistingNonString,
    );
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(doc["cpu"]["count"].as_str(), Some("forty-two"));
}

#[test]
fn write_module_param_set_add_creates_missing_section() {
    let path = unique_temp_path("param-set-create-section");
    std::fs::write(&path, "modules = [\"cpu\"]\n").unwrap();

    let result = BarConfig::write_module_param_set(
        &path,
        "cpu",
        "format",
        "hi",
        &ParamWriteExpect::KeyAbsent,
    );
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(doc["cpu"]["format"].as_str(), Some("hi"));
}

#[test]
fn write_module_param_set_add_errors_when_key_already_present() {
    let path = unique_temp_path("param-set-add-dup");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nformat = \"x\"\n").unwrap();

    let result = BarConfig::write_module_param_set(
        &path,
        "cpu",
        "format",
        "y",
        &ParamWriteExpect::KeyAbsent,
    );
    let _ = std::fs::remove_file(&path);
    assert!(result.is_err());
}

#[test]
fn write_module_param_set_errors_on_non_table_top_level_key() {
    let path = unique_temp_path("param-set-non-table");
    std::fs::write(&path, "modules = [\"cpu\"]\ncpu = \"not-a-table\"\n").unwrap();

    let result = BarConfig::write_module_param_set(
        &path,
        "cpu",
        "format",
        "x",
        &ParamWriteExpect::KeyAbsent,
    );
    let _ = std::fs::remove_file(&path);
    assert!(result.is_err());
}

#[test]
fn write_module_param_remove_leaves_empty_table_when_last_key_gone() {
    let path = unique_temp_path("param-remove-last");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nformat = \"x\"\n").unwrap();

    let result = BarConfig::write_module_param_remove(&path, "cpu", "format");
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(content.contains("[cpu]"));
    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    assert!(doc["cpu"].as_table().unwrap().is_empty());
}

#[test]
fn write_module_param_remove_errors_when_key_missing() {
    let path = unique_temp_path("param-remove-missing");
    std::fs::write(&path, "modules = [\"cpu\"]\n\n[cpu]\nformat = \"x\"\n").unwrap();

    let result = BarConfig::write_module_param_remove(&path, "cpu", "nope");
    let _ = std::fs::remove_file(&path);
    assert!(result.is_err());
}

#[test]
fn write_module_param_rename_moves_item_preserving_value_and_neighbor_keys() {
    let path = unique_temp_path("param-rename");
    std::fs::write(
        &path,
        "modules = [\"cpu\"]\n\n[cpu]\nbefore = 0\nold = \"keep\" # trail\nafter = 1\n",
    )
    .unwrap();

    let result = BarConfig::write_module_param_rename(&path, "cpu", "old", "new");
    assert!(result.is_ok(), "{result:?}");

    let content = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(content.contains("before = 0"));
    assert!(content.contains("after = 1"));
    assert!(content.contains("# trail"));
    let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
    assert!(doc["cpu"].get("old").is_none());
    assert_eq!(doc["cpu"]["new"].as_str(), Some("keep"));
    let keys: Vec<&str> = doc["cpu"]
        .as_table()
        .unwrap()
        .iter()
        .map(|(k, _)| k)
        .collect();
    assert_eq!(keys, vec!["before", "new", "after"]);
}

#[test]
fn write_module_param_rename_errors_when_new_key_present() {
    let path = unique_temp_path("param-rename-dup");
    std::fs::write(
        &path,
        "modules = [\"cpu\"]\n\n[cpu]\na = \"1\"\nb = \"2\"\n",
    )
    .unwrap();

    let result = BarConfig::write_module_param_rename(&path, "cpu", "a", "b");
    let _ = std::fs::remove_file(&path);
    assert!(result.is_err());
}
