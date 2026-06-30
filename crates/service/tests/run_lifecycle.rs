use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    Router,
};
use local_refactor_service::{
    router, Database, ModelFuture, ModelGateway, ModelProgressSink, ModelSummary, ModelsResponse,
    ServiceState,
};
use serde_json::{json, Value};
use std::{path::PathBuf, process::Command as StdCommand, sync::Arc, time::Duration};
use tempfile::TempDir;
use tower::ServiceExt;

struct ReadyModelGateway;

impl ModelGateway for ReadyModelGateway {
    fn list_models(&self) -> ModelFuture<'_, ModelsResponse> {
        Box::pin(async {
            Ok(ModelsResponse {
                provider: "ollama",
                models: vec![ModelSummary {
                    name: "qwen2.5-coder:7b".to_string(),
                    label: "Qwen2.5 Coder 7B",
                    description: "Ready test model",
                    downloaded: true,
                }],
            })
        })
    }

    fn ensure_model_available_with_progress<'a>(
        &'a self,
        _name: &'a str,
        _on_progress: ModelProgressSink,
    ) -> ModelFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn generate_patch_plan<'a>(
        &'a self,
        _model: &'a str,
        request: local_refactor_service::PatchPlanModelRequest,
    ) -> ModelFuture<'a, String> {
        Box::pin(async move { Ok(fake_patch_plan(&request.rule_id)) })
    }
}

struct RunFixture {
    source: &'static str,
    expected_content: &'static str,
    validation_commands: Vec<&'static str>,
}

