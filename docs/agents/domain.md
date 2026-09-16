# Domain Context

`local-refactor` is a semantic formatter/refactoring normalizer for working code. It moves code toward the repository's preferred form without intentionally changing behavior.

The product boundary is:

```text
formatter / linter
  deterministic, syntax-level
        ↓
local-refactor
  semantic, low-risk, behavior-preserving cleanup
        ↓
full coding agent
  bugs, features, architecture, ambiguous design work
```

This means generic coding-agent lifecycle work, feature implementation, bug fixing, migrations, and architectural redesign are outside the product model. A refactoring that would require one of those should be reported as out of scope rather than executed under a broader prompt.

Prefer deterministic execution whenever a reliable algorithm exists. Use the local model only for bounded semantic refactorings that need limited judgment, with rule-specific policy, constrained context, validated patch plans, journaled writes, and fail-closed validation.

Before changing behavior, read:

- `CONTEXT.md` for product language, domain terms, architecture, and invariants.
- Relevant ADRs in `docs/adr/` for architectural constraints.

Shared `coding-agent-conventions` and repository-local instructions are policy inputs; local-refactor consumes them rather than becoming their source of truth.
