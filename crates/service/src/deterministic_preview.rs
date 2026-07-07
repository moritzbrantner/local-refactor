use crate::{
    analyzer::{self, AnalyzerRequest, AnalyzerSourceFile},
    db::PatchAction,
    diff,
    edit_journal::JournaledEdit,
    effective_rules, prepare_source_request, request_target_path, RunCreateRequest,
};
use anyhow::{anyhow, Context, Result};
use local_refactor_core::{
    path_policy::PathPolicy,
    rules::{rule_by_id, Language, RuleExecutionKind},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeterministicPreviewResponse {
    pub target_relative_path: String,
    pub rules: Vec<String>,
    pub preview_fingerprint: String,
    pub files: Vec<DeterministicPreviewFile>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeterministicPreviewFile {
    pub relative_path: String,
    pub file_path: String,
    pub rule_ids: Vec<String>,
    pub summaries: Vec<String>,
    pub original_content_hash: String,
    pub new_content_hash: String,
    pub diff: String,
}

pub(crate) struct DeterministicPlan {
    pub response: DeterministicPreviewResponse,
    pub steps: Vec<DeterministicPlanStep>,
}

pub(crate) struct DeterministicPlanStep {
    pub policy: PathPolicy,
    pub edits: Vec<JournaledEdit>,
}

#[derive(Debug, Clone)]
struct RuleStep {
    rule_id: String,
    request: RunCreateRequest,
}

#[derive(Debug, Clone)]
struct FileAggregate {
    file_path: PathBuf,
    original_content: String,
    new_content: String,
    rule_ids: BTreeSet<String>,
    summaries: Vec<String>,
}

pub(crate) async fn plan(
    analyzer_script: &Path,
    request: &RunCreateRequest,
) -> Result<DeterministicPlan> {
    let rule_ids = effective_rules(request);
    validate_deterministic_rules(&rule_ids)?;
    let display_root = display_root(request)?;
    let steps = rule_steps(request)?;
    let mut contents = BTreeMap::<String, String>::new();
    let mut aggregates = BTreeMap::<String, FileAggregate>::new();
    let mut plan_steps = Vec::new();
    let mut diagnostics = Vec::new();

    for step in steps {
        let (policy, files) = prepare_source_request(&step.request, Language::TypeScript)?;
        if files.is_empty() {
            plan_steps.push(DeterministicPlanStep {
                policy,
                edits: Vec::new(),
            });
            continue;
        }

        let analyzer_files = files
            .iter()
            .map(|file| {
                let content = current_content(&mut contents, file)?;
                Ok(AnalyzerSourceFile::Content {
                    file_path: file.clone(),
                    content,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let response = analyzer::run(
            analyzer_script,
            AnalyzerRequest {
                files: analyzer_files,
                rules: vec![step.rule_id.clone()],
            },
        )
        .await?;
        diagnostics.extend(response.diagnostics);

        let edits = response
            .edits
            .into_iter()
            .map(|edit| {
                contents.insert(edit.file_path.clone(), edit.new_content.clone());
                let aggregate =
                    aggregates
                        .entry(edit.file_path.clone())
                        .or_insert_with(|| FileAggregate {
                            file_path: PathBuf::from(&edit.file_path),
                            original_content: edit.original_content.clone(),
                            new_content: edit.new_content.clone(),
                            rule_ids: BTreeSet::new(),
                            summaries: Vec::new(),
                        });
                aggregate.new_content = edit.new_content.clone();
                aggregate.rule_ids.insert(edit.rule_id.clone());
                if !edit.summary.is_empty() {
                    aggregate.summaries.push(edit.summary.clone());
                }

                JournaledEdit {
                    file_path: PathBuf::from(edit.file_path),
                    original_content: edit.original_content,
                    new_content: edit.new_content,
                    rule_id: edit.rule_id,
                    summary: edit.summary,
                    action: PatchAction::Update,
                }
            })
            .collect::<Vec<_>>();

        plan_steps.push(DeterministicPlanStep { policy, edits });
    }

    let files = aggregates
        .values()
        .map(|aggregate| preview_file(&display_root, aggregate))
        .collect::<Vec<_>>();
    let preview_fingerprint = preview_fingerprint(request, &rule_ids, &files);
    let target_relative_path = target_relative_path(request, &display_root)?;

    Ok(DeterministicPlan {
        response: DeterministicPreviewResponse {
            target_relative_path,
            rules: rule_ids,
            preview_fingerprint,
            files,
            diagnostics,
        },
        steps: plan_steps,
    })
}

pub(crate) fn validate_fingerprint(
    plan: &DeterministicPlan,
    expected_fingerprint: Option<&str>,
) -> Result<()> {
    let Some(expected) = expected_fingerprint else {
        return Ok(());
    };
    if plan.response.preview_fingerprint == expected {
        return Ok(());
    }
    Err(anyhow!(
        "deterministic preview changed; refresh the preview before applying"
    ))
}

fn validate_deterministic_rules(rule_ids: &[String]) -> Result<()> {
    for rule_id in rule_ids {
        let rule =
            rule_by_id(rule_id).ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))?;
        if rule.language != Language::TypeScript
            || rule.execution_kind != RuleExecutionKind::Deterministic
        {
            return Err(anyhow!(
                "Deterministic Preview only supports deterministic TypeScript rules"
            ));
        }
    }
    Ok(())
}

fn rule_steps(request: &RunCreateRequest) -> Result<Vec<RuleStep>> {
    if let Some(plan) = request.rule_selection_plan.as_ref() {
        let repository_root = request
            .repository_root_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("rule selection plan requires repository root path"))?;
        let mut steps = Vec::new();
        for segment in &plan.segments {
            let segment_path = if segment.relative_path.is_empty() {
                repository_root.clone()
            } else {
                repository_root.join(&segment.relative_path)
            };
            for rule_id in &segment.rules {
                let mut segment_request = request.clone();
                segment_request.target_path = Some(segment_path.to_string_lossy().to_string());
                segment_request.rules = vec![rule_id.clone()];
                steps.push(RuleStep {
                    rule_id: rule_id.clone(),
                    request: segment_request,
                });
            }
        }
        return Ok(steps);
    }

    Ok(effective_rules(request)
        .into_iter()
        .map(|rule_id| RuleStep {
            rule_id: rule_id.clone(),
            request: RunCreateRequest {
                rules: vec![rule_id],
                ..request.clone()
            },
        })
        .collect())
}

fn current_content(contents: &mut BTreeMap<String, String>, file: &str) -> Result<String> {
    if let Some(content) = contents.get(file) {
        return Ok(content.clone());
    }
    let content =
        std::fs::read_to_string(file).with_context(|| format!("failed to read {file}"))?;
    contents.insert(file.to_string(), content.clone());
    Ok(content)
}

fn preview_file(display_root: &Path, aggregate: &FileAggregate) -> DeterministicPreviewFile {
    DeterministicPreviewFile {
        relative_path: relative_path(display_root, &aggregate.file_path),
        file_path: aggregate.file_path.to_string_lossy().to_string(),
        rule_ids: aggregate.rule_ids.iter().cloned().collect(),
        summaries: aggregate.summaries.clone(),
        original_content_hash: stable_hash(&aggregate.original_content),
        new_content_hash: stable_hash(&aggregate.new_content),
        diff: diff::unified(
            &aggregate.file_path.to_string_lossy(),
            &aggregate.original_content,
            &aggregate.new_content,
        ),
    }
}

fn preview_fingerprint(
    request: &RunCreateRequest,
    rule_ids: &[String],
    files: &[DeterministicPreviewFile],
) -> String {
    let mut hasher = Sha256::new();
    hash_field(
        &mut hasher,
        request_target_path(request).unwrap_or_default(),
    );
    hash_field(
        &mut hasher,
        request
            .test_file_mode
            .map(test_file_mode_name)
            .unwrap_or(""),
    );
    for protected in &request.protected_paths {
        hash_field(&mut hasher, protected);
    }
    for command in &request.validation_commands {
        hash_field(&mut hasher, command);
    }
    for rule_id in rule_ids {
        hash_field(&mut hasher, rule_id);
    }
    for file in files {
        hash_field(&mut hasher, &file.file_path);
        for rule_id in &file.rule_ids {
            hash_field(&mut hasher, rule_id);
        }
        for summary in &file.summaries {
            hash_field(&mut hasher, summary);
        }
        hash_field(&mut hasher, &file.original_content_hash);
        hash_field(&mut hasher, &file.new_content_hash);
    }
    hex(hasher.finalize().as_slice())
}

fn stable_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex(hasher.finalize().as_slice())
}

fn hash_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
    hasher.update(b"\n");
}

fn test_file_mode_name(mode: local_refactor_core::config::TestFileMode) -> &'static str {
    match mode {
        local_refactor_core::config::TestFileMode::ReadOnly => "readOnly",
        local_refactor_core::config::TestFileMode::Mutable => "mutable",
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn display_root(request: &RunCreateRequest) -> Result<PathBuf> {
    if let Some(root) = request.repository_root_path.as_deref() {
        return std::fs::canonicalize(root)
            .with_context(|| format!("repository path is unavailable: {root}"));
    }
    let target_path = PathBuf::from(request_target_path(request)?);
    let target_path = std::fs::canonicalize(&target_path)
        .with_context(|| format!("target path does not exist: {}", target_path.display()))?;
    if target_path.is_file() {
        return target_path
            .parent()
            .ok_or_else(|| anyhow!("target file has no parent directory"))
            .map(Path::to_path_buf);
    }
    Ok(target_path)
}

fn target_relative_path(request: &RunCreateRequest, display_root: &Path) -> Result<String> {
    if let Some(relative) = request.target_relative_path.as_ref() {
        return Ok(if relative.is_empty() {
            ".".to_string()
        } else {
            relative.clone()
        });
    }
    let target = PathBuf::from(request_target_path(request)?);
    let target = std::fs::canonicalize(&target)
        .with_context(|| format!("target path does not exist: {}", target.display()))?;
    Ok(relative_path(display_root, &target))
}

fn relative_path(root: &Path, path: &Path) -> String {
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();
    if relative.is_empty() {
        ".".to_string()
    } else {
        relative
    }
}
