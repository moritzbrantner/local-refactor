# local-refactor

local-refactor is a localhost-only refactoring service with a browser UI. The current implementation is an MVP of the planned architecture:

- Rust service on `127.0.0.1:7373`
- React/Vite web UI on `127.0.0.1:5173`
- SQLite run history and patch journal
- TypeScript analyzer worker run through Bun
- model-planned TypeScript and Rust refactoring rules
- Ollama-backed local coding model selection at `/api/models`
- deterministic `simplify-conditional` refactor rule

## Requirements

- Rust 1.96+
- Bun 1.3+
- Ollama at `http://127.0.0.1:11434` unless `OLLAMA_BASE_URL` is set. Runs can select `qwen2.5-coder:7b` or `deepseek-coder:6.7b`; the service asks Ollama to download the selected model before each run if it is missing.

## Install

```sh
bun install
```

## Run

Terminal 1:

```sh
bun run dev:service
```

Terminal 2:

```sh
bun run dev:web
```

Open `http://127.0.0.1:5173`.

## Run A Refactor

In the browser UI:

1. Choose a local Git repository root folder as a Repository Source.
2. Select the repository.
3. Select the repository root or a subfolder as the Target Folder.
4. Select the local coding model and configure rules, test file mode, validation commands, and protected paths.
5. Start the run.

By default, files under the selected Target Folder are mutable, tests are read-only, parent/sibling paths are read-only, and common build outputs/lockfiles are protected. Repository Sources are only local-refactor list entries; removing one from the UI does not delete files or existing run history.

The backend still accepts a legacy absolute `targetPath` for direct API callers. Runs created through the browser UI submit `repositoryId` and `targetRelativePath`, and the service stores both that repository context and the resolved absolute target path.

The implemented deterministic TypeScript rule is `simplify-conditional`, which rewrites simple boolean-return conditionals such as:

```ts
if (value) {
  return true;
}
return false;
```

to:

```ts
return value;
```

Each write is recorded in SQLite before the file is changed. If validation commands fail, the service reverts its own writes from the patch journal.

Rust support is model-planned. `rust-extract-helper-function` and
`rust-add-documentation-comments` send mutable `.rs` files to the local model
with Rust-specific prompt context. TypeScript documentation work is also
available through `add-documentation-comments`, which adds JSDoc without
changing runtime code. When no validation commands are configured and the target
is inside a Cargo project, the service detects `Cargo.toml` and runs
`cargo check --all-targets`; if clippy is available it also runs
`cargo clippy --all-targets -- -D warnings`.

Model-planned rules also receive runtime rule policy before the model is asked
for a `patch-plan-v1`. The policy is built from the selected refactoring rule,
detected stack context, and Test File Mode, and it states behavior preservation,
public contract, structure, testing, forbidden-action, and stack-specific
constraints. The model still returns only the strict JSON patch plan; the
service validates that plan before writing.

## Configuration

Global config:

```toml
# $XDG_CONFIG_HOME/local-refactor/config.toml
rules = ["simplify-conditional"]
protectedPaths = ["src/generated/**"]
validationCommands = ["bun test"]
testFileMode = "readOnly"
```

Project config:

```toml
# refactor-rules.toml
rules = ["simplify-conditional"]
protectedPaths = ["src/generated/**"]
validationCommands = ["bun test", "bun run typecheck"]
testFileMode = "readOnly"
```

Run settings from the web UI override global and project settings.

## Check

```sh
bun run check
```

`bun run check` is deterministic and does not require Ollama. It runs the Rust
tests, refactoring catalog validation, TypeScript analyzer tests, web build, and
browser workflow tests.

## Local Verification

Verification is split into fast deterministic checks and opt-in local model
checks:

| Command | Requires Ollama | Purpose |
| --- | --- | --- |
| `bun run check` | No | CI-safe Rust, catalog, analyzer, build, and browser checks. |
| `bun run check:catalog` | No | Validates refactoring catalog metadata, fixtures, public rules, and run-supported gates. |
| `bun run check:llm` | Yes | Runs the TypeScript LLM eval matrix against a real local model. |
| `bun run check:local` | Yes | Runs deterministic checks plus the real local LLM eval matrix. |

```sh
bun run check
bun run check:catalog
bun run check:llm
bun run check:llm -- --task extract-duplicate-block
bun run check:local
```

`bun run check:llm` is opt-in because it requires Ollama and a local model. It
defaults to:

```sh
OLLAMA_BASE_URL=http://127.0.0.1:11434
LOCAL_REFACTOR_LLM_MODEL=qwen2.5-coder:7b
LOCAL_REFACTOR_LLM_TIMEOUT_MS=120000
```

The LLM eval connects to Ollama, checks that the selected model is installed, and
asks it for strict JSON `patch-plan-v1` plans for TypeScript refactoring tasks.
The current matrix covers split-file, split-function, duplicate-block
extraction, local rename, pure-helper isolation, type extraction, and guard
clause conversion. A passing run prints:

```text
LLM eval model: qwen2.5-coder:7b
Ollama: reachable at http://127.0.0.1:11434
Model: installed
convert-nested-if-to-guard-clause: passed
extract-duplicate-block: passed
extract-type-definition: passed
improve-local-name: passed
isolate-side-effect-free-helper: passed
split-file-by-responsibility: passed
split-oversized-function: passed
Result: passed
```

Failures explain whether Ollama is unreachable, the model is missing, generation
timed out, the response was not JSON, a required split file was missing, or a
required export was not preserved.

## Refactoring Catalog

Refactoring kinds are tracked by language in `refactoring-catalog/typescript.json`
and `refactoring-catalog/rust.json`. Current catalog entries are
production-supported through `/api/runs`. The catalog still keeps the promotion
statuses explicit for future rules:

- `cataloged`: documented target, not supported by analyzer or runs.
- `llm-eval-only`: local model eval can produce a validated plan, but runs do
  not execute it.
- `deterministic-rule`: analyzer can produce edits for fixture cases.
- `run-supported`: service lifecycle tests prove the rule works through
  `/api/runs`.

Run-supported rules execute in one of two ways. Narrow, syntax-local TypeScript
rules run through the TypeScript analyzer worker. Broader extraction, multi-file,
documentation, and Rust rules request a local model `patch-plan-v1`, validate
the plan against mutable scope, protected paths, and write mode, then write
through the patch journal before running validation commands.

Catalog metadata is mirrored in the compiled runtime rule definitions. The
catalog check fails if fields such as planning profile, safety level, preserved
properties, allowed writes, type-information needs, or import-graph needs drift
from `local_refactor_core`.

To add a new refactoring kind:

1. Add a catalog entry with a unique kebab-case id.
2. Add or create its fixture directory under the language's catalog or worker fixture path.
3. Add matching runtime metadata, including planning profile and preserved properties.
4. Add an analyzer manifest and fixtures if it is deterministic.
5. Add an LLM eval task under `scripts/llm-evals/<language>` when that language has an eval runner.
6. Add a service run-supported helper assertion before marking it `run-supported`.
7. Run `bun run check` and, for model-planned kinds, `bun run check:llm`.
