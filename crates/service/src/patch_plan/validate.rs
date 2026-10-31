use super::{CoveragePatchPlanV1, PatchPlanAction, PatchPlanModelRequest, PatchPlanV1};
use anyhow::{anyhow, Result};
use local_refactor_core::rules::AllowedWrites;
use std::path::{Component, Path, PathBuf};

pub(super) fn validate_coverage_plan_shape(
    request: &PatchPlanModelRequest,
    plan: &CoveragePatchPlanV1,
) -> Result<()> {
    if plan.summary.trim().is_empty() {
        return Err(anyhow!("coverage patch plan summary must be non-empty"));
    }
    if !request.validation_commands.is_empty() && plan.validation_command.trim().is_empty() {
        return Err(anyhow!(
            "coverage patch plan validationCommand must be non-empty when explicit validation commands are configured"
        ));
    }
    if !plan.files.is_empty() && plan.behavior_claims.is_empty() {
        return Err(anyhow!(
            "coverage patch plan must include behaviorClaims when files change"
        ));
    }
    Ok(())
}

pub(super) fn reject_weakened_test_markers(path: &str, content: &str) -> Result<()> {
    for marker in [
        ".skip(",
        ".only(",
        "test.skip",
        "test.only",
        "describe.skip",
        "describe.only",
    ] {
        if content.contains(marker) {
            return Err(anyhow!(
                "{path}: coverage solidification may not add disabled or exclusive test marker {marker}"
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_plan_shape(
    request: &PatchPlanModelRequest,
    plan: &PatchPlanV1,
) -> Result<()> {
    if plan.summary.trim().is_empty() {
        return Err(anyhow!("patch plan summary must be non-empty"));
    }
    if !request.validation_commands.is_empty() && plan.validation_command.trim().is_empty() {
        return Err(anyhow!(
            "patch plan validationCommand must be non-empty when explicit validation commands are configured"
        ));
    }
    let _ = &plan.preserved_exports;
    Ok(())
}

pub(super) fn validate_allowed_writes(allowed: AllowedWrites, plan: &PatchPlanV1) -> Result<()> {
    match allowed {
        AllowedWrites::SingleFile => {
            if plan.files.len() > 1 {
                return Err(anyhow!("single-file rules may update at most one file"));
            }
            if plan
                .files
                .first()
                .is_some_and(|file| file.action != PatchPlanAction::Update)
            {
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

pub(super) fn validate_relative_path(path: &str) -> Result<PathBuf> {
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
