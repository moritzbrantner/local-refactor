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
use std::{
    path::PathBuf,
    process::Command as StdCommand,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
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

struct RepairingModelGateway {
    generated_plans: Arc<AtomicUsize>,
}

struct RecordingModelGateway {
    requests: Arc<Mutex<Vec<local_refactor_service::PatchPlanModelRequest>>>,
    response: RecordingModelResponse,
}

#[derive(Clone, Copy)]
enum RecordingModelResponse {
    FakePatchPlan,
    DocumentationPerFile,
    Noop,
}

impl ModelGateway for RepairingModelGateway {
    fn list_models(&self) -> ModelFuture<'_, ModelsResponse> {
        ReadyModelGateway.list_models()
    }

    fn ensure_model_available_with_progress<'a>(
        &'a self,
        name: &'a str,
        on_progress: ModelProgressSink,
    ) -> ModelFuture<'a, ()> {
        ReadyModelGateway.ensure_model_available_with_progress(name, on_progress)
    }

    fn generate_patch_plan<'a>(
        &'a self,
        _model: &'a str,
        request: local_refactor_service::PatchPlanModelRequest,
    ) -> ModelFuture<'a, String> {
        self.generated_plans.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(if request.repair_context.is_some() {
                repair_success_patch_plan()
            } else {
                fake_patch_plan(&request.rule_id)
            })
        })
    }
}

impl ModelGateway for RecordingModelGateway {
    fn list_models(&self) -> ModelFuture<'_, ModelsResponse> {
        ReadyModelGateway.list_models()
    }

    fn ensure_model_available_with_progress<'a>(
        &'a self,
        name: &'a str,
        on_progress: ModelProgressSink,
    ) -> ModelFuture<'a, ()> {
        ReadyModelGateway.ensure_model_available_with_progress(name, on_progress)
    }

    fn generate_patch_plan<'a>(
        &'a self,
        _model: &'a str,
        request: local_refactor_service::PatchPlanModelRequest,
    ) -> ModelFuture<'a, String> {
        self.requests.lock().unwrap().push(request.clone());
        let response = self.response;
        Box::pin(async move {
            Ok(match response {
                RecordingModelResponse::FakePatchPlan => fake_patch_plan(&request.rule_id),
                RecordingModelResponse::DocumentationPerFile => {
                    documentation_patch_plan_for_request(&request)
                }
                RecordingModelResponse::Noop => noop_patch_plan(),
            })
        })
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
async fn model_planned_split_file_request_includes_planning_context() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let harness = Harness::with_repo_and_gateway(
        tempfile::tempdir().unwrap(),
        Arc::new(RecordingModelGateway {
            requests: requests.clone(),
            response: RecordingModelResponse::FakePatchPlan,
        }),
    );
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, user_profile_source()).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["split-file-by-responsibility"],
                "model": "qwen2.5-coder:7b",
                "testFileMode": "readOnly",
                "validationCommands": ["true"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();
    harness.poll_run(&run_id, "succeeded").await;

    let request = requests.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request.rule_id, "split-file-by-responsibility");
    assert!(request
        .planning_context
        .preservation_rules
        .iter()
        .any(|rule| rule.contains("Preserve external imports and exports")));
    assert!(request
        .planning_context
        .structure_rules
        .iter()
        .any(|rule| rule.contains("compatibility shims")));
    assert!(request
        .planning_context
        .forbidden_actions
        .iter()
        .any(|rule| rule.contains("Do not delete files")));
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

#[tokio::test]
async fn add_documentation_comments_satisfies_run_supported_contract() {
    assert_run_supported_rule(
        "add-documentation-comments",
        RunFixture {
            source: report_source(),
            expected_content: "/** Renders a report summary label.",
            validation_commands: vec!["grep -q 'Renders a report summary label' src/sample.ts"],
        },
    )
    .await;
}

