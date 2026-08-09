# Feature Specifications

Use `specs/<feature-name>.md` for substantial local-first feature design when a GitHub PRD issue has not been explicitly requested. A specification records the problem and observable contract; it is not a disguised implementation plan. Small, obvious edits do not need a specification file.

Before writing a spec, inspect existing behavior and resolve meaningful design branches through grilling. Acceptance criteria should be testable. Prefer statements such as “Given X, when Y occurs, Z is returned and state A remains unchanged” over “should work correctly.”

When a GitHub PRD issue is authoritative, either keep the specification there or make the local file link to it; do not maintain two divergent copies.

## Template

```markdown
# Feature

## Problem

## Goals

## Non-goals

## Relevant existing behavior

## Proposed behavior

## Invariants

## Acceptance criteria

- Given ..., when ..., then ...

## Failure cases

## Compatibility / migration

## Testing strategy

## Open questions
```
