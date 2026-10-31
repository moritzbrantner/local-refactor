use super::*;
use local_refactor_core::{
    config::TestFileMode,
    coverage::CoverageEvidenceItem,
    path_policy::PathPolicy,
    rules::{AllowedWrites, Language, RulePlanningContext, StackContext},
};
use serde_json::Value;
use std::path::PathBuf;
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

fn schema_example(prompt: &str) -> Value {
    let example = prompt
        .split("\n\n")
        .find(|part| part.starts_with('{'))
        .expect("prompt should contain a JSON schema example");
    serde_json::from_str(example).expect("schema example should be valid JSON")
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
fn normal_prompt_json_escapes_explicit_validation_command() {
    let dir = tempdir().unwrap();
    let mut request = request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile);
    let command = "grep -q \"expected value\" path\\file\nprintf done";
    request.validation_commands = vec![command.to_string()];

    let example = schema_example(&build_prompt(&request));

    assert_eq!(example["validationCommand"], command);
}

#[test]
fn coverage_prompt_json_escapes_explicit_validation_command() {
    let dir = tempdir().unwrap();
    let mut request = coverage_request(dir.path().to_path_buf());
    let command = "grep -q \"expected value\" path\\file\nprintf done";
    request.validation_commands = vec![command.to_string()];

    let example = schema_example(&build_prompt(&request));

    assert_eq!(example["validationCommand"], command);
}

#[test]
fn automatic_validation_accepts_empty_patch_plan_validation_command() {
    let dir = tempdir().unwrap();
    let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
    let mut request = request(dir.path().to_path_buf(), "", AllowedWrites::SingleFile);
    request.validation_commands.clear();
    let response =
        r#"{"summary":"No safe change","files":[],"preservedExports":[],"validationCommand":""}"#;

    let edits = parse_patch_plan_response(&request, &policy, response).unwrap();

    assert!(edits.is_empty());
    assert!(build_prompt(&request).contains("Set validationCommand to an empty string"));
}

#[test]
fn explicit_validation_still_requires_patch_plan_validation_command() {
    let dir = tempdir().unwrap();
    let policy = PathPolicy::new(dir.path(), &[], TestFileMode::Mutable).unwrap();
    let response =
        r#"{"summary":"No safe change","files":[],"preservedExports":[],"validationCommand":""}"#;

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

    let (edits, claims) = parse_coverage_patch_plan_response(&request, &policy, response).unwrap();

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
