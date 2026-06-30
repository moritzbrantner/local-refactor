use serde::Serialize;

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub execution_kind: RuleExecutionKind,
    pub allowed_writes: AllowedWrites,
}

pub const INITIAL_RULES: &[RuleDefinition] = &[
    RuleDefinition {
        id: "split-oversized-function",
        name: "Split Oversized Function",
        description: "Detects long functions for later extraction planning.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "extract-duplicate-block",
        name: "Extract Duplicate Block",
        description: "Detects repeated local logic for helper extraction.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "simplify-conditional",
        name: "Simplify Conditional",
        description: "Rewrites simple boolean-return conditionals into direct return expressions.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "improve-local-name",
        name: "Improve Local Name",
        description: "Restricts renames to local symbols that can be proven safe.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "isolate-side-effect-free-helper",
        name: "Isolate Side-Effect-Free Helper",
        description:
            "Moves pure helper logic out of orchestration code inside the mutable boundary.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "split-file-by-responsibility",
        name: "Split File By Responsibility",
        description: "Splits a file into responsibility-focused modules while preserving exports.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::MultiFileWithinTarget,
    },
    RuleDefinition {
        id: "extract-type-definition",
        name: "Extract Type Definition",
        description: "Moves inline object type annotations into named types.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "inline-trivial-helper",
        name: "Inline Trivial Helper",
        description: "Replaces a one-use pure helper with its expression.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "convert-nested-if-to-guard-clause",
        name: "Convert Nested If To Guard Clause",
        description: "Converts nested conditionals to early returns.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "sort-independent-declarations",
        name: "Sort Independent Declarations",
        description: "Reorders independent declarations only when order is provably safe.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "normalize-imports",
        name: "Normalize Imports",
        description: "Groups duplicate imports without changing imported bindings.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "extract-parameter-object",
        name: "Extract Parameter Object",
        description: "Converts repeated parameter lists to a typed parameter object.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::MultiFileWithinTarget,
    },
];

pub fn rule_by_id(id: &str) -> Option<&'static RuleDefinition> {
    INITIAL_RULES.iter().find(|rule| rule.id == id)
}
