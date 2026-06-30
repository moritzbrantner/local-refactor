import type { Page, Route } from "@playwright/test";

type RepositoryRecord = {
  id: string;
  label: string;
  rootPath: string;
  available: boolean;
  createdAt: string;
  updatedAt: string;
};

type RunRecord = {
  id: string;
  targetPath: string;
  status: string;
  createdAt: string;
  updatedAt: string;
  rules: string[];
  model: string;
  testFileMode: string;
  validationCommands: string[];
  protectedPaths: string[];
  repositoryId: string;
  repositoryRootPath: string;
  targetRelativePath: string;
};

type FixtureApiOptions = {
  repositories?: RepositoryRecord[];
  runs?: RunRecord[];
  models?: ModelSummary[];
};

type ModelSummary = {
  name: string;
  label: string;
  description: string;
  downloaded: boolean;
};

const now = "2026-06-30T12:00:00.000Z";

export class FixtureApi {
  repositories: RepositoryRecord[];
  runs: RunRecord[];
  models: ModelSummary[];
  lastRunRequest: Record<string, unknown> | null = null;
  revertCalls: string[] = [];

  constructor(options: FixtureApiOptions = {}) {
    this.repositories = options.repositories ?? [];
    this.runs = options.runs ?? [];
    this.models = options.models ?? [readyModel()];
  }

  async install(page: Page) {
    await page.route("**/api/**", async (route) => {
      const request = route.request();
      const url = new URL(request.url());
      const path = url.pathname;
      const method = request.method();

      if (method === "GET" && path === "/api/rules") {
        return fulfillJson(route, {
          rules: [
            {
              id: "simplify-conditional",
              name: "Simplify Conditional",
              description:
                "Rewrites simple boolean-return conditionals into direct return expressions.",
            },
          ],
        });
      }

      if (method === "GET" && path === "/api/models") {
        return fulfillJson(route, {
          provider: "ollama",
          models: this.models,
        });
      }

      if (method === "GET" && path === "/api/repositories") {
        return fulfillJson(route, this.repositories);
      }

      if (method === "POST" && path === "/api/repositories/pick") {
        const repository = defaultRepository();
        if (!this.repositories.some((item) => item.id === repository.id)) {
          this.repositories = [repository, ...this.repositories];
        }
        return fulfillJson(route, { repository });
      }

      const folderMatch = path.match(/^\/api\/repositories\/([^/]+)\/folders$/);
      if (method === "GET" && folderMatch) {
        const repository = this.repositories.find((item) => item.id === folderMatch[1]);
        const relativePath = url.searchParams.get("path") ?? ".";
        return fulfillJson(route, {
          repositoryId: folderMatch[1],
          path: relativePath,
          entries:
            repository && relativePath === "."
              ? [{ name: "src", relativePath: "src" }]
              : [],
        });
      }

      if (method === "GET" && path === "/api/runs") {
        const repositoryId = url.searchParams.get("repositoryId");
        const runs = repositoryId
          ? this.runs.filter((run) => run.repositoryId === repositoryId)
          : this.runs;
        return fulfillJson(route, runs);
      }

      if (method === "POST" && path === "/api/runs") {
        const body = request.postDataJSON() as Record<string, unknown>;
        this.lastRunRequest = body;
        const repository = this.repositories.find(
          (item) => item.id === body.repositoryId,
        ) ?? defaultRepository();
        const run = succeededRun({
          id: "run-created",
          repository,
          targetRelativePath: String(body.targetRelativePath ?? "."),
          rules: body.rules as string[],
          model: String(body.model),
          testFileMode: String(body.testFileMode),
          validationCommands: body.validationCommands as string[],
          protectedPaths: body.protectedPaths as string[],
        });
        this.runs = [run, ...this.runs.filter((item) => item.id !== run.id)];
        return fulfillJson(route, { id: run.id }, 202);
      }

      const reviewMatch = path.match(/^\/api\/runs\/([^/]+)\/review$/);
      if (method === "GET" && reviewMatch) {
        const run = this.runs.find((item) => item.id === reviewMatch[1]);
        if (!run) return route.fulfill({ status: 404 });
        return fulfillJson(route, {
          run,
          events: [
            {
              id: 1,
              runId: reviewMatch[1],
              timestamp: now,
              message: "Run completed successfully",
            },
          ],
          diff: diffResponse(reviewMatch[1]),
        });
      }

      const diffMatch = path.match(/^\/api\/runs\/([^/]+)\/diff$/);
      if (method === "GET" && diffMatch) {
        return fulfillJson(route, diffResponse(diffMatch[1]));
      }

      const eventsMatch = path.match(/^\/api\/runs\/([^/]+)\/events$/);
      if (method === "GET" && eventsMatch) {
        const event = {
          id: 1,
          runId: eventsMatch[1],
          timestamp: now,
          message: "Run completed successfully",
        };
        return route.fulfill({
          status: 200,
          headers: { "content-type": "text/event-stream" },
          body: `event: run-event\ndata: ${JSON.stringify(event)}\n\n`,
        });
      }

      const revertMatch = path.match(/^\/api\/runs\/([^/]+)\/revert$/);
      if (method === "POST" && revertMatch) {
        this.revertCalls.push(revertMatch[1]);
        this.runs = this.runs.map((run) =>
          run.id === revertMatch[1]
            ? { ...run, status: "reverted", updatedAt: now }
            : run,
        );
        return fulfillJson(route, null, 204);
      }

      return route.fulfill({
        status: 404,
        body: `Unhandled fixture route: ${method} ${path}`,
      });
    });
  }
}

