use crate::{
    model_provider, patch_plan, relative_path_from_root, resolve_repository_folder, validation,
    Database, PatchPlanModelRequest, PatchPlanSourceFile, RunCreateRequest,
};
use anyhow::{anyhow, Context, Result};
use local_refactor_core::{
    config::{
        default_protected_paths, find_project_config, load_config_file, ConfigLayer,
        EffectiveConfig,
    },
    path_policy::PathPolicy,
    rules::{planning_context_for, Language, RuleDefinition, StackContext},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(crate) fn normalize_request(
    db: &Database,
    mut request: RunCreateRequest,
) -> Result<RunCreateRequest> {
    resolve_request_target(db, &mut request)?;
    let run_layer = ConfigLayer {
        rules: if request.rules.is_empty() {
            None
        } else {
            Some(request.rules.clone())
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
    };

    let target_path = request_target_path(&request)?;
    let config = effective_config_for(Path::new(target_path), run_layer)?;
    request.rules = config.rules;
    request.protected_paths = config.protected_paths;
    request.validation_commands = config.validation_commands;
    request.test_file_mode = Some(config.test_file_mode);
    request.model = Some(selected_model(&request)?);
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
    let target_root = if target_path.is_file() {
        target_path
            .parent()
            .ok_or_else(|| anyhow!("target file has no parent directory"))?
            .to_path_buf()
    } else {
        target_path
    };

    let mut protected = default_protected_paths();
    protected.extend(request.protected_paths.iter().cloned());

    let policy = PathPolicy::new(
        target_root,
        &protected,
        request.test_file_mode.unwrap_or_default(),
    )?;
    let files = policy
        .mutable_source_files(language)?
        .into_iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();

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

    if config.validation_commands.is_empty() {
        config.validation_commands = validation::detect_commands(target_path);
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
