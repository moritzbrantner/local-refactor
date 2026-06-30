# TypeScript analyzer fixtures

Each deterministic rule should have its own directory and a `manifest.json`.
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

Future deterministic rules should follow this layout before they are promoted
from catalog metadata to analyzer support.