#[tokio::test]
async fn simplify_conditional_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "simplify-conditional",
        RunFixture {
            source: conditional_source(),
            expected_content: "return value;",
            validation_commands: vec!["grep -q 'return value;' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn convert_nested_if_to_guard_clause_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "convert-nested-if-to-guard-clause",
        RunFixture {
            source: guard_clause_source(),
            expected_content: "if (!user) return \"guest\";",
            validation_commands: vec!["grep -q 'if (!user) return \"guest\";' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn extract_type_definition_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "extract-type-definition",
        RunFixture {
            source: report_source(),
            expected_content: "export type ReportInput",
            validation_commands: vec!["grep -q 'export type ReportInput' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn inline_trivial_helper_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "inline-trivial-helper",
        RunFixture {
            source: inline_helper_source(),
            expected_content: "return (value * 2) + 1;",
            validation_commands: vec!["grep -q 'return (value \\* 2) + 1;' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn normalize_imports_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "normalize-imports",
        RunFixture {
            source: duplicate_import_source(),
            expected_content: "import { alpha, beta } from \"./tools\";",
            validation_commands: vec!["grep -q 'import { alpha, beta }' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn sort_independent_declarations_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "sort-independent-declarations",
        RunFixture {
            source: unsorted_declarations_source(),
            expected_content: "const alpha = \"a\";",
            validation_commands: vec!["grep -q 'const alpha = \"a\";' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn improve_local_name_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "improve-local-name",
        RunFixture {
            source: cart_summary_source(),
            expected_content: "const itemCount = items.length;",
            validation_commands: vec!["grep -q 'const itemCount = items.length;' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn extract_duplicate_block_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "extract-duplicate-block",
        RunFixture {
            source: duplicate_block_source(),
            expected_content: "function formatRecipient",
            validation_commands: vec!["grep -q 'function formatRecipient' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn split_oversized_function_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "split-oversized-function",
        RunFixture {
            source: oversized_function_source(),
            expected_content: "function subtotal",
            validation_commands: vec!["grep -q 'function subtotal' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn isolate_side_effect_free_helper_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "isolate-side-effect-free-helper",
        RunFixture {
            source: order_source(),
            expected_content: "function calculateOrderTotal",
            validation_commands: vec!["grep -q 'function calculateOrderTotal' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn split_file_by_responsibility_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "split-file-by-responsibility",
        RunFixture {
            source: user_profile_source(),
            expected_content: "export type { User } from \"./user\";",
            validation_commands: vec![
                "grep -q 'export type { User }' src/sample.ts",
                "grep -q 'export type User' src/user.ts",
                "grep -q 'formatUserLabel' src/user-format.ts",
                "grep -q 'isValidUser' src/user-validation.ts",
            ],
        },
    )
    .await;
}

#[tokio::test]
async fn extract_parameter_object_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "extract-parameter-object",
        RunFixture {
            source: parameter_list_source(),
            expected_content: "export type UserParams",
            validation_commands: vec!["grep -q 'export type UserParams' src/sample.ts"],
        },
    )
    .await;
}

async fn assert_run_supported_rule(rule_id: &str, fixture: RunFixture) {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    let protected = harness.repo.path().join("src/generated/client.ts");
    let test_file = harness.repo.path().join("src/sample.test.ts");
    std::fs::create_dir_all(protected.parent().unwrap()).unwrap();
    std::fs::write(&sample, fixture.source).unwrap();
    std::fs::write(&protected, fixture.source).unwrap();
    std::fs::write(&test_file, fixture.source).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": [rule_id],
                "model": "qwen2.5-coder:7b",
                "testFileMode": "readOnly",
                "protectedPaths": ["src/generated/**"],
                "validationCommands": fixture.validation_commands
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::ACCEPTED);
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "succeeded").await;
    assert_eq!(run["status"], "succeeded");
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains(fixture.expected_content));
    assert_eq!(std::fs::read_to_string(&protected).unwrap(), fixture.source);
    assert_eq!(std::fs::read_to_string(&test_file).unwrap(), fixture.source);

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.status, StatusCode::OK);
    let expected_diff_count = if rule_id == "split-file-by-responsibility" {
        4
    } else {
        1
    };
    assert_eq!(
        diff.json["files"].as_array().unwrap().len(),
        expected_diff_count
    );
    assert_eq!(diff.json["files"][0]["ruleId"], rule_id);

    let review = harness
        .get_json(&format!("/api/runs/{run_id}/review"))
        .await;
    assert_eq!(review.status, StatusCode::OK);
    assert_eq!(review.json["run"]["id"], run_id);
    assert_eq!(
        review.json["diff"]["files"].as_array().unwrap().len(),
        expected_diff_count
    );

    let event_messages = harness.event_messages(&run_id);
    let mut expected_events = vec![
        "Ensuring local model qwen2.5-coder:7b is downloaded",
        "Collecting mutable TypeScript files",
        "Running validation checks",
        "Run completed successfully",
    ];
    if is_model_planned(rule_id) {
        expected_events.extend([
            "Requesting model patch plan",
            "Applying model patch-plan edits",
        ]);
    } else {
        expected_events.extend([
            "Running TypeScript analyzer worker",
            "Applying deterministic analyzer edits",
        ]);
    }
    for expected in expected_events {
        assert!(
            event_messages
                .iter()
                .any(|message| message.contains(expected)),
            "missing event containing {expected:?}: {event_messages:#?}"
        );
    }
}

fn is_model_planned(rule_id: &str) -> bool {
    matches!(
        rule_id,
        "split-oversized-function"
            | "extract-duplicate-block"
            | "isolate-side-effect-free-helper"
            | "split-file-by-responsibility"
            | "extract-parameter-object"
    )
}

#[tokio::test]
async fn metrics_are_recorded_for_deterministic_runs() {
    let review = run_rule_and_get_review(
        "simplify-conditional",
        conditional_source(),
        vec!["grep -q 'return value;' src/sample.ts"],
        "succeeded",
    )
    .await;

    assert_number(&review["metrics"]["totalRunMs"]);
    assert_number(&review["metrics"]["modelEnsureAvailableMs"]);
    assert_number(&review["metrics"]["fileCollectionMs"]);
    assert_number(&review["metrics"]["analyzerPlanningMs"]);
    assert!(review["metrics"]["modelPlanningMs"].is_null());
    assert!(review["metrics"]["patchPlanValidationMs"].is_null());
    assert_number(&review["metrics"]["editApplicationMs"]);
    assert_number(&review["metrics"]["validationMs"]);
}

#[tokio::test]
async fn metrics_are_recorded_for_model_planned_runs() {
    let review = run_rule_and_get_review(
        "extract-duplicate-block",
        duplicate_block_source(),
        vec!["grep -q 'function formatRecipient' src/sample.ts"],
        "succeeded",
    )
    .await;

    assert_number(&review["metrics"]["totalRunMs"]);
    assert_number(&review["metrics"]["modelEnsureAvailableMs"]);
    assert_number(&review["metrics"]["fileCollectionMs"]);
    assert!(review["metrics"]["analyzerPlanningMs"].is_null());
    assert_number(&review["metrics"]["modelPlanningMs"]);
    assert_number(&review["metrics"]["patchPlanValidationMs"]);
    assert_number(&review["metrics"]["editApplicationMs"]);
    assert_number(&review["metrics"]["validationMs"]);
}

#[tokio::test]
async fn failed_validation_still_records_timing_metrics() {
    let review = run_rule_and_get_review(
        "simplify-conditional",
        conditional_source(),
        vec!["exit 1"],
        "failed",
    )
    .await;

    assert_number(&review["metrics"]["totalRunMs"]);
    assert_number(&review["metrics"]["modelEnsureAvailableMs"]);
    assert_number(&review["metrics"]["fileCollectionMs"]);
    assert_number(&review["metrics"]["analyzerPlanningMs"]);
    assert_number(&review["metrics"]["editApplicationMs"]);
    assert_number(&review["metrics"]["validationMs"]);
    assert!(review["run"]["error"]
        .as_str()
        .unwrap()
        .contains("validation failed"));
}

#[tokio::test]
async fn root_folder_run_applies_known_refactor_and_keeps_worktree_scoped() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    let protected = harness.repo.path().join("src/generated/client.ts");
    let test_file = harness.repo.path().join("src/sample.test.ts");
    std::fs::create_dir_all(protected.parent().unwrap()).unwrap();
    let original = conditional_source();
    std::fs::write(&sample, original).unwrap();
    std::fs::write(&protected, original).unwrap();
    std::fs::write(&test_file, original).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional"],
                "model": "qwen2.5-coder:7b",
                "testFileMode": "readOnly",
                "protectedPaths": ["src/generated/**"],
                "validationCommands": ["grep -q 'return value;' src/sample.ts"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "succeeded").await;
    assert_eq!(run["status"], "succeeded");
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains("return value;"));
    assert_eq!(std::fs::read_to_string(&protected).unwrap(), original);
    assert_eq!(std::fs::read_to_string(&test_file).unwrap(), original);

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 1);
    assert_eq!(
        diff.json["files"][0]["filePath"].as_str().unwrap(),
        sample.to_string_lossy()
    );

    let event_messages = harness.event_messages(&run_id);
    for expected in [
        "Ensuring local model qwen2.5-coder:7b is downloaded",
        "Collecting mutable TypeScript files",
        "Running TypeScript analyzer worker",
        "Applying deterministic analyzer edits",
        "Running validation checks",
        "Run completed successfully",
    ] {
        assert!(
            event_messages
                .iter()
                .any(|message| message.contains(expected)),
            "missing event containing {expected:?}: {event_messages:#?}"
        );
    }
}

#[tokio::test]
async fn run_applies_simplify_conditional_and_records_diff() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, conditional_source()).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["grep -q 'return value;' src/sample.ts"]
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::ACCEPTED);
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "succeeded").await;
    assert_eq!(run["status"], "succeeded");
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains("return value;"));

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.status, StatusCode::OK);
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 1);
    let diff_text = diff.json["files"][0]["diff"].as_str().unwrap();
    assert!(diff_text.contains("-  if (value) {"));
    assert!(diff_text.contains("+  return value;"));

    let event_messages = harness.event_messages(&run_id);
    for expected in [
        "Collecting mutable TypeScript files",
        "Running TypeScript analyzer worker",
        "Applying deterministic analyzer edits",
        "Running validation checks",
        "Run completed successfully",
    ] {
        assert!(
            event_messages
                .iter()
                .any(|message| message.contains(expected)),
            "missing event containing {expected:?}: {event_messages:#?}"
        );
    }
}

#[tokio::test]
async fn repository_source_run_resolves_target_folder_and_records_context() {
    let harness = Harness::new_git_repo();
    let src = harness.repo.path().join("src");
    let sample = src.join("sample.ts");
    let sibling = harness.repo.path().join("sibling.ts");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(&sample, conditional_source()).unwrap();
    let sibling_original = conditional_source();
    std::fs::write(&sibling, sibling_original).unwrap();

    let created_repository = harness
        .post_json(
            "/api/repositories",
            json!({
                "path": harness.repo.path().to_string_lossy(),
            }),
        )
        .await;
    assert_eq!(created_repository.status, StatusCode::OK);
    let repository_id = created_repository.json["id"].as_str().unwrap().to_string();

    let repositories = harness.get_json("/api/repositories").await;
    assert_eq!(repositories.status, StatusCode::OK);
    assert!(repositories
        .json
        .as_array()
        .unwrap()
        .iter()
        .any(|repository| repository["id"] == repository_id));

    let folders = harness
        .get_json(&format!("/api/repositories/{repository_id}/folders?path=."))
        .await;
    assert_eq!(folders.status, StatusCode::OK);
    assert!(folders.json["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["relativePath"] == "src"));

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "repositoryId": repository_id,
                "targetRelativePath": "src",
                "rules": ["simplify-conditional"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["grep -q 'return value;' sample.ts"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "succeeded").await;
    assert_eq!(run["repositoryId"], repository_id);
    assert_eq!(
        run["repositoryRootPath"],
        harness
            .repo
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(run["targetRelativePath"], "src");
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains("return value;"));
    assert_eq!(std::fs::read_to_string(&sibling).unwrap(), sibling_original);
}

#[tokio::test]
async fn run_reverts_patch_when_validation_fails() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    let original = conditional_source();
    std::fs::write(&sample, original).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["exit 1"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "failed").await;
    assert_eq!(std::fs::read_to_string(&sample).unwrap(), original);
    assert!(run["error"].as_str().unwrap().contains("validation failed"));
    assert!(run["validationOutput"]
        .as_str()
        .unwrap()
        .contains("$ exit 1"));

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn validation_failure_reverts_all_patches_in_reverse_order() {
    let harness = Harness::new();
    let first = harness.repo.path().join("src/a.ts");
    let second = harness.repo.path().join("src/b.ts");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    let first_original = conditional_source().replace("isReady", "isFirstReady");
    let second_original = conditional_source().replace("isReady", "isSecondReady");
    std::fs::write(&first, &first_original).unwrap();
    std::fs::write(&second, &second_original).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["exit 1"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "failed").await;
    assert_eq!(std::fs::read_to_string(&first).unwrap(), first_original);
    assert_eq!(std::fs::read_to_string(&second).unwrap(), second_original);
    assert!(run["error"].as_str().unwrap().contains("validation failed"));
    assert!(run["validationOutput"]
        .as_str()
        .unwrap()
        .contains("$ exit 1"));

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 2);

    let event_messages = harness.event_messages(&run_id);
    assert!(event_messages
        .iter()
        .any(|message| message.contains("Validation failed")));
    assert!(event_messages
        .iter()
        .any(|message| message.contains("Run failed and changes were reverted")));
}

#[tokio::test]
async fn model_planned_validation_failure_removes_created_files() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    let created_type = harness.repo.path().join("src/user.ts");
    let created_format = harness.repo.path().join("src/user-format.ts");
    let created_validation = harness.repo.path().join("src/user-validation.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    let original = user_profile_source();
    std::fs::write(&sample, original).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["split-file-by-responsibility"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["exit 1"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "failed").await;
    assert_eq!(std::fs::read_to_string(&sample).unwrap(), original);
    assert!(!created_type.exists());
    assert!(!created_format.exists());
    assert!(!created_validation.exists());
    assert!(run["error"].as_str().unwrap().contains("validation failed"));

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn run_leaves_test_files_read_only_by_default() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.test.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    let original = conditional_source();
    std::fs::write(&sample, original).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional"],
                "model": "qwen2.5-coder:7b"
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "succeeded").await;
    assert_eq!(run["status"], "succeeded");
    assert_eq!(std::fs::read_to_string(&sample).unwrap(), original);

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert!(diff.json["files"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn no_edit_run_records_analyzer_diagnostics() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(
        &sample,
        "export function label(value: string) {\n  return value.trim();\n}\n",
    )
    .unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["true"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "succeeded").await;
    assert_eq!(run["status"], "succeeded");

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert!(diff.json["files"].as_array().unwrap().is_empty());

    let event_messages = harness.event_messages(&run_id);
    assert!(event_messages.iter().any(|message| {
        message.contains("Analyzed") && message.contains("1 function declarations")
    }));
    assert!(event_messages
        .iter()
        .any(|message| message == "Analyzer produced no edits"));
}

#[tokio::test]
async fn run_review_is_loaded_from_persisted_database() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, conditional_source()).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["grep -q 'return value;' src/sample.ts"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();
    harness.poll_run(&run_id, "succeeded").await;

    let reopened = Harness::from_existing_database(harness.db_path.clone(), harness.repo);
    let review = reopened
        .get_json(&format!("/api/runs/{run_id}/review"))
        .await;

    assert_eq!(review.status, StatusCode::OK);
    assert_eq!(review.json["run"]["id"], run_id);
    assert_eq!(review.json["run"]["status"], "succeeded");
    assert!(review.json["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["message"] == "Run completed successfully"));
    assert_eq!(review.json["diff"]["files"].as_array().unwrap().len(), 1);
    assert_eq!(
        review.json["diff"]["files"][0]["ruleId"],
        "simplify-conditional"
    );
    assert!(review.json["diff"]["files"][0]["diff"]
        .as_str()
        .unwrap()
        .contains("+  return value;"));
    assert_number(&review.json["metrics"]["totalRunMs"]);
    assert_number(&review.json["metrics"]["validationMs"]);
}

struct Harness {
    app: Router,
    db: Arc<Database>,
    repo: TempDir,
    db_path: PathBuf,
    _db_dir: TempDir,
}

struct TestResponse {
    status: StatusCode,
    json: Value,
}

impl Harness {
    fn new() -> Self {
        let repo = tempfile::tempdir().unwrap();
        Self::with_repo(repo)
    }

    fn new_git_repo() -> Self {
        let repo = tempfile::tempdir().unwrap();
        let status = StdCommand::new("git")
            .arg("init")
            .arg(repo.path())
            .status()
            .unwrap();
        assert!(status.success());
        Self::with_repo(repo)
    }

    fn with_repo(repo: TempDir) -> Self {
        let db_dir = tempfile::tempdir().unwrap();
        let db_path = db_dir.path().join("local-refactor.sqlite");
        let db = Arc::new(Database::open(db_path.clone()).unwrap());
        let analyzer_script = analyzer_script_path();
        let state = ServiceState::new(db.clone(), analyzer_script, Arc::new(ReadyModelGateway));
        let app = router(state);

        Self {
            app,
            db,
            repo,
            db_path,
            _db_dir: db_dir,
        }
    }

    fn from_existing_database(db_path: PathBuf, repo: TempDir) -> Self {
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(db_path.clone()).unwrap());
        let analyzer_script = analyzer_script_path();
        let state = ServiceState::new(db.clone(), analyzer_script, Arc::new(ReadyModelGateway));
        let app = router(state);

        Self {
            app,
            db,
            repo,
            db_path,
            _db_dir: db_dir,
        }
    }

    async fn post_json(&self, uri: &str, body: Value) -> TestResponse {
        self.request_json(Method::POST, uri, Some(body)).await
    }

    async fn get_json(&self, uri: &str) -> TestResponse {
        self.request_json(Method::GET, uri, None).await
    }

    async fn request_json(&self, method: Method, uri: &str, body: Option<Value>) -> TestResponse {
        let mut builder = Request::builder().method(method).uri(uri);
        let request_body = if let Some(body) = body {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&body).unwrap())
        } else {
            Body::empty()
        };
        let response = self
            .app
            .clone()
            .oneshot(builder.body(request_body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };

        TestResponse { status, json }
    }

    async fn poll_run(&self, run_id: &str, expected_status: &str) -> Value {
        let mut last_response = Value::Null;
        for _ in 0..80 {
            let response = self.get_json(&format!("/api/runs/{run_id}")).await;
            assert_eq!(response.status, StatusCode::OK);
            if response.json["status"] == expected_status {
                return response.json;
            }
            last_response = response.json;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        panic!("run {run_id} did not reach {expected_status}; last response: {last_response}");
    }

    fn event_messages(&self, run_id: &str) -> Vec<String> {
        self.db
            .events_for_run(run_id)
            .unwrap()
            .into_iter()
            .map(|event| event.message)
            .collect()
    }
}

async fn run_rule_and_get_review(
    rule_id: &str,
    source: &str,
    validation_commands: Vec<&str>,
    expected_status: &str,
) -> Value {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, source).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": [rule_id],
                "model": "qwen2.5-coder:7b",
                "validationCommands": validation_commands
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();
    harness.poll_run(&run_id, expected_status).await;

    let review = harness
        .get_json(&format!("/api/runs/{run_id}/review"))
        .await;
    assert_eq!(review.status, StatusCode::OK);
    review.json
}

fn assert_number(value: &Value) {
    assert!(value.is_number(), "expected JSON number, got {value:?}");
}

fn conditional_source() -> &'static str {
    "export function isReady(value: boolean) {\n  if (value) {\n    return true;\n  }\n  return false;\n}\n"
}

fn guard_clause_source() -> &'static str {
    "export function accessLabel(user: { active: boolean; admin: boolean } | null) {\n  if (user) {\n    if (user.active) {\n      if (user.admin) {\n        return \"admin\";\n      }\n      return \"member\";\n    }\n    return \"disabled\";\n  }\n  return \"guest\";\n}\n"
}

fn report_source() -> &'static str {
    "export function renderReport(input: { title: string; total: number }) {\n  return `${input.title}: ${input.total}`;\n}\n"
}

fn inline_helper_source() -> &'static str {
    "function double(value: number) {\n  return value * 2;\n}\n\nexport function score(value: number) {\n  return double(value) + 1;\n}\n"
}

fn duplicate_import_source() -> &'static str {
    "import { beta } from \"./tools\";\nimport { alpha } from \"./tools\";\n\nexport function label() {\n  return `${alpha()} ${beta()}`;\n}\n"
}

