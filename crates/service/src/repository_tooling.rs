use crate::validation::ValidationResult;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};
use tokio::process::Command;

const DEFAULT_BINARY: &str = "coding-tooling";
const FINAL_GATE: &str = "gate:final";

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum ToolingStatus {
    Passed,
    Failed,
    Unavailable,
    Error,
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

async fn run_final_gate_with_binary(
    root: &Path,
    binary: &OsStr,
) -> Result<ValidationResult> {
    let output = Command::new(binary)
        .args([
            "check",
            FINAL_GATE,
            "--root",
            &root.to_string_lossy(),
            "--json",
        ])
        .current_dir(root)
        .output()
        .await
        .with_context(|| {
            format!(
                "failed to start coding-tooling; install it or set LOCAL_REFACTOR_CODING_TOOLING_BIN (attempted {})",
                Path::new(binary).display()
            )
        })?;

    let stdout = String::from_utf8(output.stdout)
        .context("coding-tooling stdout was not valid UTF-8")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stdout.trim().is_empty() {
        return Err(anyhow!(
            "coding-tooling returned no JSON output{}",
            formatted_stderr(&stderr)
        ));
    }

    let envelope: ToolingEnvelope = serde_json::from_str(&stdout)
        .context("coding-tooling returned an invalid JSON envelope")?;
    if envelope.schema_version != 1 {
        return Err(anyhow!(
            "unsupported coding-tooling schema version: {}",
            envelope.schema_version
        ));
    }
    if envelope.operation != "check" {
        return Err(anyhow!(
            "coding-tooling returned operation {}, expected check",
            envelope.operation
        ));
    }

    let success = envelope.status == ToolingStatus::Passed;
    if success != output.status.success() {
        return Err(anyhow!(
            "coding-tooling status/exit-code mismatch: status {:?}, exit code {:?}",
            envelope.status,
            output.status.code()
        ));
    }

    Ok(ValidationResult {
        success,
        retryable: envelope.status == ToolingStatus::Failed,
        output: format!(
            "$ coding-tooling check {FINAL_GATE} --root {} --json\n{}{}",
            root.display(),
            stdout,
            formatted_stderr(&stderr)
        ),
    })
}

fn formatted_stderr(stderr: &str) -> String {
    if stderr.trim().is_empty() {
        String::new()
    } else {
        format!("\nstderr:\n{stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[tokio::test]
    async fn failed_gate_is_a_retryable_validation_failure() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_binary(
            dir.path(),
            r#"{"schemaVersion":1,"operation":"check","status":"failed","durationMs":1,"data":{},"diagnostics":[]}"#,
            1,
        );

        let result = run_final_gate_with_binary(dir.path(), binary.as_os_str())
            .await
            .unwrap();

        assert!(!result.success);
        assert!(result.retryable);
        assert!(result.output.contains("\"status\":\"failed\""));
    }

    #[tokio::test]
    async fn unavailable_gate_is_not_repairable_by_the_model() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_binary(
            dir.path(),
            r#"{"schemaVersion":1,"operation":"check","status":"unavailable","durationMs":1,"data":{},"diagnostics":[]}"#,
            2,
        );

        let result = run_final_gate_with_binary(dir.path(), binary.as_os_str())
            .await
            .unwrap();

        assert!(!result.success);
        assert!(!result.retryable);
    }

    fn fake_binary(root: &Path, json: &str, exit_code: i32) -> std::path::PathBuf {
        let path = root.join("coding-tooling");
        fs::write(
            &path,
            format!("#!/bin/sh\nprintf '%s\\n' '{json}'\nexit {exit_code}\n"),
        )
        .unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }
}
