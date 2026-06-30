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
use std::{path::PathBuf, sync::Arc, time::Duration};
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

struct Harness {
    app: Router,
    db: Arc<Database>,
    repo: TempDir,
    _db_dir: TempDir,
}

struct TestResponse {
    status: StatusCode,
    json: Value,
}

impl Harness {
    fn new() -> Self {
        let repo = tempfile::tempdir().unwrap();
        let db_dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(db_dir.path().join("local-refactor.sqlite")).unwrap());
        let analyzer_script = analyzer_script_path();
        let state = ServiceState::new(db.clone(), analyzer_script, Arc::new(ReadyModelGateway));
        let app = router(state);

        Self {
            app,
            db,
            repo,
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
        for _ in 0..80 {
            let response = self.get_json(&format!("/api/runs/{run_id}")).await;
            assert_eq!(response.status, StatusCode::OK);
            if response.json["status"] == expected_status {
                return response.json;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        panic!("run {run_id} did not reach {expected_status}");
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
