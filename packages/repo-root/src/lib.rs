use std::fs;
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;

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
