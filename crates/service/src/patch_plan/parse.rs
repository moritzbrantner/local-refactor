use super::{
    doc_only::validate_documentation_only_update,
    validate::{
        reject_weakened_test_markers, validate_allowed_writes, validate_coverage_plan_shape,
        validate_plan_shape, validate_relative_path,
    },
    CoveragePatchPlanV1, PatchPlanAction, PatchPlanEdit, PatchPlanModelRequest, PatchPlanV1,
};
use crate::db::PatchAction;
use anyhow::{anyhow, Context, Result};
use local_refactor_core::{
    coverage::BehaviorClaim,
    path_policy::{is_test_file, PathDecision, PathPolicy},
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

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

    let value: Value =
        serde_json::from_str(response).with_context(|| "patch plan response was not valid JSON")?;
    let plan: PatchPlanV1 =
        serde_json::from_value(value).map_err(|error| patch_plan_shape_error(response, error))?;
    validate_plan_shape(request, &plan)?;
    validate_allowed_writes(request.allowed_writes, &plan)?;

    let sources = source_contents(request);

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
                ensure_unchanged_on_disk(&absolute, original)?;
                validate_documentation_only_update(request, &file.path, original, &content)?;
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

pub(crate) fn parse_coverage_patch_plan_response(
    request: &PatchPlanModelRequest,
    policy: &PathPolicy,
    response: &str,
) -> Result<(Vec<PatchPlanEdit>, Vec<BehaviorClaim>)> {
    if response.contains("```") {
        return Err(anyhow!(
            "coverage patch plan response must be raw JSON without Markdown fences"
        ));
    }

    let value: Value = serde_json::from_str(response)
        .with_context(|| "coverage patch plan response was not valid JSON")?;
    let plan: CoveragePatchPlanV1 =
        serde_json::from_value(value).map_err(|error| patch_plan_shape_error(response, error))?;
    validate_coverage_plan_shape(request, &plan)?;

    let evidence_ids = request
        .coverage_evidence
        .iter()
        .map(|evidence| evidence.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for claim in &plan.behavior_claims {
        if !evidence_ids.contains(claim.evidence_id.as_str()) {
            return Err(anyhow!(
                "behavior claim {} referenced unknown evidence {}",
                claim.id,
                claim.evidence_id
            ));
        }
    }

    let sources = source_contents(request);

    let mut edits = Vec::new();
    for file in plan.files {
        let relative = validate_relative_path(&file.path)?;
        if !is_test_file(Path::new(&relative)) {
            return Err(anyhow!(
                "{}: coverage solidification may only edit test files",
                file.path
            ));
        }
        let absolute = policy.target_root().join(&relative);
        if policy.decision_for(&absolute) != PathDecision::Mutable {
            return Err(anyhow!(
                "coverage patch plan attempted to edit non-mutable path {}",
                file.path
            ));
        }

        match file.action {
            PatchPlanAction::Update => {
                let content = file
                    .content
                    .ok_or_else(|| anyhow!("{}: update requires content", file.path))?;
                reject_weakened_test_markers(&file.path, &content)?;
                let original = sources.get(file.path.as_str()).ok_or_else(|| {
                    anyhow!(
                        "{}: update target was not in coverage planning input",
                        file.path
                    )
                })?;
                ensure_unchanged_on_disk(&absolute, original)?;
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
                reject_weakened_test_markers(&file.path, &content)?;
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
                    "{}: delete is not supported in coverage patch plans",
                    file.path
                ));
            }
        }
    }

    Ok((edits, plan.behavior_claims))
}

fn source_contents(request: &PatchPlanModelRequest) -> BTreeMap<&str, &str> {
    request
        .files
        .iter()
        .map(|file| (file.relative_path.as_str(), file.content.as_str()))
        .collect()
}

fn ensure_unchanged_on_disk(absolute: &Path, original: &str) -> Result<()> {
    let current = std::fs::read_to_string(absolute)
        .with_context(|| format!("failed to read {}", absolute.display()))?;
    if current != original {
        return Err(anyhow!(
            "external modification conflict while editing {}",
            absolute.display()
        ));
    }
    Ok(())
}

/// Re-parses `response` so the success path can consume its `Value` without cloning it.
fn patch_plan_shape_error(response: &str, error: serde_json::Error) -> anyhow::Error {
    let value = serde_json::from_str::<Value>(response).ok();
    if let Some(object) = value.as_ref().and_then(Value::as_object) {
        let keys = object.keys().cloned().collect::<Vec<_>>().join(", ");
        let keys = if keys.is_empty() {
            "<none>".to_string()
        } else {
            keys
        };
        if !object.contains_key("summary") {
            return anyhow!(
                "patch plan response did not match patch-plan-v1; expected top-level key `summary`, got keys: {}",
                keys
            );
        }
        return anyhow!(
            "patch plan response did not match patch-plan-v1; got keys: {}; {}",
            keys,
            error
        );
    }

    anyhow!("patch plan response did not match patch-plan-v1: {error}")
}
