# local-refactor

local-refactor is a localhost-only refactoring service with a browser UI. The current implementation is an MVP of the planned architecture:

- Rust service on `127.0.0.1:7373`
- React/Vite web UI on `127.0.0.1:5173`
- SQLite run history and patch journal
- TypeScript analyzer worker run through Bun
- Ollama model-list adapter at `/api/models`
- deterministic `simplify-conditional` refactor rule

## Requirements

- Rust 1.96+
- Bun 1.3+
- Ollama is optional for the current deterministic MVP, but `/api/models` expects Ollama at `http://127.0.0.1:11434` unless `OLLAMA_BASE_URL` is set.

## Install

```sh
bun install
```

## Run

Terminal 1:

```sh
bun run dev:service
```

Terminal 2:

```sh
bun run dev:web
```

Open `http://127.0.0.1:5173`.

## Run A Refactor

Use an absolute target path. By default, files under that target directory are mutable, tests are read-only, parent/sibling paths are read-only, and common build outputs/lockfiles are protected.

The implemented write rule is `simplify-conditional`, which rewrites simple boolean-return conditionals such as:

```ts
if (value) {
  return true;
}
return false;
```

to:

```ts
return value;
```

Each write is recorded in SQLite before the file is changed. If validation commands fail, the service reverts its own writes from the patch journal.

## Configuration

Global config:

```toml
# $XDG_CONFIG_HOME/local-refactor/config.toml
rules = ["simplify-conditional"]
protectedPaths = ["src/generated/**"]
validationCommands = ["bun test"]
testFileMode = "readOnly"
```

Project config:

```toml
# refactor-rules.toml
rules = ["simplify-conditional"]
protectedPaths = ["src/generated/**"]
validationCommands = ["bun test", "bun run typecheck"]
testFileMode = "readOnly"
```

Run settings from the web UI override global and project settings.

## Check

```sh
bun run check
```

