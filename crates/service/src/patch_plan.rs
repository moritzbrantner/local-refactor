mod doc_only;
mod parse;
mod prompt;
#[cfg(test)]
mod tests;
mod validate;

use crate::db::PatchAction;
use local_refactor_core::{
    config::TestFileMode,
    coverage::{BehaviorClaim, CoverageEvidenceItem},
    rules::{AllowedWrites, Language, RulePlanningContext, StackContext},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) use parse::{parse_coverage_patch_plan_response, parse_patch_plan_response};
pub use prompt::build_prompt;

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
    pub planning_context: RulePlanningContext,
    pub test_file_mode: TestFileMode,
    pub stack_contexts: Vec<StackContext>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage_evidence: Vec<CoverageEvidenceItem>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CoveragePatchPlanV1 {
    summary: String,
    files: Vec<PatchPlanFile>,
    behavior_claims: Vec<BehaviorClaim>,
    validation_command: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PatchPlanAction {
    Create,
    Update,
    Delete,
}
