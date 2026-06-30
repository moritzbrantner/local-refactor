#!/usr/bin/env bun

import { spawn, spawnSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";

type SuiteName =
  | "fixture-smoke"
  | "medium-repo"
  | "large-repo"
  | "validation-realistic"
  | "model-planned-stress";

type ExecutionKind = "deterministic" | "model-planned";

type TextExpectation = {
  file: string;
  text: string;
};

export type RuleCase = {
  id: string;
  executionKind: ExecutionKind;
  source: string;
  extraFiles?: Record<string, string>;
  requiredChangedFiles: string[];
  requiredTextSnippets: TextExpectation[];
  forbiddenTextSnippets?: TextExpectation[];
  expectedDiffFileCount?: number;
};

type MemorySample = {
  timestampMs: number;
  serviceRssMb: number | null;
  ollamaRssMb: number | null;
  nvidiaTotalVramMb: number | null;
  nvidiaOllamaVramMb: number | null;
};

type RunMetrics = {
  totalRunMs: number | null;
  modelEnsureAvailableMs: number | null;
  fileCollectionMs: number | null;
  analyzerPlanningMs: number | null;
  modelPlanningMs: number | null;
  patchPlanValidationMs: number | null;
  editApplicationMs: number | null;
  validationMs: number | null;
};

type DiffFile = {
  filePath: string;
  ruleId: string;
  summary: string;
  diff: string;
};

type RunReview = {
  events: unknown[];
  diff: { files: DiffFile[] };
  metrics: RunMetrics;
};

type RuleBenchmarkSample = {
  model: string;
  suite: SuiteName;
  phase: "cold" | "warm";
  iteration: number;
  ruleId: string;
  executionKind: ExecutionKind;
  status: string;
  wallMs: number;
  diffFiles: number;
  events: number;
  error?: string;
  metrics: RunMetrics | null;
  servicePeakRssMb: number | null;
  ollamaPeakRssMb: number | null;
  nvidiaPeakTotalVramMb: number | null;
  nvidiaPeakOllamaVramMb: number | null;
  samples: MemorySample[];
};

type StatSummary = {
  min: number;
  median: number;
  p95: number;
  max: number;
  stddev: number;
};

type RuleSummary = {
  model: string;
  suite: SuiteName;
  ruleId: string;
  executionKind: ExecutionKind;
  samples: number;
  succeeded: number;
  failed: number;
  wallMs: StatSummary | null;
  metrics: Record<keyof RunMetrics, StatSummary | null>;
  servicePeakRssMb: number | null;
  ollamaPeakRssMb: number | null;
  nvidiaPeakOllamaVramMb: number | null;
};

type BenchmarkReport = {
  schemaVersion: 1;
  generatedAt: string;
  baseUrl: string;
  suite: SuiteName;
  iterations: number;
  models: string[];
  notes: string[];
  environment: Record<string, string | number | null | boolean>;
  rawSamples: RuleBenchmarkSample[];
  summary: {
    totalSamples: number;
    succeeded: number;
    failed: number;
    coldSamples: number;
    warmSamples: number;
    byRule: RuleSummary[];
    byModel: Array<{
      model: string;
      samples: number;
      succeeded: number;
      failed: number;
      successRate: number;
      medianModelPlanningMs: number | null;
      outputValidityRate: number;
      patchPlanRejectionRate: number;
      peakServiceRssMb: number | null;
      peakOllamaRssMb: number | null;
      peakOllamaVramMb: number | null;
    }>;
  };
};

type CliOptions = {
  suite: SuiteName;
  iterations: number;
  models: string[];
  outputDir: string;
};

const root = resolve(new URL("..", import.meta.url).pathname);
const defaultPort = Number(process.env.LOCAL_REFACTOR_BENCH_PORT ?? "7385");
const baseUrl = `http://127.0.0.1:${defaultPort}`;
const defaultResultsDir = join(root, "benchmark-results");
const defaultModel = process.env.LOCAL_REFACTOR_LLM_MODEL ?? "qwen2.5-coder:7b";
const supportedSuites: SuiteName[] = [
  "fixture-smoke",
  "medium-repo",
  "large-repo",
  "validation-realistic",
  "model-planned-stress",
];

const baseRuleCases: RuleCase[] = [
  {
    id: "simplify-conditional",
    executionKind: "deterministic",
    source:
      "export function isReady(value: boolean) {\n  if (value) {\n    return true;\n  }\n  return false;\n}\n",
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [{ file: "src/sample.ts", text: "return value;" }],
    forbiddenTextSnippets: [{ file: "src/sample.ts", text: "return true;" }],
    expectedDiffFileCount: 1,
  },
  {
    id: "convert-nested-if-to-guard-clause",
    executionKind: "deterministic",
    source:
      'export function accessLabel(user: { active: boolean; admin: boolean } | null) {\n  if (user) {\n    if (user.active) {\n      if (user.admin) {\n        return "admin";\n      }\n      return "member";\n    }\n    return "disabled";\n  }\n  return "guest";\n}\n',
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: 'if (!user) return "guest";' },
    ],
    expectedDiffFileCount: 1,
  },
  {
    id: "extract-type-definition",
    executionKind: "deterministic",
    source:
      "export function renderReport(input: { title: string; total: number }) {\n  return `${input.title}: ${input.total}`;\n}\n",
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: "export type ReportInput" },
    ],
    expectedDiffFileCount: 1,
  },
  {
    id: "inline-trivial-helper",
    executionKind: "deterministic",
    source:
      "function double(value: number) {\n  return value * 2;\n}\n\nexport function score(value: number) {\n  return double(value) + 1;\n}\n",
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: "return (value * 2) + 1;" },
    ],
    forbiddenTextSnippets: [{ file: "src/sample.ts", text: "function double" }],
    expectedDiffFileCount: 1,
  },
  {
    id: "normalize-imports",
    executionKind: "deterministic",
    source:
      'import { beta } from "./tools";\nimport { alpha } from "./tools";\n\nexport function label() {\n  return `${alpha()} ${beta()}`;\n}\n',
    extraFiles: {
      "src/tools.ts":
        'export function alpha() { return "a"; }\nexport function beta() { return "b"; }\n',
    },
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: 'import { alpha, beta } from "./tools";' },
    ],
    expectedDiffFileCount: 1,
  },
  {
    id: "sort-independent-declarations",
    executionKind: "deterministic",
    source:
      'const zebra = "z";\nconst alpha = "a";\nconst middle = "m";\n\nexport function label() {\n  return `${alpha}${middle}${zebra}`;\n}\n',
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: 'const alpha = "a";\nconst middle = "m";\nconst zebra = "z";' },
    ],
    expectedDiffFileCount: 1,
  },
  {
    id: "improve-local-name",
    executionKind: "deterministic",
    source:
      "export function summarizeCart(items: Array<{ price: number }>) {\n  const x = items.length;\n  const y = items.reduce((total, item) => total + item.price, 0);\n  return { itemCount: x, subtotal: y };\n}\n",
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: "const itemCount = items.length;" },
      { file: "src/sample.ts", text: "const subtotal = items.reduce" },
    ],
    expectedDiffFileCount: 1,
  },
  {
    id: "extract-duplicate-block",
    executionKind: "model-planned",
    source:
      "export function sendWelcomeEmail(user: { name: string; email: string }) {\n  const recipient = `${user.name} <${user.email}>`;\n  return `Welcome ${recipient}`;\n}\n\nexport function sendResetEmail(user: { name: string; email: string }) {\n  const recipient = `${user.name} <${user.email}>`;\n  return `Reset ${recipient}`;\n}\n",
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: "function formatRecipient" },
    ],
    expectedDiffFileCount: 1,
  },
  {
    id: "split-oversized-function",
    executionKind: "model-planned",
    source:
      "export function calculateInvoiceTotal(items: Array<{ price: number; quantity: number }>, discountRate: number, taxRate: number) {\n  let subtotal = 0;\n  for (const item of items) {\n    subtotal += item.price * item.quantity;\n  }\n  const discounted = subtotal - subtotal * discountRate;\n  const tax = discounted * taxRate;\n  return discounted + tax;\n}\n",
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [{ file: "src/sample.ts", text: "function subtotal" }],
    expectedDiffFileCount: 1,
  },
  {
    id: "isolate-side-effect-free-helper",
    executionKind: "model-planned",
    source:
      "export function submitOrder(items: Array<{ price: number; quantity: number }>, logger: { info(message: string): void }, saveOrder: (total: number) => void) {\n  let total = 0;\n  for (const item of items) {\n    total += item.price * item.quantity;\n  }\n  logger.info(`saving order ${total}`);\n  saveOrder(total);\n  return total;\n}\n",
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: "function calculateOrderTotal" },
    ],
    expectedDiffFileCount: 1,
  },
  {
    id: "split-file-by-responsibility",
    executionKind: "model-planned",
    source:
      'export type User = { id: string; name: string; email: string };\n\nexport function formatUserLabel(user: User) {\n  return `${user.name} <${user.email}>`;\n}\n\nexport function isValidUser(user: User) {\n  return Boolean(user.id && user.email.includes("@"));\n}\n',
    requiredChangedFiles: [
      "src/sample.ts",
      "src/user.ts",
      "src/user-format.ts",
      "src/user-validation.ts",
    ],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: 'export type { User } from "./user";' },
      { file: "src/user.ts", text: "export type User" },
      { file: "src/user-format.ts", text: "formatUserLabel" },
      { file: "src/user-validation.ts", text: "isValidUser" },
    ],
    expectedDiffFileCount: 4,
  },
  {
    id: "extract-parameter-object",
    executionKind: "model-planned",
    source:
      'export function createUser(name: string, email: string) {\n  return `${name} <${email}>`;\n}\n\nexport function renderUser() {\n  return createUser("Ada", "ada@example.com");\n}\n',
    requiredChangedFiles: ["src/sample.ts"],
    requiredTextSnippets: [
      { file: "src/sample.ts", text: "export type UserParams" },
    ],
    expectedDiffFileCount: 1,
  },
];

