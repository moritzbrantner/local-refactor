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

Every catalog entry names a `fixtureDirectory`. Deterministic and run-supported
rules must include a `manifest.json` and fixture files under:

```text
positive/
no-edit/
invalid/
```

Cataloged and LLM-eval-only entries may use placeholder fixture directories so
the promotion path is explicit.

## LLM eval requirements

Entries with `llmEvalDirectory` must have a matching task JSON file in that
directory. The task validates a `patch-plan-v1` response and must state required
files, exports, text, forbidden paths, and forbidden text.

## Promotion checklist

```text
[ ] Catalog entry exists
[ ] Fixtures exist
[ ] Catalog check passes
[ ] Analyzer positive/no-edit/invalid cases pass, if deterministic
[ ] Service run test passes, if run-supported
[ ] LLM eval passes, if model-planned
[ ] README or catalog docs explain safety boundaries
```
