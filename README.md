# local-refactor

local-refactor is a localhost-only refactoring service with a browser UI. The current implementation is an MVP of the planned architecture:

- Rust service on `127.0.0.1:7373`
- React/Vite web UI on `127.0.0.1:5173`
- SQLite run history and patch journal
- TypeScript analyzer worker run through Bun
- Ollama-backed local coding model selection at `/api/models`
- deterministic `simplify-conditional` refactor rule

## Requirements

- Rust 1.96+
- Bun 1.3+
- Ollama at `http://127.0.0.1:11434` unless `OLLAMA_BASE_URL` is set. Runs can select `qwen2.5-coder:7b` or `deepseek-coder:6.7b`; the service asks Ollama to download the selected model before each run if it is missing.

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

In the browser UI:

1. Choose a local Git repository root folder as a Repository Source.
2. Select the repository.
3. Select the repository root or a subfolder as the Target Folder.
4. Select the local coding model and configure rules, test file mode, validation commands, and protected paths.
5. Start the run.

By default, files under the selected Target Folder are mutable, tests are read-only, parent/sibling paths are read-only, and common build outputs/lockfiles are protected. Repository Sources are only local-refactor list entries; removing one from the UI does not delete files or existing run history.

The backend still accepts a legacy absolute `targetPath` for direct API callers. Runs created through the browser UI submit `repositoryId` and `targetRelativePath`, and the service stores both that repository context and the resolved absolute target path.

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
