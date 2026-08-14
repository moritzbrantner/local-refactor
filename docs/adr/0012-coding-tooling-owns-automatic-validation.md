# ADR-0012: coding-tooling owns automatic validation execution

## Status

Proposed

## Context

local-refactor currently discovers package and Cargo commands itself, runs them through a shell, and treats an empty command list as successful skipped validation. That duplicates deterministic repository discovery, conflates unavailable tooling with passing validation, and makes a behavior-preserving refactor appear successful without an applicable gate.

The separate coding-tooling repository defines a stable, deterministic JSON process contract for repository inspection and validation capabilities. local-refactor must preserve its own ownership of rule selection, model planning, path policy, patch journaling, rollback, run history, and repair lifecycle.

## Decision

When a run has no explicit legacy validation commands, local-refactor invokes:

```text
coding-tooling check gate:final --root <target> --json
```

The integration is a Rust subprocess adapter, not a source dependency. It validates schema version 1, operation `check`, status, and exit-code consistency.

A `passed` result completes validation. A `failed` result may enter the existing bounded model-repair flow. `unavailable`, `error`, malformed output, and process-start failures fail the run without model repair. All failures retain the existing patch-journal rollback behavior.

Explicit `validationCommands` remain as a compatibility override. The executable defaults to `coding-tooling` and may be overridden with `LOCAL_REFACTOR_CODING_TOOLING_BIN`.

## Alternatives considered

- Keep automatic discovery in local-refactor. Rejected because it duplicates coding-tooling and preserves inconsistent status semantics.
- Link coding-tooling as a Rust library. Rejected because the CLI is implemented in TypeScript/Bun and the process/JSON boundary keeps the repositories independently releasable.
- Silently fall back to guessed commands when coding-tooling is unavailable. Rejected because it changes the validation contract without evidence and can turn an unavailable gate into a false success.
- Run affected-scope checks only. Rejected as the final validation path because affected selection is early feedback, not permission to skip the complete applicable gate.

## Consequences

- coding-tooling becomes a local runtime requirement for automatic validation.
- Repositories need to declare a complete `check` script to expose `gate:final`.
- local-refactor no longer guesses package-manager or Cargo validation commands.
- Existing explicit shell commands continue to work during migration.
- Structured validation evidence is currently persisted as JSON text in the existing validation output; first-class database columns can be added separately.
