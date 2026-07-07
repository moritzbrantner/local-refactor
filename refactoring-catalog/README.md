# Refactoring catalog

The catalog records refactoring kinds before they become production behavior.
Catalog entries are intentionally stricter than UI labels: a refactoring kind is
not supported by runs until its status says `run-supported`.

## Statuses

- `cataloged`: documented target, not supported by analyzer or runs.
- `llm-eval-only`: a local model eval can produce a validated plan, but runs do
  not execute it.
- `deterministic-rule`: the analyzer can produce edits for fixture cases.
- `run-supported`: service lifecycle tests prove the rule works through
  `/api/runs`.

## Fixture requirements

Every catalog entry names a `fixtureDirectory`. Run-supported rules must include
a `manifest.json`. Deterministic analyzer rules include concrete fixture files
under:

```text
positive/
no-edit/
invalid/
```

Model-planned run-supported rules may use an empty analyzer manifest because
their behavior is exercised through service lifecycle tests and LLM eval tasks.
Cataloged and LLM-eval-only entries may use placeholder fixture directories so
the promotion path is explicit.

## Planning profiles

Every catalog entry declares a `planningProfile`. The profile is runtime policy,
not just documentation: `local_refactor_core` uses it to build model-planned
guidance for preservation, structure, test refactoring, forbidden actions, and
stack-specific rules.

Current profiles are:

- `local-transformation`: keep edits local to an existing file.
- `local-extraction`: extract cohesive helper behavior with clear inputs and
  outputs.
- `module-split`: split files by current responsibilities while preserving
  public import paths with compatibility shims or re-exports.
- `public-contract-shape`: preserve exported types, schemas, serialized data,
  and other public contract shapes.
- `documentation-only`: add comments without changing runtime code.

`bun run check:catalog` compares catalog metadata with the compiled runtime rule
definitions, including `planningProfile`, safety level, preserved properties,
write mode, type-information needs, and import-graph needs.

## LLM eval requirements

Entries with `llmEvalDirectory` must have a matching task JSON file in that
directory. The task validates a `patch-plan-v1` response and must state required
files, exports, text, forbidden paths, and forbidden text.

During `/api/runs`, model-planned rules validate generated `patch-plan-v1`
responses before writing. Plans may update mutable files and create files inside
the target scope when the catalog allows multi-file writes. Plans may not delete
files, write outside the mutable scope, write protected paths, or write
read-only test files. When test files are mutable, Test Refactoring Rules still
forbid weakened coverage and unrelated target-wide test cleanup.

Documentation rules are model-planned and single-file in this release. They may
add JSDoc or Rust `///` comments, but the patch plan must preserve runtime code,
public API, and typecheck or cargo validation.

## Coverage evidence

Coverage Evidence Rules live in `coverage-evidence-catalog/`, not this
Refactoring catalog. They are deterministic detectors that produce Coverage
Evidence Preview items and do not edit files. Coverage Solidification Rules use
that evidence in tests-only Coverage Solidification Runs.

This separation preserves the project language: a Refactoring Rule performs a
behavior-preserving code transformation, while a Coverage Evidence Rule records
where behavior coverage may need strengthening before refactoring.

## Promotion checklist

```text
[ ] Catalog entry exists
[ ] Planning profile matches the runtime rule definition
[ ] Fixtures exist
[ ] Catalog check passes
[ ] Analyzer positive/no-edit/invalid cases pass, if deterministic
[ ] Service run test passes, if run-supported
[ ] LLM eval passes, if model-planned
[ ] README or catalog docs explain safety boundaries
```
