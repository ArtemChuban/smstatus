use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use smstatus::{
    HOST_EXTENSIONS_API, HOST_MODULES_API, calver_ord, format_api_version, is_legal_api_step,
    parse_alpha_release_tag, parse_calver, parse_package_version, release_notes_body,
};
use toml_edit::DocumentMut;

#[derive(Parser)]
#[command(
    name = "release-check",
    about = "Validate and prepare smstatus alpha releases"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Validate(ValidateArgs),
    FormatReleaseBody,
    SetVersion(SetVersionArgs),
    ManifestField(ManifestFieldArgs),
}

#[derive(Parser)]
struct ValidateArgs {
    #[arg(long)]
    tag: String,
    #[arg(long)]
    cargo_toml: Option<PathBuf>,
    #[arg(long)]
    previous_calver: Option<String>,
    #[arg(long)]
    previous_modules_api: Option<String>,
    #[arg(long)]
    previous_extensions_api: Option<String>,
    #[arg(long)]
    bootstrap: bool,
}

#[derive(Parser)]
struct SetVersionArgs {
    #[arg(long)]
    calver: String,
    #[arg(long)]
    cargo_toml: Option<PathBuf>,
}

#[derive(Parser)]
struct ManifestFieldArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    key: String,
}

fn default_cargo_toml(override_path: Option<&Path>) -> Result<PathBuf, String> {
    match override_path {
        Some(path) => Ok(path.to_path_buf()),
        None => {
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            Ok(repo_root::find_repo_root(&cwd)?.join("Cargo.toml"))
        }
    }
}

fn read_package_version(cargo_toml: &Path) -> Result<String, String> {
    let text = fs::read_to_string(cargo_toml).map_err(|e| e.to_string())?;
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    doc.get("package")
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("missing [package].version in {}", cargo_toml.display()))
}

fn validate(args: &ValidateArgs) -> Result<(), String> {
    let cargo_path = default_cargo_toml(args.cargo_toml.as_deref())?;
    let cargo_version = read_package_version(&cargo_path)?;
    let calver_base = parse_alpha_release_tag(&args.tag).map_err(|e| e.to_string())?;
    if calver_base != cargo_version {
        return Err(format!(
            "tag calver `{calver_base}` does not match Cargo.toml version `{cargo_version}`"
        ));
    }
    let new_calver = parse_calver(&calver_base).map_err(|e| e.to_string())?;
    if args.bootstrap {
        return Ok(());
    }
    let previous_calver = args
        .previous_calver
        .as_deref()
        .ok_or("missing --previous-calver")?;
    let previous_modules_api_str = args
        .previous_modules_api
        .as_deref()
        .ok_or("missing --previous-modules-api")?;
    let previous_extensions_api_str = args
        .previous_extensions_api
        .as_deref()
        .ok_or("missing --previous-extensions-api")?;
    let previous_calver_tuple = parse_calver(previous_calver).map_err(|e| e.to_string())?;
    if calver_ord(new_calver, previous_calver_tuple) == Ordering::Less {
        return Err(format!(
            "calver `{calver_base}` is before previous release `{previous_calver}`"
        ));
    }
    let previous_modules_api =
        parse_package_version(previous_modules_api_str).map_err(|e| e.to_string())?;
    let previous_extensions_api =
        parse_package_version(previous_extensions_api_str).map_err(|e| e.to_string())?;
    if !is_legal_api_step(previous_modules_api, HOST_MODULES_API) {
        return Err(format!(
            "modules-api step from `{}` to `{}` is not legal",
            format_api_version(previous_modules_api),
            format_api_version(HOST_MODULES_API)
        ));
    }
    if !is_legal_api_step(previous_extensions_api, HOST_EXTENSIONS_API) {
        return Err(format!(
            "extensions-api step from `{}` to `{}` is not legal",
            format_api_version(previous_extensions_api),
            format_api_version(HOST_EXTENSIONS_API)
        ));
    }
    Ok(())
}

