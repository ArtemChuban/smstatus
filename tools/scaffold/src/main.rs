mod validate;
mod workspace;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use validate::is_safe_name;
use workspace::{
    author_from_git, collect_package_names, copy_template_dir, display_name_from, find_repo_root,
    insert_workspace_member, read_host_api_floor, workspace_members,
};

#[derive(Parser)]
#[command(name = "scaffold", about = "Scaffold smstatus modules and extensions")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new module crate under modules/<name>
    Module { name: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Module { name } => match create_module(&name) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        },
    }
}

fn create_module(name: &str) -> Result<(), String> {
    if !is_safe_name(name) {
        return Err(format!("invalid name `{name}`"));
    }

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = find_repo_root(&cwd)?;
    let member = format!("modules/{name}");
    let dest = root.join(&member);

    if dest.exists() {
        return Err(format!("`{member}` already exists"));
    }

    let cargo_path = root.join("Cargo.toml");
    let cargo_text = fs::read_to_string(&cargo_path).map_err(|e| e.to_string())?;
    let doc: toml_edit::DocumentMut = cargo_text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let members = workspace_members(&doc)?;
    if members.iter().any(|m| m == &member) {
        return Err(format!("workspace member `{member}` already listed"));
    }

    let package_names = collect_package_names(&root, &members)?;
    if package_names.iter().any(|p| p == name) {
        return Err(format!("package name `{name}` already exists in workspace"));
    }

    let version_rs = fs::read_to_string(root.join("src/version.rs")).map_err(|e| e.to_string())?;
    let (major, minor) = read_host_api_floor(&version_rs, "HOST_MODULES_API")?;

    let placeholders = [
        ("name", name.to_string()),
        ("display_name", display_name_from(name)),
        ("author", author_from_git()),
        ("modules_api_major", major.to_string()),
        ("modules_api_minor", minor.to_string()),
    ];

    let template = template_dir(&root, "module")?;
    copy_template_dir(&template, &dest, &placeholders)?;

    let updated = insert_workspace_member(&cargo_text, &member, "modules/")?;
    fs::write(&cargo_path, updated).map_err(|e| e.to_string())?;

    println!("created {member}");
    println!("next: just pack-module {name}");
    Ok(())
}

fn template_dir(root: &Path, kind: &str) -> Result<PathBuf, String> {
    let path = root.join("templates").join(kind);
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("template directory missing: {}", path.display()))
    }
}
