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
