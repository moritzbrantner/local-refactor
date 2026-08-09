# local-refactor Agent Instructions

Read `CONTEXT.md` before changing domain behavior. Read relevant ADRs in `docs/adr/` before changing architecture, persistence, provider boundaries, or worker/service responsibilities.

## Agent skills

This repo uses GitHub Issues for agent workflow coordination. See:

- `docs/agents/issue-tracker.md`
- `docs/agents/triage-labels.md`
- `docs/agents/domain.md`
- `docs/agents/planning-workflow.md`

### Planning workflow

Substantial new work should be planned as a PRD instead of implemented directly. Use a GitHub PRD issue when remote issue mutation is explicitly authorized; otherwise use a local specification under `specs/`. See `docs/agents/planning-workflow.md`.

## Agent development workflow

Use `docs/agents/development-workflow.md` for the durable, local-first workflow and the currently installed Codex skill names.

### Challenge before building

For non-trivial features, architectural changes, new subsystems, and substantial refactors, do not immediately implement the user's proposed solution. First inspect the existing architecture, determine the underlying requirement, and challenge the proposal where doing so resolves a real design branch.

When a request says "implement X using Y", treat X as a potential requirement and Y as a proposed implementation unless the surrounding context establishes both. It is acceptable to argue against Y.

Actively look for:

- hidden assumptions and requirements disguised as implementation details;
- simpler designs, unnecessary features, overengineering, and premature or overdue abstractions;
- incorrect boundaries, duplicated responsibilities, unclear state ownership, and avoidable coupling;
- missing invariants, error and partial-failure cases, concurrency hazards, and unsupported performance assumptions;
- security, migration, compatibility, and consistency with the existing architecture;
- symptoms being solved in the wrong layer instead of their underlying cause.

Ask concrete questions whose answers change the design or reduce meaningful uncertainty. Useful questions include:

- What requirement or evidence supports this assumption?
- Why does this responsibility belong here, and who owns the resulting state?
- What invariant must remain true during success, failure, and partial failure?
- Why is this state persisted instead of derived?
- What is the simplest design that satisfies the requirement?
- What happens when two instances act concurrently?
- Which constraints are real, and what would make this design wrong?
- How will we know the feature is complete?

Tell the user when the premise appears wrong, expose trade-offs explicitly, and suggest a simpler design when one exists. Do not manufacture objections when the design is already sound; say so and proceed.

### Planning gate

For substantial work:

1. understand the existing architecture and behavior;
2. grill the user about unresolved design branches;
3. record resolved decisions and relevant context;
4. define scope, non-goals, invariants, and testable acceptance criteria;
5. identify risks, failure cases, compatibility concerns, and open questions;
6. establish the definition of done;
7. produce an implementation plan;
8. implement only after the design is sufficiently resolved.

During an explicit grilling or planning phase, do not modify production code or opportunistically implement the feature. Planning and documentation artifacts may change. Autonomous execution may move through already resolved decisions without repeatedly asking permission, but it must still challenge unsafe or materially flawed assumptions.

Use GitHub PRD issues when the user authorizes issue-tracker updates. Otherwise keep local specifications under `specs/` and execution checkpoints under `tasks/`; remote Git operations and issue mutations require an explicit request.

### Implementation and checkpoints

Implement in small, independently verifiable slices. Prefer `RED -> GREEN -> REFACTOR -> VERIFY` where practical. After each meaningful slice, run the relevant tests and applicable type/build checks, inspect the diff, and confirm the slice still satisfies its specification. Do not accumulate a large unverified implementation.

For long-running work, keep this loop visible in repository artifacts:

```text
SPEC -> PLAN -> IMPLEMENT ONE SLICE -> TEST -> INSPECT DIFF -> REVIEW
  ^                                                        |
  |------------------------ REVISE ------------------------|
                           PASS -> CHECKPOINT -> NEXT SLICE
```

A fresh agent should be able to discover what is being built, why, what was decided, what remains, what passed, what failed, and what happens next without relying on a long chat transcript.

### Review and definition of done

Passing tests are necessary but not sufficient. Perform a separate review for acceptance-criteria mismatches, regressions, unnecessary complexity, architectural degradation, missing or brittle tests, duplication, weak naming or boundaries, accidental API changes, security risks, and failure-mode problems. When possible, use a fresh agent, subagent, or context for this review.

Record the review result as `PASS`, `REVISE`, or `BLOCKED`. `PASS` requires evidence.

For non-trivial work, completion normally requires:

- acceptance criteria satisfied;
- relevant tests and builds passing;
- type checking, linting, and formatting checks passing where the repository provides them;
- no unexplained TODOs or known correctness issues;
- necessary documentation updated;
- the final diff reviewed against the specification.

Do not claim completion without verification.

### Local-first Git policy

Work locally by default. Local branches, small intentional commits, and sibling-directory Git worktrees are allowed; never destroy unrelated user changes. Do not push, create pull requests, publish, tag, mutate remote branches, or otherwise change remote state without an explicit user request.
