use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use toml_edit::{DocumentMut, Value};

pub fn find_repo_root(start: &Path) -> Result<PathBuf, String> {
    let mut dir = start.to_path_buf();
    loop {
        let cargo = dir.join("Cargo.toml");
        if cargo.is_file()
            && let Ok(text) = fs::read_to_string(&cargo)
            && text.contains("name = \"smstatus\"")
            && text.contains("[workspace]")
        {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err("could not find smstatus workspace root".into());
        }
    }
}

pub fn read_host_api_floor(version_rs: &str, const_name: &str) -> Result<(u32, u32), String> {
    let pattern = format!(r"{const_name}:\s*\(u32,\s*u32,\s*u32\)\s*=\s*\((\d+),\s*(\d+),\s*\d+\)");
    let re = Regex::new(&pattern).map_err(|e| e.to_string())?;
    let caps = re
        .captures(version_rs)
        .ok_or_else(|| format!("could not find {const_name} in src/version.rs"))?;
    let major: u32 = caps[1]
        .parse()
        .map_err(|_| format!("bad major in {const_name}"))?;
    let minor: u32 = caps[2]
        .parse()
        .map_err(|_| format!("bad minor in {const_name}"))?;
    Ok((major, minor))
}

pub fn workspace_members(doc: &DocumentMut) -> Result<Vec<String>, String> {
    let members = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .ok_or_else(|| "workspace.members missing".to_string())?;
    let mut out = Vec::new();
    for item in members.iter() {
        let Some(s) = item.as_str() else {
            continue;
        };
        out.push(s.to_string());
    }
    Ok(out)
}

pub fn package_name_from_cargo_toml(text: &str) -> Option<String> {
    let doc: DocumentMut = text.parse().ok()?;
    doc.get("package")?.get("name")?.as_str().map(str::to_owned)
}

pub fn collect_package_names(root: &Path, members: &[String]) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let root_cargo = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    if let Some(name) = package_name_from_cargo_toml(&root_cargo) {
        names.push(name);
    }
    for member in members {
        let cargo = root.join(member).join("Cargo.toml");
        let text = fs::read_to_string(&cargo)
            .map_err(|e| format!("failed to read {}: {e}", cargo.display()))?;
        if let Some(name) = package_name_from_cargo_toml(&text) {
            names.push(name);
        }
    }
    Ok(names)
}

pub fn insert_workspace_member(
    cargo_toml: &str,
    member: &str,
    group_prefix: &str,
) -> Result<String, String> {
    let mut doc: DocumentMut = cargo_toml
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let members = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("members"))
        .and_then(|m| m.as_array_mut())
        .ok_or_else(|| "workspace.members missing".to_string())?;

    let existing: Vec<String> = members
        .iter()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect();
    if existing.iter().any(|m| m == member) {
        return Err(format!("workspace member `{member}` already listed"));
    }

    let mut insert_at = None;
    for (i, m) in existing.iter().enumerate() {
        if m.starts_with(group_prefix) {
            insert_at = Some(i + 1);
        }
    }
    let idx = insert_at.unwrap_or(existing.len());
    members.insert(idx, Value::from(member));
    Ok(doc.to_string())
}

pub fn display_name_from(name: &str) -> String {
    name.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut s = first.to_uppercase().collect::<String>();
                    s.push_str(chars.as_str());
                    s
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escape a value for a TOML basic double-quoted string.
pub fn escape_toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

pub fn author_from_git() -> String {
    let raw = std::process::Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if name.is_empty() { None } else { Some(name) }
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Author".to_string());
    escape_toml_basic_string(&raw)
}

pub fn render_template(text: &str, placeholders: &[(&str, String)]) -> String {
    let mut out = text.to_string();
    for (key, value) in placeholders {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

pub fn copy_template_dir(
    template_dir: &Path,
    dest_dir: &Path,
    placeholders: &[(&str, String)],
) -> Result<(), String> {
    copy_template_dir_inner(template_dir, dest_dir, placeholders)
}

fn copy_template_dir_inner(
    src: &Path,
    dest: &Path,
    placeholders: &[(&str, String)],
) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if path.is_dir() {
            copy_template_dir_inner(&path, &dest_path, placeholders)?;
        } else {
            let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let rendered = render_template(&text, placeholders);
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&dest_path, rendered).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_host_api_floor_parses_modules_and_extensions() {
        let src = r#"
pub(crate) const HOST_MODULES_API: (u32, u32, u32) = (0, 1, 0);
pub(crate) const HOST_EXTENSIONS_API: (u32, u32, u32) = (2, 3, 0);
"#;
        assert_eq!(
            read_host_api_floor(src, "HOST_MODULES_API").unwrap(),
            (0, 1)
        );
        assert_eq!(
            read_host_api_floor(src, "HOST_EXTENSIONS_API").unwrap(),
            (2, 3)
        );
    }

    #[test]
    fn display_name_title_cases_splits() {
        assert_eq!(display_name_from("scratch-module"), "Scratch Module");
        assert_eq!(display_name_from("my_thing"), "My Thing");
        assert_eq!(display_name_from("cpu"), "Cpu");
    }

    #[test]
    fn render_template_replaces_placeholders() {
        let out = render_template(
            "name={{name}} display={{display_name}}",
            &[("name", "foo".into()), ("display_name", "Foo".into())],
        );
        assert_eq!(out, "name=foo display=Foo");
    }

    #[test]
    fn escape_toml_basic_string_quotes_and_controls() {
        assert_eq!(escape_toml_basic_string(r#"Foo "Bar""#), r#"Foo \"Bar\""#);
        assert_eq!(escape_toml_basic_string("a\\b"), "a\\\\b");
        assert_eq!(escape_toml_basic_string("a\nb"), "a\\nb");
    }

    #[test]
    fn insert_workspace_member_after_last_modules_entry() {
        let cargo = r#"
[workspace]
members = ["modules/a", "modules/b", "packages/x", "extensions/y"]
"#;
        let updated = insert_workspace_member(cargo, "modules/c", "modules/").unwrap();
        let doc: DocumentMut = updated.parse().unwrap();
        let members = workspace_members(&doc).unwrap();
        assert_eq!(
            members,
            vec![
                "modules/a".to_string(),
                "modules/b".to_string(),
                "modules/c".to_string(),
                "packages/x".to_string(),
                "extensions/y".to_string(),
            ]
        );
    }

    #[test]
    fn insert_workspace_member_rejects_duplicate() {
        let cargo = r#"
[workspace]
members = ["modules/a", "packages/x"]
"#;
        let err = insert_workspace_member(cargo, "modules/a", "modules/").unwrap_err();
        assert!(err.contains("already listed"));
    }

    #[test]
    fn package_name_collision_scan_includes_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["modules/cpu"]

[package]
name = "smstatus"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("modules/cpu")).unwrap();
        fs::write(
            dir.path().join("modules/cpu/Cargo.toml"),
            r#"
[package]
name = "cpu"
version = "0.1.0"
"#,
        )
        .unwrap();
        let names = collect_package_names(dir.path(), &["modules/cpu".to_string()]).unwrap();
        assert!(names.contains(&"smstatus".to_string()));
        assert!(names.contains(&"cpu".to_string()));
    }

    #[test]
    fn find_repo_root_walks_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("Cargo.toml"),
            r#"
[workspace]
members = []

[package]
name = "smstatus"
version = "0.1.0"
"#,
        )
        .unwrap();
        let nested = root.join("a/b");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_repo_root(&nested).unwrap(), root);
    }
}
