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

pub async fn run_commands(root: &Path, commands: &[String]) -> Result<ValidationResult> {
    if commands.is_empty() {
        return Ok(ValidationResult {
            success: false,
            retryable: false,
            output: "No explicit validation commands were configured; automatic coding-tooling validation is required."
                .to_string(),
        });
    }

    let mut combined = String::new();
    for command in commands {
        combined.push_str(&format!("$ {command}\n"));
        let output = match Command::new("bash")
            .arg("-lc")
            .arg(command)
            .current_dir(root)
            .output()
            .await
        {
            Ok(output) => output,
            Err(error) => {
                combined.push_str(&format!("failed to start validation shell: {error}\n"));
                return Ok(ValidationResult {
                    success: false,
                    retryable: false,
                    output: combined,
                });
            }
        };

        combined.push_str(&String::from_utf8_lossy(&output.stdout));
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
        if !output.status.success() {
            let exit_code = output.status.code();
            if let Some(code) = exit_code {
                combined.push_str(&format!("validation command exited with code {code}\n"));
            } else {
                combined.push_str("validation command terminated without an exit code\n");
            }
            return Ok(ValidationResult {
                success: false,
                retryable: !matches!(exit_code, Some(126 | 127) | None),
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

    #[tokio::test]
    async fn ordinary_command_failure_is_retryable() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_commands(dir.path(), &["false".to_string()])
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.retryable);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_not_found_is_a_non_retryable_tooling_failure() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_commands(
            dir.path(),
            &["local-refactor-command-that-does-not-exist".to_string()],
        )
        .await
        .unwrap();
        assert!(!result.success);
        assert!(!result.retryable);
        assert!(result.output.contains("code 127"));
    }
}