#[tokio::test]
async fn model_planned_runs_request_one_primary_source_file_at_a_time() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let harness = Harness::with_repo_and_gateway(
        tempfile::tempdir().unwrap(),
        Arc::new(RecordingModelGateway {
            requests: requests.clone(),
            response: RecordingModelResponse::DocumentationPerFile,
        }),
    );
    let alpha = harness.repo.path().join("src/alpha.ts");
    let beta = harness.repo.path().join("src/beta.ts");
    std::fs::create_dir_all(alpha.parent().unwrap()).unwrap();
    std::fs::write(
        &alpha,
        "export function alpha() {\n  return \"alpha\";\n}\n",
    )
    .unwrap();
    std::fs::write(&beta, "export function beta() {\n  return \"beta\";\n}\n").unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["add-documentation-comments"],
                "model": "qwen2.5-coder:7b",
                "testFileMode": "readOnly",
                "validationCommands": [
                    "grep -q 'Documentation for src/alpha.ts' src/alpha.ts",
                    "grep -q 'Documentation for src/beta.ts' src/beta.ts"
                ]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();
    harness.poll_run(&run_id, "succeeded").await;

    let recorded = requests.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].files.len(), 1);
    assert_eq!(recorded[0].files[0].relative_path, "src/alpha.ts");
    assert_eq!(recorded[1].files.len(), 1);
    assert_eq!(recorded[1].files[0].relative_path, "src/beta.ts");
    assert!(std::fs::read_to_string(&alpha)
        .unwrap()
        .contains("Documentation for src/alpha.ts"));
    assert!(std::fs::read_to_string(&beta)
        .unwrap()
        .contains("Documentation for src/beta.ts"));

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 2);

    let event_messages = harness.event_messages(&run_id);
    assert!(event_messages
        .iter()
        .any(|message| message.contains("add-documentation-comments (1/2)")));
    assert!(event_messages
        .iter()
        .any(|message| message.contains("add-documentation-comments (2/2)")));
}

#[tokio::test]
async fn file_target_model_planned_runs_only_collect_target_file() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let harness = Harness::with_repo_and_gateway(
        tempfile::tempdir().unwrap(),
        Arc::new(RecordingModelGateway {
            requests: requests.clone(),
            response: RecordingModelResponse::DocumentationPerFile,
        }),
    );
    let alpha = harness.repo.path().join("src/alpha.ts");
    let beta = harness.repo.path().join("src/beta.ts");
    let beta_original = "export function beta() {\n  return \"beta\";\n}\n";
    std::fs::create_dir_all(alpha.parent().unwrap()).unwrap();
    std::fs::write(
        &alpha,
        "export function alpha() {\n  return \"alpha\";\n}\n",
    )
    .unwrap();
    std::fs::write(&beta, beta_original).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": alpha.to_string_lossy(),
                "rules": ["add-documentation-comments"],
                "model": "qwen2.5-coder:7b",
                "testFileMode": "readOnly",
                "validationCommands": ["grep -q 'Documentation for alpha.ts' alpha.ts"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();
    harness.poll_run(&run_id, "succeeded").await;

    let recorded = requests.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].files.len(), 1);
    assert_eq!(recorded[0].files[0].relative_path, "alpha.ts");
    assert!(std::fs::read_to_string(&alpha)
        .unwrap()
        .contains("Documentation for alpha.ts"));
    assert_eq!(std::fs::read_to_string(&beta).unwrap(), beta_original);

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 1);
    assert_eq!(
        diff.json["files"][0]["filePath"].as_str().unwrap(),
        alpha.to_string_lossy()
    );
}

