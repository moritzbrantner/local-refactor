# Rust Service With TypeScript Worker

local-refactor uses a Rust service for orchestration, filesystem policy, persistence, validation, and rollback, while a Node worker owns TypeScript-aware analysis and deterministic code transformations. This keeps the safety-critical service small and robust while using the TypeScript ecosystem where it is strongest.

