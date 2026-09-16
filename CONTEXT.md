# local-refactor

local-refactor is a local semantic formatter/refactoring normalizer for working code. It brings code toward a repository's preferred form while preserving intended behavior, and it deliberately stays between deterministic formatting/linting and a full coding agent.

## Product boundary

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

Feature implementation, bug fixing, migrations, architectural redesign, and open-ended implementation are outside local-refactor's product boundary. When a desired cleanup requires one of those, the system should report the work as out of scope rather than broaden the refactoring task.

Reliable deterministic transformations are preferred. The local model is reserved for bounded semantic refactorings that require limited judgment and still fit a cataloged rule with explicit safety constraints.

## Language

**Refactoring**:
A behavior-preserving transformation that improves structure, readability, or maintainability without intentionally changing runtime behavior.
_Avoid_: Fix, rewrite, migration

**Refactoring Rule**:
A named, versioned rule that explains when a transformation is allowed, discouraged, or forbidden.
_Avoid_: Prompt, instruction

**Rule Policy**:
Machine-readable preservation, structure, test refactoring, and stack-specific constraints derived from a Refactoring Rule before a model-planned run.
_Avoid_: Prompt text, agent note

**Test Refactoring Rule**:
The test-specific part of Rule Policy that constrains when a Run may create, update, or refactor test code. It is not a cataloged Refactoring Rule.
_Avoid_: Testing prompt, test cleanup note

**Planning Profile**:
The rule policy category that shapes model-planned guidance, such as local transformation, local extraction, module split, public contract shape, or documentation only.
_Avoid_: Refactor type, prompt flavor

**Rule Layer**:
One of global, project, or run configuration. Layers merge as global, then project, then run.
_Avoid_: Settings bucket

**Rule Selection Plan**:
A computed, run-ready mapping from Target Folder subtrees to selected Refactoring Rules and selection reasons.
_Avoid_: Ruleset, guess result

**Rule Selection Segment**:
One non-overlapping folder subtree inside a Rule Selection Plan, with the Refactoring Rules that apply to that subtree.
_Avoid_: Child run, mini job

**Candidate File Preview**:
A pre-run summary of source files eligible to be modified by the selected Run settings, grouped by rule and/or Rule Selection Segment. Candidate files are not guaranteed edits.
_Avoid_: Affected files, predicted edits, dry-run diff

**Deterministic Preview**:
A side-effect-free, whole-run preview of concrete edits produced by deterministic Refactoring Rules for the current run settings. A Deterministic Preview contains real diffs, but it is not a Run until applied.
_Avoid_: Candidate File Preview, dry run, predicted edits

**Convention Settings**:
Machine-readable formatter and ordering preferences used by deterministic convention Refactoring Rules. These local settings are not the shared `coding-agent-conventions` engineering policy.
_Avoid_: Shared engineering conventions, style guide

**Convention Layer**:
Project configuration plus local Repository Source override, merged to produce effective Convention Settings for a Run.
_Avoid_: UI settings, style source

**Selection Evidence**:
A recorded reason explaining why a Refactoring Rule was selected, such as config inheritance, source-file shape, language fallback, or folder markers.
_Avoid_: Heuristic log

**Run**:
One bounded refactoring attempt over an explicit target scope, selected rules, model settings, and validation policy.
_Avoid_: Generic coding-agent job, open-ended task

**Mutable Scope**:
The selected file or directory subtree that the refactoring may modify by default.
_Avoid_: Workspace, project

**Read-Only Scope**:
Files outside the mutable scope that the refactoring may inspect but not modify unless explicitly included.
_Avoid_: Context files

**Repository Source**:
A saved local Git repository root that local-refactor can offer as a source for new runs. Repository Sources are user-managed entries and do not imply ownership of the filesystem repository.
_Avoid_: Workspace, project, repo list item

**Target Folder**:
The repository root or subdirectory selected from a Repository Source as the target for a Run. The Target Folder becomes the Run's Mutable Scope unless path policy rules make individual files read-only or protected.
_Avoid_: Path suffix, folder choice

**Protected Path**:
A file or glob that is never writable during a run, even if it is inside mutable scope.
_Avoid_: Ignore path

**Test File Mode**:
A run setting controlling whether detected test files are read-only or mutable.
_Avoid_: Test policy

**Patch Journal**:
The rollback record storing original file content before every write.
_Avoid_: Backup

**Validation Check**:
A required verification step used to provide evidence that a run preserved behavior.
_Avoid_: Best-effort test command

**Coverage Solidification Run**:
A tests-only run that adds or strengthens behavior coverage before a later Refactoring Run. Production code is read-only during this run.
_Avoid_: Test refactor run, coverage fix

**Coverage Evidence Rule**:
A deterministic, side-effect-free rule that identifies behavior surfaces that may need tests. It produces evidence, not code edits.
_Avoid_: Refactoring Rule, validation check

**Coverage Evidence Preview**:
A pre-run summary of behavior surfaces, likely owning test layers, nearby tests, and coverage-gap reasons produced by Coverage Evidence Rules.
_Avoid_: Candidate File Preview, coverage report

