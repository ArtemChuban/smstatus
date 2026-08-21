use super::*;

#[test]
fn module_section_string_entries_missing_section() {
    let config = BarConfig::from_table(toml::Table::new());
    assert_eq!(
        config.module_section_string_entries("cpu"),
        ModuleSectionView::Missing
    );
}

#[test]
fn module_section_string_entries_empty_table() {
    let table: toml::Table = toml::from_str("[cpu]\n").unwrap();
    let config = BarConfig::from_table(table);
    assert_eq!(
        config.module_section_string_entries("cpu"),
        ModuleSectionView::Empty
    );
}

#[test]
fn module_section_string_entries_non_table_key_is_missing() {
    let table: toml::Table = toml::from_str("cpu = 1\n").unwrap();
    let config = BarConfig::from_table(table);
    assert_eq!(
        config.module_section_string_entries("cpu"),
        ModuleSectionView::Missing
    );
}

#[test]
fn module_section_string_entries_string_and_non_string_preserve_order() {
    let table: toml::Table = toml::from_str(
        r#"
        [cpu]
        format = "{usage}"
        interval = 5
        nested = { a = 1 }
        tags = ["a", "b"]
        label = "main"
        "#,
    )
    .unwrap();
    let config = BarConfig::from_table(table.clone());
    let expected: Vec<(String, ModuleParamValue)> = table["cpu"]
        .as_table()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            let param = match v.as_str() {
                Some(s) => ModuleParamValue::String(s.to_string()),
                None => ModuleParamValue::NonString,
            };
            (k.clone(), param)
        })
        .collect();
    assert_eq!(
        config.module_section_string_entries("cpu"),
        ModuleSectionView::Entries(expected)
    );
    let ModuleSectionView::Entries(entries) = config.module_section_string_entries("cpu") else {
        panic!("expected Entries");
    };
    assert!(
        entries
            .iter()
            .any(|(k, v)| k == "format" && matches!(v, ModuleParamValue::String(_)))
    );
    assert!(
        entries
            .iter()
            .any(|(k, v)| k == "interval" && matches!(v, ModuleParamValue::NonString))
    );
    assert!(
        entries
            .iter()
            .any(|(k, v)| k == "nested" && matches!(v, ModuleParamValue::NonString))
    );
    assert!(
        entries
            .iter()
            .any(|(k, v)| k == "tags" && matches!(v, ModuleParamValue::NonString))
    );
}

#[test]
fn module_section_string_entries_uses_full_instance_name() {
    let table: toml::Table = toml::from_str(
        r#"
        ["disk#root"]
        path = "/"

        ["disk#home"]
        path = "/home"
        "#,
    )
    .unwrap();
    let config = BarConfig::from_table(table);
    assert_eq!(
        config.module_section_string_entries("disk#root"),
        ModuleSectionView::Entries(vec![(
            "path".to_string(),
            ModuleParamValue::String("/".to_string())
        )])
    );
    assert_eq!(
        config.module_section_string_entries("disk"),
        ModuleSectionView::Missing
    );
}
