# SQLite Run History

local-refactor stores run history, events, validation output, and patch journal entries in SQLite outside the target repository by default. This keeps target repositories clean while preserving enough durable state for web UI review and rollback without depending on git.

