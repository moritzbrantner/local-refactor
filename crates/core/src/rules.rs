use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub const INITIAL_RULES: &[RuleDefinition] = &[
    RuleDefinition {
        id: "split-oversized-function",
        name: "Split Oversized Function",
        description: "Detects long functions for later extraction planning.",
    },
    RuleDefinition {
        id: "extract-duplicate-block",
        name: "Extract Duplicate Block",
        description: "Detects repeated local logic for helper extraction.",
    },
    RuleDefinition {
        id: "simplify-conditional",
        name: "Simplify Conditional",
        description: "Rewrites simple boolean-return conditionals into direct return expressions.",
    },
    RuleDefinition {
        id: "improve-local-name",
        name: "Improve Local Name",
        description: "Restricts renames to local symbols that can be proven safe.",
    },
    RuleDefinition {
        id: "isolate-side-effect-free-helper",
        name: "Isolate Side-Effect-Free Helper",
        description:
            "Moves pure helper logic out of orchestration code inside the mutable boundary.",
    },
];
