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
    assert_eq!(diff.json["files"].as_array().unwrap().len(), 1);
    assert_eq!(diff.json["files"][0]["ruleId"], rule_id);

    let review = harness
        .get_json(&format!("/api/runs/{run_id}/review"))
        .await;
    assert_eq!(review.status, StatusCode::OK);
    assert_eq!(review.json["run"]["id"], run_id);
    assert_eq!(review.json["diff"]["files"].as_array().unwrap().len(), 1);

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

fn conditional_source() -> &'static str {
    "export function isReady(value: boolean) {\n  if (value) {\n    return true;\n  }\n  return false;\n}\n"
}

fn analyzer_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("workers/typescript-analyzer/src/main.ts")
}
