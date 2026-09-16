use crate::db::PatchAction;
use anyhow::{anyhow, Context, Result};
use local_refactor_core::{
    config::TestFileMode,
    coverage::{BehaviorClaim, CoverageEvidenceItem},
    path_policy::{is_test_file, PathDecision, PathPolicy},
    rules::{AllowedWrites, Language, RulePlanningContext, StackContext},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

pub fn build_prompt(request: &PatchPlanModelRequest) -> String {
    if !request.coverage_evidence.is_empty() {
        return build_coverage_prompt(request);
    }

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
        "Use exactly these top-level keys: summary, files, preservedExports, validationCommand.".to_string(),
        "If no safe change is available, return a patch-plan-v1 object with files: [] and a short summary explaining why.".to_string(),
        r#"Do not return wrapper objects like { "response": "..." }."#.to_string(),
        format!("Language: {}", request.language.display_name()),
        format!("Rule id: {}", request.rule_id),
        format!("Rule name: {}", request.rule_name),
        format!("Rule description: {}", request.rule_description),
        format!("Allowed writes: {:?}", request.allowed_writes),
        format!("Test file mode: {:?}", request.test_file_mode),
        format!(
            "Detected stack contexts: {}",
            request
                .stack_contexts
                .iter()
                .map(|context| context.display_name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        format!(
            "Validation commands: {}",
            if request.validation_commands.is_empty() {
                "none".to_string()
            } else {
                request.validation_commands.join(" && ")
            }
        ),
        format_policy_section(
            "Workflow rules",
            &request.planning_context.workflow_rules,
        ),
        format_policy_section(
            "Preservation rules",
            &request.planning_context.preservation_rules,
        ),
        format_policy_section(
            "Structure rules",
            &request.planning_context.structure_rules,
        ),
        format_policy_section(
            "Test refactoring rules",
            &request.planning_context.test_refactoring_rules,
        ),
        format_policy_section(
            "Forbidden actions",
            &request.planning_context.forbidden_actions,
        ),
        format_policy_section("Stack rules", &request.planning_context.stack_rules),
        "Use action update for existing files and action create for new files. Do not use action delete.".to_string(),
        "Preserve runtime behavior, exports, and typecheck unless the rule explicitly requires internal code movement.".to_string(),
        "Use the provided validation commands to reason about whether the refactor is safe.".to_string(),
        "Current mutable source files:".to_string(),
        files,
    ];
    if is_documentation_only_rule(&request.rule_id) {
        parts.push(
            "Documentation-only rules may add documentation comments, but must not rewrite code, reorder declarations, or add compiler directive comments."
                .to_string(),
        );
        if request.language == Language::TypeScript {
            parts.push(
                "Do not add TypeScript directive comments such as @ts-nocheck, @ts-check, @ts-ignore, or @ts-expect-error."
                    .to_string(),
            );
        }
    }
    if request.validation_commands.is_empty() {
        parts.push(
            "No explicit validation commands were provided. Set validationCommand to an empty string; final validation is supplied automatically by coding-tooling. Keep the plan limited to changes that can be reasoned about from static source inspection and the rule policy."
                .to_string(),
        );
    }
    if let Some(repair) = &request.repair_context {
        parts.push("Repair context:".to_string());
        parts.push(format!(
            "Repair attempt: {}\nThe previous patch plan failed validation. Keep the current patched state unless a change is needed to satisfy validation. Validation output:\n{}",
            repair.attempt, repair.validation_output
        ));
    }
    parts.join("\n\n")
}

fn build_coverage_prompt(request: &PatchPlanModelRequest) -> String {
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

    let validation_command = request.validation_commands.join(" && ");
    let mut parts = vec![
        "You are a local coverage solidification assistant.".to_string(),
        "Return only valid JSON. Do not use Markdown fences outside JSON strings.".to_string(),
        "The response must match coverage-patch-plan-v1 exactly:".to_string(),
        format!(
            r#"{{"summary":"short summary","files":[{{"path":"src/example.test.ts","action":"create","content":"complete test file"}}],"behaviorClaims":[{{"id":"claim-id","evidenceId":"evidence-id","sourcePaths":["src/example.ts"],"publicEntrypoint":"example","behavior":"observable behavior","owningTestLayer":"TypeScript unit test","testPath":"src/example.test.ts","assertionSummary":"asserts observable behavior","existingCoverageReason":"no nearby coverage"}}],"validationCommand":"{}"}}"#,
            validation_command
        ),
        "Use exactly these top-level keys: summary, files, behaviorClaims, validationCommand.".to_string(),
        "If no safe coverage improvement is available, return files: [] and behaviorClaims: [].".to_string(),
        format!("Language: {}", request.language.display_name()),
        format!("Rule id: {}", request.rule_id),
        format!("Rule name: {}", request.rule_name),
        format!("Rule description: {}", request.rule_description),
        format!(
            "Validation commands: {}",
            if request.validation_commands.is_empty() {
                "none".to_string()
            } else {
                validation_command
            }
        ),
        format_policy_section("Workflow rules", &request.planning_context.workflow_rules),
        format_policy_section(
            "Preservation rules",
            &request.planning_context.preservation_rules,
        ),
        format_policy_section("Structure rules", &request.planning_context.structure_rules),
        format_policy_section(
            "Test refactoring rules",
            &request.planning_context.test_refactoring_rules,
        ),
        format_policy_section("Forbidden actions", &request.planning_context.forbidden_actions),
        "Coverage evidence:".to_string(),
        serde_json::to_string_pretty(&request.coverage_evidence)
            .unwrap_or_else(|_| "[]".to_string()),
        "Production files are read-only context. Create or update test files only.".to_string(),
        "Current source and test context files:".to_string(),
        files,
    ];
    if request.validation_commands.is_empty() {
        parts.push(
            "No explicit validation commands were provided. Set validationCommand to an empty string; final validation is supplied automatically by coding-tooling."
                .to_string(),
        );
    }
    parts.join("\n\n")
}

fn format_policy_section(title: &str, rules: &[String]) -> String {
    if rules.is_empty() {
        return format!("{title}: none");
    }

    format!(
        "{title}:\n{}",
        rules
            .iter()
            .map(|rule| format!("- {rule}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
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

    let value: Value =
        serde_json::from_str(response).with_context(|| "patch plan response was not valid JSON")?;
    let plan: PatchPlanV1 = serde_json::from_value(value.clone())
        .map_err(|error| patch_plan_shape_error(&value, error))?;
    validate_plan_shape(request, &plan)?;
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
    let plan: CoveragePatchPlanV1 = serde_json::from_value(value.clone())
        .map_err(|error| patch_plan_shape_error(&value, error))?;
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

    let sources = request
        .files
        .iter()
        .map(|file| (file.relative_path.as_str(), file.content.as_str()))
        .collect::<BTreeMap<_, _>>();

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

fn validate_coverage_plan_shape(
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

fn reject_weakened_test_markers(path: &str, content: &str) -> Result<()> {
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

fn validate_plan_shape(request: &PatchPlanModelRequest, plan: &PatchPlanV1) -> Result<()> {
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

fn validate_allowed_writes(allowed: AllowedWrites, plan: &PatchPlanV1) -> Result<()> {
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

fn validate_documentation_only_update(
    request: &PatchPlanModelRequest,
    path: &str,
    original: &str,
    updated: &str,
) -> Result<()> {
    if !is_documentation_only_rule(&request.rule_id) {
        return Ok(());
    }

    if request.language == Language::TypeScript {
        for directive in ["@ts-nocheck", "@ts-check", "@ts-ignore", "@ts-expect-error"] {
            if updated.contains(directive) && !original.contains(directive) {
                return Err(anyhow!(
                    "{}: documentation-only patch added forbidden TypeScript directive comment {}",
                    path,
                    directive
                ));
            }
        }
    }

    let original_code = code_without_comments(request.language, original);
    let updated_code = code_without_comments(request.language, updated);
    if normalize_code_text(&original_code) != normalize_code_text(&updated_code) {
        return Err(anyhow!(
            "{}: documentation-only patch changed non-comment code",
            path
        ));
    }

    Ok(())
}

fn is_documentation_only_rule(rule_id: &str) -> bool {
    matches!(
        rule_id,
        "add-documentation-comments" | "rust-add-documentation-comments"
    )
}

fn normalize_code_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn code_without_comments(language: Language, value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for comment_character in chars.by_ref() {
                    if comment_character == '\n' {
                        output.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for comment_character in chars.by_ref() {
                    if previous == '*' && comment_character == '/' {
                        break;
                    }
                    if comment_character == '\n' {
                        output.push('\n');
                    }
                    previous = comment_character;
                }
            }
            '"' => {
                output.push(character);
                copy_quoted(&mut chars, &mut output, '"');
            }
            '\'' if language == Language::TypeScript => {
                output.push(character);
                copy_quoted(&mut chars, &mut output, '\'');
            }
            '\'' if language == Language::Rust => {
                output.push(character);
                copy_quoted(&mut chars, &mut output, '\'');
            }
            '`' if language == Language::TypeScript => {
                output.push(character);
                copy_quoted(&mut chars, &mut output, '`');
            }
            _ => output.push(character),
        }
    }
    output
}

fn copy_quoted(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    output: &mut String,
    quote: char,
) {
    let mut escaped = false;
    for character in chars.by_ref() {
        output.push(character);
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote {
            break;
        }
    }
}

fn patch_plan_shape_error(value: &Value, error: serde_json::Error) -> anyhow::Error {
    if let Some(object) = value.as_object() {
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
            planning_context: local_refactor_core::rules::planning_context_for(
                local_refactor_core::rules::rule_by_id("extract-duplicate-block").unwrap(),
                TestFileMode::Mutable,
                &[StackContext::TypeScriptBackend],
            ),
            test_file_mode: TestFileMode::Mutable,
            stack_contexts: vec![StackContext::TypeScriptBackend],
            coverage_evidence: Vec::new(),
            repair_context: None,
        }
    }

    fn coverage_request(root: PathBuf) -> PatchPlanModelRequest {
        PatchPlanModelRequest {
            rule_id: "characterize-public-entrypoint".to_string(),
            language: Language::TypeScript,
            rule_name: "Characterize Public Entrypoint".to_string(),
            rule_description: "Adds tests for observable behavior through public APIs.".to_string(),
            target_root: root,
            files: vec![PatchPlanSourceFile {
                relative_path: "src/calculator.ts".to_string(),
                content: "export function calculateTotal() { return 0; }\n".to_string(),
            }],
            validation_commands: vec!["bun test".to_string()],
            allowed_writes: AllowedWrites::MultiFileWithinTarget,
            planning_context: RulePlanningContext {
                workflow_rules: Vec::new(),
                preservation_rules: Vec::new(),
                structure_rules: Vec::new(),
                test_refactoring_rules: Vec::new(),
                forbidden_actions: Vec::new(),
                stack_rules: Vec::new(),
            },
            test_file_mode: TestFileMode::Mutable,
            stack_contexts: vec![StackContext::TypeScriptBackend],
            coverage_evidence: vec![CoverageEvidenceItem {
                id: "public-entrypoint-without-nearby-test:src/calculator.ts:calculateTotal"
                    .to_string(),
                rule_id: "public-entrypoint-without-nearby-test".to_string(),
                language: Language::TypeScript,
                source_path: "src/calculator.ts".to_string(),
                public_entrypoint: "calculateTotal".to_string(),
                owning_test_layer: "TypeScript unit test".to_string(),
                nearby_test_paths: Vec::new(),
                gap_reason: "No nearby test appears to cover calculateTotal.".to_string(),
                suggested_solidification_rules: vec!["characterize-public-entrypoint".to_string()],
            }],
            repair_context: None,
        }
    }

    #[test]
    fn prompt_includes_policy_context_and_patch_plan_contract() {
        let dir = tempdir().unwrap();
        let request = request(
            dir.path().to_path_buf(),
            "export function value() { return true; }",
            AllowedWrites::MultiFileWithinTarget,
        );

        let prompt = build_prompt(&request);

        assert!(prompt.contains("Return only valid JSON"));
        assert!(prompt.contains("patch-plan-v1"));
        assert!(prompt.contains("Preservation rules:"));
        assert!(prompt.contains("Test refactoring rules:"));
        assert!(!prompt.contains("Testing rules:"));
        assert!(prompt.contains("Preserve behavior"));
        assert!(prompt.contains("TypeScript backend"));
        assert!(prompt.contains("Allowed writes: MultiFileWithinTarget"));
        assert!(prompt.contains("Test file mode: Mutable"));
        assert!(prompt.contains("Use exactly these top-level keys"));
        assert!(prompt.contains("files: []"));
        assert!(prompt.contains("Do not return wrapper objects"));
    }

    #[test]
    fn automatic_validation_accepts_empty_patch_plan_validation_command() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let mut request = request(
            dir.path().to_path_buf(),
            "",
            AllowedWrites::SingleFile,
        );
        request.validation_commands.clear();
        let response = r#"{"summary":"No safe change","files":[],"preservedExports":[],"validationCommand":""}"#;

        let edits = parse_patch_plan_response(&request, &policy, response).unwrap();

        assert!(edits.is_empty());
        assert!(build_prompt(&request).contains("Set validationCommand to an empty string"));
    }

    #[test]
    fn explicit_validation_still_requires_patch_plan_validation_command() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"No safe change","files":[],"preservedExports":[],"validationCommand":""}"#;

        let error = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("must be non-empty when explicit validation commands are configured"));
    }

    #[test]
    fn automatic_validation_accepts_empty_coverage_validation_command() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let mut request = coverage_request(dir.path().to_path_buf());
        request.validation_commands.clear();
        let response = r#"{"summary":"No safe coverage change","files":[],"behaviorClaims":[],"validationCommand":""}"#;

        let (edits, claims) =
            parse_coverage_patch_plan_response(&request, &policy, response).unwrap();

        assert!(edits.is_empty());
        assert!(claims.is_empty());
        assert!(build_prompt(&request).contains("Set validationCommand to an empty string"));
    }

    #[test]
    fn coverage_patch_plan_rejects_production_file_edits() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("src/calculator.ts");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, "export function calculateTotal() { return 0; }\n").unwrap();
        let request = coverage_request(dir.path().to_path_buf());
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = serde_json::json!({
            "summary": "Change production",
            "files": [{
                "path": "src/calculator.ts",
                "action": "update",
                "content": "export function calculateTotal() { return 1; }\n"
            }],
            "behaviorClaims": [{
                "id": "claim-total",
                "evidenceId": "public-entrypoint-without-nearby-test:src/calculator.ts:calculateTotal",
                "sourcePaths": ["src/calculator.ts"],
                "publicEntrypoint": "calculateTotal",
                "behavior": "returns zero",
                "owningTestLayer": "TypeScript unit test",
                "testPath": "src/calculator.test.ts",
                "assertionSummary": "asserts zero",
                "existingCoverageReason": "no nearby coverage"
            }],
            "validationCommand": "bun test"
        })
        .to_string();

        let error = parse_coverage_patch_plan_response(&request, &policy, &response).unwrap_err();

        assert!(error
            .to_string()
            .contains("coverage solidification may only edit test files"));
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
    fn rejects_single_file_deletes() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"x","files":[{"path":"src/sample.ts","action":"delete"}],"preservedExports":[],"validationCommand":"true"}"#;

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
    fn accepts_noop_patch_plans() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"No safe documentation change found","files":[],"preservedExports":[],"validationCommand":"true"}"#;

        let edits = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap();

        assert!(edits.is_empty());
    }

    #[test]
    fn rejects_single_file_multiple_updates() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"x","files":[{"path":"src/a.ts","action":"update","content":"x"},{"path":"src/b.ts","action":"update","content":"x"}],"preservedExports":[],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("single-file rules may update at most one file"));
    }

    #[test]
    fn reports_wrapper_response_keys() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"response":"I'm sorry, but I can't assist with that request."}"#;

        let error = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("expected top-level key `summary`"));
        assert!(message.contains("got keys: response"));
    }

    #[test]
    fn reports_keys_for_incomplete_patch_plan_objects() {
        let dir = tempdir().unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let response = r#"{"summary":"x","response":"not a plan"}"#;

        let error = parse_patch_plan_response(
            &request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile),
            &policy,
            response,
        )
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("got keys: response, summary"));
        assert!(message.contains("missing field"));
    }

    #[test]
    fn documentation_only_rules_accept_comment_only_updates() {
        let dir = tempdir().unwrap();
        let sample = dir.path().join("src/sample.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        let original = "export function value() {\n  return true;\n}\n";
        std::fs::write(&sample, original).unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let mut request = request(
            dir.path().to_path_buf(),
            original,
            AllowedWrites::SingleFile,
        );
        request.rule_id = "add-documentation-comments".to_string();
        let response = r#"{"summary":"x","files":[{"path":"src/sample.ts","action":"update","content":"/** Returns true. */\nexport function value() {\n  return true;\n}\n"}],"preservedExports":["value"],"validationCommand":"true"}"#;

        let edits = parse_patch_plan_response(&request, &policy, response).unwrap();

        assert_eq!(edits.len(), 1);
    }

    #[test]
    fn documentation_only_rules_reject_typecheck_directives() {
        let dir = tempdir().unwrap();
        let sample = dir.path().join("src/sample.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        let original = "export function value() {\n  return true;\n}\n";
        std::fs::write(&sample, original).unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let mut request = request(
            dir.path().to_path_buf(),
            original,
            AllowedWrites::SingleFile,
        );
        request.rule_id = "add-documentation-comments".to_string();
        let response = r#"{"summary":"x","files":[{"path":"src/sample.ts","action":"update","content":"// @ts-nocheck\n/** Returns true. */\nexport function value() {\n  return true;\n}\n"}],"preservedExports":["value"],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(&request, &policy, response).unwrap_err();

        assert!(error
            .to_string()
            .contains("forbidden TypeScript directive comment @ts-nocheck"));
    }

    #[test]
    fn documentation_only_rules_reject_non_comment_code_changes() {
        let dir = tempdir().unwrap();
        let sample = dir.path().join("src/sample.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        let original = "export function value() {\n  return true;\n}\n";
        std::fs::write(&sample, original).unwrap();
        let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
        let mut request = request(
            dir.path().to_path_buf(),
            original,
            AllowedWrites::SingleFile,
        );
        request.rule_id = "add-documentation-comments".to_string();
        let response = r#"{"summary":"x","files":[{"path":"src/sample.ts","action":"update","content":"/** Returns false. */\nexport function value() {\n  return false;\n}\n"}],"preservedExports":["value"],"validationCommand":"true"}"#;

        let error = parse_patch_plan_response(&request, &policy, response).unwrap_err();

        assert!(error
            .to_string()
            .contains("documentation-only patch changed non-comment code"));
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
