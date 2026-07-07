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
  ruleSelectionPlan?: RuleSelectionPlan | null;
  model: string | null;
  testFileMode: string;
  validationCommands: string[];
  protectedPaths: string[];
  repositoryId: string;
  repositoryRootPath: string;
  targetRelativePath: string;
};

type RuleSelectionPlan = {
  targetRelativePath: string;
  segments: Array<{
    relativePath: string;
    rules: string[];
    reasons: Array<{
      ruleId: string;
      source: "config" | "content" | "fallback";
      message: string;
    }>;
  }>;
};

type FixtureApiOptions = {
  repositories?: RepositoryRecord[];
  runs?: RunRecord[];
  models?: ModelSummary[];
  candidatePreviewFails?: boolean;
  candidatePreview?: CandidateFilePreviewResponse;
};

type ModelSummary = {
  name: string;
  label: string;
  description: string;
  downloaded: boolean;
};

type CandidateFilePreviewResponse = {
  targetRelativePath: string;
  totalCandidateFiles: number;
  limitPerGroup: number;
  groups: Array<{
    id: string;
    label: string;
    segmentRelativePath?: string;
    ruleId: string;
    ruleName: string;
    language: "typescript" | "rust";
    totalFiles: number;
    hiddenFiles: number;
    files: Array<{ relativePath: string }>;
  }>;
};

type DeterministicPreviewResponse = {
  targetRelativePath: string;
  rules: string[];
  previewFingerprint: string;
  files: Array<{
    relativePath: string;
    filePath: string;
    ruleIds: string[];
    summaries: string[];
    originalContentHash: string;
    newContentHash: string;
    diff: string;
  }>;
  diagnostics: string[];
};

const now = "2026-06-30T12:00:00.000Z";

export class FixtureApi {
  repositories: RepositoryRecord[];
  runs: RunRecord[];
  models: ModelSummary[];
  candidatePreviewFails: boolean;
  candidatePreview: CandidateFilePreviewResponse | null;
  lastRunRequest: Record<string, unknown> | null = null;
  revertCalls: string[] = [];

