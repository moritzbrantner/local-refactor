use crate::{
    analyzer::{self, AnalyzerRequest},
    edit_journal::{self, JournaledEdit},
    patch_plan::{self, PatchPlanEdit, PatchPlanRepairContext},
    run_cancellation::RunCancellationToken,
    run_status::RunStatus,
    validation, RunCreateRequest, RunMetrics, ServiceState,
};
use anyhow::{anyhow, Result};
use local_refactor_core::{
    path_policy::PathPolicy,
    rules::{rule_by_id, Language, RuleDefinition, RuleExecutionKind},
};
use std::{error::Error, fmt, sync::Arc, time::Instant};

#[derive(Debug)]
struct RunCancelled;

struct RepairAttempt<'a> {
    model: &'a str,
    rule: &'a RuleDefinition,
    initial_validation_output: String,
}

impl fmt::Display for RunCancelled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("run was cancelled")
    }
}

impl Error for RunCancelled {}

pub(crate) async fn run_job(
    state: ServiceState,
    id: String,
    request: RunCreateRequest,
    cancellation: RunCancellationToken,
) -> Result<()> {
    let started = Instant::now();
    let mut metrics = RunMetrics::default();
    let result = run_job_inner(&state, &id, &request, &cancellation, &mut metrics).await;
    metrics.total_run_ms = Some(elapsed_ms(started));
    if let Err(error) = state.db.set_run_metrics(&id, &metrics) {
        tracing::warn!(run_id = id, error = %error, "failed to persist run metrics");
    }
    state.cancellations.unregister(&id);

    match result {
        Ok(()) => Ok(()),
        Err(error) if error.downcast_ref::<RunCancelled>().is_some() => {
            let _ = edit_journal::revert_patches(&state.db, &id);
            transition(&state, &id, RunStatus::Cancelled, "Run cancelled by user")?;
            Ok(())
        }
        Err(error) => {
            let revert_result = edit_journal::revert_patches(&state.db, &id);
            let error_message = match revert_result {
                Ok(_) => format!("{error:#}"),
                Err(revert_error) => {
                    format!("{error:#}; additionally failed to revert patches: {revert_error:#}")
                }
            };
            state.db.set_error(&id, &error_message)?;
            transition(
                &state,
                &id,
                RunStatus::Failed,
                "Run failed and changes were reverted",
            )?;
            Err(anyhow!(error_message))
        }
    }
}

