# Testing Layer Strategy

local-refactor now spans a Rust core, Rust HTTP service, TypeScript analyzer worker, and React browser UI. A single test style would either miss important boundary behavior or duplicate too much implementation detail.

We will use behavior-matrix coverage across dedicated layers:

- Rust core unit tests for policy, rules, and rule selection.
- Rust service integration tests for Repository Sources, Runs, persistence, validation, rollback, diff, review, and provider boundaries.
- TypeScript analyzer fixture tests for deterministic Refactoring Rules.
- Vitest and Testing Library for web helpers and presentational component states.
- Playwright fixture workflows for deterministic browser UX.
- One small full-stack smoke path for browser-to-service deterministic Runs.
- Storybook for reviewable component states.

Rejected alternatives:

- Exhaustive private-helper testing. It creates brittle tests without better product confidence.
- Full-stack-only e2e. It is slower, less deterministic, and makes UI state failures harder to diagnose.
- Immediate visual regression. Storybook state coverage is useful first; screenshot baselines can be added later if visual churn becomes costly.
- Immediate numeric coverage thresholds. The first useful milestone is named behavior ownership and stable commands.

Consequences:

- Web presentational components may be extracted from `App` when the extraction is behavior-preserving and improves testability.
- `bun run check` must remain deterministic and must not require Ollama.
- LLM-backed checks stay opt-in through `check:llm` and `check:local`.
