use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDefinition {
    pub id: &'static str,
    pub language: Language,
    pub name: &'static str,
    pub description: &'static str,
    pub execution_kind: RuleExecutionKind,
    pub allowed_writes: AllowedWrites,
}

pub const INITIAL_RULES: &[RuleDefinition] = &[
    RuleDefinition {
        id: "split-oversized-function",
        language: Language::TypeScript,
        name: "Split Oversized Function",
        description: "Detects long functions for later extraction planning.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "extract-duplicate-block",
        language: Language::TypeScript,
        name: "Extract Duplicate Block",
        description: "Detects repeated local logic for helper extraction.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "simplify-conditional",
        language: Language::TypeScript,
        name: "Simplify Conditional",
        description: "Rewrites simple boolean-return conditionals into direct return expressions.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "improve-local-name",
        language: Language::TypeScript,
        name: "Improve Local Name",
        description: "Restricts renames to local symbols that can be proven safe.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "isolate-side-effect-free-helper",
        language: Language::TypeScript,
        name: "Isolate Side-Effect-Free Helper",
        description:
            "Moves pure helper logic out of orchestration code inside the mutable boundary.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "split-file-by-responsibility",
        language: Language::TypeScript,
        name: "Split File By Responsibility",
        description: "Splits a file into responsibility-focused modules while preserving exports.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::MultiFileWithinTarget,
    },
    RuleDefinition {
        id: "extract-type-definition",
        language: Language::TypeScript,
        name: "Extract Type Definition",
        description: "Moves inline object type annotations into named types.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "inline-trivial-helper",
        language: Language::TypeScript,
        name: "Inline Trivial Helper",
        description: "Replaces a one-use pure helper with its expression.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "convert-nested-if-to-guard-clause",
        language: Language::TypeScript,
        name: "Convert Nested If To Guard Clause",
        description: "Converts nested conditionals to early returns.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "sort-independent-declarations",
        language: Language::TypeScript,
        name: "Sort Independent Declarations",
        description: "Reorders independent declarations only when order is provably safe.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "normalize-imports",
        language: Language::TypeScript,
        name: "Normalize Imports",
        description: "Groups duplicate imports without changing imported bindings.",
        execution_kind: RuleExecutionKind::Deterministic,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "extract-parameter-object",
        language: Language::TypeScript,
        name: "Extract Parameter Object",
        description: "Converts repeated parameter lists to a typed parameter object.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::MultiFileWithinTarget,
    },
    RuleDefinition {
        id: "add-documentation-comments",
        language: Language::TypeScript,
        name: "Add Documentation Comments",
        description:
            "Adds JSDoc comments to exported TypeScript APIs without changing runtime code.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "rust-extract-helper-function",
        language: Language::Rust,
        name: "Extract Rust Helper Function",
        description:
            "Extracts cohesive Rust logic into a private helper while preserving public behavior.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
    RuleDefinition {
        id: "rust-add-documentation-comments",
        language: Language::Rust,
        name: "Add Rust Documentation Comments",
        description: "Adds Rust doc comments to public items without changing compiled behavior.",
        execution_kind: RuleExecutionKind::ModelPlanned,
        allowed_writes: AllowedWrites::SingleFile,
    },
];

pub fn rule_by_id(id: &str) -> Option<&'static RuleDefinition> {
    INITIAL_RULES.iter().find(|rule| rule.id == id)
}
