# local-refactor

local-refactor is a localhost-only semantic formatter/refactoring normalizer that takes working code and moves it toward a repository's preferred form without intentionally changing behavior.

It combines a Rust service, React/Vite browser UI, SQLite run history and patch journal, a Bun TypeScript analyzer worker, deterministic refactoring executors, and bounded local-model refactoring through Ollama.

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

Behavior preservation is the hard invariant. Refactorings remain cataloged and bounded rather than becoming free-form coding-agent tasks. Prefer deterministic execution whenever a reliable algorithm exists; use the local model only for narrow semantic transformations that require limited judgment.

Feature implementation, bug fixing, migrations, architectural redesign, and open-ended implementation are outside local-refactor's product boundary. Shared `coding-agent-conventions` and repository-local instructions are policy inputs; local-refactor consumes them rather than becoming their source of truth.

## Requirements

- Rust 1.96+
- Bun 1.3+
- `coding-tooling` with the schema-v1 tier contract available on `PATH` for automatic repository validation, or `LOCAL_REFACTOR_CODING_TOOLING_BIN` pointing to a compatible executable. Explicit `validationCommands` can be used as a deliberate compatibility override.
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
4. Select the local coding model and configure rules, test file mode, optional validation-command overrides, and protected paths.
5. For deterministic-only TypeScript rule selections, preview the concrete diffs and apply them.
6. For model-planned or mixed rule selections, start the model-backed run.

By default, files under the selected Target Folder are mutable, tests are read-only, parent/sibling paths are read-only, and common build outputs/lockfiles are protected. Repository Sources are only local-refactor list entries; removing one from the UI does not delete files or existing run history.

The backend still accepts a legacy absolute `targetPath` for direct API callers. Runs created through the browser UI submit `repositoryId` and `targetRelativePath`, and the service stores both that repository context and the resolved absolute target path.

Deterministic TypeScript rules can be previewed before they are applied. A
Candidate File Preview shows eligible files only; it does not predict edits. A
Deterministic Preview runs the deterministic analyzer against the current run
settings and returns actual diffs. Applying a Deterministic Preview recomputes
the same preview on the server, verifies its fingerprint, creates a normal Run,
writes through the patch journal, runs validation, and supports the same review
and revert flow as any other Run. This path does not require Ollama or model
availability.

The implemented deterministic TypeScript rules include `simplify-conditional`, which rewrites simple boolean-return conditionals such as:

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

Each write is recorded in SQLite before the file is changed. If required final validation fails, the service reverts its own writes from the patch journal.

Rust support is model-planned. `rust-extract-helper-function` and
`rust-add-documentation-comments` send mutable `.rs` files to the local model
with Rust-specific prompt context. TypeScript documentation work is also
available through `add-documentation-comments`, which adds JSDoc without
changing runtime code.

When the effective `validationCommands` list is empty, final validation is delegated to `coding-tooling` with its current complete tier contract:

```sh
coding-tooling run --tier full --strict --json
```

The command runs from the selected repository/target context and `coding-tooling` owns repository-root discovery, applicable capability discovery, and full-tier composition. `local-refactor` accepts only a schema-v1 `operation: "run"` envelope whose status matches the documented process exit code. `passed` is the only success status. `failed` means executed repository checks failed and may trigger bounded model repair; `unavailable` and `error`, as well as missing binaries, malformed output, or status/exit mismatches, fail closed without asking the model to change code. A valid JSON envelope is stored unchanged as machine-readable `validationOutput` evidence.

Explicit `validationCommands` are a deliberate compatibility override for the automatic tier. Ordinary non-zero check results are treated as code-validation failures, while command-not-found or shell execution failures are treated as non-repairable tooling failures.

Model-planned rules also receive runtime rule policy before the model is asked
for a `patch-plan-v1`. The policy is built from the selected refactoring rule,
detected stack context, and Test File Mode, and it states behavior preservation,
public contract, structure, test refactoring, forbidden-action, and stack-specific
constraints. The model still returns only the strict JSON patch plan; the
service validates that plan before writing. Read-only test mode forbids test
edits; mutable test mode allows colocated test refactoring and missing-coverage
test creation without weakening behavior coverage.

## Solidify Coverage Before Refactoring

