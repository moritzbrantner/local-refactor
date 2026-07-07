# local-refactor

local-refactor is a local autonomous refactoring service. Its language distinguishes behavior-preserving refactoring from more general code generation or cleanup.

## Language

**Refactoring**:
A behavior-preserving transformation that improves structure, readability, or maintainability without intentionally changing runtime behavior.
_Avoid_: Fix, rewrite, migration

**Refactoring Rule**:
A named, versioned rule that explains when a transformation is allowed, discouraged, or forbidden.
_Avoid_: Prompt, instruction

**Rule Policy**:
Machine-readable preservation, structure, testing, and stack-specific constraints derived from a Refactoring Rule before a model-planned run.
_Avoid_: Prompt text, agent note

**Planning Profile**:
The rule policy category that shapes model-planned guidance, such as local transformation, local extraction, module split, public contract shape, or documentation only.
_Avoid_: Refactor type, prompt flavor

**Rule Layer**:
One of global, project, or run configuration. Layers merge as global, then project, then run.
_Avoid_: Settings bucket

**Rule Selection Plan**:
A computed, run-ready mapping from Target Folder subtrees to selected Refactoring Rules and selection reasons.
_Avoid_: Ruleset, guess result

**Rule Selection Segment**:
One non-overlapping folder subtree inside a Rule Selection Plan, with the Refactoring Rules that apply to that subtree.
_Avoid_: Child run, mini job

**Candidate File Preview**:
A pre-run summary of source files eligible to be modified by the selected Run settings, grouped by rule and/or Rule Selection Segment. Candidate files are not guaranteed edits.
_Avoid_: Affected files, predicted edits, dry-run diff

**Deterministic Preview**:
A side-effect-free, whole-run preview of concrete edits produced by deterministic Refactoring Rules for the current run settings. A Deterministic Preview contains real diffs, but it is not a Run until applied.
_Avoid_: Candidate File Preview, dry run, predicted edits

**Convention Settings**:
Machine-readable formatter and ordering preferences used by deterministic convention Refactoring Rules.
_Avoid_: Style guide, lint preferences

**Convention Layer**:
Project configuration plus local Repository Source override, merged to produce effective Convention Settings for a Run.
_Avoid_: UI settings, style source

**Selection Evidence**:
A recorded reason explaining why a Refactoring Rule was selected, such as config inheritance, source-file shape, language fallback, or folder markers.
_Avoid_: Heuristic log

**Run**:
One autonomous refactoring attempt over an explicit target scope, selected rules, model settings, and validation policy.
_Avoid_: Job, task

**Mutable Scope**:
The selected file or directory subtree that the agent may modify by default.
_Avoid_: Workspace, project

**Read-Only Scope**:
Files outside the mutable scope that the agent may inspect but not modify unless explicitly included.
_Avoid_: Context files

**Repository Source**:
A saved local Git repository root that local-refactor can offer as a source for new runs. Repository Sources are user-managed entries and do not imply ownership of the filesystem repository.
_Avoid_: Workspace, project, repo list item

**Target Folder**:
The repository root or subdirectory selected from a Repository Source as the target for a Run. The Target Folder becomes the Run's Mutable Scope unless path policy rules make individual files read-only or protected.
_Avoid_: Path suffix, folder choice

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