#[tokio::test]
async fn oversized_model_planned_file_fails_before_generation() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let harness = Harness::with_repo_and_gateway(
        tempfile::tempdir().unwrap(),
        Arc::new(RecordingModelGateway {
            requests: requests.clone(),
            response: RecordingModelResponse::Noop,
        }),
    );
    let huge = harness.repo.path().join("src/huge.ts");
    std::fs::create_dir_all(huge.parent().unwrap()).unwrap();
    std::fs::write(
        &huge,
        format!("export const huge = `{}`;\n", "a".repeat(100_000)),
    )
    .unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["add-documentation-comments"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["true"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();
    let run = harness.poll_run(&run_id, "failed").await;

    assert!(requests.lock().unwrap().is_empty());
    let error = run["error"].as_str().unwrap();
    assert!(error.contains("model-planned request for add-documentation-comments is too large"));
    assert!(error.contains("src/huge.ts"));
    assert!(error.contains("chars"));
}

#[tokio::test]
async fn rust_extract_helper_function_satisfies_run_supported_contract() {
    assert_rust_run_supported_rule("rust-extract-helper-function", "fn subtotal").await;
}

#[tokio::test]
async fn rust_add_documentation_comments_satisfies_run_supported_contract() {
    assert_rust_run_supported_rule(
        "rust-add-documentation-comments",
        "/// Calculates an invoice total after subtracting a discount.",
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
        "Collecting mutable TypeScript files",
        "Running validation checks",
        "Run completed successfully",
    ];
    if is_model_planned(rule_id) {
        expected_events.extend([
            "Ensuring local model qwen2.5-coder:7b is downloaded",
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

async fn assert_rust_run_supported_rule(rule_id: &str, expected_content: &str) {
    let harness = Harness::new();
    let cargo_toml = harness.repo.path().join("Cargo.toml");
    let lib = harness.repo.path().join("src/lib.rs");
    let test_file = harness.repo.path().join("tests/integration.rs");
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    std::fs::create_dir_all(test_file.parent().unwrap()).unwrap();
    std::fs::write(
        &cargo_toml,
        "[package]\nname = \"rust_refactor_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(&lib, rust_invoice_source()).unwrap();
    std::fs::write(&test_file, "use rust_refactor_fixture::calculate_invoice_total;\n\n#[test]\nfn calculates_total() {\n    assert_eq!(calculate_invoice_total(&[(100, 2), (50, 1)], 25), 225);\n}\n").unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": [rule_id],
                "model": "qwen2.5-coder:7b",
                "testFileMode": "readOnly",
                "validationCommands": ["true"]
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::ACCEPTED);
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "succeeded").await;
    assert_eq!(run["status"], "succeeded");
    assert!(std::fs::read_to_string(&lib)
        .unwrap()
        .contains(expected_content));
    assert_eq!(run["validationCommands"].as_array().unwrap()[0], "true");
    assert!(run["validationOutput"].as_str().unwrap().contains("$ true"));

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.status, StatusCode::OK);
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 1);
    assert_eq!(diff.json["files"][0]["ruleId"], rule_id);

    let event_messages = harness.event_messages(&run_id);
    for expected in [
        "Ensuring local model qwen2.5-coder:7b is downloaded",
        "Collecting mutable Rust files",
        "Requesting model patch plan",
        "Applying model patch-plan edits",
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

fn is_model_planned(rule_id: &str) -> bool {
    matches!(
        rule_id,
        "split-oversized-function"
            | "extract-duplicate-block"
            | "isolate-side-effect-free-helper"
            | "split-file-by-responsibility"
            | "extract-parameter-object"
            | "add-documentation-comments"
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
    assert!(review["metrics"]["modelEnsureAvailableMs"].is_null());
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
    assert!(review["metrics"]["modelEnsureAvailableMs"].is_null());
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
async fn deterministic_preview_returns_whole_run_diff_without_creating_run() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, conditional_source()).unwrap();

    let preview = harness
        .post_json(
            "/api/runs/deterministic-preview",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional"],
                "validationCommands": ["true"]
            }),
        )
        .await;

    assert_eq!(preview.status, StatusCode::OK);
    assert_eq!(preview.json["rules"], json!(["simplify-conditional"]));
    assert!(preview.json["previewFingerprint"].as_str().unwrap().len() > 20);
    assert_eq!(preview.json["files"].as_array().unwrap().len(), 1);
    let file = &preview.json["files"][0];
    assert_eq!(file["relativePath"], "src/sample.ts");
    assert!(file["diff"].as_str().unwrap().contains("+  return value;"));
    assert!(harness.db.list_runs(None).unwrap().is_empty());
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains("return false;"));
}

#[tokio::test]
async fn deterministic_preview_rejects_mixed_rule_sets() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, conditional_source()).unwrap();

    let preview = harness
        .post_json(
            "/api/runs/deterministic-preview",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["simplify-conditional", "add-documentation-comments"]
            }),
        )
        .await;

    assert_eq!(preview.status, StatusCode::BAD_REQUEST);
    assert!(preview.json["error"]
        .as_str()
        .unwrap()
        .contains("deterministic TypeScript rules"));
}