fn unsorted_declarations_source() -> &'static str {
    "const zebra = \"z\";\nconst alpha = \"a\";\nconst middle = \"m\";\n\nexport function label() {\n  return `${alpha}${middle}${zebra}`;\n}\n"
}

fn cart_summary_source() -> &'static str {
    "export function summarizeCart(items: Array<{ price: number }>) {\n  const x = items.length;\n  const y = items.reduce((total, item) => total + item.price, 0);\n  return { itemCount: x, subtotal: y };\n}\n"
}

fn duplicate_block_source() -> &'static str {
    "export function sendWelcomeEmail(user: { name: string; email: string }) {\n  const recipient = `${user.name} <${user.email}>`;\n  return `Welcome ${recipient}`;\n}\n\nexport function sendResetEmail(user: { name: string; email: string }) {\n  const recipient = `${user.name} <${user.email}>`;\n  return `Reset ${recipient}`;\n}\n"
}

fn oversized_function_source() -> &'static str {
    "export function calculateInvoiceTotal(items: Array<{ price: number; quantity: number }>, discountRate: number, taxRate: number) {\n  let subtotal = 0;\n  for (const item of items) {\n    subtotal += item.price * item.quantity;\n  }\n  const discounted = subtotal - subtotal * discountRate;\n  const tax = discounted * taxRate;\n  return discounted + tax;\n}\n"
}

