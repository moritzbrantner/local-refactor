# Local Agent Development Workflow

`AGENTS.md` enforces the methodology even when a skill is not explicitly invoked. Explicit skill invocation is useful when a phase needs the skill's focused procedure.

The preferred T3 Code/Codex flow uses the globally installed Matt Pocock skill names:

```text
$grill-with-docs
  -> resolved decisions and durable context
$to-spec
  -> testable specification (local unless remote issue mutation is authorized)
challenge and review the specification
  -> implementation plan and definition of done
$implement, using $tdd for test-first slices
  -> verification and diff inspection
$code-review in a fresh context when possible
  -> PASS or REVISE, then a durable checkpoint
```

Use `$diagnosing-bugs` for hard bugs or performance regressions and `$improve-codebase-architecture` for architecture scans. The repository bootstrap skill is `$setup-matt-pocock-skills`; this repository's issue-tracker and domain-doc configuration is already present under `docs/agents/`.

The supporting `$grilling`, `$domain-modeling`, and `$codebase-design` skills are also installed because the primary workflow skills invoke or consult them.

`$to-spec` may publish to the configured GitHub issue tracker. Do not allow it or another skill to create or update remote issues unless the user explicitly authorizes that remote mutation. In local-only work, write the equivalent artifact under `specs/`.

## Worktree isolation

Create concurrent agent work in a predictable sibling directory with a local `agent/<name>` branch:

```sh
./scripts/agent-worktree create parser-refactor
./scripts/agent-worktree list
./scripts/agent-worktree remove parser-refactor
```

If the main checkout is `/path/local-refactor`, the first command creates `/path/local-refactor-worktrees/parser-refactor`. Removal refuses dirty worktrees and retains the local branch. The Bun alias is also available:

```sh
bun run agent:worktree -- create parser-refactor
```
