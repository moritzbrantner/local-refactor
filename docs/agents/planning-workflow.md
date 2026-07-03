# Planning Workflow

This repo follows the shared agent-loop planning workflow in `~/.codex/skills/moenarch-setup-agent-loop-skills/planning-workflow.md`.

GitHub Issues are the durable work queue for `moritzbrantner/local-refactor`.

Substantial future work should default to a GitHub PRD issue instead of direct implementation. Tiny one-shot changes may be implemented directly, and explicit user direction to implement directly wins over this default.

PRD issues must be labeled `prd` and `ready-for-agent` only when they include acceptance criteria and out-of-scope boundaries.

Implementation slice issues must include a parent PRD link before they receive `ready-for-agent`:

```markdown
## Parent

#<parent-prd-issue-number>
```

The planning thread should stop after creating the PRD issue unless the user explicitly asks for direct implementation. The planning thread should not create implementation slice issues by default; `moenarch-agent-loop` or a later `moenarch-to-issues` pass handles slicing.
