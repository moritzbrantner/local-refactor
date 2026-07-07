# Hybrid Deterministic Convention Rules

Formatting and ordering conventions are deterministic `Refactoring Rule`s, not a separate run type. They use the existing Deterministic Preview, Run, patch journal, validation, review, and revert lifecycle.

Convention Settings merge from project config first and a local Repository Source override second. The project config is the reproducible baseline; the local override is a UI convenience and is stored in local-refactor SQLite state. Applied runs store the effective Convention Settings snapshot that produced their deterministic preview fingerprint.

The convention engine is hybrid. Formatting delegates to established formatter commands and requires project formatter config when configured to do so. TypeScript structural ordering remains in the TypeScript analyzer worker with `ts-morph`. Rust structural ordering is gated by tree-sitter parsing and then applies conservative rewrites only when macros and parse errors are absent.

Alternatives rejected:

- A separate convention run lifecycle, which would duplicate rollback, validation, and review behavior.
- A tree-sitter-only implementation for all languages, which would replace stronger TypeScript-native tooling without improving formatting.
- local-refactor-owned formatter defaults, which would make previews depend on hidden style choices rather than explicit project settings.
