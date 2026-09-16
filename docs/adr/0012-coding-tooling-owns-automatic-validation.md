# ADR 0012: coding-tooling owns automatic validation

Status: Accepted

## Context

`local-refactor` must prove that a behavior-preserving refactor still satisfies the repository's required checks. Historically the service guessed validation commands from nearby `package.json` or `Cargo.toml` files. That duplicates repository/tooling knowledge, drifts from shared conventions, and cannot reliably distinguish a code failure from missing or broken validation infrastructure.

`coding-tooling` is the shared deterministic checks layer. Its current schema-v1 CLI contract exposes semantic capabilities and tier execution. The complete repository validation boundary is `coding-tooling run --tier full --strict --json`.

## Decision

When a run has no explicit `validationCommands`, `local-refactor` delegates final validation to `coding-tooling` by running:

```sh
coding-tooling run --tier full --strict --json
```

from the selected repository/target context. `coding-tooling` resolves the repository root and owns capability discovery, tier composition, required/optional capability policy, command execution, and the JSON result envelope.

`local-refactor` validates the integration boundary before trusting the result:

- `schemaVersion` must be `1`;
- `operation` must be `run`;
- the documented status/exit-code mapping must match exactly: `passed`/0, `failed`/1, `unavailable`/2, `error`/3.

Only `passed` is successful validation. A valid `failed` result means repository code checks ran and failed; for a model-planned refactor this may consume the bounded model-repair budget. `unavailable` and `error` are tooling/environment failures and must not prompt the model to change code. Missing executables, malformed JSON, unsupported schema versions, wrong operations, and status/exit mismatches are also non-repairable validation failures.

Valid `coding-tooling` JSON is stored unchanged in the run's existing `validationOutput`, providing machine-readable validation evidence without introducing a second persistence model. Human-readable integration errors use the same field.

Explicit `validationCommands` remain supported as deliberate repository/user compatibility overrides. They bypass the automatic tier for that run. Shell command-not-found/execute failures are treated as tooling failures rather than model-repairable code failures.

Every unsuccessful validation result still follows the existing patch-journal rollback boundary. Deterministic runs, model-planned runs, and coverage-solidification runs use the same automatic-vs-explicit selection semantics.

## Consequences

- `local-refactor` no longer guesses package-manager or Cargo validation commands.
- Repository validation policy remains authoritative in `coding-tooling` and its repository configuration rather than being copied into this service.
- Automatic runs require a compatible `coding-tooling` executable when no explicit override is supplied; `LOCAL_REFACTOR_CODING_TOOLING_BIN` may point to a specific executable.
- Missing required capabilities fail closed under `--strict` instead of being reported as successful skipped validation.
- Model repair is reserved for executed code-validation failures, not tooling or environment repair.
- The integration is intentionally pinned to the stable schema/status contract rather than implementation-specific check names such as the obsolete `gate:final` capability.
