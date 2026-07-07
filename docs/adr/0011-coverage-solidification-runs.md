# Coverage Solidification Runs Precede Refactoring Runs

local-refactor treats behavior coverage improvement as a first-class tests-only run before normal refactoring. Deterministic Coverage Evidence Rules identify likely gaps without writing files, while model-planned Coverage Solidification Rules add tests through public entrypoints and record Behavior Claims.

This keeps behavior preservation reviewable: tests are strengthened before production structure changes, production code stays read-only during coverage work, and later Refactoring Runs can link back to the Coverage Solidification Run that established characterization coverage.

Rejected alternatives:

- Adding tests in the same run as production refactoring, which makes review harder and can hide behavior changes.
- Numeric coverage as the primary goal, which conflicts with the existing behavior-matrix testing strategy.
- Treating evidence-only detectors as Refactoring Rules, which would blur the project's definition of refactoring.
