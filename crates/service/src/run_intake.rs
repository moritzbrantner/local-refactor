use crate::{
    model_provider, patch_plan,
    repository_source::{relative_path_from_root, resolve_repository_folder},
    Database, PatchPlanModelRequest, PatchPlanSourceFile, RunCreateRequest,
};
use anyhow::{anyhow, Context, Result};
use local_refactor_core::{
    config::{
        default_protected_paths, find_project_config, load_config_file, ConfigLayer,
        EffectiveConfig,
    },
    conventions::ConventionSettings,
    path_policy::{is_source_for_language, PathDecision, PathPolicy},
    rule_selection::{select_rules, RuleSelectionInput, RuleSelectionPlan},
    rules::{
        planning_context_for, rule_by_id, Language, RuleDefinition, RuleExecutionKind, StackContext,
    },
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const DEFAULT_CANDIDATE_FILE_LIMIT_PER_GROUP: usize = 50;
const MAX_CANDIDATE_FILE_LIMIT_PER_GROUP: usize = 200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CandidateFilePreviewResponse {
    pub target_relative_path: String,
    pub total_candidate_files: usize,
    pub limit_per_group: usize,
    pub groups: Vec<CandidateFilePreviewGroup>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CandidateFilePreviewGroup {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_relative_path: Option<String>,
    pub rule_id: String,
    pub rule_name: String,
    pub language: Language,
    pub total_files: usize,
    pub hidden_files: usize,
    pub files: Vec<CandidateFilePreviewFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CandidateFilePreviewFile {
    pub relative_path: String,
}

pub(crate) fn normalize_request(
    db: &Database,
    mut request: RunCreateRequest,
) -> Result<RunCreateRequest> {
    let has_explicit_rules = !request.rules.is_empty();
    resolve_request_target(db, &mut request)?;
    let run_layer = ConfigLayer {
        rules: if has_explicit_rules {
            Some(request.rules.clone())
        } else {
            None
        },
        protected_paths: if request.protected_paths.is_empty() {
            None
        } else {
            Some(request.protected_paths.clone())
        },
        validation_commands: if request.validation_commands.is_empty() {
            None
        } else {
            Some(request.validation_commands.clone())
        },
        test_file_mode: request.test_file_mode,
        conventions: None,
    };

    let target_path = request_target_path(&request)?.to_string();
    let config = effective_config_for(Path::new(&target_path), run_layer)?;
    request.convention_snapshot = Some(effective_conventions_for_request(db, &request, &config)?);
    request.protected_paths = config.protected_paths;
    request.validation_commands = config.validation_commands;
    request.test_file_mode = Some(config.test_file_mode);
    if has_explicit_rules {
        request.rules = config.rules;
        request.rule_selection_plan = None;
    } else {
        let repository_root = repository_root_for_request(&request)?;
        request.repository_root_path = Some(repository_root.to_string_lossy().to_string());
        if request.rule_selection_plan.is_none() {
            request.rule_selection_plan = Some(select_rules(RuleSelectionInput {
                repository_root: Some(repository_root),
                target_path: PathBuf::from(&target_path),
                protected_paths: request.protected_paths.clone(),
                test_file_mode: request.test_file_mode.unwrap_or_default(),
            })?);
        }
        request.rules = flattened_plan_rules(request.rule_selection_plan.as_ref());
    }
    request.model = if request_rules_need_model(&request.rules)? {
        Some(selected_model(&request)?)
    } else {
        None
    };
    Ok(request)
}

pub(crate) fn effective_rules(request: &RunCreateRequest) -> Vec<String> {
    if request.rules.is_empty() {
        return vec!["simplify-conditional".to_string()];
    }

    request
        .rules
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn request_rules_need_model(rule_ids: &[String]) -> Result<bool> {
    rule_ids.iter().try_fold(false, |needs_model, rule_id| {
        let rule =
            rule_by_id(rule_id).ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))?;
        Ok(needs_model || rule.execution_kind == RuleExecutionKind::ModelPlanned)
    })
}

pub(crate) fn rule_selection_plan_for_request(
    db: &Database,
    mut request: RunCreateRequest,
) -> Result<(RuleSelectionPlan, EffectiveConfig)> {
    resolve_request_target(db, &mut request)?;
    let run_layer = ConfigLayer {
        rules: None,
        protected_paths: if request.protected_paths.is_empty() {
            None
        } else {
            Some(request.protected_paths.clone())
        },
        validation_commands: if request.validation_commands.is_empty() {
            None
        } else {
            Some(request.validation_commands.clone())
        },
        test_file_mode: request.test_file_mode,
        conventions: None,
    };
    let target_path = request_target_path(&request)?;
    let config = effective_config_for(Path::new(target_path), run_layer)?;
    let repository_root = repository_root_for_request(&request)?;
    let plan = select_rules(RuleSelectionInput {
        repository_root: Some(repository_root),
        target_path: PathBuf::from(target_path),
        protected_paths: config.protected_paths.clone(),
        test_file_mode: config.test_file_mode,
    })?;
    Ok((plan, config))
}

fn effective_conventions_for_request(
    db: &Database,
    request: &RunCreateRequest,
    config: &EffectiveConfig,
) -> Result<ConventionSettings> {
    let mut conventions = config.conventions.clone();
    if let Some(repository_id) = request.repository_id.as_deref() {
        if let Some(local_override) = db.get_repository_convention_override(repository_id)? {
            conventions.apply_partial(local_override);
        }
    }
    Ok(conventions)
}

pub(crate) fn candidate_file_preview_for_request(
    db: &Database,
    request: RunCreateRequest,
    limit_per_group: Option<usize>,
) -> Result<CandidateFilePreviewResponse> {
    let is_manual = !request.rules.is_empty();
    let request = normalize_request(db, request)?;
    let limit_per_group = clamp_candidate_file_limit(limit_per_group);
    let repository_root =
        if request.repository_id.is_some() || request.repository_root_path.is_some() {
            repository_root_for_request(&request).ok()
        } else {
            None
        };
    let target_path = PathBuf::from(request_target_path(&request)?);
    let target_root = candidate_target_root(&target_path)?;
    let display_root = repository_root.as_deref().unwrap_or(&target_root);
    let target_relative_path = request
        .target_relative_path
        .clone()
        .unwrap_or_else(|| relative_path(display_root, &target_root));

    let mut all_candidate_files = BTreeSet::new();
    let groups = if is_manual {
        manual_candidate_file_groups(
            &request,
            display_root,
            limit_per_group,
            &mut all_candidate_files,
        )?
    } else {
        automatic_candidate_file_groups(
            &request,
            display_root,
            limit_per_group,
            &mut all_candidate_files,
        )?
    };

    Ok(CandidateFilePreviewResponse {
        target_relative_path,
        total_candidate_files: all_candidate_files.len(),
        limit_per_group,
        groups,
    })
}

pub(crate) fn flattened_plan_rules(plan: Option<&RuleSelectionPlan>) -> Vec<String> {
    plan.map(|plan| {
        plan.segments
            .iter()
            .flat_map(|segment| segment.rules.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    })
    .unwrap_or_default()
}

fn clamp_candidate_file_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_CANDIDATE_FILE_LIMIT_PER_GROUP)
        .clamp(1, MAX_CANDIDATE_FILE_LIMIT_PER_GROUP)
}

fn manual_candidate_file_groups(
    request: &RunCreateRequest,
    display_root: &Path,
    limit_per_group: usize,
    all_candidate_files: &mut BTreeSet<String>,
) -> Result<Vec<CandidateFilePreviewGroup>> {
    effective_rules(request)
        .into_iter()
        .map(|rule_id| {
            let rule = rule_by_id(&rule_id)
                .ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))?;
            let files = candidate_files_for_rule(request, display_root, rule, None)?;
            Ok(candidate_file_group(
                format!("rule:{}", rule.id),
                rule.name.to_string(),
                None,
                rule,
                files,
                limit_per_group,
                all_candidate_files,
            ))
        })
        .collect()
}

