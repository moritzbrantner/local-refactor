# local-refactor Agent Instructions

Read `CONTEXT.md` before changing product or domain behavior. Read relevant ADRs in `docs/adr/` before changing architecture, persistence, provider boundaries, validation semantics, or worker/service responsibilities.

## Product boundary

`local-refactor` is a semantic formatter/refactoring normalizer for working code. Its hard invariant is behavior preservation.

It sits between two neighboring layers:

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

Do not turn this repository into a generic coding-agent orchestrator. Feature implementation, bug fixing, migrations, architectural redesign, and open-ended repository work are outside the product boundary. When a desired cleanup would require one of those, report it as out of scope instead of disguising it as a refactor.

## Execution boundary

Refactorings must remain cataloged and bounded. Prefer a deterministic executor whenever a reliable algorithm can perform the transformation. Use the local model only for narrow semantic transformations that require limited judgment, and require a validated bounded patch plan before writes.

Shared `coding-agent-conventions` and repository-local instructions are policy inputs. Do not copy their full configuration into this repository or make local-refactor authoritative for shared engineering conventions.

## Safety invariants

- Preserve intended behavior and public contracts unless a rule explicitly narrows a safe structural change.
- Respect Mutable Scope, Read-Only Scope, Protected Paths, and Test File Mode on every run.
- Record original content in the Patch Journal before every write.
- Revert journaled writes when required validation fails.
- Treat missing, unavailable, or broken validation tooling as failure, never as successful skipped validation.
- Keep deterministic preview side-effect free; applying a preview must recompute and verify its fingerprint before writing.
- Coverage Solidification Runs may write tests only and keep production code read-only.

## Change discipline

Implement the smallest coherent slice that preserves the boundaries above. Use the owning test layer while iterating, then run the repository verification gate before completion. Record consequential, difficult-to-reverse architectural decisions in `docs/adr/`; use focused specifications under `specs/` when a change needs acceptance criteria beyond an issue.

Do not add generic worktree, issue-management, planning-agent, or autonomous lifecycle machinery to the product model.

## Commands

```sh
bun install
bun run dev:service
bun run dev:web
bun run check
```

Useful narrower checks:

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

Model-backed verification is opt-in:

```sh
bun run check:llm
bun run check:local
```