#[tokio::test]
async fn deterministic_preview_apply_creates_normal_run_and_can_revert() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, conditional_source()).unwrap();

    let run_request = json!({
        "targetPath": harness.repo.path().to_string_lossy(),
        "rules": ["simplify-conditional"],
        "validationCommands": ["grep -q 'return value;' src/sample.ts"]
    });
    let preview = harness
        .post_json("/api/runs/deterministic-preview", run_request.clone())
        .await;
    assert_eq!(preview.status, StatusCode::OK);

    let created = harness
        .post_json(
            "/api/runs/deterministic-preview/apply",
            json!({
                "run": run_request,
                "previewFingerprint": preview.json["previewFingerprint"]
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::ACCEPTED);
    let run_id = created.json["id"].as_str().unwrap().to_string();
    let run = harness.poll_run(&run_id, "succeeded").await;

    assert_eq!(run["model"], Value::Null);
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains("return value;"));
    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 1);
    let review = harness
        .get_json(&format!("/api/runs/{run_id}/review"))
        .await;
    assert!(review.json["metrics"]["modelEnsureAvailableMs"].is_null());
    assert!(review.json["metrics"]["modelPlanningMs"].is_null());

    let reverted = harness
        .post_json(&format!("/api/runs/{run_id}/revert"), json!({}))
        .await;
    assert_eq!(reverted.status, StatusCode::NO_CONTENT);
    assert_eq!(
        std::fs::read_to_string(&sample).unwrap(),
        conditional_source()
    );
}

#[tokio::test]
async fn deterministic_preview_apply_rejects_stale_preview_without_creating_run() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, conditional_source()).unwrap();

    let run_request = json!({
        "targetPath": harness.repo.path().to_string_lossy(),
        "rules": ["simplify-conditional"],
        "validationCommands": ["true"]
    });
    let preview = harness
        .post_json("/api/runs/deterministic-preview", run_request.clone())
        .await;
    assert_eq!(preview.status, StatusCode::OK);
    std::fs::write(&sample, "export const unchanged = true;\n").unwrap();

    let apply = harness
        .post_json(
            "/api/runs/deterministic-preview/apply",
            json!({
                "run": run_request,
                "previewFingerprint": preview.json["previewFingerprint"]
            }),
        )
        .await;

    assert_eq!(apply.status, StatusCode::CONFLICT);
    assert!(harness.db.list_runs(None).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(&sample).unwrap(),
        "export const unchanged = true;\n"
    );
}

