# local-refactor

local-refactor is a local autonomous refactoring service. Its language distinguishes behavior-preserving refactoring from more general code generation or cleanup.

## Language

**Refactoring**:
A behavior-preserving transformation that improves structure, readability, or maintainability without intentionally changing runtime behavior.
_Avoid_: Fix, rewrite, migration

**Refactoring Rule**:
A named, versioned rule that explains when a transformation is allowed, discouraged, or forbidden.
_Avoid_: Prompt, instruction

**Rule Layer**:
One of global, project, or run configuration. Layers merge as global, then project, then run.
_Avoid_: Settings bucket

**Run**:
One autonomous refactoring attempt over an explicit target scope, selected rules, model settings, and validation policy.
_Avoid_: Job, task

**Mutable Scope**:
The selected file or directory subtree that the agent may modify by default.
_Avoid_: Workspace, project

**Read-Only Scope**:
Files outside the mutable scope that the agent may inspect but not modify unless explicitly included.
_Avoid_: Context files

**Protected Path**:
A file or glob that is never writable during a run, even if it is inside mutable scope.
_Avoid_: Ignore path

**Test File Mode**:
A run setting controlling whether detected test files are read-only or mutable.
_Avoid_: Test policy

**Patch Journal**:
The rollback record storing original file content before every write.
_Avoid_: Backup

**Validation Check**:
A configured command used to verify that a run preserved behavior.
_Avoid_: Test command

