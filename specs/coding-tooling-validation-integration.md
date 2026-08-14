# coding-tooling validation integration

## Requirement

Make local-refactor use coding-tooling for deterministic repository validation without moving refactoring policy, model orchestration, write ownership, or rollback into coding-tooling.

## Scope

- add a typed subprocess adapter for the coding-tooling schema-version-1 JSON envelope;
- use `gate:final` when no explicit validation commands are configured;
- preserve explicit validation commands as a legacy override;
- distinguish product-code failures that may be model-repaired from unavailable or broken tooling;
- retain patch-journal rollback on every validation failure;
- remove local automatic package/Cargo command guessing;
- document installation and the executable override.

## Non-goals

- worktree or agent lifecycle orchestration;
- selecting rules from coding-tooling affected output;
- moving analyzers, model planning, path policy, patch journaling, or run history;
- database migration for structured evidence;
- replacing explicit validation commands in existing stored requests.

## Invariants

1. Empty validation configuration never means success.
2. Only a coding-tooling `passed` envelope with a successful process exit passes the automatic gate.
3. A coding-tooling status/exit-code mismatch is an integration error.
4. Tooling unavailability cannot consume model repair budget.
5. Validation failure still reverts all journaled writes owned by the run.
6. No fallback changes the validation contract silently.

## Acceptance criteria

- a passing `gate:final` envelope succeeds;
- a failed envelope is marked retryable;
- an unavailable envelope is non-retryable;
- a missing executable, malformed JSON, unsupported schema, wrong operation, or exit mismatch fails;
- explicit validation commands still run through the existing legacy path;
- local-refactor itself has no explicit project validation command and therefore dogfoods the automatic final gate;
- README and ADR describe the dependency and responsibility boundary.

## Verification plan

- focused Rust unit tests for failed and unavailable envelopes;
- existing validation tests for empty legacy commands;
- full `bun run check` after coding-tooling PR #2 is installed on the validation environment;
- final diff review against this specification.

## Review status

BLOCKED pending the full repository gate in CI; focused implementation review complete.