  constructor(options: FixtureApiOptions = {}) {
    this.repositories = options.repositories ?? [];
    this.runs = options.runs ?? [];
    this.models = options.models ?? [readyModel()];
    this.candidatePreviewFails = options.candidatePreviewFails ?? false;
    this.candidatePreview = options.candidatePreview ?? null;
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
              language: "typescript",
              name: "Simplify Conditional",
              description:
                "Rewrites simple boolean-return conditionals into direct return expressions.",
              executionKind: "deterministic",
              allowedWrites: "single-file",
              category: "control-flow",
              safetyLevel: "test-required",
              preserves: ["runtime-behavior", "typecheck"],
              requiresTypeInformation: false,
              requiresImportGraph: false,
              planningProfile: "local-transformation",
            },
            {
              id: "normalize-imports",
              language: "typescript",
              name: "Normalize Imports",
              description: "Sorts and groups local import declarations.",
              executionKind: "deterministic",
              allowedWrites: "single-file",
              category: "declaration-organization",
              safetyLevel: "typecheck-required",
              preserves: ["runtime-behavior", "exports", "typecheck"],
              requiresTypeInformation: false,
              requiresImportGraph: false,
              planningProfile: "local-transformation",
            },
            {
              id: "rust-add-documentation-comments",
              language: "rust",
              name: "Add Rust Documentation Comments",
              description: "Adds Rust doc comments to public items without changing compiled behavior.",
              executionKind: "modelPlanned",
              allowedWrites: "single-file",
              category: "documentation",
              safetyLevel: "typecheck-required",
              preserves: ["runtime-behavior", "public-api", "typecheck", "comments"],
              requiresTypeInformation: false,
              requiresImportGraph: false,
              planningProfile: "documentation-only",
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

      if (method === "POST" && path === "/api/rule-selection/plan") {
        const body = request.postDataJSON() as Record<string, unknown>;
        const targetRelativePath = String(body.targetRelativePath ?? ".");
        return fulfillJson(route, {
          plan: defaultRuleSelectionPlan(targetRelativePath),
          effectiveConfig: {
            rules: ["simplify-conditional"],
            protectedPaths: ["src/generated/**"],
            validationCommands: ["bun test"],
            testFileMode: "readOnly",
          },
        });
      }

      if (method === "POST" && path === "/api/runs/candidate-file-preview") {
        if (this.candidatePreviewFails) {
          return fulfillJson(route, { error: "candidate preview failed" }, 400);
        }
        const body = request.postDataJSON() as Record<string, unknown>;
        const targetRelativePath = String(body.targetRelativePath ?? ".");
        const testFileMode = String(body.testFileMode ?? "readOnly");
        const rules = ((body.rules as string[] | undefined)?.length
          ? (body.rules as string[])
          : ["simplify-conditional"]) as string[];
        return fulfillJson(
          route,
          this.candidatePreview ??
            defaultCandidateFilePreview(targetRelativePath, rules, testFileMode),
        );
      }

      if (method === "POST" && path === "/api/analyze") {
        const body = request.postDataJSON() as Record<string, unknown>;
        const targetPath = String(body.targetPath ?? "");
        const rules = new Set((body.rules as string[] | undefined) ?? []);
        const repository =
          this.repositories.find((item) => targetPath.startsWith(item.rootPath)) ??
          defaultRepository();
        const relativePath = targetPath
          .slice(repository.rootPath.length)
          .replace(/^[/\\]+/, "")
          .replaceAll("\\", "/");
        const originalContent = fixtureFileContent(relativePath);
        const newContent =
          originalContent && rules.has("simplify-conditional")
            ? fixtureSimplifiedContent(relativePath)
            : null;
        return fulfillJson(route, {
          edits:
            originalContent && newContent && originalContent !== newContent
              ? [
                  {
                    filePath: targetPath,
                    originalContent,
                    newContent,
                    ruleId: "simplify-conditional",
                    summary: "Replaced boolean conditional with direct return.",
                  },
                ]
              : [],
          diagnostics: [`Analyzed ${targetPath}`],
        });
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

      const filePreviewMatch = path.match(/^\/api\/repositories\/([^/]+)\/file-preview$/);
      if (method === "GET" && filePreviewMatch) {
        const repository = this.repositories.find((item) => item.id === filePreviewMatch[1]);
        if (!repository) return route.fulfill({ status: 404 });
        const relativePath = url.searchParams.get("path") ?? "";
        const content = fixtureFileContent(relativePath);
        if (content === null) return route.fulfill({ status: 404 });
        return fulfillJson(route, {
          repositoryId: repository.id,
          relativePath,
          language: relativePath.endsWith(".rs") ? "rust" : "typescript",
          content,
          sizeBytes: new TextEncoder().encode(content).length,
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
          rules: ((body.rules as string[] | undefined)?.length
            ? body.rules
            : ["simplify-conditional"]) as string[],
          ruleSelectionPlan: body.ruleSelectionPlan as RuleSelectionPlan | undefined,
          model: String(body.model),
          testFileMode: String(body.testFileMode),
          validationCommands: body.validationCommands as string[],
          protectedPaths: body.protectedPaths as string[],
        });
        this.runs = [run, ...this.runs.filter((item) => item.id !== run.id)];
        return fulfillJson(route, { id: run.id }, 202);
      }

      if (method === "POST" && path === "/api/runs/deterministic-preview") {
        const body = request.postDataJSON() as Record<string, unknown>;
        return fulfillJson(route, deterministicPreviewForRequest(body));
      }

      if (method === "POST" && path === "/api/runs/deterministic-preview/apply") {
        const body = request.postDataJSON() as {
          run: Record<string, unknown>;
          previewFingerprint: string;
        };
        this.lastRunRequest = body.run;
        const repository = this.repositories.find(
          (item) => item.id === body.run.repositoryId,
        ) ?? defaultRepository();
        const run = succeededRun({
          id: "run-created",
          repository,
          targetRelativePath: String(body.run.targetRelativePath ?? "."),
          rules: ((body.run.rules as string[] | undefined)?.length
            ? body.run.rules
            : ["simplify-conditional"]) as string[],
          ruleSelectionPlan: body.run.ruleSelectionPlan as RuleSelectionPlan | undefined,
          model: null,
          testFileMode: String(body.run.testFileMode),
          validationCommands: body.run.validationCommands as string[],
          protectedPaths: body.run.protectedPaths as string[],
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
  ruleSelectionPlan?: RuleSelectionPlan;
  model?: string | null;
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
    ruleSelectionPlan: options.ruleSelectionPlan ?? null,
    model: options.model ?? "qwen2.5-coder:7b",
    testFileMode: options.testFileMode ?? "readOnly",
    validationCommands: options.validationCommands ?? ["bun test"],
    protectedPaths: options.protectedPaths ?? ["src/generated/**"],
    repositoryId: repository.id,
    repositoryRootPath: repository.rootPath,
    targetRelativePath,
  };
}

function defaultRuleSelectionPlan(targetRelativePath: string): RuleSelectionPlan {
  return {
    targetRelativePath,
    segments: [
      {
        relativePath: targetRelativePath === "." ? "src" : targetRelativePath,
        rules: ["simplify-conditional"],
        reasons: [
          {
            ruleId: "simplify-conditional",
            source: "fallback",
            message: "TypeScript source files are present in this segment",
          },
        ],
      },
    ],
  };
}

function defaultCandidateFilePreview(
  targetRelativePath: string,
  rules: string[],
  testFileMode: string,
): CandidateFilePreviewResponse {
  const segment = targetRelativePath === "." ? "src" : targetRelativePath;
  const files = [
    { relativePath: `${segment}/sample.ts` },
    { relativePath: `${segment}/other.ts` },
    ...(testFileMode === "mutable" ? [{ relativePath: `${segment}/sample.test.ts` }] : []),
  ];
  return {
    targetRelativePath,
    totalCandidateFiles: files.length,
    limitPerGroup: 50,
    groups: rules.map((ruleId) => {
      const isRust = ruleId.startsWith("rust-");
      const ruleName = isRust
        ? "Add Rust Documentation Comments"
        : ruleId === "normalize-imports"
          ? "Normalize Imports"
          : "Simplify Conditional";
      return {
        id: `segment:${segment}:rule:${ruleId}`,
        label: `${segment} - ${ruleName}`,
        segmentRelativePath: segment,
        ruleId,
        ruleName,
        language: isRust ? "rust" : "typescript",
        totalFiles: files.length,
        hiddenFiles: 0,
        files,
      };
    }),
  };
}

function deterministicPreviewForRequest(
  body: Record<string, unknown>,
): DeterministicPreviewResponse {
  const rules = ((body.rules as string[] | undefined)?.length
    ? (body.rules as string[])
    : ["simplify-conditional"]) as string[];
  return {
    targetRelativePath: String(body.targetRelativePath ?? "."),
    rules,
    previewFingerprint: "fixture-preview-fingerprint",
    diagnostics: ["Analyzed /tmp/local-refactor-fixture/src/sample.ts"],
    files: [
      {
        relativePath: "src/sample.ts",
        filePath: "/tmp/local-refactor-fixture/src/sample.ts",
        ruleIds: rules,
        summaries: ["Replaced boolean conditional with direct return."],
        originalContentHash: "original-hash",
        newContentHash: "new-hash",
        diff: diffResponse("preview").files[0].diff,
      },
    ],
  };
}

function fixtureFileContent(relativePath: string): string | null {
  if (relativePath.endsWith(".rs")) {
    return [
      "pub fn invoice_total(items: &[(u32, u32)]) -> u32 {",
      "    items.iter().map(|(price, quantity)| price * quantity).sum()",
      "}",
      "",
    ].join("\n");
  }

  const typeScriptFiles: Record<string, string> = {
    "src/sample.ts": [
      "export function isReady(value: boolean) {",
      "  if (value) {",
      "    return true;",
      "  }",
      "  return false;",
      "}",
      "",
    ].join("\n"),
    "src/other.ts": [
      "export const otherValue: string = \"ready\";",
      "",
      "export function labelFor(value: string) {",
      "  return value.trim();",
      "}",
      "",
    ].join("\n"),
    "src/sample.test.ts": [
      "import { expect, test } from \"bun:test\";",
      "import { isReady } from \"./sample\";",
      "",
      "test(\"isReady\", () => {",
      "  expect(isReady(true)).toBe(true);",
      "});",
      "",
    ].join("\n"),
  };

  return typeScriptFiles[relativePath] ?? null;
}

function fixtureSimplifiedContent(relativePath: string): string | null {
  if (relativePath !== "src/sample.ts") return fixtureFileContent(relativePath);
  return [
    "export function isReady(value: boolean) {",
    "  return value;",
    "}",
    "",
  ].join("\n");
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
