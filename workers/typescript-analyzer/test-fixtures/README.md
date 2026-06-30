# TypeScript analyzer fixtures

Each deterministic rule should have its own directory. Editing cases use matching
`*.input.ts` and `*.expected.ts` files; no-edit cases only need `*.input.ts`.

Tests copy input files to an OS temp directory before calling the public `plan`
function, so fixtures stay read-only during normal test runs.

Future deterministic rules such as `sort-functions` and `sort-fields` should
follow this layout before they are added to the required harness.
