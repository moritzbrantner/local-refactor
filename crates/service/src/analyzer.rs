use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::{
    io::AsyncWriteExt,
    process::{ChildStdin, Command},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzerRequest {
    pub files: Vec<String>,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzerResponse {
    pub edits: Vec<AnalyzerEdit>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzerEdit {
    pub file_path: String,
    pub original_content: String,
    pub new_content: String,
    pub rule_id: String,
    pub summary: String,
}

pub async fn run(script: &Path, request: AnalyzerRequest) -> Result<AnalyzerResponse> {
    let mut child = Command::new("bun")
        .arg(script)
        .arg("plan")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "failed to start TypeScript analyzer worker with `bun`")?;

    write_json(child.stdin.take(), &request).await?;
    let output = child.wait_with_output().await?;
    if !output.status.success() {
        return Err(anyhow!(
            "analyzer failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "invalid analyzer response: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

async fn write_json(stdin: Option<ChildStdin>, request: &AnalyzerRequest) -> Result<()> {
    let mut stdin = stdin.ok_or_else(|| anyhow!("analyzer stdin unavailable"))?;
    let payload = serde_json::to_vec(request)?;
    stdin.write_all(&payload).await?;
    stdin.shutdown().await?;
    Ok(())
}
