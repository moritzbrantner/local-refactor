# Task and Checkpoint Documents

Use `tasks/<feature-or-slice>.md` for substantial work that benefits from durable execution checkpoints or autonomous handoff. Create slices from an accepted specification. Do not turn every five-minute edit into a task document.

Keep the file current after each slice: record what changed, verification evidence, failures, review findings, and the next action. A fresh agent should be able to resume without reconstructing a long chat transcript.

## Template

```markdown
# Task: ...

## Source specification

Link to `specs/...`, an ADR, or an authorized tracker issue.

## Goal

## Files/subsystems likely involved

## Preconditions

## Implementation steps

- [ ] ...
- [ ] ...

## Verification

- [ ] relevant test command
- [ ] typecheck or build where applicable
- [ ] lint or format check where available
- [ ] manual or behavioral verification if needed
- [ ] final diff inspected against acceptance criteria

## Checkpoint

- Completed:
- Remaining:
- Last verified:
- Failures or blockers:
- Next action:

## Review result

Pending | PASS | REVISE | BLOCKED

Evidence:

## Notes
```