async fn run_job_inner(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &RunCancellationToken,
    metrics: &mut RunMetrics,
) -> Result<()> {
    let rule_ids = effective_rules(request);
    let rules = rule_ids
        .iter()
        .map(|rule_id| {
            rule_by_id(rule_id).ok_or_else(|| anyhow!("unknown refactoring rule: {rule_id}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let needs_model = rules
        .iter()
        .any(|rule| rule.execution_kind == RuleExecutionKind::ModelPlanned);
    let model = if needs_model {
        Some(selected_model(request)?)
    } else {
        None
    };

    if let Some(model) = model.as_deref() {
        transition(
            state,
            id,
            RunStatus::Preparing,
            &format!("Ensuring local model {model} is downloaded"),
        )?;
        check_cancelled(cancellation)?;
        let progress_state = state.clone();
        let progress_run_id = id.to_string();
        let started = Instant::now();
        let ensure_result = state
            .model_gateway
            .ensure_model_available_with_progress(
                model,
                Arc::new(move |progress| {
                    let _ = append_run_event(&progress_state, &progress_run_id, &progress.message);
                }),
            )
            .await;
        metrics.model_ensure_available_ms = Some(elapsed_ms(started));
        ensure_result?;
        check_cancelled(cancellation)?;
    }

    for rule in &rules {
        execute_rule(
            state,
            id,
            request,
            cancellation,
            metrics,
            model.as_deref(),
            rule,
        )
        .await?;
    }

    let validation_result = run_validation(state, id, request, metrics).await?;
    check_cancelled(cancellation)?;
    if validation_result.success {
        transition(
            state,
            id,
            RunStatus::Succeeded,
            "Run completed successfully",
        )?;
        return Ok(());
    }

    if request.repair_budget > 0 {
        if let (Some(model), Some(repair_rule)) =
            (model.as_deref(), first_model_planned_rule(&rules))
        {
            let repaired = attempt_repairs(
                state,
                id,
                request,
                cancellation,
                metrics,
                RepairAttempt {
                    model,
                    rule: repair_rule,
                    initial_validation_output: validation_result.output.clone(),
                },
            )
            .await?;
            if repaired {
                transition(
                    state,
                    id,
                    RunStatus::Succeeded,
                    "Run completed successfully",
                )?;
                return Ok(());
            }
        }
    }

    append_run_event(state, id, "Validation failed; reverting run changes")?;
    Err(anyhow!(
        "validation failed: {}",
        validation_result.output.trim()
    ))
}

async fn execute_rule(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &RunCancellationToken,
    metrics: &mut RunMetrics,
    model: Option<&str>,
    rule: &RuleDefinition,
) -> Result<()> {
    check_cancelled(cancellation)?;
    transition(
        state,
        id,
        RunStatus::Analyzing,
        &format!("Collecting mutable {} files", rule.language.display_name()),
    )?;
    let started = Instant::now();
    let prepared = prepare_source_request(request, rule.language);
    add_elapsed_ms(&mut metrics.file_collection_ms, started);
    let (policy, files) = prepared?;
    check_cancelled(cancellation)?;

    match rule.execution_kind {
        RuleExecutionKind::Deterministic => {
            if rule.language != Language::TypeScript {
                return Err(anyhow!(
                    "deterministic analyzer rules are only available for TypeScript"
                ));
            }
            transition(
                state,
                id,
                RunStatus::Planning,
                &format!("Running TypeScript analyzer worker for {}", rule.id),
            )?;
            let started = Instant::now();
            let analyzer_response = analyzer::run(
                &state.analyzer_script,
                AnalyzerRequest {
                    files,
                    rules: vec![rule.id.to_string()],
                },
            )
            .await;
            add_elapsed_ms(&mut metrics.analyzer_planning_ms, started);
            let analyzer_response = analyzer_response?;
            check_cancelled(cancellation)?;

            let started = Instant::now();
            let edit_result = apply_analyzer_response(state, id, &policy, analyzer_response);
            add_elapsed_ms(&mut metrics.edit_application_ms, started);
            edit_result?;
        }
        RuleExecutionKind::ModelPlanned => {
            let model = model.ok_or_else(|| anyhow!("model-planned rule requires a model"))?;
            transition(
                state,
                id,
                RunStatus::Planning,
                &format!("Requesting model patch plan for {}", rule.id),
            )?;
            let patch_request = patch_plan_request(rule, request, &policy, &files, None)?;
            let started = Instant::now();
            let model_response = state
                .model_gateway
                .generate_patch_plan(model, patch_request.clone())
                .await;
            add_elapsed_ms(&mut metrics.model_planning_ms, started);
            let model_response = model_response?;
            check_cancelled(cancellation)?;

            let started = Instant::now();
            let edits =
                patch_plan::parse_patch_plan_response(&patch_request, &policy, &model_response);
            add_elapsed_ms(&mut metrics.patch_plan_validation_ms, started);
            let edits = edits?;

            let started = Instant::now();
            let edit_result = apply_patch_plan_response(
                state,
                id,
                &policy,
                edits,
                "Applying model patch-plan edits",
            );
            add_elapsed_ms(&mut metrics.edit_application_ms, started);
            edit_result?;
        }
    }

    Ok(())
}

async fn attempt_repairs(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    cancellation: &RunCancellationToken,
    metrics: &mut RunMetrics,
    repair: RepairAttempt<'_>,
) -> Result<bool> {
    let mut validation_output = repair.initial_validation_output;
    for attempt in 1..=request.repair_budget {
        check_cancelled(cancellation)?;
        transition(
            state,
            id,
            RunStatus::Repairing,
            &format!(
                "Attempting model repair {attempt}/{}",
                request.repair_budget
            ),
        )?;
        let started = Instant::now();
        let prepared = prepare_source_request(request, repair.rule.language);
        add_elapsed_ms(&mut metrics.file_collection_ms, started);
        let (policy, files) = prepared?;
        let patch_request = patch_plan_request(
            repair.rule,
            request,
            &policy,
            &files,
            Some(PatchPlanRepairContext {
                attempt,
                validation_output: truncated_validation_output(&validation_output),
            }),
        )?;
        let started = Instant::now();
        let model_response = state
            .model_gateway
            .generate_patch_plan(repair.model, patch_request.clone())
            .await;
        add_elapsed_ms(&mut metrics.model_planning_ms, started);
        let model_response = model_response?;
        check_cancelled(cancellation)?;

        let started = Instant::now();
        let edits = patch_plan::parse_patch_plan_response(&patch_request, &policy, &model_response);
        add_elapsed_ms(&mut metrics.patch_plan_validation_ms, started);
        let edits = edits?;

        let started = Instant::now();
        let edit_result = apply_patch_plan_response(
            state,
            id,
            &policy,
            edits,
            "Applying model repair patch-plan edits",
        );
        add_elapsed_ms(&mut metrics.edit_application_ms, started);
        edit_result?;

        let validation_result = run_validation(state, id, request, metrics).await?;
        check_cancelled(cancellation)?;
        if validation_result.success {
            return Ok(true);
        }
        validation_output = validation_result.output;
    }

    Err(anyhow!(
        "validation failed after {} repair attempt(s): {}",
        request.repair_budget,
        validation_output.trim()
    ))
}

async fn run_validation(
    state: &ServiceState,
    id: &str,
    request: &RunCreateRequest,
    metrics: &mut RunMetrics,
) -> Result<validation::ValidationResult> {
    transition(
        state,
        id,
        RunStatus::Validating,
        "Running validation checks",
    )?;
    let validation_dir = validation_root(request_target_path(request)?);
    let started = Instant::now();
    let validation_result =
        validation::run_commands(&validation_dir, &request.validation_commands).await;
    add_elapsed_ms(&mut metrics.validation_ms, started);
    let validation_result = validation_result?;
    state
        .db
        .set_validation_output(id, &validation_result.output)?;
    Ok(validation_result)
}

fn apply_analyzer_response(
    state: &ServiceState,
    run_id: &str,
    policy: &PathPolicy,
    response: analyzer::AnalyzerResponse,
) -> Result<()> {
    for diagnostic in response.diagnostics {
        state.db.append_event(run_id, &diagnostic)?;
    }

    if response.edits.is_empty() {
        state
            .db
            .append_event(run_id, "Analyzer produced no edits")?;
        return Ok(());
    }

    transition(
        state,
        run_id,
        RunStatus::Editing,
        "Applying deterministic analyzer edits",
    )?;

    let edits = response
        .edits
        .into_iter()
        .map(JournaledEdit::from)
        .collect::<Vec<_>>();
    edit_journal::apply_edits(&state.db, run_id, policy, edits.clone())?;
    append_applied_events(state, run_id, edits)
}

fn apply_patch_plan_response(
    state: &ServiceState,
    run_id: &str,
    policy: &PathPolicy,
    edits: Vec<PatchPlanEdit>,
    message: &str,
) -> Result<()> {
    if edits.is_empty() {
        state
            .db
            .append_event(run_id, "Patch plan produced no edits")?;
        return Ok(());
    }

    transition(state, run_id, RunStatus::Editing, message)?;

    let edits = edits
        .into_iter()
        .map(JournaledEdit::from)
        .collect::<Vec<_>>();
    edit_journal::apply_edits(&state.db, run_id, policy, edits.clone())?;
    append_applied_events(state, run_id, edits)
}

fn append_applied_events(
    state: &ServiceState,
    run_id: &str,
    edits: Vec<JournaledEdit>,
) -> Result<()> {
    for edit in edits {
        state.db.append_event(
            run_id,
            &format!("Applied {} to {}", edit.rule_id, edit.file_path.display()),
        )?;
    }
    Ok(())
}

fn first_model_planned_rule<'a>(rules: &'a [&'a RuleDefinition]) -> Option<&'a RuleDefinition> {
    rules
        .iter()
        .copied()
        .find(|rule| rule.execution_kind == RuleExecutionKind::ModelPlanned)
}

fn check_cancelled(cancellation: &RunCancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        return Err(anyhow!(RunCancelled));
    }
    Ok(())
}

fn transition(state: &ServiceState, run_id: &str, status: RunStatus, message: &str) -> Result<()> {
    state.db.update_status(run_id, status.as_str())?;
    append_run_event(state, run_id, message)
}

fn append_run_event(state: &ServiceState, run_id: &str, message: &str) -> Result<()> {
    let event = state.db.append_event(run_id, message)?;
    let _ = state.events.send(event);
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn add_elapsed_ms(slot: &mut Option<u64>, started: Instant) {
    let elapsed = elapsed_ms(started);
    *slot = Some(slot.unwrap_or(0).saturating_add(elapsed));
}

fn truncated_validation_output(output: &str) -> String {
    const MAX_VALIDATION_OUTPUT_CHARS: usize = 20_000;
    let char_count = output.chars().count();
    if char_count <= MAX_VALIDATION_OUTPUT_CHARS {
        return output.to_string();
    }
    output
        .chars()
        .skip(char_count - MAX_VALIDATION_OUTPUT_CHARS)
        .collect()
}

use crate::{
    effective_rules, patch_plan_request, prepare_source_request, request_target_path,
    selected_model, validation_root,
};
