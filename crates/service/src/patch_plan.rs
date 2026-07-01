use crate::db::PatchAction;
use anyhow::{anyhow, Context, Result};
use local_refactor_core::{
    path_policy::{PathDecision, PathPolicy},
    rules::{AllowedWrites, Language},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPlanModelRequest {
    pub rule_id: String,
    pub language: Language,
    pub rule_name: String,
    pub rule_description: String,
    pub target_root: PathBuf,
    pub files: Vec<PatchPlanSourceFile>,
    pub validation_commands: Vec<String>,
    pub allowed_writes: AllowedWrites,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair_context: Option<PatchPlanRepairContext>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPlanSourceFile {
    pub relative_path: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPlanRepairContext {
    pub attempt: u32,
    pub validation_output: String,
}

#[derive(Debug, Clone)]
pub struct PatchPlanEdit {
    pub file_path: String,
    pub original_content: String,
    pub new_content: String,
    pub rule_id: String,
    pub summary: String,
    pub action: PatchAction,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchPlanV1 {
    summary: String,
    files: Vec<PatchPlanFile>,
    preserved_exports: Vec<String>,
    validation_command: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchPlanFile {
    path: String,
    action: PatchPlanAction,
    content: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PatchPlanAction {
    Create,
    Update,
    Delete,
}

pub fn build_prompt(request: &PatchPlanModelRequest) -> String {
    let code_fence = request.language.code_fence();
    let files = request
        .files
        .iter()
        .map(|file| {
            format!(
                "File: {}\n```{}\n{}\n```",
                file.relative_path, code_fence, file.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let mut parts = vec![
        "You are a local refactoring assistant.".to_string(),
        "Return only valid JSON. Do not use Markdown fences outside JSON strings.".to_string(),
        "The response must match patch-plan-v1 exactly:".to_string(),
        format!(
            r#"{{ "summary": "short summary", "files": [{{ "path": "{}", "action": "update", "content": "{}" }}], "preservedExports": ["ExportName"], "validationCommand": "{}" }}"#,
            request.language.example_path(),
            request.language.example_content(),
            request.validation_commands.join(" && ")
        ),
        format!("Language: {}", request.language.display_name()),
        format!("Rule id: {}", request.rule_id),
        format!("Rule name: {}", request.rule_name),
        format!("Rule description: {}", request.rule_description),
        format!("Allowed writes: {:?}", request.allowed_writes),
        format!(
            "Validation commands: {}",
            request.validation_commands.join(" && ")
        ),
        "Use action update for existing files and action create for new files. Do not use action delete.".to_string(),
        "Preserve runtime behavior, exports, and typecheck unless the rule explicitly requires internal code movement.".to_string(),
        "Use the provided validation commands to reason about whether the refactor is safe.".to_string(),
        "Current mutable source files:".to_string(),
        files,
    ];
    if let Some(repair) = &request.repair_context {
        parts.push("Repair context:".to_string());
        parts.push(format!(
            "Repair attempt: {}\nThe previous patch plan failed validation. Keep the current patched state unless a change is needed to satisfy validation. Validation output:\n{}",
            repair.attempt, repair.validation_output
        ));
    }
    parts.join("\n\n")
}

pub(crate) fn parse_patch_plan_response(
    request: &PatchPlanModelRequest,
    policy: &PathPolicy,
    response: &str,
) -> Result<Vec<PatchPlanEdit>> {
    if response.contains("```") {
        return Err(anyhow!(
            "patch plan response must be raw JSON without Markdown fences"
        ));
    }

    let plan: PatchPlanV1 =
        serde_json::from_str(response).with_context(|| "patch plan response was not valid JSON")?;
    validate_plan_shape(&plan)?;
    validate_allowed_writes(request.allowed_writes, &plan)?;

    let sources = request
        .files
        .iter()
        .map(|file| (file.relative_path.as_str(), file.content.as_str()))
        .collect::<BTreeMap<_, _>>();

    let mut edits = Vec::new();
    for file in plan.files {
        let relative = validate_relative_path(&file.path)?;
        let absolute = policy.target_root().join(&relative);
        if policy.decision_for(&absolute) != PathDecision::Mutable {
            return Err(anyhow!(
                "patch plan attempted to edit non-mutable path {}",
                file.path
            ));
        }

        match file.action {
            PatchPlanAction::Update => {
                let content = file
                    .content
                    .ok_or_else(|| anyhow!("{}: update requires content", file.path))?;
                let original = sources.get(file.path.as_str()).ok_or_else(|| {
                    anyhow!("{}: update target was not in planning input", file.path)
                })?;
                let current = std::fs::read_to_string(&absolute)
                    .with_context(|| format!("failed to read {}", absolute.display()))?;
                if current != *original {
                    return Err(anyhow!(
                        "external modification conflict while editing {}",
                        absolute.display()
                    ));
                }
                edits.push(PatchPlanEdit {
                    file_path: absolute.to_string_lossy().to_string(),
                    original_content: (*original).to_string(),
                    new_content: content,
                    rule_id: request.rule_id.clone(),
                    summary: plan.summary.clone(),
                    action: PatchAction::Update,
                });
            }
            PatchPlanAction::Create => {
                let content = file
                    .content
                    .ok_or_else(|| anyhow!("{}: create requires content", file.path))?;
                if absolute.exists() {
                    return Err(anyhow!("{}: create target already exists", file.path));
                }
                edits.push(PatchPlanEdit {
                    file_path: absolute.to_string_lossy().to_string(),
                    original_content: String::new(),
                    new_content: content,
                    rule_id: request.rule_id.clone(),
                    summary: plan.summary.clone(),
                    action: PatchAction::Create,
                });
            }
            PatchPlanAction::Delete => {
                return Err(anyhow!(
                    "{}: delete is not supported in patch-plan-v1 runs",
                    file.path
                ));
            }
        }
    }

    Ok(edits)
}

fn validate_plan_shape(plan: &PatchPlanV1) -> Result<()> {
    if plan.summary.trim().is_empty() {
        return Err(anyhow!("patch plan summary must be non-empty"));
    }
    if plan.files.is_empty() {
        return Err(anyhow!("patch plan files must be non-empty"));
    }
    if plan.validation_command.trim().is_empty() {
        return Err(anyhow!("patch plan validationCommand must be non-empty"));
    }
    let _ = &plan.preserved_exports;
    Ok(())
}

fn validate_allowed_writes(allowed: AllowedWrites, plan: &PatchPlanV1) -> Result<()> {
    match allowed {
        AllowedWrites::SingleFile => {
            if plan.files.len() != 1 {
                return Err(anyhow!("single-file rules must update exactly one file"));
            }
            if plan.files[0].action != PatchPlanAction::Update {
                return Err(anyhow!(
                    "single-file rules may only update an existing file"
                ));
            }
        }
        AllowedWrites::MultiFileWithinTarget => {
            if plan
                .files
                .iter()
                .any(|file| file.action == PatchPlanAction::Delete)
            {
                return Err(anyhow!(
                    "multi-file rules may not delete files in this release"
                ));
            }
        }
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<PathBuf> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(anyhow!("{path}: paths must be relative"));
    }
    for component in candidate.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!("{path}: paths must stay inside target"));
            }
        }
    }
    Ok(candidate.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use local_refactor_core::config::TestFileMode;
    use tempfile::tempdir;

    fn request(
        root: PathBuf,
        file_content: &str,
        allowed_writes: AllowedWrites,
    ) -> PatchPlanModelRequest {
        PatchPlanModelRequest {
            rule_id: "extract-duplicate-block".to_string(),
            language: Language::TypeScript,
            rule_name: "Extract Duplicate Block".to_string(),
            rule_description: "Extract duplicate local logic.".to_string(),
            target_root: root,
            files: vec![PatchPlanSourceFile {
                relative_path: "src/sample.ts".to_string(),
                content: file_content.to_string(),
            }],
            validation_commands: vec!["true".to_string()],
            allowed_writes,
            repair_context: None,
        }
    }

    #[test]
    fn rejects_absolute_paths() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"x","files":[{"path":"/tmp/x.ts","action":"update","content":"x"}],"preservedExports":[],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap_err();

        assert!(error.to_string().contains("paths must be relative"));
    }

    #[test]
    fn rejects_parent_paths() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"x","files":[{"path":"../x.ts","action":"update","content":"x"}],"preservedExports":[],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap_err();

        assert!(error.to_string().contains("stay inside target"));
    }

    #[test]
    fn rejects_single_file_creates() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"x","files":[{"path":"src/new.ts","action":"create","content":"x"}],"preservedExports":[],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("single-file rules may only update"));
    }

    #[test]
    fn rejects_protected_paths() {
        let dir = tempdir().unwrap();
        let protected = dir.path().join("src/generated/client.ts");
        std::fs::create_dir_all(protected.parent().unwrap()).unwrap();
        std::fs::write(&protected, "old").unwrap();
        let policy = PathPolicy::new(
            dir.path(),
            &["src/generated/**".to_string()],
            TestFileMode::Mutable,
        )
        .unwrap();
        let mut request = request(dir.path().to_path_buf(), "old", AllowedWrites::SingleFile);
        request.files = vec![PatchPlanSourceFile {
            relative_path: "src/generated/client.ts".to_string(),
            content: "old".to_string(),
        }];
        let response = r#"{"summary":"x","files":[{"path":"src/generated/client.ts","action":"update","content":"new"}],"preservedExports":[],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(&request, &policy, response).unwrap_err();

        assert!(error.to_string().contains("non-mutable path"));
    }

    #[test]
    fn rejects_read_only_test_files() {
        let dir = tempdir().unwrap();
        let test_file = dir.path().join("src/sample.test.ts");
        std::fs::create_dir_all(test_file.parent().unwrap()).unwrap();
        std::fs::write(&test_file, "old").unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::ReadOnly).unwrap();
        let mut request = request(dir.path().to_path_buf(), "old", AllowedWrites::SingleFile);
        request.files = vec![PatchPlanSourceFile {
            relative_path: "src/sample.test.ts".to_string(),
            content: "old".to_string(),
        }];
        let response = r#"{"summary":"x","files":[{"path":"src/sample.test.ts","action":"update","content":"new"}],"preservedExports":[],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(&request, &policy, response).unwrap_err();

        assert!(error.to_string().contains("non-mutable path"));
    }

    #[test]
    fn rejects_existing_create_targets() {
        let dir = tempdir().unwrap();
        let new_file = dir.path().join("src/new.ts");
        std::fs::create_dir_all(new_file.parent().unwrap()).unwrap();
        std::fs::write(&new_file, "existing").unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"x","files":[{"path":"src/new.ts","action":"create","content":"new"}],"preservedExports":[],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(
            &request(
                dir.path().to_path_buf(),
                "",
                AllowedWrites::MultiFileWithinTarget,
            ),
            &policy,
            response,
        )
        .unwrap_err();

        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn rejects_update_conflicts() {
        let dir = tempdir().unwrap();
        let sample = dir.path().join("src/sample.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        std::fs::write(&sample, "changed").unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"x","files":[{"path":"src/sample.ts","action":"update","content":"new"}],"preservedExports":[],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "old", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap_err();

        assert!(error.to_string().contains("external modification conflict"));
    }
}
