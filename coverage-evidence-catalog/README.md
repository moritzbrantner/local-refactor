# Coverage Evidence Catalog

Coverage Evidence Rules are deterministic detectors. They produce Coverage Evidence Preview items and never edit files.

Evidence rules are separate from Refactoring Rules because they do not perform behavior-preserving transformations. Coverage Solidification Rules consume this evidence during tests-only Coverage Solidification Runs.

Initial preview-supported rules:

- `public-entrypoint-without-nearby-test`
- `branch-or-error-path-needs-characterization`
- `boundary-input-needs-characterization`
