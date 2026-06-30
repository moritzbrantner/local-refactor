# TypeScript analyzer fixtures

Each deterministic analyzer rule has its own directory and a `manifest.json`.
Editing cases use matching `*.input.ts` and `*.expected.ts` files; no-edit and
invalid cases only need `*.input.ts`.

```text
<rule-id>/
  manifest.json
  positive/
    <case>.input.ts
    <case>.expected.ts
  no-edit/
    <case>.input.ts
  invalid/
    <case>.input.ts
```

`manifest.json` uses this shape:

```json
{
  "ruleId": "simplify-conditional",
  "language": "typescript",
  "positiveCases": ["true-false"],
  "noEditCases": ["no-edit"],
  "invalidCases": ["syntax-error"],
  "requiredDiagnostics": ["Analyzed"]
}
```

Tests copy input files to an OS temp directory before calling the public `plan`
function, so fixtures stay read-only during normal test runs.

Model-planned rules still include an analyzer manifest so catalog checks can see
an explicit fixture directory, but those manifests may have empty case arrays.
Their executable behavior is covered by `/api/runs` service tests with fake model
responses and by `scripts/llm-evals/typescript` tasks for real local-model
responses.
