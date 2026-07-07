use crate::config::TestFileMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Language {
    #[serde(rename = "typescript")]
    TypeScript,
    #[serde(rename = "rust")]
    Rust,
}

impl Language {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::TypeScript => "TypeScript",
            Self::Rust => "Rust",
        }
    }

    pub fn code_fence(self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::Rust => "rust",
        }
    }

    pub fn example_path(self) -> &'static str {
        match self {
            Self::TypeScript => "src/file.ts",
            Self::Rust => "src/lib.rs",
        }
    }

    pub fn example_content(self) -> &'static str {
        match self {
            Self::TypeScript => "complete TypeScript source",
            Self::Rust => "complete Rust source",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuleExecutionKind {
    Deterministic,
    ModelPlanned,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AllowedWrites {
    SingleFile,
    MultiFileWithinTarget,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuleCategory {
    ControlFlow,
    Naming,
    Extraction,
    Deduplication,
    ModuleOrganization,
    TypeStructure,
    DeclarationOrganization,
    Documentation,
    Formatting,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SafetyLevel {
    SyntaxOnly,
    TypecheckRequired,
    TestRequired,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PreservedProperty {
    RuntimeBehavior,
    Exports,
    PublicApi,
    Typecheck,
    Comments,
    FormattingIntent,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PlanningProfile {
    LocalTransformation,
    LocalExtraction,
    ModuleSplit,
    PublicContractShape,
    DocumentationOnly,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum StackContext {
    RustBackend,
    TypeScriptBackend,
    TypeScriptReact,
}

impl StackContext {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::RustBackend => "Rust backend",
            Self::TypeScriptBackend => "TypeScript backend",
            Self::TypeScriptReact => "TypeScript React",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RulePlanningContext {
    pub workflow_rules: Vec<String>,
    pub preservation_rules: Vec<String>,
    pub structure_rules: Vec<String>,
    pub test_refactoring_rules: Vec<String>,
    pub forbidden_actions: Vec<String>,
    pub stack_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDefinition {
    pub id: &'static str,
    pub language: Language,
    pub name: &'static str,
    pub description: &'static str,
    pub execution_kind: RuleExecutionKind,
    pub allowed_writes: AllowedWrites,
    pub category: RuleCategory,
    pub safety_level: SafetyLevel,
    pub preserves: &'static [PreservedProperty],
    pub requires_type_information: bool,
    pub requires_import_graph: bool,
    pub planning_profile: PlanningProfile,
}

pub const INITIAL_RULES: &[RuleDefinition] = &[
    RuleDefinition {
        id: "format-typescript",
        language: Language::TypeScript,
        name: "Format TypeScript",
        description:
            "Formats TypeScript and JavaScript files using the project's configured formatter.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Formatting,
        safety_level: SafetyLevel::SyntaxOnly,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
            PreservedProperty::FormattingIntent,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "split-oversized-function",
        language: Language::TypeScript,
        name: "Split Oversized Function",
        description: "Detects long functions for later extraction planning.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Extraction,
        safety_level: SafetyLevel::TestRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalExtraction,
    },
    RuleDefinition {
        id: "extract-duplicate-block",
        language: Language::TypeScript,
        name: "Extract Duplicate Block",
        description: "Detects repeated local logic for helper extraction.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Deduplication,
        safety_level: SafetyLevel::TestRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalExtraction,
    },
    RuleDefinition {
        id: "simplify-conditional",
        language: Language::TypeScript,
        name: "Simplify Conditional",
        description: "Rewrites simple boolean-return conditionals into direct return expressions.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::ControlFlow,
        safety_level: SafetyLevel::TestRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "improve-local-name",
        language: Language::TypeScript,
        name: "Improve Local Name",
        description: "Restricts renames to local symbols that can be proven safe.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Naming,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Exports,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: true,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "isolate-side-effect-free-helper",
        language: Language::TypeScript,
        name: "Isolate Side-Effect-Free Helper",
        description:
            "Moves pure helper logic out of orchestration code inside the mutable boundary.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Extraction,
        safety_level: SafetyLevel::TestRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalExtraction,
    },
    RuleDefinition {
        id: "split-file-by-responsibility",
        language: Language::TypeScript,
        name: "Split File By Responsibility",
        description: "Splits a file into responsibility-focused modules while preserving exports.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::MultiFileWithinTarget,
        category: RuleCategory::ModuleOrganization,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Exports,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: false,
        requires_import_graph: true,
        planning_profile: PlanningProfile::ModuleSplit,
    },
    RuleDefinition {
        id: "extract-type-definition",
        language: Language::TypeScript,
        name: "Extract Type Definition",
        description: "Moves inline object type annotations into named types.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::TypeStructure,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
            PreservedProperty::PublicApi,
        ],
        requires_type_information: true,
        requires_import_graph: false,
        planning_profile: PlanningProfile::PublicContractShape,
    },
    RuleDefinition {
        id: "inline-trivial-helper",
        language: Language::TypeScript,
        name: "Inline Trivial Helper",
        description: "Replaces a one-use pure helper with its expression.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Extraction,
        safety_level: SafetyLevel::TestRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "convert-nested-if-to-guard-clause",
        language: Language::TypeScript,
        name: "Convert Nested If To Guard Clause",
        description: "Converts nested conditionals to early returns.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::ControlFlow,
        safety_level: SafetyLevel::TestRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "sort-independent-declarations",
        language: Language::TypeScript,
        name: "Sort Independent Declarations",
        description: "Reorders independent declarations only when order is provably safe.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::DeclarationOrganization,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
            PreservedProperty::FormattingIntent,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "normalize-imports",
        language: Language::TypeScript,
        name: "Normalize Imports",
        description: "Groups duplicate imports without changing imported bindings.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::ModuleOrganization,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
            PreservedProperty::FormattingIntent,
        ],
        requires_type_information: false,
        requires_import_graph: true,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "sort-typescript-class-members",
        language: Language::TypeScript,
        name: "Sort TypeScript Class Members",
        description: "Reorders class members inside conservative safe groups.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::DeclarationOrganization,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
            PreservedProperty::FormattingIntent,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "extract-parameter-object",
        language: Language::TypeScript,
        name: "Extract Parameter Object",
        description: "Converts repeated parameter lists to a typed parameter object.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::MultiFileWithinTarget,
        category: RuleCategory::TypeStructure,
        safety_level: SafetyLevel::TestRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::PublicApi,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: true,
        requires_import_graph: true,
        planning_profile: PlanningProfile::PublicContractShape,
    },
    RuleDefinition {
        id: "add-documentation-comments",
        language: Language::TypeScript,
        name: "Add Documentation Comments",
        description:
            "Adds JSDoc comments to exported TypeScript APIs without changing runtime code.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Documentation,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::PublicApi,
            PreservedProperty::Typecheck,
            PreservedProperty::Comments,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::DocumentationOnly,
    },
    RuleDefinition {
        id: "format-rust",
        language: Language::Rust,
        name: "Format Rust",
        description: "Formats Rust files using the project's configured rustfmt settings.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Formatting,
        safety_level: SafetyLevel::SyntaxOnly,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
            PreservedProperty::FormattingIntent,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "sort-rust-use-items",
        language: Language::Rust,
        name: "Sort Rust Use Items",
        description:
            "Sorts simple contiguous Rust use items when macros and parse errors are absent.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::ModuleOrganization,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
            PreservedProperty::FormattingIntent,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "sort-rust-impl-members",
        language: Language::Rust,
        name: "Sort Rust Impl Members",
        description: "Reorders Rust impl members inside conservative safe groups.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::DeclarationOrganization,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::Typecheck,
            PreservedProperty::FormattingIntent,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalTransformation,
    },
    RuleDefinition {
        id: "rust-extract-helper-function",
        language: Language::Rust,
        name: "Extract Rust Helper Function",
        description:
            "Extracts cohesive Rust logic into a private helper while preserving public behavior.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Extraction,
        safety_level: SafetyLevel::TestRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::PublicApi,
            PreservedProperty::Typecheck,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::LocalExtraction,
    },
    RuleDefinition {
        id: "rust-add-documentation-comments",
        language: Language::Rust,
        name: "Add Rust Documentation Comments",
        description: "Adds Rust doc comments to public items without changing compiled behavior.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
        category: RuleCategory::Documentation,
        safety_level: SafetyLevel::TypecheckRequired,
        preserves: &[
            PreservedProperty::RuntimeBehavior,
            PreservedProperty::PublicApi,
            PreservedProperty::Typecheck,
            PreservedProperty::Comments,
        ],
        requires_type_information: false,
        requires_import_graph: false,
        planning_profile: PlanningProfile::DocumentationOnly,
    },
];

pub fn rule_by_id(id: &str) -> Option<&'static RuleDefinition> {
    INITIAL_RULES.iter().find(|rule| rule.id == id)
}

pub fn planning_context_for(
    rule: &RuleDefinition,
    test_file_mode: TestFileMode,
    stack_contexts: &[StackContext],
) -> RulePlanningContext {
    let mut preservation_rules = vec![
        "Preserve behavior; do not mix feature work with refactoring.".to_string(),
        "Preserve public contracts unless the selected rule explicitly permits a contract-preserving structural change.".to_string(),
        "Preserve external imports and exports by default; use compatibility shims or re-exports for multi-file moves.".to_string(),
    ];
    preservation_rules.extend(
        rule.preserves
            .iter()
            .map(|property| format!("This rule must preserve {}.", property.display_name())),
    );

    let mut structure_rules = vec![
        "Move code before rewriting it.".to_string(),
        "Extract around responsibilities, not arbitrary line counts.".to_string(),
        "Prefer agent-friendly locality unless documented or tooling-enforced repo conventions override it.".to_string(),
        "Keep public interfaces small and intentional.".to_string(),
        "Keep internals private where the language allows.".to_string(),
    ];
    structure_rules.extend(profile_structure_rules(rule.planning_profile));

    let mut forbidden_actions = vec![
        "Do not introduce speculative seams, adapters, traits, or dependency injection.".to_string(),
        "Do not delete code unless compiler feedback, tests, or direct replacement proves it is unused.".to_string(),
    ];
    if rule.allowed_writes == AllowedWrites::MultiFileWithinTarget {
        forbidden_actions.push(
            "Do not delete files; patch-plan-v1 supports create and update only.".to_string(),
        );
    }

    let mut test_refactoring_rules = vec![
        "Prefer characterization through public entrypoints.".to_string(),
        "Use the provided validation commands to reason about behavior preservation.".to_string(),
        "Treat test code as a behavior-preserving refactoring surface, not a place for unrelated feature assertions.".to_string(),
    ];
    match test_file_mode {
        TestFileMode::ReadOnly => test_refactoring_rules.push(
            "Do not create, update, or refactor test files because the run testFileMode is readOnly; rely on validation commands and source-only refactoring."
                .to_string(),
        ),
        TestFileMode::Mutable => {
            test_refactoring_rules.push(
                "You may refactor colocated tests that cover the production behavior or module being refactored."
                    .to_string(),
            );
            test_refactoring_rules.push(
                "You may create tests only when changed behavior lacks suitable existing coverage."
                    .to_string(),
            );
            test_refactoring_rules
                .push("Place new tests in the owning test layer for the behavior.".to_string());
            test_refactoring_rules.push(
                "Do not weaken tests: do not delete coverage, skip cases, loosen assertions, or rewrite snapshots/fixtures unless the resulting check is equivalent or stronger."
                    .to_string(),
            );
            test_refactoring_rules.push(
                "Keep test refactors reviewable and avoid target-wide cleanup unrelated to the refactored behavior."
                    .to_string(),
            );
        }
    }

    RulePlanningContext {
        workflow_rules: vec![
            "Return only raw patch-plan-v1 JSON; do not return a Markdown refactor plan.".to_string(),
            "Keep the patch small enough that the user can review it through the run diff and revert flow.".to_string(),
        ],
        preservation_rules,
        structure_rules,
        test_refactoring_rules,
        forbidden_actions,
        stack_rules: stack_contexts
            .iter()
            .flat_map(|context| stack_rules_for(*context))
            .collect(),
    }
}

impl PreservedProperty {
    fn display_name(self) -> &'static str {
        match self {
            Self::RuntimeBehavior => "runtime behavior",
            Self::Exports => "exports",
            Self::PublicApi => "public API",
            Self::Typecheck => "typecheck",
            Self::Comments => "comments",
            Self::FormattingIntent => "formatting intent",
        }
    }
}

fn profile_structure_rules(profile: PlanningProfile) -> Vec<String> {
    match profile {
        PlanningProfile::LocalTransformation => vec![
            "Keep the transformation local to the existing file and avoid changing module shape."
                .to_string(),
        ],
        PlanningProfile::LocalExtraction => vec![
            "Extract cohesive helper behavior only when the helper has clear inputs and outputs."
                .to_string(),
            "Keep extracted helpers private unless callers already depend on them.".to_string(),
        ],
        PlanningProfile::ModuleSplit => vec![
            "Split files by responsibilities that are visible in the current code.".to_string(),
            "Retain the original public import path with compatibility shims or re-exports when callers could depend on it.".to_string(),
            "Place new files near the code they explain rather than forcing a generic architecture."
                .to_string(),
        ],
        PlanningProfile::PublicContractShape => vec![
            "Treat exported types, schemas, request and response shapes, and serialized data as public contract."
                .to_string(),
            "Introduce named contract types only when they reduce caller-facing complexity.".to_string(),
        ],
        PlanningProfile::DocumentationOnly => vec![
            "Only add or update documentation comments; do not change runtime code.".to_string(),
        ],
    }
}

fn stack_rules_for(context: StackContext) -> Vec<String> {
    match context {
        StackContext::RustBackend => vec![
            "Rust backend: prefer feature or domain modules with a small public interface."
                .to_string(),
            "Rust backend: use private submodules and pub(crate) over pub unless external callers need access."
                .to_string(),
            "Rust backend: add traits only for real variation or meaningful test substitutes."
                .to_string(),
            "Rust backend: preserve serde shapes, error variants, async and cancellation behavior, ownership expectations, and public items."
                .to_string(),
        ],
        StackContext::TypeScriptBackend => vec![
            "TypeScript backend: organize around capabilities and domain responsibilities."
                .to_string(),
            "TypeScript backend: treat route, job, CLI, and framework entrypoints as adapters into deeper modules."
                .to_string(),
            "TypeScript backend: preserve exported functions, types, schemas, request and response shapes, error modes, auth assumptions, transaction assumptions, and ordering constraints."
                .to_string(),
        ],
        StackContext::TypeScriptReact => vec![
            "TypeScript React: keep props as the main UI module interface.".to_string(),
            "TypeScript React: extract hooks only for cohesive reusable state and effects."
                .to_string(),
            "TypeScript React: extract pure helpers for calculations, parsing, formatting, filtering, and sorting."
                .to_string(),
            "TypeScript React: prefer styling via tailwindcss utility classes when styling changes are needed."
                .to_string(),
            "TypeScript React: add browser or API adapters only when duplicated, noisy, or genuinely variable."
                .to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_split_policy_preserves_exports_and_locality_without_deletes() {
        let rule = rule_by_id("split-file-by-responsibility").unwrap();
        let context = planning_context_for(
            rule,
            TestFileMode::ReadOnly,
            &[StackContext::TypeScriptBackend],
        );
        let combined = combined_policy(&context);

        assert!(combined.contains("Preserve external imports and exports"));
        assert!(combined.contains("compatibility shims"));
        assert!(combined.contains("Do not delete files"));
        assert!(combined.contains("agent-friendly locality"));
    }

    #[test]
    fn rust_policy_includes_visibility_and_trait_guidance() {
        let rule = rule_by_id("rust-extract-helper-function").unwrap();
        let context =
            planning_context_for(rule, TestFileMode::ReadOnly, &[StackContext::RustBackend]);
        let combined = combined_policy(&context);

        assert!(combined.contains("pub(crate) over pub"));
        assert!(combined.contains("add traits only for real variation"));
    }

    #[test]
    fn react_policy_prefers_tailwindcss_styling() {
        let rule = rule_by_id("split-file-by-responsibility").unwrap();
        let context = planning_context_for(
            rule,
            TestFileMode::ReadOnly,
            &[StackContext::TypeScriptReact],
        );
        let combined = combined_policy(&context);

        assert!(combined.contains("prefer styling via tailwindcss"));
    }

    #[test]
    fn readonly_test_mode_forbids_test_file_creation_updates_and_refactors() {
        let rule = rule_by_id("split-file-by-responsibility").unwrap();
        let context = planning_context_for(rule, TestFileMode::ReadOnly, &[]);
        let combined = combined_policy(&context);

        assert!(combined.contains("Do not create, update, or refactor test files"));
        assert!(combined.contains("testFileMode is readOnly"));
    }

    #[test]
    fn mutable_test_mode_allows_colocated_test_refactors_without_weakening_coverage() {
        let rule = rule_by_id("split-file-by-responsibility").unwrap();
        let context = planning_context_for(rule, TestFileMode::Mutable, &[]);
        let combined = combined_policy(&context);

        assert!(combined.contains("refactor colocated tests"));
        assert!(combined
            .contains("create tests only when changed behavior lacks suitable existing coverage"));
        assert!(combined.contains("Place new tests in the owning test layer"));
        assert!(combined.contains("Do not weaken tests"));
        assert!(combined.contains("avoid target-wide cleanup unrelated to the refactored behavior"));
    }

    fn combined_policy(context: &RulePlanningContext) -> String {
        [
            context.workflow_rules.as_slice(),
            context.preservation_rules.as_slice(),
            context.structure_rules.as_slice(),
            context.test_refactoring_rules.as_slice(),
            context.forbidden_actions.as_slice(),
            context.stack_rules.as_slice(),
        ]
        .concat()
        .join("\n")
    }
}