export function createFixtureApi(options?: FixtureApiOptions) {
  return new FixtureApi(options);
}

export function defaultRepository(): RepositoryRecord {
  return {
    id: "repo-1",
    label: "Fixture Repo",
    rootPath: "/tmp/local-refactor-fixture",
    available: true,
    createdAt: now,
    updatedAt: now,
  };
}

export function readyModel(): ModelSummary {
  return {
    name: "qwen2.5-coder:7b",
    label: "Qwen2.5 Coder 7B",
    description: "Ready fixture model",
    downloaded: true,
  };
}

export function missingModel(): ModelSummary {
  return {
    name: "deepseek-coder:6.7b",
    label: "DeepSeek Coder 6.7B",
    description: "Missing fixture model",
    downloaded: false,
  };
}

export function succeededRun(options: {
  id?: string;
  repository?: RepositoryRecord;
  targetRelativePath?: string;
  rules?: string[];
  model?: string;
  testFileMode?: string;
  validationCommands?: string[];
  protectedPaths?: string[];
} = {}): RunRecord {
  const repository = options.repository ?? defaultRepository();
  const targetRelativePath = options.targetRelativePath ?? ".";
  return {
    id: options.id ?? "run-1",
    targetPath:
      targetRelativePath === "."
        ? repository.rootPath
        : `${repository.rootPath}/${targetRelativePath}`,
    status: "succeeded",
    createdAt: now,
    updatedAt: now,
    rules: options.rules ?? ["simplify-conditional"],
    model: options.model ?? "qwen2.5-coder:7b",
    testFileMode: options.testFileMode ?? "readOnly",
    validationCommands: options.validationCommands ?? ["bun test"],
    protectedPaths: options.protectedPaths ?? ["src/generated/**"],
    repositoryId: repository.id,
    repositoryRootPath: repository.rootPath,
    targetRelativePath,
  };
}

async function fulfillJson(route: Route, body: unknown, status = 200) {
  await route.fulfill({
    status,
    headers: { "content-type": "application/json" },
    body: body === null ? "" : JSON.stringify(body),
  });
}

function diffResponse(runId: string) {
  return {
    runId,
    files: [
      {
        filePath: "/tmp/local-refactor-fixture/src/sample.ts",
        ruleId: "simplify-conditional",
        summary: "Replaced boolean conditional with direct return.",
        diff:
          "--- /tmp/local-refactor-fixture/src/sample.ts\n+++ /tmp/local-refactor-fixture/src/sample.ts\n export function isReady(value: boolean) {\n-  if (value) {\n+  return value;\n-    return true;\n-  }\n-  return false;\n }\n",
      },
    ],
  };
}
