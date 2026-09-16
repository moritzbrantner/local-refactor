# coding-tooling validation integration

## Goal

Make repository validation a single fail-closed safety boundary shared by deterministic refactors, local-model refactors, and coverage solidification, while leaving repository check discovery and composition authoritative in `coding-tooling`.

## Scope

- Remove local package/Cargo command guessing from automatic validation.
- Keep explicit `validationCommands` as deliberate repository/user overrides.
- When no explicit override exists, run the current `coding-tooling` complete tier contract: `run --tier full --strict --json`.
- Validate schema version, operation, status, and exit-code consistency before trusting tooling output.
- Distinguish executed code-check failure (`failed`) from missing/broken tooling (`unavailable`/`error` or integration failure).
- Permit bounded model repair only for code-check failures.
- Preserve patch-journal rollback for every unsuccessful final validation.
- Persist a valid coding-tooling schema-v1 envelope as machine-readable validation evidence in the existing `validationOutput` field.
- Apply the same automatic-vs-explicit semantics to coverage-solidification runs.

## Non-goals

- Reimplement capability discovery or tier policy inside `local-refactor`.
- Add a second validation configuration language.
- Teach the local model to repair toolchains, package installation, missing capabilities, or CI environments.
- Depend on implementation-specific historical capabilities such as `gate:final`.
- Introduce a new database schema solely for the coding-tooling envelope; the existing validation evidence field is sufficient for this slice.

## Contract

Automatic validation invokes, from the selected repository/target context:

```sh
coding-tooling run --tier full --strict --json
```

A trusted result must have:

- `schemaVersion: 1`;
- `operation: "run"`;
- one of `passed`, `failed`, `unavailable`, `error`;
- the matching process exit code: 0, 1, 2, or 3 respectively.

Interpretation:

| Status | Meaning for local-refactor | Model repair |
| --- | --- | --- |
| `passed` | validation succeeded | no |
| `failed` | repository code/check execution failed | allowed within configured budget |
| `unavailable` | required capability/tooling is unavailable | no |
| `error` | tooling/config/environment error | no |

Missing binaries, malformed JSON, unsupported schema versions, wrong operations, status/exit-code mismatches, and explicit-command execution failures are fail-closed and non-repairable.

## Acceptance criteria

- Automatic validation uses `coding-tooling run --tier full --strict --json`; no `gate:final` assumptions remain.
- A valid `passed` envelope is the only automatic success path.
- `failed` and tooling/environment failure classes remain distinguishable at the local-refactor boundary.
- Missing required capabilities cannot become successful skipped validation.
- Valid automatic validation envelopes are retained as machine-readable run evidence.
- Explicit validation commands continue to work as an intentional override.
- Deterministic and model-planned runs share the same final validation selection and rollback semantics.
- Coverage solidification can use the same automatic validation path when no explicit override is present.
- Focused validation tests and the repository verification workflow pass on the exact PR head.

## Review checkpoint

Status: IN PROGRESS — implementation restacked onto current `main`; exact-head verification and final review remain.
