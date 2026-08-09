# Architecture Decision Records

Use a small Architecture Decision Record (ADR) when a decision is consequential, difficult to reverse, likely to be questioned later, or important for a fresh agent to understand. Do not create ADRs for routine implementation choices, and do not retroactively manufacture rationale for existing decisions.

Number new records sequentially using `NNNN-short-title.md`. The next available number is `0012`. Link superseding and superseded records in both directions when practical.

## Existing decisions

- [0001: Rust service with TypeScript worker](0001-rust-service-typescript-worker.md)
- [0002: Ollama default provider](0002-ollama-default-provider.md)
- [0003: SQLite run history](0003-sqlite-run-history.md)
- [0004: Rule policy as runtime planning context](0004-rule-policy-as-runtime-planning-context.md)
- [0005: Segmented rule selection plans](0005-segmented-rule-selection-plans.md)
- [0006: Candidate File Preview uses policy candidates](0006-candidate-file-preview-uses-policy-candidates.md)
- [0007: Testing layer strategy](0007-testing-layer-strategy.md)
- [0008: Stateless Deterministic Preview apply](0008-stateless-deterministic-preview-apply.md)
- [0009: Hybrid deterministic convention rules](0009-hybrid-deterministic-convention-rules.md)
- [0010: Test refactoring rules](0010-test-refactoring-rules.md)
- [0011: Coverage Solidification Runs precede Refactoring Runs](0011-coverage-solidification-runs.md)

Existing ADRs are concise decision statements and may predate the template below. Preserve them as historical records.

## Template

```markdown
# ADR-NNNN: Title

## Status

Proposed | Accepted | Superseded

## Context

What forces or constraints make this decision necessary?

## Decision

What is being decided?

## Alternatives considered

What credible alternatives were considered, and why were they not chosen?

## Consequences

What becomes easier, harder, or constrained as a result?
```
