use crate::validation::ValidationResult;
use anyhow::Result;
use serde::Deserialize;
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};
use tokio::process::Command;

const DEFAULT_BINARY: &str = "coding-tooling";
const FINAL_TIER: &str = "full";

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ToolingStatus {
    Passed,
    Failed,
    Unavailable,
    Error,
}

impl ToolingStatus {
    fn expected_exit_code(self) -> i32 {
        match self {
            Self::Passed => 0,
            Self::Failed => 1,
            Self::Unavailable => 2,
            Self::Error => 3,
        }
    }

    fn retryable(self) -> bool {
        self == Self::Failed
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolingEnvelope {
    schema_version: u32,
    operation: String,
    status: ToolingStatus,
}

pub async fn run_final_gate(root: &Path) -> Result<ValidationResult> {
    let binary = std::env::var_os("LOCAL_REFACTOR_CODING_TOOLING_BIN")
        .unwrap_or_else(|| OsString::from(DEFAULT_BINARY));
    run_final_gate_with_binary(root, &binary).await
}

async fn run_final_gate_with_binary(root: &Path, binary: &OsStr) -> Result<ValidationResult> {
    let output = match Command::new(binary)
        .arg("run")
        .arg("--tier")
        .arg(FINAL_TIER)
        .arg("--strict")
        .arg("--json")
        .current_dir(root)
        .output()
        .await
    {
        Ok(output) => output,
        Err(error) => {
            return Ok(tooling_error(format!(
                "failed to start coding-tooling; install it or set LOCAL_REFACTOR_CODING_TOOLING_BIN (attempted {}): {error}",
                Path::new(binary).display()
            )));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if stdout.trim().is_empty() {
        return Ok(tooling_error_with_process_output(
            "coding-tooling returned no JSON output",
            &stdout,
            &stderr,
        ));
    }

    let envelope: ToolingEnvelope = match serde_json::from_str(stdout.trim()) {
        Ok(envelope) => envelope,
        Err(error) => {
            return Ok(tooling_error_with_process_output(
                format!("coding-tooling returned an invalid JSON envelope: {error}"),
                &stdout,
                &stderr,
            ));
        }
    };
    if envelope.schema_version != 1 {
        return Ok(tooling_error_with_process_output(
            format!(
                "unsupported coding-tooling schema version: {}",
                envelope.schema_version
            ),
            &stdout,
            &stderr,
        ));
    }
    if envelope.operation != "run" {
        return Ok(tooling_error_with_process_output(
            format!(
                "coding-tooling returned operation {}, expected run",
                envelope.operation
            ),
            &stdout,
            &stderr,
        ));
    }

    let expected_exit_code = envelope.status.expected_exit_code();
    if output.status.code() != Some(expected_exit_code) {
        return Ok(tooling_error_with_process_output(
            format!(
                "coding-tooling status/exit-code mismatch: status {:?} requires exit code {expected_exit_code}, got {:?}",
                envelope.status,
                output.status.code()
            ),
            &stdout,
            &stderr,
        ));
    }

    Ok(ValidationResult {
        success: envelope.status == ToolingStatus::Passed,
        retryable: envelope.status.retryable(),
        output: stdout.trim().to_string(),
    })
}

fn tooling_error(message: impl Into<String>) -> ValidationResult {
    ValidationResult {
        success: false,
        retryable: false,
        output: message.into(),
    }
}

fn tooling_error_with_process_output(
    message: impl Into<String>,
    stdout: &str,
    stderr: &str,
) -> ValidationResult {
    let mut message = message.into();
    if !stdout.trim().is_empty() {
        message.push_str("\nstdout:\n");
        message.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        message.push_str("\nstderr:\n");
        message.push_str(stderr.trim());
    }
    tooling_error(message)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[tokio::test]
    async fn invokes_current_full_strict_tier_contract() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_binary(
            dir.path(),
            r#"if [ "$*" != "run --tier full --strict --json" ]; then
  printf '%s\n' '{"schemaVersion":1,"operation":"run","status":"error","durationMs":1,"data":{},"diagnostics":[]}'
  exit 3
fi
printf '%s\n' '{"schemaVersion":1,"operation":"run","status":"passed","durationMs":1,"data":{"tier":"full"},"diagnostics":[]}'
exit 0"#,
        );

        let result = run_final_gate_with_binary(dir.path(), binary.as_os_str())
            .await
            .unwrap();

        assert!(result.success);
        assert!(!result.retryable);
        let evidence: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(evidence["operation"], "run");
        assert_eq!(evidence["data"]["tier"], "full");
    }

    #[tokio::test]
    async fn failed_run_is_retryable_code_validation_failure() {
        let result = run_status(ToolingStatus::Failed, 1).await;
        assert!(!result.success);
        assert!(result.retryable);
    }

    #[tokio::test]
    async fn unavailable_run_is_not_repairable_by_the_model() {
        let result = run_status(ToolingStatus::Unavailable, 2).await;
        assert!(!result.success);
        assert!(!result.retryable);
    }

    #[tokio::test]
    async fn tooling_error_is_not_repairable_by_the_model() {
        let result = run_status(ToolingStatus::Error, 3).await;
        assert!(!result.success);
        assert!(!result.retryable);
    }

    #[tokio::test]
    async fn status_exit_code_mismatch_is_a_non_retryable_integration_failure() {
        let result = run_status(ToolingStatus::Passed, 1).await;
        assert!(!result.success);
        assert!(!result.retryable);
        assert!(result.output.contains("status/exit-code mismatch"));
    }

    #[tokio::test]
    async fn malformed_output_is_a_non_retryable_integration_failure() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_binary(dir.path(), "printf '%s\\n' 'not-json'\nexit 3");
        let result = run_final_gate_with_binary(dir.path(), binary.as_os_str())
            .await
            .unwrap();
        assert!(!result.success);
        assert!(!result.retryable);
        assert!(result.output.contains("invalid JSON envelope"));
    }

    #[tokio::test]
    async fn missing_binary_is_a_non_retryable_tooling_failure() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_final_gate_with_binary(
            dir.path(),
            dir.path().join("missing-coding-tooling").as_os_str(),
        )
        .await
        .unwrap();
        assert!(!result.success);
        assert!(!result.retryable);
        assert!(result.output.contains("failed to start coding-tooling"));
    }

    async fn run_status(status: ToolingStatus, exit_code: i32) -> ValidationResult {
        let dir = tempfile::tempdir().unwrap();
        let status_name = match status {
            ToolingStatus::Passed => "passed",
            ToolingStatus::Failed => "failed",
            ToolingStatus::Unavailable => "unavailable",
            ToolingStatus::Error => "error",
        };
        let binary = fake_binary(
            dir.path(),
            &format!(
                "printf '%s\\n' '{{\"schemaVersion\":1,\"operation\":\"run\",\"status\":\"{status_name}\",\"durationMs\":1,\"data\":{{}},\"diagnostics\":[]}}'\nexit {exit_code}"
            ),
        );
        run_final_gate_with_binary(dir.path(), binary.as_os_str())
            .await
            .unwrap()
    }

    fn fake_binary(root: &Path, body: &str) -> std::path::PathBuf {
        let path = root.join("coding-tooling");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }
}
