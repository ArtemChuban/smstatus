use crate::config::{default_config_dir, init_config_layout};
use crate::error::Result;

pub(crate) fn cmd_init(force: bool) -> Result<String> {
    let config_dir = default_config_dir()?;
    init_config_layout(&config_dir, force)?;
    Ok(format!("initialized config at {}", config_dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{init_config_layout, program_config_path, test_fixtures};

    #[test]
    fn init_config_layout_errors_when_files_exist_without_force() {
        let dir = test_fixtures::unique_config_dir("init-no-force");
        init_config_layout(&dir, false).unwrap();
        let err = init_config_layout(&dir, false).unwrap_err();
        assert!(err.to_string().contains("already exists"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_config_layout_succeeds_when_dirs_exist_without_config() {
        let dir = test_fixtures::unique_config_dir("init-partial");
        std::fs::create_dir_all(dir.join("modules")).unwrap();
        std::fs::create_dir_all(dir.join("extensions")).unwrap();

        init_config_layout(&dir, false).unwrap();
        assert!(program_config_path(&dir).is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