fn automatic_candidate_file_groups(
    request: &RunCreateRequest,
    display_root: &Path,
    limit_per_group: usize,
    all_candidate_files: &mut BTreeSet<String>,
) -> Result<Vec<CandidateFilePreviewGroup>> {
    let plan = request
        .rule_selection_plan
        .as_ref()
        .ok_or_else(|| anyhow!("rule selection plan is required for automatic preview"))?;
    let mut groups = Vec::new();
    for segment in &plan.segments {
        for rule_id in &segment.rules {
            let rule = rule_by_id(rule_id)
                .ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))?;
            let files = candidate_files_for_rule(
                request,
                display_root,
                rule,
                Some(&segment.relative_path),
            )?;
            let segment_label = display_segment_label(&segment.relative_path);
            groups.push(candidate_file_group(
                format!("segment:{}:rule:{}", segment.relative_path, rule.id),
                format!("{segment_label} - {}", rule.name),
                Some(segment.relative_path.clone()),
                rule,
                files,
                limit_per_group,
                all_candidate_files,
            ));
        }
    }
    Ok(groups)
}

fn candidate_files_for_rule(
    request: &RunCreateRequest,
    display_root: &Path,
    rule: &RuleDefinition,
    segment_relative_path: Option<&str>,
) -> Result<Vec<String>> {
    let files = prepare_source_request(request, rule.language)?.1;
    let mut files = files
        .into_iter()
        .map(PathBuf::from)
        .map(|path| relative_path(display_root, &path))
        .filter(|path| {
            segment_relative_path
                .map(|segment| path_is_inside_segment(path, segment))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    Ok(files)
}

fn candidate_file_group(
    id: String,
    label: String,
    segment_relative_path: Option<String>,
    rule: &RuleDefinition,
    files: Vec<String>,
    limit_per_group: usize,
    all_candidate_files: &mut BTreeSet<String>,
) -> CandidateFilePreviewGroup {
    let total_files = files.len();
    all_candidate_files.extend(files.iter().cloned());
    let files = files
        .into_iter()
        .take(limit_per_group)
        .map(|relative_path| CandidateFilePreviewFile { relative_path })
        .collect::<Vec<_>>();
    let hidden_files = total_files.saturating_sub(files.len());

    CandidateFilePreviewGroup {
        id,
        label,
        segment_relative_path,
        rule_id: rule.id.to_string(),
        rule_name: rule.name.to_string(),
        language: rule.language,
        total_files,
        hidden_files,
        files,
    }
}

fn path_is_inside_segment(path: &str, segment: &str) -> bool {
    let segment = segment.trim_matches('/');
    if segment.is_empty() || segment == "." {
        return true;
    }
    path == segment || path.starts_with(&format!("{segment}/"))
}

fn display_segment_label(segment: &str) -> String {
    let segment = segment.trim_matches('/');
    if segment.is_empty() || segment == "." {
        "Repository root".to_string()
    } else {
        segment.to_string()
    }
}

fn candidate_target_root(target_path: &Path) -> Result<PathBuf> {
    let target_path = std::fs::canonicalize(target_path)
        .with_context(|| format!("target path does not exist: {}", target_path.display()))?;
    if target_path.is_file() {
        return target_path
            .parent()
            .ok_or_else(|| anyhow!("target file has no parent directory"))
            .map(Path::to_path_buf);
    }
    Ok(target_path)
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

pub(crate) fn selected_model(request: &RunCreateRequest) -> Result<String> {
    let requested_model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model = match requested_model {
        Some(model) => model,
        None => model_provider::default_model_name(),
    };

    if !model_provider::is_supported_model(model) {
        return Err(anyhow!("unsupported local coding model: {model}"));
    }

    Ok(model.to_string())
}

pub(crate) fn prepare_source_request(
    request: &RunCreateRequest,
    language: Language,
) -> Result<(PathPolicy, Vec<String>)> {
    let target_path = request_target_path(request)?;
    let target_path = std::fs::canonicalize(target_path)
        .with_context(|| format!("target path does not exist: {target_path}"))?;
    let target_is_file = target_path.is_file();
    let target_root = if target_is_file {
        target_path
            .parent()
            .ok_or_else(|| anyhow!("target file has no parent directory"))?
            .to_path_buf()
    } else {
        target_path.clone()
    };

    let mut protected = default_protected_paths();
    protected.extend(request.protected_paths.iter().cloned());

    let policy = PathPolicy::new(
        target_root,
        &protected,
        request.test_file_mode.unwrap_or_default(),
    )?;
    let files = if target_is_file {
        if is_source_for_language(&target_path, language)
            && policy.decision_for(&target_path) == PathDecision::Mutable
        {
            vec![target_path.to_string_lossy().to_string()]
        } else {
            Vec::new()
        }
    } else {
        policy
            .mutable_source_files(language)?
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect()
    };

    Ok((policy, files))
}

pub(crate) fn patch_plan_request(
    rule: &RuleDefinition,
    run_request: &RunCreateRequest,
    policy: &PathPolicy,
    files: &[String],
    repair_context: Option<patch_plan::PatchPlanRepairContext>,
) -> Result<PatchPlanModelRequest> {
    let mut stack_contexts = BTreeSet::new();
    let sources = files
        .iter()
        .map(|file| {
            let path = PathBuf::from(file);
            let relative = path
                .strip_prefix(policy.target_root())
                .with_context(|| format!("{} is outside target root", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            if let Some(context) = detect_stack_context(rule.language, &relative, &content) {
                stack_contexts.insert(context);
            }
            Ok(PatchPlanSourceFile {
                relative_path: relative,
                content,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let stack_contexts = if stack_contexts.is_empty() {
        default_stack_contexts(rule.language)
    } else {
        stack_contexts.into_iter().collect()
    };
    let test_file_mode = run_request.test_file_mode.unwrap_or_default();
    let planning_context = planning_context_for(rule, test_file_mode, &stack_contexts);

    Ok(PatchPlanModelRequest {
        rule_id: rule.id.to_string(),
        language: rule.language,
        rule_name: rule.name.to_string(),
        rule_description: rule.description.to_string(),
        target_root: policy.target_root().to_path_buf(),
        files: sources,
        validation_commands: run_request.validation_commands.clone(),
        allowed_writes: rule.allowed_writes,
        planning_context,
        test_file_mode,
        stack_contexts,
        coverage_evidence: Vec::new(),
        repair_context,
    })
}

pub(crate) fn request_target_path(request: &RunCreateRequest) -> Result<&str> {
    request
        .target_path
        .as_deref()
        .ok_or_else(|| anyhow!("run target path was not resolved"))
}

pub(crate) fn validation_root(target_path: &str) -> PathBuf {
    let path = PathBuf::from(target_path);
    if path.is_file() {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        path
    }
}

pub(crate) fn effective_config_for(
    target_path: &Path,
    run_layer: ConfigLayer,
) -> Result<EffectiveConfig> {
    effective_config_for_paths(
        target_path,
        run_layer,
        default_refactor_rules_path(),
        global_config_path(),
    )
}

pub(crate) fn effective_config_for_paths(
    target_path: &Path,
    run_layer: ConfigLayer,
    default_path: Option<PathBuf>,
    global_path: Option<PathBuf>,
) -> Result<EffectiveConfig> {
    let mut config = EffectiveConfig::default();

    if let Some(default_path) = default_path.filter(|path| path.exists()) {
        let default_layer = load_config_file(&default_path)?;
        config.apply_layer(ConfigLayer {
            rules: default_layer.rules,
            ..ConfigLayer::default()
        });
    }

    if let Some(global_path) = global_path.filter(|path| path.exists()) {
        config.apply_layer(load_config_file(&global_path)?);
    }

    if let Some(project_config) = find_project_config(target_path) {
        config.apply_layer(load_config_file(&project_config)?);
    }


    config.apply_layer(run_layer);
    Ok(config)
}

fn resolve_request_target(db: &Database, request: &mut RunCreateRequest) -> Result<()> {
    if let Some(repository_id) = request.repository_id.as_deref() {
        let repository = db
            .get_repository(repository_id)?
            .ok_or_else(|| anyhow!("repository source was not found"))?;
        let relative = request
            .target_relative_path
            .as_deref()
            .ok_or_else(|| anyhow!("target relative path is required with repositoryId"))?;
        let root = std::fs::canonicalize(&repository.root_path)
            .with_context(|| format!("repository path is unavailable: {}", repository.root_path))?;
        let target = resolve_repository_folder(&root, relative)?;
        request.target_path = Some(target.to_string_lossy().to_string());
        request.repository_root_path = Some(root.to_string_lossy().to_string());
        request.target_relative_path = Some(relative_path_from_root(&root, &target));
        return Ok(());
    }

    let target_path = request
        .target_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("targetPath is required"))?;
    request.target_path = Some(target_path.to_string());
    Ok(())
}

fn repository_root_for_request(request: &RunCreateRequest) -> Result<PathBuf> {
    if let Some(root) = request.repository_root_path.as_deref() {
        return std::fs::canonicalize(root)
            .with_context(|| format!("repository path is unavailable: {root}"));
    }
    let target_path = request_target_path(request)?;
    let target_path = std::fs::canonicalize(target_path)
        .with_context(|| format!("target path does not exist: {target_path}"))?;
    let target_dir = if target_path.is_file() {
        target_path
            .parent()
            .ok_or_else(|| anyhow!("target file has no parent directory"))?
            .to_path_buf()
    } else {
        target_path
    };
    Ok(infer_context_root(&target_dir))
}

fn infer_context_root(target_dir: &Path) -> PathBuf {
    let mut current = target_dir;
    loop {
        if current.join(".git").exists()
            || current.join("Cargo.toml").exists()
            || current.join("package.json").exists()
        {
            return current.to_path_buf();
        }
        let Some(parent) = current.parent() else {
            return target_dir.to_path_buf();
        };
        if parent == current {
            return target_dir.to_path_buf();
        }
        current = parent;
    }
}

fn global_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("local-refactor/config.toml"))
}

fn default_refactor_rules_path() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(|workspace_root| workspace_root.join("refactor-rules.toml"))
}

fn detect_stack_context(
    language: Language,
    relative_path: &str,
    content: &str,
) -> Option<StackContext> {
    match language {
        Language::Rust => Some(StackContext::RustBackend),
        Language::TypeScript => {
            if relative_path.ends_with(".tsx") || looks_like_react(content) {
                Some(StackContext::TypeScriptReact)
            } else if relative_path.ends_with(".ts") {
                Some(StackContext::TypeScriptBackend)
            } else {
                None
            }
        }
    }
}

fn default_stack_contexts(language: Language) -> Vec<StackContext> {
    match language {
        Language::Rust => vec![StackContext::RustBackend],
        Language::TypeScript => vec![StackContext::TypeScriptBackend],
    }
}

fn looks_like_react(content: &str) -> bool {
    content.contains("from \"react\"")
        || content.contains("from 'react'")
        || content.contains("react/jsx-runtime")
        || content.contains("React.")
}