#[tokio::test]
async fn multi_rule_deterministic_preview_matches_applied_content() {
    let harness = Harness::new();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(
        &sample,
        [
            "import { beta } from \"./tools\";",
            "import { alpha } from \"./tools\";",
            "",
            "export function isReady(value: boolean) {",
            "  if (value) {",
            "    return true;",
            "  }",
            "  return false;",
            "}",
            "",
        ]
        .join("\n"),
    )
    .unwrap();

    let run_request = json!({
        "targetPath": harness.repo.path().to_string_lossy(),
        "rules": ["normalize-imports", "simplify-conditional"],
        "validationCommands": ["grep -q 'return value;' src/sample.ts"]
    });
    let preview = harness
        .post_json("/api/runs/deterministic-preview", run_request.clone())
        .await;
    assert_eq!(preview.status, StatusCode::OK);
    let preview_diff = preview.json["files"][0]["diff"].as_str().unwrap();
    assert!(preview_diff.contains("import { alpha, beta }"));
    assert!(preview_diff.contains("+  return value;"));

    let created = harness
        .post_json(
            "/api/runs/deterministic-preview/apply",
            json!({
                "run": run_request,
                "previewFingerprint": preview.json["previewFingerprint"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();
    harness.poll_run(&run_id, "succeeded").await;
    let applied = std::fs::read_to_string(&sample).unwrap();
    assert!(applied.contains("import { alpha, beta }"));
    assert!(applied.contains("return value;"));
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
async fn rule_selection_plan_endpoint_returns_segment_recommendations() {
    let harness = Harness::new_git_repo();
    let package = harness.repo.path().join("packages/web");
    let sample = package.join("src/report.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(package.join("package.json"), "{}").unwrap();
    std::fs::write(&sample, report_source()).unwrap();

    let repository = harness
        .post_json(
            "/api/repositories",
            json!({ "path": harness.repo.path().to_string_lossy() }),
        )
        .await;
    let repository_id = repository.json["id"].as_str().unwrap();

    let response = harness
        .post_json(
            "/api/rule-selection/plan",
            json!({
                "repositoryId": repository_id,
                "targetRelativePath": ".",
                "testFileMode": "readOnly"
            }),
        )
        .await;

    assert_eq!(response.status, StatusCode::OK);
    let segments = response.json["plan"]["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0]["relativePath"], "packages/web");
    let rules = segments[0]["rules"].as_array().unwrap();
    assert!(rules.iter().any(|rule| rule == "simplify-conditional"));
    assert!(rules.iter().any(|rule| rule == "normalize-imports"));
    assert!(rules.iter().any(|rule| rule == "extract-type-definition"));
    assert!(segments[0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| {
            reason["ruleId"] == "extract-type-definition" && reason["source"] == "content"
        }));
}

#[tokio::test]
async fn automatic_segmented_run_executes_each_segment_and_persists_plan() {
    let harness = Harness::new_git_repo();
    let first = harness.repo.path().join("packages/a/src/sample.ts");
    let second = harness.repo.path().join("packages/b/src/sample.ts");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::create_dir_all(second.parent().unwrap()).unwrap();
    std::fs::write(harness.repo.path().join("packages/a/package.json"), "{}").unwrap();
    std::fs::write(harness.repo.path().join("packages/b/package.json"), "{}").unwrap();
    std::fs::write(&first, conditional_source()).unwrap();
    std::fs::write(&second, conditional_source()).unwrap();

    let repository = harness
        .post_json(
            "/api/repositories",
            json!({ "path": harness.repo.path().to_string_lossy() }),
        )
        .await;
    let repository_id = repository.json["id"].as_str().unwrap();
    let plan_response = harness
        .post_json(
            "/api/rule-selection/plan",
            json!({
                "repositoryId": repository_id,
                "targetRelativePath": "."
            }),
        )
        .await;
    assert_eq!(plan_response.status, StatusCode::OK);
    assert_eq!(
        plan_response.json["plan"]["segments"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "repositoryId": repository_id,
                "targetRelativePath": ".",
                "ruleSelectionPlan": plan_response.json["plan"],
                "rules": [],
                "model": "qwen2.5-coder:7b",
                "validationCommands": [
                    "grep -q 'return value;' packages/a/src/sample.ts",
                    "grep -q 'return value;' packages/b/src/sample.ts"
                ]
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::ACCEPTED);
    let run_id = created.json["id"].as_str().unwrap();
    let run = harness.poll_run(run_id, "succeeded").await;

    assert!(std::fs::read_to_string(&first)
        .unwrap()
        .contains("return value;"));
    assert!(std::fs::read_to_string(&second)
        .unwrap()
        .contains("return value;"));
    assert_eq!(
        run["ruleSelectionPlan"]["segments"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(run["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule == "simplify-conditional"));

    let event_messages = harness.event_messages(run_id);
    assert!(event_messages
        .iter()
        .any(|message| message.contains("Running automatic rule segment packages/a")));
    assert!(event_messages
        .iter()
        .any(|message| message.contains("Running automatic rule segment packages/b")));
}

#[tokio::test]
async fn manual_rules_override_supplied_rule_selection_plan() {
    let harness = Harness::new_git_repo();
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, duplicate_import_source()).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["normalize-imports"],
                "ruleSelectionPlan": {
                    "targetRelativePath": "",
                    "segments": [{
                        "relativePath": "",
                        "rules": ["simplify-conditional"],
                        "reasons": [{
                            "ruleId": "simplify-conditional",
                            "source": "fallback",
                            "message": "ignored"
                        }]
                    }]
                },
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["grep -q 'import { alpha, beta }' src/sample.ts"]
            }),
        )
        .await;
    assert_eq!(created.status, StatusCode::ACCEPTED);
    let run_id = created.json["id"].as_str().unwrap();
    let run = harness.poll_run(run_id, "succeeded").await;

    assert!(run["ruleSelectionPlan"].is_null());
    assert_eq!(run["rules"], json!(["normalize-imports"]));
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains("import { alpha, beta }"));
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
                "validationCommands": ["exit 1"],
                "repairBudget": 0
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
async fn model_planned_run_repairs_failed_validation_before_succeeding() {
    let generated_plans = Arc::new(AtomicUsize::new(0));
    let harness = Harness::with_repo_and_gateway(
        tempfile::tempdir().unwrap(),
        Arc::new(RepairingModelGateway {
            generated_plans: generated_plans.clone(),
        }),
    );
    let sample = harness.repo.path().join("src/sample.ts");
    std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
    std::fs::write(&sample, duplicate_block_source()).unwrap();

    let created = harness
        .post_json(
            "/api/runs",
            json!({
                "targetPath": harness.repo.path().to_string_lossy(),
                "rules": ["extract-duplicate-block"],
                "model": "qwen2.5-coder:7b",
                "validationCommands": ["grep -q 'repairSucceeded' src/sample.ts"],
                "repairBudget": 2
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    let run = harness.poll_run(&run_id, "succeeded").await;
    assert_eq!(run["status"], "succeeded");
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains("repairSucceeded"));
    assert_eq!(generated_plans.load(Ordering::SeqCst), 2);

    let event_messages = harness.event_messages(&run_id);
    assert!(event_messages
        .iter()
        .any(|message| message.contains("Attempting model repair 1/2")));
    assert!(event_messages
        .iter()
        .any(|message| message.contains("Applying model repair patch-plan edits")));
}

#[tokio::test]
async fn cancelling_run_after_edits_reverts_patch_journal() {
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
                "validationCommands": ["sleep 1"]
            }),
        )
        .await;
    let run_id = created.json["id"].as_str().unwrap().to_string();

    for _ in 0..100 {
        if std::fs::read_to_string(&sample)
            .unwrap()
            .contains("return value;")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(std::fs::read_to_string(&sample)
        .unwrap()
        .contains("return value;"));

    let cancelled = harness
        .post_json(&format!("/api/runs/{run_id}/cancel"), json!({}))
        .await;
    assert_eq!(cancelled.status, StatusCode::NO_CONTENT);

    let run = harness.poll_run(&run_id, "cancelled").await;
    assert_eq!(run["status"], "cancelled");
    assert_eq!(std::fs::read_to_string(&sample).unwrap(), original);

    let diff = harness.get_json(&format!("/api/runs/{run_id}/diff")).await;
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 1);
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
        Self::with_repo_and_gateway(repo, Arc::new(ReadyModelGateway))
    }

    fn with_repo_and_gateway(repo: TempDir, model_gateway: Arc<dyn ModelGateway>) -> Self {
        let db_dir = tempfile::tempdir().unwrap();
        let db_path = db_dir.path().join("local-refactor.sqlite");
        let db = Arc::new(Database::open(db_path.clone()).unwrap());
        let analyzer_script = analyzer_script_path();
        let state = ServiceState::new(db.clone(), analyzer_script, model_gateway);
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
        for _ in 0..300 {
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

fn rust_invoice_source() -> &'static str {
    "pub fn calculate_invoice_total(items: &[(u32, u32)], discount_cents: u32) -> u32 {\n    let mut total = 0;\n    for &(price, quantity) in items {\n        total += price * quantity;\n    }\n    total.saturating_sub(discount_cents)\n}\n"
}

fn documentation_patch_plan_for_request(
    request: &local_refactor_service::PatchPlanModelRequest,
) -> String {
    let file = request
        .files
        .first()
        .expect("documentation request has a file");
    json!({
        "summary": format!("Add documentation to {}", file.relative_path),
        "files": [{
            "path": file.relative_path.clone(),
            "action": "update",
            "content": format!(
                "/** Documentation for {}. */\n{}",
                file.relative_path,
                file.content
            )
        }],
        "preservedExports": [],
        "validationCommand": "true"
    })
    .to_string()
}

fn noop_patch_plan() -> String {
    json!({
        "summary": "No safe change found",
        "files": [],
        "preservedExports": [],
        "validationCommand": "true"
    })
    .to_string()
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
        "add-documentation-comments" => json!({
            "summary": "Add JSDoc to report rendering API",
            "files": [{
                "path": "src/sample.ts",
                "action": "update",
                "content": "/** Renders a report summary label. */\nexport function renderReport(input: { title: string; total: number }) {\n  return `${input.title}: ${input.total}`;\n}\n"
            }],
            "preservedExports": ["renderReport"],
            "validationCommand": "true"
        })
        .to_string(),
        "rust-extract-helper-function" => json!({
            "summary": "Extract Rust subtotal helper",
            "files": [{
                "path": "src/lib.rs",
                "action": "update",
                "content": "fn subtotal(items: &[(u32, u32)]) -> u32 {\n    items.iter().map(|&(price, quantity)| price * quantity).sum()\n}\n\npub fn calculate_invoice_total(items: &[(u32, u32)], discount_cents: u32) -> u32 {\n    subtotal(items).saturating_sub(discount_cents)\n}\n"
            }],
            "preservedExports": ["calculate_invoice_total"],
            "validationCommand": "cargo check --all-targets"
        })
        .to_string(),
        "rust-add-documentation-comments" => json!({
            "summary": "Add Rust docs to invoice total API",
            "files": [{
                "path": "src/lib.rs",
                "action": "update",
                "content": "/// Calculates an invoice total after subtracting a discount.\npub fn calculate_invoice_total(items: &[(u32, u32)], discount_cents: u32) -> u32 {\n    let mut total = 0;\n    for &(price, quantity) in items {\n        total += price * quantity;\n    }\n    total.saturating_sub(discount_cents)\n}\n"
            }],
            "preservedExports": ["calculate_invoice_total"],
            "validationCommand": "cargo check --all-targets"
        })
        .to_string(),
        other => panic!("missing fake patch plan for {other}"),
    }
}

fn repair_success_patch_plan() -> String {
    json!({
        "summary": "Repair validation marker",
        "files": [{
            "path": "src/sample.ts",
            "action": "update",
            "content": "function formatRecipient(user: { name: string; email: string }) {\n  return `${user.name} <${user.email}>`;\n}\n\nexport const repairSucceeded = true;\n\nexport function sendWelcomeEmail(user: { name: string; email: string }) {\n  const recipient = formatRecipient(user);\n  return `Welcome ${recipient}`;\n}\n\nexport function sendResetEmail(user: { name: string; email: string }) {\n  const recipient = formatRecipient(user);\n  return `Reset ${recipient}`;\n}\n"
        }],
        "preservedExports": ["sendWelcomeEmail", "sendResetEmail", "repairSucceeded"],
        "validationCommand": "grep -q 'repairSucceeded' src/sample.ts"
    })
    .to_string()
}

fn analyzer_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("workers/typescript-analyzer/src/main.ts")
}
