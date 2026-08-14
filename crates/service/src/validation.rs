use anyhow::Result;
use serde::Serialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub success: bool,
    pub retryable: bool,
    pub output: String,
}

pub async fn run_commands(
    root: &Path,
    commands: &[String],
) -> Result<ValidationResult> {
    if commands.is_empty() {
        return Ok(ValidationResult {
            success: false,
            retryable: false,
            output: "No explicit validation commands were configured; the coding-tooling final gate is required.".to_string(),
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
                retryable: true,
                output: combined,
            });
        }
    }

    Ok(ValidationResult {
        success: true,
        retryable: false,
        output: combined,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_legacy_command_list_is_not_success() {
        let dir = tempfile::tempdir().unwrap();

        let result = run_commands(dir.path(), &[]).await.unwrap();

        assert!(!result.success);
        assert!(!result.retryable);
    }
}
