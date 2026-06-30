use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub success: bool,
    pub output: String,
}

pub fn detect_commands(target_path: &Path) -> Vec<String> {
    let mut dir = if target_path.is_file() {
        target_path.parent().map(Path::to_path_buf)
    } else {
        Some(target_path.to_path_buf())
    };

    while let Some(current) = dir {
        let package_json = current.join("package.json");
        if package_json.exists() {
            return detect_package_scripts(&package_json);
        }
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            return detect_cargo_commands();
        }
        dir = current.parent().map(Path::to_path_buf);
    }

    Vec::new()
}

pub async fn run_commands(root: &Path, commands: &[String]) -> Result<ValidationResult> {
    if commands.is_empty() {
        return Ok(ValidationResult {
            success: true,
            output: "No validation commands configured; validation skipped.".to_string(),
        });
    }

    let mut combined = String::new();
    for command in commands {
        combined.push_str(&format!("$ {command}\n"));
        let output = Command::new("bash")
            .arg("-lc")
            .arg(command)
            .current_dir(root)
            .output()
            .await?;

        combined.push_str(&String::from_utf8_lossy(&output.stdout));
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
        if !output.status.success() {
            return Ok(ValidationResult {
                success: false,
                output: combined,
            });
        }
    }

    Ok(ValidationResult {
        success: true,
        output: combined,
    })
}

fn detect_package_scripts(package_json: &Path) -> Vec<String> {
    let Ok(contents) = std::fs::read_to_string(package_json) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return Vec::new();
    };

    let scripts = value
        .get("scripts")
        .and_then(|scripts| scripts.as_object())
        .cloned()
        .unwrap_or_default();

    ["test", "typecheck", "lint"]
        .into_iter()
        .filter(|name| scripts.contains_key(*name))
        .map(package_manager_command(package_json))
        .collect()
}

fn package_manager_command(package_json: &Path) -> impl Fn(&str) -> String + '_ {
    let root = package_json.parent().unwrap_or(Path::new("."));
    let package_manager = if root.join("bun.lock").exists() {
        "bun run"
    } else if root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if root.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm run"
    };

    move |script| format!("{package_manager} {script}")
}

fn detect_cargo_commands() -> Vec<String> {
    let mut commands = vec!["cargo check --all-targets".to_string()];
    if cargo_clippy_available() {
        commands.push("cargo clippy --all-targets -- -D warnings".to_string());
    }
    commands
}

fn cargo_clippy_available() -> bool {
    std::process::Command::new("cargo")
        .args(["clippy", "--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[allow(dead_code)]
fn nearest_existing_dir(path: &Path) -> PathBuf {
    if path.is_dir() {
        return path.to_path_buf();
    }
    path.parent().unwrap_or(Path::new(".")).to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cargo_validation_commands_from_rust_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        let commands = detect_commands(&src);

        assert_eq!(commands[0], "cargo check --all-targets");
        if cargo_clippy_available() {
            assert!(commands
                .iter()
                .any(|command| command == "cargo clippy --all-targets -- -D warnings"));
        }
    }
}