fn order_source() -> &'static str {
    "export function submitOrder(items: Array<{ price: number; quantity: number }>, logger: { info(message: string): void }, saveOrder: (total: number) => void) {\n  let total = 0;\n  for (const item of items) {\n    total += item.price * item.quantity;\n  }\n  logger.info(`saving order ${total}`);\n  saveOrder(total);\n  return total;\n}\n"
}

fn user_profile_source() -> &'static str {
    "export type User = { id: string; name: string; email: string };\n\nexport function formatUserLabel(user: User) {\n  return `${user.name} <${user.email}>`;\n}\n\nexport function isValidUser(user: User) {\n  return Boolean(user.id && user.email.includes(\"@\"));\n}\n"
}

fn parameter_list_source() -> &'static str {
    "export function createUser(name: string, email: string) {\n  return `${name} <${email}>`;\n}\n\nexport function renderUser() {\n  return createUser(\"Ada\", \"ada@example.com\");\n}\n"
}

fn fake_patch_plan(rule_id: &str) -> String {
    match rule_id {
        "extract-duplicate-block" => json!({
            "summary": "Extract duplicate recipient formatting",
            "files": [{
                "path": "src/sample.ts",
                "action": "update",
                "content": "function formatRecipient(user: { name: string; email: string }) {\n  return `${user.name} <${user.email}>`;\n}\n\nexport function sendWelcomeEmail(user: { name: string; email: string }) {\n  const recipient = formatRecipient(user);\n  return `Welcome ${recipient}`;\n}\n\nexport function sendResetEmail(user: { name: string; email: string }) {\n  const recipient = formatRecipient(user);\n  return `Reset ${recipient}`;\n}\n"
            }],
            "preservedExports": ["sendWelcomeEmail", "sendResetEmail"],
            "validationCommand": "true"
        })
        .to_string(),
        "split-oversized-function" => json!({
            "summary": "Split invoice total helpers",
            "files": [{
                "path": "src/sample.ts",
                "action": "update",
                "content": "function subtotal(items: Array<{ price: number; quantity: number }>) {\n  let total = 0;\n  for (const item of items) {\n    total += item.price * item.quantity;\n  }\n  return total;\n}\n\nfunction discount(total: number, discountRate: number) {\n  return total - total * discountRate;\n}\n\nfunction tax(total: number, taxRate: number) {\n  return total * taxRate;\n}\n\nexport function calculateInvoiceTotal(items: Array<{ price: number; quantity: number }>, discountRate: number, taxRate: number) {\n  const discounted = discount(subtotal(items), discountRate);\n  return discounted + tax(discounted, taxRate);\n}\n"
            }],
            "preservedExports": ["calculateInvoiceTotal"],
            "validationCommand": "true"
        })
        .to_string(),
        "isolate-side-effect-free-helper" => json!({
            "summary": "Extract pure order total helper",
            "files": [{
                "path": "src/sample.ts",
                "action": "update",
                "content": "function calculateOrderTotal(items: Array<{ price: number; quantity: number }>) {\n  let total = 0;\n  for (const item of items) {\n    total += item.price * item.quantity;\n  }\n  return total;\n}\n\nexport function submitOrder(items: Array<{ price: number; quantity: number }>, logger: { info(message: string): void }, saveOrder: (total: number) => void) {\n  const total = calculateOrderTotal(items);\n  logger.info(`saving order ${total}`);\n  saveOrder(total);\n  return total;\n}\n"
            }],
            "preservedExports": ["submitOrder"],
            "validationCommand": "true"
        })
        .to_string(),
        "split-file-by-responsibility" => json!({
            "summary": "Split user profile responsibilities",
            "files": [
                {
                    "path": "src/sample.ts",
                    "action": "update",
                    "content": "export type { User } from \"./user\";\nexport { formatUserLabel } from \"./user-format\";\nexport { isValidUser } from \"./user-validation\";\n"
                },
                {
                    "path": "src/user.ts",
                    "action": "create",
                    "content": "export type User = { id: string; name: string; email: string };\n"
                },
                {
                    "path": "src/user-format.ts",
                    "action": "create",
                    "content": "import type { User } from \"./user\";\n\nexport function formatUserLabel(user: User) {\n  return `${user.name} <${user.email}>`;\n}\n"
                },
                {
                    "path": "src/user-validation.ts",
                    "action": "create",
                    "content": "import type { User } from \"./user\";\n\nexport function isValidUser(user: User) {\n  return Boolean(user.id && user.email.includes(\"@\"));\n}\n"
                }
            ],
            "preservedExports": ["User", "formatUserLabel", "isValidUser"],
            "validationCommand": "true"
        })
        .to_string(),
        "extract-parameter-object" => json!({
            "summary": "Extract user parameter object",
            "files": [{
                "path": "src/sample.ts",
                "action": "update",
                "content": "export type UserParams = { name: string; email: string };\n\nexport function createUser(params: UserParams) {\n  return `${params.name} <${params.email}>`;\n}\n\nexport function renderUser() {\n  return createUser({ name: \"Ada\", email: \"ada@example.com\" });\n}\n"
            }],
            "preservedExports": ["UserParams", "createUser", "renderUser"],
            "validationCommand": "true"
        })
        .to_string(),
        other => panic!("missing fake patch plan for {other}"),
    }
}

fn analyzer_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("workers/typescript-analyzer/src/main.ts")
}
