mod validate;
mod workspace;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use validate::is_safe_name;
use workspace::{
    author_from_git, collect_package_names, copy_template_dir, display_name_from,
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
    /// Create a new extension crate under extensions/<name>
    Extension { name: String },
}

enum CrateKind {
    Module,
    Extension,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Module { name } => create_crate(&name, CrateKind::Module),
        Command::Extension { name } => create_crate(&name, CrateKind::Extension),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn create_crate(name: &str, kind: CrateKind) -> Result<(), String> {
    let (kind_dir, template, api_const, group_prefix) = match kind {
        CrateKind::Module => ("modules", "module", "HOST_MODULES_API", "modules/"),
        CrateKind::Extension => (
            "extensions",
            "extension",
            "HOST_EXTENSIONS_API",
            "extensions/",
        ),
    };

    let root = prepare_crate(name, kind_dir)?;
    let version_rs = fs::read_to_string(root.join("src/version.rs")).map_err(|e| e.to_string())?;
    let (major, minor) = read_host_api_floor(&version_rs, api_const)?;

    let mut placeholders = vec![("name", name.to_string()), ("author", author_from_git())];
    match kind {
        CrateKind::Module => {
            placeholders.push(("display_name", display_name_from(name)));
            placeholders.push(("modules_api_major", major.to_string()));
            placeholders.push(("modules_api_minor", minor.to_string()));
        }
        CrateKind::Extension => {
            placeholders.push(("extensions_api_major", major.to_string()));
            placeholders.push(("extensions_api_minor", minor.to_string()));
        }
    }

    let member = format!("{kind_dir}/{name}");
    let dest = root.join(&member);
    let template_path = template_dir(&root, template)?;

    if let Err(e) = copy_template_dir(&template_path, &dest, &placeholders) {
        let _ = fs::remove_dir_all(&dest);
        return Err(e);
    }

    let cargo_path = root.join("Cargo.toml");
    let write_member = (|| {
        let cargo_text = fs::read_to_string(&cargo_path).map_err(|e| e.to_string())?;
        let updated = insert_workspace_member(&cargo_text, &member, group_prefix)?;
        fs::write(&cargo_path, updated).map_err(|e| e.to_string())
    })();
    if let Err(e) = write_member {
        let _ = fs::remove_dir_all(&dest);
        return Err(e);
    }

    println!("created {member}");
    match kind {
        CrateKind::Module => println!("next: just pack-module {name}"),
        CrateKind::Extension => {
            println!("next: just pack-extension {name} {name}");
            println!("note: archive binary member is always named `extension`");
        }
    }
    Ok(())
}

fn prepare_crate(name: &str, kind: &str) -> Result<PathBuf, String> {
    if !is_safe_name(name) {
        return Err(format!("invalid name `{name}`"));
    }

    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = repo_root::find_repo_root(&cwd)?;
    let member = format!("{kind}/{name}");
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

    Ok(root)
}

fn template_dir(root: &Path, kind: &str) -> Result<PathBuf, String> {
    let path = root.join("templates").join(kind);
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("template directory missing: {}", path.display()))
    }
}