Coverage Solidification Runs add or strengthen behavior tests before production
refactoring. They start with a Coverage Evidence Preview, which deterministically
identifies public behavior surfaces that may need tests. A model-planned
Coverage Solidification Run can then create or update test files only, record
Behavior Claims, run validation, and preserve the same patch journal, review,
diff, and revert lifecycle as normal runs.

Coverage solidification uses the same final validation boundary as normal runs: explicit `validationCommands` when configured, otherwise the strict `coding-tooling` full tier. Missing or unavailable validation fails the run instead of being treated as successful skipped validation. Production source files are read-only during coverage solidification.

Preview coverage evidence:

```http
POST /api/coverage/evidence-preview
```

```json
{
  "repositoryId": "saved-repository-id",
  "targetRelativePath": "src",
  "evidenceRules": ["public-entrypoint-without-nearby-test"]
}
```

Start a coverage solidification run:

```http
POST /api/coverage/runs
```

```json
{
  "repositoryId": "saved-repository-id",
  "targetRelativePath": "src",
  "rules": ["characterize-public-entrypoint"],
  "model": "qwen2.5-coder:7b",
  "validationCommands": ["bun test"],
  "coverageEvidence": []
}
```

Omit `validationCommands` in the request when the repository should use automatic `coding-tooling` validation.

## Configuration

Runs can use automatic rule selection. When the web UI target changes, the
service builds a `Rule Selection Plan` from the selected folder, its subfolders,
and parent-folder context. The plan is segmented by semantic folder boundaries
such as package, crate, source, and `refactor-rules.toml` folders. Each segment
stores selected rule IDs and short reasons. Users can accept the automatic plan
or switch to manual global rule selection before starting a run.

Automatic selection is conservative. TypeScript folders fall back to
`simplify-conditional` and `normalize-imports`; Rust folders do not get a
fallback rule unless config supplies one. Documentation rules and multi-file
model-planned rules are never guessed from file contents; they must come from
config or manual selection.

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

Run settings from the web UI override global and project settings. If the effective `validationCommands` list is empty, the service uses the automatic strict `coding-tooling` full tier instead of guessing local package or Cargo commands.

Project convention settings can also define deterministic formatter and ordering
behavior. The browser UI exposes a separate Conventions page for a selected
Repository Source; project config is the reproducible baseline, and local UI
overrides are stored in local-refactor's SQLite database for that Repository
Source. Applied deterministic runs store the effective convention snapshot used
for their preview fingerprint.

These local Convention Settings are product configuration for rewrite/format/order behavior. They are distinct from shared engineering policy such as `coding-agent-conventions`, which remains an external policy input.

```toml
[conventions]
profile = "standard"

[conventions.typescript.formatter]
enabled = true
requireConfig = true

[conventions.typescript.ordering]
imports = true
classMembers = true
memberGroups = ["static-fields", "fields", "constructors", "methods"]
alphabeticalWithinGroups = true

[conventions.rust.formatter]
enabled = true
requireConfig = true

[conventions.rust.ordering]
useItems = true
implMembers = true
memberGroups = ["associated-types", "constants", "constructors", "methods"]
alphabeticalWithinGroups = true
```

Preview the automatic plan:

```http
POST /api/rule-selection/plan
```

```json
{
  "repositoryId": "saved-repository-id",
  "targetRelativePath": "src",
  "testFileMode": "readOnly",
  "protectedPaths": ["src/generated/**"]
}
```

Preview deterministic edits for deterministic-only TypeScript rules:

```http
POST /api/runs/deterministic-preview
```

Apply a reviewed deterministic preview:

```http
POST /api/runs/deterministic-preview/apply
```

Read effective conventions for a Repository Source:

```http
GET /api/repositories/:id/conventions
```

Save a local Repository Source convention override:

```http
PATCH /api/repositories/:id/conventions/local-override
```

Model-planned or mixed rule selections use the existing `/api/runs` flow and
local model selection.

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
The current matrix covers documentation comments, split-file, split-function,
duplicate-block extraction, local rename, pure-helper isolation, type
extraction, and guard clause conversion. A passing run prints:

```text
LLM eval model: qwen2.5-coder:7b
Ollama: reachable at http://127.0.0.1:11434
Model: installed
add-documentation-comments: passed
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
through the patch journal before final validation.

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