**Coverage Solidification Rule**:
A model-planned rule that creates or updates tests during a Coverage Solidification Run using public behavior evidence.
_Avoid_: Test Refactoring Rule, test generation prompt

**Behavior Claim**:
A recorded statement that a specific public behavior is now covered by a specific test in the owning layer.
_Avoid_: Coverage percentage, assertion note

## Purpose

`local-refactor` takes working code and moves it toward the repository's preferred form without intentionally changing behavior. It provides a localhost-only browser workflow for selecting a repository scope, choosing bounded Refactoring Rules, previewing eligible or deterministic edits, applying deterministic or local-model transformations, validating the result, reviewing diffs, and reverting its own writes.

The product is not a generic coding agent. It does not own feature stories, bug repair, migrations, architectural redesign, or open-ended planning/execution. Shared `coding-agent-conventions` and repository-local instructions are consumed as policy inputs rather than copied into local-refactor as authoritative configuration.

## Architecture

- The Rust workspace contains `local-refactor-core` for policy and planning concepts and `local-refactor-service` for HTTP orchestration, persistence, filesystem operations, validation, and rollback.
- The service listens on `127.0.0.1:7373` by default and stores Run history and the Patch Journal in SQLite.
- `workers/typescript-analyzer/` is a Bun TypeScript worker for TypeScript-aware analysis and deterministic transformations.
- `apps/web/` is a React/Vite browser UI served on `127.0.0.1:5173` during development.
- Model-planned runs use a local Ollama provider. Deterministic preview/apply and the normal deterministic verification suite do not require Ollama.

## Important invariants

- Behavior preservation is the hard product invariant. Behavior changes, feature work, bug fixing, migrations, and architecture changes are outside the refactoring boundary.
- Refactorings remain cataloged and bounded; there is no generic free-form coding-agent task contract.
- Prefer deterministic execution whenever a reliable algorithm exists; use the local model only when limited semantic judgment is required.
- Mutable Scope, Read-Only Scope, Protected Paths, and Test File Mode constrain every Run.
- The service records original content in the Patch Journal before each write and reverts its own writes after validation failure.
- Missing, unavailable, or broken required validation cannot be treated as successful skipped validation.
- A Candidate File Preview reports eligibility, not predicted edits. A Deterministic Preview is side-effect free and contains concrete edits.
- Applying a Deterministic Preview recomputes it and verifies its fingerprint before writing.
- Coverage Solidification Runs may write tests only, require validation, and keep production source read-only.
- `bun run check` must remain deterministic and must not require Ollama.

## Current architectural decisions

Accepted decisions are recorded in `docs/adr/`. The index in `docs/adr/README.md` covers the Rust-service/TypeScript-worker boundary, Ollama, SQLite history, rule policy and selection, preview semantics, testing layers, deterministic convention rules, test refactoring, and coverage solidification.

Do not infer rationale that is absent from an ADR. Propose a new ADR for a consequential, difficult-to-reverse decision instead of rewriting history.

## Known constraints

- Supported development tooling is Rust 1.96+ and Bun 1.3+.
- JavaScript package operations use `bun` or `bunx`; this repository does not use npm, npx, pnpm, or Yarn.
- The product is localhost-only. Model-backed checks require a reachable Ollama instance and an installed local model.
- Shared engineering conventions remain authoritative outside this repository; local convention rewrite/format/order settings are product configuration, not a replacement for `coding-agent-conventions`.

## Development commands

```sh
bun install
bun run dev:service
bun run dev:web
```

Run the deterministic repository verification suite with:

```sh
bun run check
```

Useful narrower checks are:

```sh
cargo fmt --all -- --check
cargo test -p local-refactor-core
cargo test -p local-refactor-service
bun run check:catalog
bun run test:benchmarks
bun run --cwd workers/typescript-analyzer test
bun run --cwd apps/web build
bun run --cwd apps/web test:unit
bun run --cwd apps/web build-storybook
bun run --cwd apps/web test:e2e
```

The repository has no separate root lint or formatting script; use the Cargo formatter command above for Rust. The web build runs TypeScript project checking through `tsc -b`. Model-backed verification is opt-in:

```sh
bun run check:llm
bun run check:local
```

## Testing strategy

Tests follow the behavior ownership matrix in `docs/testing-strategy.md`: Rust core unit tests own pure policy, Rust service integration tests own service/persistence/filesystem boundaries, analyzer fixtures own deterministic TypeScript transformations, web unit tests own view behavior, and Playwright owns browser workflows. Use the narrow owning layer while iterating, then run `bun run check` before completion.

## Current work

No feature-specific work is designated in this file. Durable feature intent belongs in GitHub issues or focused specifications under `specs/`; verification evidence belongs with the corresponding change. Keep product work inside the semantic-formatter boundary above.

## Open questions

No repository-wide architectural question is currently recorded here. Keep unresolved feature-specific questions with their specification rather than inventing answers in this file.