async function main() {
  const options = parseCliArgs(process.argv.slice(2));
  mkdirSync(options.outputDir, { recursive: true });
  buildService();

  const workRoot = mkdtempSync(join(tmpdir(), "local-refactor-bench-"));
  const dbPath = join(workRoot, "benchmark.sqlite");
  const service = spawn(serviceBinaryPath(), {
    cwd: root,
    env: {
      ...process.env,
      LOCAL_REFACTOR_PORT: String(defaultPort),
      LOCAL_REFACTOR_DB: dbPath,
      LOCAL_REFACTOR_LLM_TIMEOUT_MS:
        process.env.LOCAL_REFACTOR_LLM_TIMEOUT_MS ?? "180000",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logs: string[] = [];
  service.stdout?.on("data", (chunk) => logs.push(String(chunk)));
  service.stderr?.on("data", (chunk) => logs.push(String(chunk)));

  try {
    await waitForService();
    const rawSamples: RuleBenchmarkSample[] = [];
    for (const model of options.models) {
      console.log(`benchmarking suite=${options.suite} model=${model}`);
      rawSamples.push(
        ...(await benchmarkModel(model, options.suite, options.iterations, workRoot, service.pid ?? null)),
      );
    }

    const report = buildBenchmarkReport({
      suite: options.suite,
      iterations: options.iterations,
      models: options.models,
      rawSamples,
    });
    const artifacts = writeReportArtifacts(report, options.outputDir);
    console.log(`wrote ${artifacts.jsonPath}`);
    console.log(`wrote ${artifacts.markdownPath}`);
    console.log(`wrote ${artifacts.htmlPath}`);

    const correctnessFailures = rawSamples.filter(
      (sample) => sample.status === "correctness-failed",
    );
    if (correctnessFailures.length > 0) {
      throw new Error(
        `${correctnessFailures.length} benchmark sample(s) failed correctness checks`,
      );
    }
  } finally {
    service.kill("SIGTERM");
    await sleep(500);
    if (!service.killed) service.kill("SIGKILL");
    if (process.env.KEEP_LOCAL_REFACTOR_BENCH !== "1") {
      rmSync(workRoot, { recursive: true, force: true });
    }
  }
}

export function parseCliArgs(args: string[]): CliOptions {
  let suite: SuiteName = "fixture-smoke";
  let iterations = 5;
  let models = [defaultModel];
  let outputDir = defaultResultsDir;

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    const [flag, inlineValue] = arg.includes("=") ? arg.split(/=(.*)/s, 2) : [arg, undefined];
    const value = inlineValue ?? args[index + 1];
    const consumedSeparateValue = inlineValue === undefined;

    if (flag === "--help" || flag === "-h") {
      printHelp();
      process.exit(0);
    }
    if (flag === "--suite") {
      suite = parseSuite(requireValue(flag, value));
      if (consumedSeparateValue) index += 1;
      continue;
    }
    if (flag === "--iterations") {
      iterations = parsePositiveInteger(requireValue(flag, value), flag);
      if (consumedSeparateValue) index += 1;
      continue;
    }
    if (flag === "--model") {
      models = [requireValue(flag, value)];
      if (consumedSeparateValue) index += 1;
      continue;
    }
    if (flag === "--compare-models") {
      models = requireValue(flag, value)
        .split(",")
        .map((model) => model.trim())
        .filter(Boolean);
      if (models.length === 0) throw new Error("--compare-models requires at least one model");
      if (consumedSeparateValue) index += 1;
      continue;
    }
    if (flag === "--output") {
      outputDir = resolve(requireValue(flag, value));
      if (consumedSeparateValue) index += 1;
      continue;
    }

    throw new Error(`unknown benchmark option: ${arg}`);
  }

  return { suite, iterations, models, outputDir };
}

function parseSuite(value: string): SuiteName {
  if (supportedSuites.includes(value as SuiteName)) return value as SuiteName;
  throw new Error(`unknown suite ${value}; expected one of ${supportedSuites.join(", ")}`);
}

function parsePositiveInteger(value: string, flag: string): number {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 1) {
    throw new Error(`${flag} must be a positive integer`);
  }
  return parsed;
}

function requireValue(flag: string, value: string | undefined): string {
  if (!value || value.startsWith("--")) throw new Error(`${flag} requires a value`);
  return value;
}

function printHelp() {
  console.log(`Usage:
  bun run benchmark:refactorings -- --suite fixture-smoke --iterations 5
  bun run benchmark:refactorings -- --suite validation-realistic --model qwen2.5-coder:7b
  bun run benchmark:refactorings -- --compare-models qwen2.5-coder:7b,deepseek-coder:6.7b

Options:
  --suite <name>              ${supportedSuites.join(", ")}
  --iterations <n>            Warm samples per rule. Default: 5
  --model <name>              Single model. Default: ${defaultModel}
  --compare-models <a,b>      Run the suite once per listed model
  --output <dir>              Artifact directory. Default: benchmark-results
`);
}

function buildService() {
  const args = ["build", "-p", "local-refactor-service"];
  if (process.env.LOCAL_REFACTOR_BUILD_PROFILE === "release") args.push("--release");
  const result = spawnSync("cargo", args, {
    cwd: root,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error("failed to build local-refactor-service");
  }
}

function serviceBinaryPath() {
  const profile = process.env.LOCAL_REFACTOR_BUILD_PROFILE === "release" ? "release" : "debug";
  return join(root, `target/${profile}/local-refactor-service`);
}

async function benchmarkModel(
  model: string,
  suite: SuiteName,
  iterations: number,
  workRoot: string,
  servicePid: number | null,
): Promise<RuleBenchmarkSample[]> {
  const cases = casesForSuite(suite);
  const results: RuleBenchmarkSample[] = [];

  for (const rule of cases) {
    console.log(`cold ${model} ${suite} ${rule.id}`);
    results.push(await benchmarkRule(rule, suite, model, 0, "cold", workRoot, servicePid));
  }

  for (let iteration = 1; iteration <= iterations; iteration += 1) {
    const shuffled = shuffle(cases, `${model}:${suite}:${iteration}`);
    for (const rule of shuffled) {
      console.log(`warm ${iteration}/${iterations} ${model} ${suite} ${rule.id}`);
      results.push(
        await benchmarkRule(rule, suite, model, iteration, "warm", workRoot, servicePid),
      );
    }
  }

  return results;
}

function casesForSuite(suite: SuiteName): RuleCase[] {
  const base = baseRuleCases.map((rule) => ({ ...rule }));
  if (suite === "model-planned-stress") {
    return base
      .filter((rule) => rule.executionKind === "model-planned")
      .map((rule) => ({
        ...rule,
        source: `${rule.source}\n${stressContextInSameFile(rule.id)}`,
        extraFiles: {
          ...(rule.extraFiles ?? {}),
          ...contextFiles(24),
        },
      }));
  }
  return base;
}

async function benchmarkRule(
  rule: RuleCase,
  suite: SuiteName,
  model: string,
  iteration: number,
  phase: "cold" | "warm",
  workRoot: string,
  servicePid: number | null,
): Promise<RuleBenchmarkSample> {
  const repo = join(workRoot, `${suite}-${model.replace(/[^a-zA-Z0-9._-]/g, "_")}-${phase}-${iteration}-${rule.id}`);
  createBenchmarkRepo(repo, rule, suite);

  const samples: MemorySample[] = [];
  const sampler = setInterval(() => {
    samples.push(sampleMemory(servicePid));
  }, 250);
  samples.push(sampleMemory(servicePid));

  const started = performance.now();
  let status = "unknown";
  let error: string | undefined;
  let diffFiles = 0;
  let events = 0;
  let metrics: RunMetrics | null = null;

  try {
    const created = await postJson<{ id: string }>("/api/runs", {
      targetPath: repo,
      rules: [rule.id],
      model,
      testFileMode: "readOnly",
      protectedPaths: ["src/generated/**"],
      validationCommands: validationCommandsForSuite(suite),
    });
    const run = await pollRun(created.id);
    status = run.status;
    error = run.error;
    const review = await getJson<RunReview>(`/api/runs/${created.id}/review`);
    diffFiles = review.diff.files.length;
    events = review.events.length;
    metrics = review.metrics;

    if (status === "succeeded") {
      assertCaseCorrectness(rule, repo, review);
    }
  } catch (caught) {
    if (status === "succeeded") status = "correctness-failed";
    else if (status === "unknown") status = "benchmark-error";
    error = caught instanceof Error ? caught.message : String(caught);
  } finally {
    clearInterval(sampler);
    samples.push(sampleMemory(servicePid));
  }
  const wallMs = Math.round(performance.now() - started);

  return {
    model,
    suite,
    phase,
    iteration,
    ruleId: rule.id,
    executionKind: rule.executionKind,
    status,
    wallMs,
    diffFiles,
    events,
    error,
    metrics,
    servicePeakRssMb: peak(samples, "serviceRssMb"),
    ollamaPeakRssMb: peak(samples, "ollamaRssMb"),
    nvidiaPeakTotalVramMb: peak(samples, "nvidiaTotalVramMb"),
    nvidiaPeakOllamaVramMb: peak(samples, "nvidiaOllamaVramMb"),
    samples,
  };
}

function createBenchmarkRepo(repo: string, rule: RuleCase, suite: SuiteName) {
  mkdirSync(join(repo, "src"), { recursive: true });
  writeFileSync(join(repo, "src/sample.ts"), rule.source);
  writeFileSync(
    join(repo, "package.json"),
    JSON.stringify({ type: "module", private: true }, null, 2),
  );
  writeFileSync(
    join(repo, "tsconfig.json"),
    JSON.stringify(
      {
        compilerOptions: {
          target: "ES2022",
          module: "ESNext",
          moduleResolution: "Bundler",
          strict: true,
          skipLibCheck: true,
        },
        include: ["src"],
      },
      null,
      2,
    ),
  );

  for (const [path, content] of Object.entries(rule.extraFiles ?? {})) {
    const fullPath = join(repo, path);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(fullPath, content);
  }

  const fillerCount = suite === "medium-repo" ? 150 : suite === "large-repo" ? 2200 : 0;
  for (let index = 0; index < fillerCount; index += 1) {
    const folder = index % 11 === 0 ? "src/generated" : index % 7 === 0 ? "src/tests" : "src/filler";
    const suffix = index % 7 === 0 ? ".test.ts" : ".ts";
    const fullPath = join(repo, folder, `file-${String(index).padStart(4, "0")}${suffix}`);
    mkdirSync(dirname(fullPath), { recursive: true });
    writeFileSync(
      fullPath,
      `export const filler${index} = ${index};\nexport function value${index}() { return filler${index}; }\n`,
    );
  }

  if (suite === "model-planned-stress") {
    for (const [path, content] of Object.entries(contextFiles(80))) {
      const fullPath = join(repo, path);
      mkdirSync(dirname(fullPath), { recursive: true });
      writeFileSync(fullPath, content);
    }
  }
}

function validationCommandsForSuite(suite: SuiteName): string[] {
  if (suite !== "validation-realistic") return ["true"];
  const tsc = join(root, "workers/typescript-analyzer/node_modules/typescript/bin/tsc");
  return [`bun --bun ${shellQuote(tsc)} --noEmit --pretty false`];
}

function shellQuote(value: string) {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function contextFiles(count: number): Record<string, string> {
  const files: Record<string, string> = {};
  for (let index = 0; index < count; index += 1) {
    files[`src/context/context-${String(index).padStart(3, "0")}.ts`] =
      `export type Context${index} = { id: string; value: number };\nexport function contextValue${index}(input: Context${index}) { return input.value + ${index}; }\n`;
  }
  return files;
}

function stressContextInSameFile(ruleId: string) {
  return Array.from({ length: 20 }, (_, index) => {
    return `function ${camelIdentifier(ruleId)}Context${index}(value: number) {\n  const adjusted = value + ${index};\n  return adjusted * 2;\n}\n`;
  }).join("\n");
}

function camelIdentifier(value: string) {
  return value.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase());
}

export function assertCaseCorrectness(rule: RuleCase, repo: string, review: RunReview) {
  if (
    typeof rule.expectedDiffFileCount === "number" &&
    review.diff.files.length !== rule.expectedDiffFileCount
  ) {
    throw new Error(
      `${rule.id}: expected ${rule.expectedDiffFileCount} diff file(s), got ${review.diff.files.length}`,
    );
  }

  const changedFiles = new Set(
    review.diff.files.map((file) => normalizeRelativePath(repo, file.filePath)),
  );
  for (const expected of rule.requiredChangedFiles) {
    if (!changedFiles.has(expected)) {
      throw new Error(`${rule.id}: expected changed file ${expected}`);
    }
  }

  for (const expected of rule.requiredTextSnippets) {
    const content = readRepoFile(repo, expected.file);
    if (!content.includes(expected.text)) {
      throw new Error(`${rule.id}: ${expected.file} missing required text ${JSON.stringify(expected.text)}`);
    }
  }

  for (const forbidden of rule.forbiddenTextSnippets ?? []) {
    const content = readRepoFile(repo, forbidden.file);
    if (content.includes(forbidden.text)) {
      throw new Error(`${rule.id}: ${forbidden.file} contains forbidden text ${JSON.stringify(forbidden.text)}`);
    }
  }
}

function readRepoFile(repo: string, path: string) {
  return readFileSync(join(repo, path), "utf8");
}

function normalizeRelativePath(repo: string, path: string) {
  return relative(repo, path).replace(/\\/g, "/");
}

async function waitForService() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      const health = await getJson<{ status: string }>("/api/health");
      if (health.status === "ok") return;
    } catch {
      await sleep(250);
    }
  }
  throw new Error(`service did not become ready at ${baseUrl}`);
}

