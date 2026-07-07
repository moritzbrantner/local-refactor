# Test Refactoring Rules

Mutable test mode permits colocated test refactoring and missing-coverage test creation, governed by Test Refactoring Rules. Tests are refactorable code, but test changes can mask behavior changes, so the policy allows cleanup only near the behavior being refactored and forbids weakening coverage.

Rejected alternatives:

- Characterization-only test edits, which would block useful cleanup of test setup, fixtures, naming, helpers, assertions, and structure.
- Whole-target test cleanup, which would make review harder and obscure whether the production refactor preserved behavior.
- Model discretion based only on passing validation, which would allow loosened or skipped tests to hide regressions.