fn set_version(args: &SetVersionArgs) -> Result<(), String> {
    parse_calver(&args.calver).map_err(|e| e.to_string())?;
    let cargo_path = default_cargo_toml(args.cargo_toml.as_deref())?;
    let text = fs::read_to_string(&cargo_path).map_err(|e| e.to_string())?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let version = doc
        .get_mut("package")
        .and_then(|package| package.get_mut("version"))
        .ok_or_else(|| format!("missing [package].version in {}", cargo_path.display()))?;
    let current = version.as_str().ok_or("invalid [package].version")?;
    if current == args.calver {
        return Ok(());
    }
    *version = toml_edit::value(&args.calver);
    fs::write(&cargo_path, doc.to_string()).map_err(|e| e.to_string())
}

/// Prints a manifest.toml field: a string value as-is, or `{ major, minor }`
/// as `"<major> <minor>"`, matching the shapes pack-release-archives.sh needs.
fn manifest_field(args: &ManifestFieldArgs) -> Result<String, String> {
    let text = fs::read_to_string(&args.manifest).map_err(|e| e.to_string())?;
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let item = doc
        .get(&args.key)
        .ok_or_else(|| format!("missing `{}` in {}", args.key, args.manifest.display()))?;
    if let Some(s) = item.as_str() {
        return Ok(s.to_string());
    }
    if let Some(table) = item.as_table_like() {
        let field = |name: &str| -> Result<i64, String> {
            table.get(name).and_then(|v| v.as_integer()).ok_or_else(|| {
                format!(
                    "missing `{name}` in `{}` of {}",
                    args.key,
                    args.manifest.display()
                )
            })
        };
        return Ok(format!("{} {}", field("major")?, field("minor")?));
    }
    Err(format!(
        "unsupported field type for `{}` in {}",
        args.key,
        args.manifest.display()
    ))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Validate(args) => validate(&args),
        Command::FormatReleaseBody => {
            print!("{}", release_notes_body());
            Ok(())
        }
        Command::SetVersion(args) => set_version(&args),
        Command::ManifestField(args) => manifest_field(&args).map(|value| print!("{value}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_subcommands() {
        Cli::command().debug_assert();
    }

    #[test]
    fn set_version_rewrites_package_version() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_path = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_path,
            r#"
[workspace]
members = []

[package]
name = "smstatus"
version = "2026.8.25"
"#,
        )
        .unwrap();
        set_version(&SetVersionArgs {
            calver: "2026.8.27".to_string(),
            cargo_toml: Some(cargo_path.clone()),
        })
        .unwrap();
        let updated = fs::read_to_string(&cargo_path).unwrap();
        assert!(updated.contains("version = \"2026.8.27\""));
    }

    #[test]
    fn set_version_no_op_when_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_path = dir.path().join("Cargo.toml");
        let original = r#"
[workspace]
members = []

[package]
name = "smstatus"
version = "2026.8.27"
"#;
        fs::write(&cargo_path, original).unwrap();
        set_version(&SetVersionArgs {
            calver: "2026.8.27".to_string(),
            cargo_toml: Some(cargo_path.clone()),
        })
        .unwrap();
        assert_eq!(fs::read_to_string(&cargo_path).unwrap(), original);
    }

    fn write_manifest(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.toml");
        fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn manifest_field_reads_string_value() {
        let (_dir, manifest) = write_manifest("name = \"battery\"\nversion = \"0.1.0\"\n");
        let value = manifest_field(&ManifestFieldArgs {
            manifest: manifest.clone(),
            key: "name".to_string(),
        })
        .unwrap();
        assert_eq!(value, "battery");
    }

    #[test]
    fn manifest_field_reads_api_major_minor() {
        let (_dir, manifest) = write_manifest("modules-api = { major = 0, minor = 1 }\n");
        let value = manifest_field(&ManifestFieldArgs {
            manifest,
            key: "modules-api".to_string(),
        })
        .unwrap();
        assert_eq!(value, "0 1");
    }

    #[test]
    fn manifest_field_errors_on_missing_key() {
        let (_dir, manifest) = write_manifest("name = \"battery\"\n");
        let err = manifest_field(&ManifestFieldArgs {
            manifest,
            key: "version".to_string(),
        })
        .unwrap_err();
        assert!(err.contains("missing `version`"));
    }
}