async function pollRun(id: string): Promise<{ status: string; error?: string }> {
  const deadline = Date.now() + 300_000;
  while (Date.now() < deadline) {
    const run = await getJson<{ status: string; error?: string }>(`/api/runs/${id}`);
    if (["succeeded", "failed", "cancelled", "reverted"].includes(run.status)) {
      return run;
    }
    await sleep(250);
  }
  throw new Error(`run ${id} did not finish before timeout`);
}

async function getJson<T>(path: string): Promise<T> {
  const response = await fetch(`${baseUrl}${path}`);
  if (!response.ok) throw new Error(`${response.status}: ${await response.text()}`);
  return (await response.json()) as T;
}

async function postJson<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(`${baseUrl}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`${response.status}: ${await response.text()}`);
  return (await response.json()) as T;
}

function sampleMemory(servicePid: number | null): MemorySample {
  const nvidia = sampleNvidia();
  return {
    timestampMs: Date.now(),
    serviceRssMb: servicePid ? rssMb(servicePid) : null,
    ollamaRssMb: ollamaRssMb(),
    nvidiaTotalVramMb: nvidia.totalVramMb,
    nvidiaOllamaVramMb: nvidia.ollamaVramMb,
  };
}

function rssMb(pid: number): number | null {
  try {
    const status = readFileSync(`/proc/${pid}/status`, "utf8");
    const match = status.match(/^VmRSS:\s+(\d+)\s+kB/m);
    return match ? Math.round((Number(match[1]) / 1024) * 10) / 10 : null;
  } catch {
    return null;
  }
}

function ollamaRssMb(): number | null {
  const pids = processIds().filter((pid) => {
    try {
      return readFileSync(`/proc/${pid}/cmdline`, "utf8").includes("ollama");
    } catch {
      return false;
    }
  });
  if (pids.length === 0) return null;
  const total = pids.reduce((sum, pid) => sum + (rssMb(pid) ?? 0), 0);
  return Math.round(total * 10) / 10;
}

function processIds(): number[] {
  const result = spawnSync("bash", ["-lc", "printf '%s\\n' /proc/[0-9]*"], {
    encoding: "utf8",
  });
  if (result.status !== 0) return [];
  return result.stdout
    .trim()
    .split("\n")
    .map((path) => Number(path.slice("/proc/".length)))
    .filter((pid) => Number.isFinite(pid));
}

function sampleNvidia(): {
  totalVramMb: number | null;
  ollamaVramMb: number | null;
} {
  if (!commandExists("nvidia-smi")) {
    return { totalVramMb: null, ollamaVramMb: null };
  }

  const total = spawnSync(
    "nvidia-smi",
    ["--query-gpu=memory.used", "--format=csv,noheader,nounits"],
    { encoding: "utf8" },
  );
  const totalVramMb = total.status === 0 ? sumNumbers(total.stdout) : null;

  const apps = spawnSync(
    "nvidia-smi",
    [
      "--query-compute-apps=pid,process_name,used_memory",
      "--format=csv,noheader,nounits",
    ],
    { encoding: "utf8" },
  );
  let ollamaVramMb: number | null = null;
  if (apps.status === 0 && apps.stdout.trim()) {
    const totalOllama = apps.stdout
      .trim()
      .split("\n")
      .filter((line) => line.toLowerCase().includes("ollama"))
      .reduce((sum, line) => {
        const parts = line.split(",").map((part) => part.trim());
        return sum + (Number(parts[2]) || 0);
      }, 0);
    ollamaVramMb = totalOllama || null;
  }

  return { totalVramMb, ollamaVramMb };
}

function commandExists(command: string) {
  const result = spawnSync("bash", ["-lc", `command -v ${command}`], {
    encoding: "utf8",
  });
  return result.status === 0;
}

function sumNumbers(value: string): number | null {
  const numbers = value
    .trim()
    .split(/\s+/)
    .map(Number)
    .filter((number) => Number.isFinite(number));
  if (numbers.length === 0) return null;
  return numbers.reduce((sum, number) => sum + number, 0);
}

function peak(
  samples: MemorySample[],
  key: keyof Omit<MemorySample, "timestampMs">,
): number | null {
  return maxNullable(samples.map((sample) => sample[key]));
}

function maxNullable(values: Array<number | null | undefined>): number | null {
  const numbers = values.filter((value): value is number => typeof value === "number");
  return numbers.length ? Math.max(...numbers) : null;
}

export function buildBenchmarkReport(input: {
  suite: SuiteName;
  iterations: number;
  models: string[];
  rawSamples: RuleBenchmarkSample[];
}): BenchmarkReport {
  const report: BenchmarkReport = {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    baseUrl,
    suite: input.suite,
    iterations: input.iterations,
    models: input.models,
    notes: [
      "Warm summary statistics exclude the separated cold pass.",
      "wallMs measures POST /api/runs through terminal run status and review fetch.",
      "metrics.* fields are emitted by /api/runs/{id}/review from service-side stage timers.",
      "servicePeakRssMb samples the local-refactor-service process RSS from /proc.",
      "ollamaPeakRssMb sums RSS for processes whose command contains ollama.",
      "nvidiaPeak* fields use nvidia-smi when available; null means unavailable or no matching process.",
      "fixture-smoke, medium-repo, large-repo, and model-planned-stress use validationCommands: [\"true\"].",
      "validation-realistic runs tsc --noEmit from the local TypeScript analyzer dependency.",
      "No benchmark telemetry leaves this machine.",
    ],
    environment: environmentMetadata(),
    rawSamples: input.rawSamples,
    summary: summarize(input.rawSamples),
  };
  return report;
}

function summarize(samples: RuleBenchmarkSample[]): BenchmarkReport["summary"] {
  const succeeded = samples.filter((sample) => sample.status === "succeeded");
  const warm = samples.filter((sample) => sample.phase === "warm");
  return {
    totalSamples: samples.length,
    succeeded: succeeded.length,
    failed: samples.length - succeeded.length,
    coldSamples: samples.length - warm.length,
    warmSamples: warm.length,
    byRule: summarizeRules(warm),
    byModel: summarizeModels(samples),
  };
}

function summarizeRules(samples: RuleBenchmarkSample[]): RuleSummary[] {
  const groups = groupBy(samples, (sample) => `${sample.model}\0${sample.suite}\0${sample.ruleId}`);
  return [...groups.values()].map((group) => {
    const first = group[0];
    return {
      model: first.model,
      suite: first.suite,
      ruleId: first.ruleId,
      executionKind: first.executionKind,
      samples: group.length,
      succeeded: group.filter((sample) => sample.status === "succeeded").length,
      failed: group.filter((sample) => sample.status !== "succeeded").length,
      wallMs: stats(group.map((sample) => sample.wallMs)),
      metrics: metricStats(group),
      servicePeakRssMb: maxNullable(group.map((sample) => sample.servicePeakRssMb)),
      ollamaPeakRssMb: maxNullable(group.map((sample) => sample.ollamaPeakRssMb)),
      nvidiaPeakOllamaVramMb: maxNullable(group.map((sample) => sample.nvidiaPeakOllamaVramMb)),
    };
  });
}

function summarizeModels(samples: RuleBenchmarkSample[]): BenchmarkReport["summary"]["byModel"] {
  const groups = groupBy(samples, (sample) => sample.model);
  return [...groups.entries()].map(([model, group]) => {
    const modelPlanned = group.filter((sample) => sample.executionKind === "model-planned");
    const invalidPatchPlans = modelPlanned.filter((sample) =>
      sample.error?.toLowerCase().includes("patch plan"),
    );
    const succeeded = group.filter((sample) => sample.status === "succeeded").length;
    return {
      model,
      samples: group.length,
      succeeded,
      failed: group.length - succeeded,
      successRate: ratio(succeeded, group.length),
      medianModelPlanningMs: stats(
        modelPlanned
          .map((sample) => sample.metrics?.modelPlanningMs)
          .filter((value): value is number => typeof value === "number"),
      )?.median ?? null,
      outputValidityRate: ratio(modelPlanned.length - invalidPatchPlans.length, modelPlanned.length),
      patchPlanRejectionRate: ratio(invalidPatchPlans.length, modelPlanned.length),
      peakServiceRssMb: maxNullable(group.map((sample) => sample.servicePeakRssMb)),
      peakOllamaRssMb: maxNullable(group.map((sample) => sample.ollamaPeakRssMb)),
      peakOllamaVramMb: maxNullable(group.map((sample) => sample.nvidiaPeakOllamaVramMb)),
    };
  });
}

function metricStats(samples: RuleBenchmarkSample[]): Record<keyof RunMetrics, StatSummary | null> {
  const keys = [
    "totalRunMs",
    "modelEnsureAvailableMs",
    "fileCollectionMs",
    "analyzerPlanningMs",
    "modelPlanningMs",
    "patchPlanValidationMs",
    "editApplicationMs",
    "validationMs",
  ] as const satisfies readonly (keyof RunMetrics)[];

  return Object.fromEntries(
    keys.map((key) => [
      key,
      stats(
        samples
          .map((sample) => sample.metrics?.[key])
          .filter((value): value is number => typeof value === "number"),
      ),
    ]),
  ) as Record<keyof RunMetrics, StatSummary | null>;
}

function groupBy<T>(items: T[], key: (item: T) => string): Map<string, T[]> {
  const groups = new Map<string, T[]>();
  for (const item of items) {
    const groupKey = key(item);
    groups.set(groupKey, [...(groups.get(groupKey) ?? []), item]);
  }
  return groups;
}

function stats(values: number[]): StatSummary | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const mean = sorted.reduce((sum, value) => sum + value, 0) / sorted.length;
  const variance =
    sorted.reduce((sum, value) => sum + (value - mean) ** 2, 0) / sorted.length;
  return {
    min: round(sorted[0]),
    median: round(percentile(sorted, 0.5)),
    p95: round(percentile(sorted, 0.95)),
    max: round(sorted[sorted.length - 1]),
    stddev: round(Math.sqrt(variance)),
  };
}

function percentile(sorted: number[], percentileValue: number) {
  const index = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(sorted.length * percentileValue) - 1),
  );
  return sorted[index];
}

function ratio(numerator: number, denominator: number) {
  if (denominator === 0) return 0;
  return round(numerator / denominator);
}

function round(value: number) {
  return Math.round(value * 100) / 100;
}

function environmentMetadata(): BenchmarkReport["environment"] {
  const gpu = commandExists("nvidia-smi")
    ? command(["nvidia-smi", "--query-gpu=name,driver_version", "--format=csv,noheader"]).trim()
    : "";
  return {
    modelDefault: defaultModel,
    gpuName: gpu ? gpu.split(",")[0]?.trim() ?? null : null,
    gpuDriverVersion: gpu ? gpu.split(",")[1]?.trim() ?? null : null,
    cpuModel: cpuModel(),
    ramTotalMb: ramTotalMb(),
    os: command(["uname", "-a"]).trim() || process.platform,
    commitSha: command(["git", "rev-parse", "HEAD"]).trim() || null,
    gitDirty: command(["git", "status", "--short"]).trim().length > 0,
    buildProfile: process.env.LOCAL_REFACTOR_BUILD_PROFILE === "release" ? "release" : "debug",
    bunVersion: command(["bun", "--version"]).trim() || null,
    rustcVersion: command(["rustc", "--version"]).trim() || null,
  };
}

function cpuModel() {
  try {
    const match = readFileSync("/proc/cpuinfo", "utf8").match(/^model name\s+:\s+(.+)$/m);
    if (match) return match[1];
  } catch {
    // Non-Linux platforms fall through to uname metadata.
  }
  return null;
}

function ramTotalMb() {
  try {
    const match = readFileSync("/proc/meminfo", "utf8").match(/^MemTotal:\s+(\d+)\s+kB/m);
    if (match) return Math.round(Number(match[1]) / 1024);
  } catch {
    // Non-Linux platforms fall through to null.
  }
  return null;
}

function command(args: string[]) {
  const result = spawnSync(args[0], args.slice(1), { cwd: root, encoding: "utf8" });
  return result.status === 0 ? result.stdout : "";
}

export function writeReportArtifacts(report: BenchmarkReport, outputDir: string) {
  const runStamp = report.generatedAt.replace(/[:.]/g, "-");
  const jsonPath = join(outputDir, `refactor-benchmark-${runStamp}.json`);
  const markdownPath = join(outputDir, `refactor-benchmark-${runStamp}.md`);
  const htmlPath = join(outputDir, "latest.html");
  writeFileSync(jsonPath, `${JSON.stringify(report, null, 2)}\n`);
  writeFileSync(markdownPath, markdownReport(report));
  writeFileSync(htmlPath, htmlReport(report));
  return { jsonPath, markdownPath, htmlPath };
}

export function markdownReport(report: BenchmarkReport) {
  const rows = report.summary.byRule
    .map((result) =>
      [
        result.model,
        result.ruleId,
        result.executionKind,
        `${result.succeeded}/${result.samples}`,
        result.wallMs?.median ?? "",
        result.metrics.fileCollectionMs?.median ?? "",
        result.metrics.analyzerPlanningMs?.median ?? "",
        result.metrics.modelPlanningMs?.median ?? "",
        result.metrics.patchPlanValidationMs?.median ?? "",
        result.metrics.editApplicationMs?.median ?? "",
        result.metrics.validationMs?.median ?? "",
        result.nvidiaPeakOllamaVramMb ?? "",
      ].join(" | "),
    )
    .join("\n");

  const modelRows = report.summary.byModel
    .map((model) =>
      [
        model.model,
        `${model.succeeded}/${model.samples}`,
        model.successRate,
        model.medianModelPlanningMs ?? "",
        model.outputValidityRate,
        model.patchPlanRejectionRate,
        model.peakOllamaVramMb ?? "",
      ].join(" | "),
    )
    .join("\n");

  return `# Refactoring Benchmark

Generated: ${report.generatedAt}

Suite: ${report.suite}
Iterations: ${report.iterations}
Models: ${report.models.join(", ")}

## Summary

- Samples: ${report.summary.totalSamples}
- Succeeded: ${report.summary.succeeded}
- Failed: ${report.summary.failed}
- Cold samples: ${report.summary.coldSamples}
- Warm samples: ${report.summary.warmSamples}

## Model Comparison

model | success | successRate | medianModelPlanningMs | outputValidityRate | patchPlanRejectionRate | peakOllamaVramMb
--- | ---: | ---: | ---: | ---: | ---: | ---:
${modelRows}

## Rule Latency

model | rule | kind | success | medianWallMs | fileCollectionMs | analyzerPlanningMs | modelPlanningMs | patchPlanValidationMs | editApplicationMs | validationMs | peakOllamaVramMb
--- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---:
${rows}

## Notes

${report.notes.map((note) => `- ${note}`).join("\n")}
`;
}

function htmlReport(report: BenchmarkReport) {
  const maxWall = Math.max(...report.summary.byRule.map((rule) => rule.wallMs?.median ?? 0), 1);
  const ruleRows = report.summary.byRule
    .map((rule) => {
      const wall = rule.wallMs?.median ?? 0;
      return `<tr>
  <td>${escapeHtml(rule.model)}</td>
  <td>${escapeHtml(rule.ruleId)}</td>
  <td>${escapeHtml(rule.executionKind)}</td>
  <td data-sort="${rule.succeeded / Math.max(rule.samples, 1)}">${rule.succeeded}/${rule.samples}</td>
  <td data-sort="${wall}"><span class="bar" style="--w:${(wall / maxWall) * 100}%"></span>${wall}</td>
  <td data-sort="${rule.metrics.fileCollectionMs?.median ?? -1}">${value(rule.metrics.fileCollectionMs?.median)}</td>
  <td data-sort="${rule.metrics.analyzerPlanningMs?.median ?? -1}">${value(rule.metrics.analyzerPlanningMs?.median)}</td>
  <td data-sort="${rule.metrics.modelPlanningMs?.median ?? -1}">${value(rule.metrics.modelPlanningMs?.median)}</td>
  <td data-sort="${rule.metrics.patchPlanValidationMs?.median ?? -1}">${value(rule.metrics.patchPlanValidationMs?.median)}</td>
  <td data-sort="${rule.metrics.validationMs?.median ?? -1}">${value(rule.metrics.validationMs?.median)}</td>
  <td data-sort="${rule.nvidiaPeakOllamaVramMb ?? -1}">${value(rule.nvidiaPeakOllamaVramMb)}</td>
</tr>`;
    })
    .join("\n");

  const modelRows = report.summary.byModel
    .map(
      (model) => `<tr>
  <td>${escapeHtml(model.model)}</td>
  <td data-sort="${model.successRate}">${model.successRate}</td>
  <td data-sort="${model.medianModelPlanningMs ?? -1}">${value(model.medianModelPlanningMs)}</td>
  <td data-sort="${model.outputValidityRate}">${model.outputValidityRate}</td>
  <td data-sort="${model.patchPlanRejectionRate}">${model.patchPlanRejectionRate}</td>
  <td data-sort="${model.peakOllamaVramMb ?? -1}">${value(model.peakOllamaVramMb)}</td>
</tr>`,
    )
    .join("\n");

  return `<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Local Refactor Benchmark</title>
<style>
body { margin: 0; font: 14px/1.45 system-ui, sans-serif; color: #1f2933; background: #f6f7f9; }
main { max-width: 1280px; margin: 0 auto; padding: 28px; }
h1 { margin: 0 0 6px; font-size: 28px; letter-spacing: 0; }
h2 { margin: 28px 0 10px; font-size: 18px; letter-spacing: 0; }
.meta { color: #52606d; margin-bottom: 18px; }
.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 10px; }
.stat { background: white; border: 1px solid #d9e2ec; border-radius: 6px; padding: 12px; }
.stat strong { display: block; font-size: 22px; }
table { width: 100%; border-collapse: collapse; background: white; border: 1px solid #d9e2ec; }
th, td { padding: 8px 10px; border-bottom: 1px solid #edf1f5; text-align: left; white-space: nowrap; }
th { cursor: pointer; color: #334e68; background: #f0f4f8; position: sticky; top: 0; }
td { position: relative; }
.bar { position: absolute; left: 0; top: 6px; bottom: 6px; width: var(--w); background: #c6f6d5; z-index: 0; }
td > *:not(.bar), td { z-index: 1; }
.scroll { overflow: auto; border-radius: 6px; }
</style>
<main>
  <h1>Local Refactor Benchmark</h1>
  <div class="meta">${escapeHtml(report.generatedAt)} · ${escapeHtml(report.suite)} · ${report.iterations} warm iterations</div>
  <section class="grid">
    <div class="stat"><span>Samples</span><strong>${report.summary.totalSamples}</strong></div>
    <div class="stat"><span>Succeeded</span><strong>${report.summary.succeeded}</strong></div>
    <div class="stat"><span>Failed</span><strong>${report.summary.failed}</strong></div>
    <div class="stat"><span>Models</span><strong>${report.models.length}</strong></div>
  </section>
  <h2>Model Comparison</h2>
  <div class="scroll"><table>
    <thead><tr><th>Model</th><th>Success Rate</th><th>Median Model Planning</th><th>Output Validity</th><th>Patch Rejection</th><th>Peak VRAM</th></tr></thead>
    <tbody>${modelRows}</tbody>
  </table></div>
  <h2>Rule Latency By Stage</h2>
  <div class="scroll"><table>
    <thead><tr><th>Model</th><th>Rule</th><th>Kind</th><th>Success</th><th>Median Wall</th><th>File Collection</th><th>Analyzer</th><th>Model</th><th>Patch Validation</th><th>Validation</th><th>Peak VRAM</th></tr></thead>
    <tbody>${ruleRows}</tbody>
  </table></div>
</main>
<script>
for (const table of document.querySelectorAll("table")) {
  for (const [index, th] of table.querySelectorAll("th").entries()) {
    th.addEventListener("click", () => {
      const rows = [...table.tBodies[0].rows];
      const current = th.dataset.dir === "asc" ? "desc" : "asc";
      th.dataset.dir = current;
      rows.sort((a, b) => {
        const left = a.cells[index].dataset.sort ?? a.cells[index].textContent;
        const right = b.cells[index].dataset.sort ?? b.cells[index].textContent;
        const leftNumber = Number(left);
        const rightNumber = Number(right);
        const compare = Number.isFinite(leftNumber) && Number.isFinite(rightNumber)
          ? leftNumber - rightNumber
          : String(left).localeCompare(String(right));
        return current === "asc" ? compare : -compare;
      });
      table.tBodies[0].append(...rows);
    });
  }
}
</script>
</html>`;
}

function value(value: number | null | undefined) {
  return typeof value === "number" ? String(value) : "";
}

function escapeHtml(value: string) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function shuffle<T>(items: readonly T[], seed: string): T[] {
  const random = seededRandom(seed);
  return [...items]
    .map((item) => ({ item, key: random() }))
    .sort((left, right) => left.key - right.key)
    .map(({ item }) => item);
}

function seededRandom(seed: string) {
  let state = 2166136261;
  for (const char of seed) {
    state ^= char.charCodeAt(0);
    state = Math.imul(state, 16777619);
  }
  return () => {
    state += 0x6d2b79f5;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

if (import.meta.main) {
  await main();
}
