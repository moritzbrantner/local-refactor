# Rule Policy As Runtime Planning Context

local-refactor vendors and adapts the generated-code refactoring rules from
agent-loop-setup into runtime rule policy owned by `local_refactor_core`.

Model-planned runs build a structured planning context from the selected
`Refactoring Rule`, detected stack, and `Test File Mode`. The Rust service sends
that context to the model with the existing `patch-plan-v1` contract, then
validates the returned patch plan against mutable scope, protected paths, write
mode, and validation checks.

The service does not load rule policy dynamically from
`/home/moenarch/agent-loop-setup`. Runtime behavior must be reproducible from
this repository, and catalog validation keeps the JSON catalog aligned with the
compiled rule definitions.
