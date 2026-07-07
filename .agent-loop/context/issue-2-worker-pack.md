# Worker Context Pack: issue #2

Repository: moritzbrantner/local-refactor
Parent PRD: #1 https://github.com/moritzbrantner/local-refactor/issues/1
Slice issue: #2 https://github.com/moritzbrantner/local-refactor/issues/2
Concurrency group: unspecified

## Goal

Replace the existing single local deterministic GitHub Actions validation job with a validation-only caller workflow that adopts the `moritzbrantner/reusable-workflows` staged contracts pinned to `workflow-standard-v1.3`. The completed slice should preserve the current effective validation coverage on pull requests and pushes to `master`, split failures by lifecycle stage, add workflow linting, avoid inherited secrets, and leave local-refactor runtime behavior untouched.

## Acceptance Criteria

- [ ] The validation workflow runs on pull requests, pushes to `master`, and manual `workflow_dispatch`.
- [ ] The old single `deterministic` local validation job is removed.
- [ ] The workflow has top-level read-only contents permission and top-level concurrency that cancels superseded runs for the same ref.
- [ ] A workflow lint job runs actionlint on every pull request and every `master` push.
- [ ] Fast, integration, e2e, and benchmark-report validation jobs call `moritzbrantner/reusable-workflows` workflows pinned to `workflow-standard-v1.3`.
- [ ] No reusable workflow call uses a branch ref such as `main`, and no job uses `secrets: inherit`.
- [ ] Every reusable workflow caller job declares explicit job-level permissions.
- [ ] CI still runs Rust tests, strict clippy with warnings denied, refactoring catalog validation, TypeScript analyzer tests, web build, Playwright e2e tests, and benchmark-report tests.
- [ ] Playwright browser dependencies are installed before browser e2e tests, and failure artifacts are uploaded from the app-scoped report and test-result outputs.
- [ ] The e2e validation job waits for fast validation, integration validation, benchmark-report validation,
...[truncated]

## Expected Write Scope

None

## Verification

None

## Required Reading

- AGENTS.md

## Blockers

None

## Recent Relevant Comments

None

## Notes

Keep implementation limited to this slice. Read more files only when the pack and required docs are insufficient.
