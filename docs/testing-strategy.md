# Testing Strategy

local-refactor uses behavior-matrix coverage. Each behavior should have one clear owning test layer, with higher layers reserved for workflows that only fail when boundaries are connected.

## Layer Ownership

| Layer | Owns |
| --- | --- |
| Rust core unit tests | path policy, rule metadata, rule planning context, rule selection evidence, and deterministic planning inputs |
| Rust service integration tests | Repository Sources, Run intake, Candidate File Preview, persistence, patch journal rollback, validation, cancellation, review, diff, metrics, and provider boundaries |
| TypeScript analyzer fixture tests | deterministic TypeScript Refactoring Rule behavior, diagnostics, invalid inputs, no-edit cases, and rule enablement |
| Web unit tests | pure view helpers and presentational component states |
| Playwright fixture workflows | browser-level UX for configuring Runs, reviewing Candidate File Preview, confirming Runs, browsing history, and inspecting diffs against deterministic fixture APIs |
| Full-stack smoke e2e | one deterministic browser-to-service Run path through a temp Repository Source |
| Storybook | reviewable component states for the web UI, not visual regression |

## Commands

```sh
cargo test -p local-refactor-core
cargo test -p local-refactor-service
bun run --cwd workers/typescript-analyzer test
bun run --cwd apps/web test:unit
bun run --cwd apps/web test:e2e
bun run --cwd apps/web build-storybook
bun run check
```

`bun run check` must remain deterministic and must not require Ollama. Local model checks remain opt-in:

```sh
bun run check:llm
bun run check:local
```

## Choosing A Layer

Use unit tests when behavior is pure, local, and easier to specify without a browser or database.

Use Rust service integration tests when behavior crosses the service boundary, touches SQLite, reads or writes repository files, validates patch plans, applies rollback, or emits Run events.

Use analyzer fixture tests for deterministic TypeScript transformations. Every deterministic rule should have positive, no-edit, invalid, disabled-rule, and diagnostic coverage.

Use web unit tests for React component states and view helpers. Do not mock the full application for every workflow.

Use Playwright fixture workflows when the browser interaction itself matters. Keep those tests deterministic by routing API calls to fixture responses.

Use full-stack e2e sparingly. It should prove that the web app, Rust service, database, and filesystem can complete one deterministic Run without Ollama.

Use Storybook for component state review. Storybook stories are not screenshot baselines, and this repo does not enforce visual regression thresholds.

## Test Refactoring Rules

Test Refactoring Rules apply to test files only when `Test File Mode` is `mutable`. Read-only test mode forbids creating, updating, or refactoring test files.

Mutable test mode allows cleanup of colocated tests for the production behavior or module being refactored. Colocated tests are tests in the same test file, adjacent test module, or nearest test directory that already covers the production file or module. Test cleanup may improve setup, fixtures, naming, helper extraction, assertions, or structure, but it must not weaken coverage.

New tests may be created only when changed behavior has no suitable existing coverage. Place new tests in the owning layer from the matrix above:

- Rust core unit tests for pure core behavior.
- Rust service integration tests for service, persistence, filesystem, validation, rollback, and provider boundaries.
- TypeScript analyzer fixtures for deterministic TypeScript rule behavior.
- Web unit tests for pure UI helpers and component states.
- Playwright fixture workflows for browser interactions.
- Full-stack smoke e2e only for connected deterministic workflows.

Forbidden test refactors:

- Do not add `.skip`, `.only`, or equivalent disabled test markers.
- Do not delete assertions unless equivalent or stronger coverage remains.
- Do not loosen assertions from specific behavior to broad truthiness.
- Do not rewrite snapshots or fixtures in a way that hides behavior changes.
- Do not change validation commands to bypass failing tests.
- Do not perform unrelated target-wide test cleanup.

## Coverage Solidification Runs

Coverage Solidification Runs are the preferred way to add missing behavior coverage before a production Refactoring Run. They use deterministic Coverage Evidence Rules to identify public behavior surfaces, then model-planned Coverage Solidification Rules create or update tests only.

Coverage Solidification Runs use behavior coverage as the standard. Numeric line, branch, or function coverage can be useful external evidence, but it is not the primary goal and is not required by local-refactor.

Coverage Solidification Runs have stricter boundaries than normal mutable-test refactoring:

- Production files are read-only.
- Test files are the only writable files.
- Validation commands are required.
- Every changed test run must record Behavior Claims.
- Rust v1 coverage solidification should prefer integration tests under the nearest crate `tests/` directory rather than adding inline `#[cfg(test)]` modules to production files.
