use super::{doc_only::is_documentation_only_rule, PatchPlanModelRequest};
use local_refactor_core::rules::Language;

pub fn build_prompt(request: &PatchPlanModelRequest) -> String {
    if !request.coverage_evidence.is_empty() {
        return build_coverage_prompt(request);
    }

    let files = render_source_files(request);
    let validation_command = request.validation_commands.join(" && ");
    let schema_example = serde_json::json!({
        "summary": "short summary",
        "files": [{
            "path": request.language.example_path(),
            "action": "update",
            "content": request.language.example_content(),
        }],
        "preservedExports": ["ExportName"],
        "validationCommand": validation_command,
    })
    .to_string();

    let mut parts = vec![
        "You are a local refactoring assistant.".to_string(),
        "Return only valid JSON. Do not use Markdown fences outside JSON strings.".to_string(),
        "The response must match patch-plan-v1 exactly:".to_string(),
        schema_example,
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
    let files = render_source_files(request);

    let validation_command = request.validation_commands.join(" && ");
    let schema_example = serde_json::json!({
        "summary": "short summary",
        "files": [{
            "path": "src/example.test.ts",
            "action": "create",
            "content": "complete test file",
        }],
        "behaviorClaims": [{
            "id": "claim-id",
            "evidenceId": "evidence-id",
            "sourcePaths": ["src/example.ts"],
            "publicEntrypoint": "example",
            "behavior": "observable behavior",
            "owningTestLayer": "TypeScript unit test",
            "testPath": "src/example.test.ts",
            "assertionSummary": "asserts observable behavior",
            "existingCoverageReason": "no nearby coverage",
        }],
        "validationCommand": validation_command,
    })
    .to_string();
    let mut parts = vec![
        "You are a local coverage solidification assistant.".to_string(),
        "Return only valid JSON. Do not use Markdown fences outside JSON strings.".to_string(),
        "The response must match coverage-patch-plan-v1 exactly:".to_string(),
        schema_example,
        "Use exactly these top-level keys: summary, files, behaviorClaims, validationCommand."
            .to_string(),
        "If no safe coverage improvement is available, return files: [] and behaviorClaims: []."
            .to_string(),
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
        format_policy_section(
            "Forbidden actions",
            &request.planning_context.forbidden_actions,
        ),
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

fn render_source_files(request: &PatchPlanModelRequest) -> String {
    let code_fence = request.language.code_fence();
    let capacity = request
        .files
        .iter()
        .map(|file| {
            "File: \n```\n\n```\n\n".len()
                + file.relative_path.len()
                + code_fence.len()
                + file.content.len()
        })
        .sum();
    let mut files = String::with_capacity(capacity);
    for (index, file) in request.files.iter().enumerate() {
        if index > 0 {
            files.push_str("\n\n");
        }
        for part in [
            "File: ",
            file.relative_path.as_str(),
            "\n```",
            code_fence,
            "\n",
            file.content.as_str(),
            "\n```",
        ] {
            files.push_str(part);
        }
    }
    files
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
