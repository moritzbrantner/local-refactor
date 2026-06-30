#!/usr/bin/env bun

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

type OllamaTagsResponse = {
  models?: Array<{
    name?: string;
    model?: string;
  }>;
};

type OllamaGenerateResponse = {
  response?: string;
  done?: boolean;
  error?: string;
};

type LlmEvalTask = {
  id: string;
  language: "typescript";
  modelDefault: "qwen2.5-coder:7b";
  prompt: string;
  responseSchema: "patch-plan-v1";
  requiredFiles: string[];
  forbiddenPaths: string[];
  requiredExports: string[];
  forbiddenText: string[];
  requiredText: string[];
  validationCommand: string;
};

type PatchPlanV1 = {
  summary: string;
  files: Array<{
    path: string;
    action: "create" | "update" | "delete";
    content?: string;
  }>;
  preservedExports: string[];
  validationCommand: string;
};

const root = new URL("..", import.meta.url).pathname;
const tasksRoot = join(root, "scripts/llm-evals/typescript");
const baseUrl = (process.env.OLLAMA_BASE_URL ?? "http://127.0.0.1:11434").replace(
  /\/+$/,
  "",
);
const timeoutMs = Number(process.env.LOCAL_REFACTOR_LLM_TIMEOUT_MS ?? "120000");
const selectedTask = argValue("--task");

async function main() {
  const tasks = loadTasks();
  if (tasks.length === 0) {
    fail("No LLM eval tasks found.");
  }

  const taskModels = [
    ...new Set(
      tasks.map((task) => process.env.LOCAL_REFACTOR_LLM_MODEL ?? task.modelDefault),
    ),
  ];
  console.log(`LLM eval model: ${taskModels.join(", ")}`);

  const tags = await fetchJson<OllamaTagsResponse>("/api/tags", {
    method: "GET",
    failureMessage: `Ollama not reachable at ${baseUrl}`,
  });
  console.log(`Ollama: reachable at ${baseUrl}`);

  const installedModels = new Set(
    (tags.models ?? []).flatMap((entry) => [entry.name, entry.model].filter(Boolean)),
  );
  for (const model of taskModels) {
    if (!installedModels.has(model)) {
      fail(
        `Model not installed: ${model}. Install it with \`ollama pull ${model}\` or set LOCAL_REFACTOR_LLM_MODEL.`,
      );
    }
  }
  console.log("Model: installed");

  let failures = 0;
  for (const task of tasks) {
    const passed = await runTask(task);
    if (!passed) failures += 1;
  }

  if (failures > 0) {
    fail(`${failures} LLM eval task(s) failed.`);
  }

  console.log("Result: passed");
}

function loadTasks() {
  const taskFiles = readdirSync(tasksRoot)
    .filter((name) => name.endsWith(".json"))
    .sort();
  const tasks = taskFiles.map((name) =>
    readTask(join(tasksRoot, name)),
  );
  const filtered = selectedTask
    ? tasks.filter((task) => task.id === selectedTask)
    : tasks;

  if (selectedTask && filtered.length === 0) {
    fail(`No LLM eval task found for --task ${selectedTask}`);
  }

  return filtered;
}

function readTask(path: string): LlmEvalTask {
  const task = JSON.parse(readFileSync(path, "utf8")) as LlmEvalTask;
  validateTaskShape(task, path);
  return task;
}

function validateTaskShape(task: LlmEvalTask, path: string) {
  if (!/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(task.id)) {
    fail(`${path}: invalid task id`);
  }
  if (task.language !== "typescript") {
    fail(`${task.id}: language must be typescript`);
  }
  if (task.responseSchema !== "patch-plan-v1") {
    fail(`${task.id}: responseSchema must be patch-plan-v1`);
  }
  for (const key of [
    "requiredFiles",
    "forbiddenPaths",
    "requiredExports",
    "forbiddenText",
    "requiredText",
  ] as const) {
    if (!Array.isArray(task[key])) {
      fail(`${task.id}: ${key} must be an array`);
    }
  }
  if (typeof task.prompt !== "string" || task.prompt.trim().length === 0) {
    fail(`${task.id}: prompt must be non-empty`);
  }
  if (
    typeof task.validationCommand !== "string" ||
    task.validationCommand.trim().length === 0
  ) {
    fail(`${task.id}: validationCommand must be non-empty`);
  }
}

async function runTask(task: LlmEvalTask) {
  const model = process.env.LOCAL_REFACTOR_LLM_MODEL ?? task.modelDefault;
  try {
    const generated = await fetchJson<OllamaGenerateResponse>("/api/generate", {
      method: "POST",
      body: JSON.stringify({
        model,
        prompt: taskPrompt(task),
        stream: false,
        format: "json",
        options: {
          temperature: 0,
          num_predict: 1600,
        },
      }),
      headers: {
        "content-type": "application/json",
      },
      failureMessage: `${task.id}: generation failed for ${model}`,
    });

    if (generated.error) {
      throw new Error(generated.error);
    }

    const response = generated.response ?? "";
    if (!response.trim()) {
      throw new Error("generation returned an empty response");
    }
    if (response.includes("```")) {
      throw new Error("generation included Markdown fences; expected raw JSON only");
    }

    let plan: unknown;
    try {
      plan = JSON.parse(response);
    } catch (error) {
      throw new Error(
        `response was not valid JSON: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }

    validatePatchPlan(task, plan);
    console.log(`${task.id}: passed`);
    return true;
  } catch (error) {
    console.error(
      `${task.id}: failed: ${error instanceof Error ? error.message : String(error)}`,
    );
    return false;
  }
}

function validatePatchPlan(task: LlmEvalTask, value: unknown): asserts value is PatchPlanV1 {
  if (!isRecord(value)) {
    throw new Error("patch plan must be a JSON object");
  }
  if (typeof value.summary !== "string" || value.summary.trim().length === 0) {
    throw new Error("patch plan is missing a non-empty summary");
  }
  if (!Array.isArray(value.files)) {
    throw new Error("patch plan is missing files array");
  }
  if (!Array.isArray(value.preservedExports)) {
    throw new Error("patch plan is missing preservedExports array");
  }
  if (
    typeof value.validationCommand !== "string" ||
    value.validationCommand.trim().length === 0
  ) {
    throw new Error("patch plan is missing a non-empty validationCommand");
  }

  const files = value.files;
  const paths = files.map((file) => (isRecord(file) ? file.path : undefined));
  const sortedPaths = [...paths].sort();
  const sortedRequiredPaths = [...task.requiredFiles].sort();
  if (JSON.stringify(sortedPaths) !== JSON.stringify(sortedRequiredPaths)) {
    throw new Error(`required files mismatch. Got: ${paths.join(", ")}`);
  }

  const combinedContent: string[] = [];
  for (const file of files) {
    if (!isRecord(file)) {
      throw new Error("each file entry must be an object");
    }
    if (typeof file.path !== "string") {
      throw new Error("each file entry must include string path");
    }
    if (!["create", "update", "delete"].includes(String(file.action))) {
      throw new Error(`${file.path}: action must be create, update, or delete`);
    }
    validatePath(task, file.path);

    if (file.action !== "delete") {
      if (typeof file.content !== "string" || file.content.trim().length === 0) {
        throw new Error(`${file.path}: create/update entries require content`);
      }
      if (file.content.includes("```")) {
        throw new Error(`${file.path}: content included Markdown fences`);
      }
      combinedContent.push(file.content);
    }
  }

  for (const exportName of task.requiredExports) {
    if (!value.preservedExports.includes(exportName)) {
      throw new Error(`preservedExports is missing ${exportName}`);
    }
  }

  const joinedContent = combinedContent.join("\n");
  for (const expected of task.requiredText) {
    if (!joinedContent.includes(expected)) {
      throw new Error(`combined content must contain ${expected}`);
    }
  }
  for (const forbidden of task.forbiddenText) {
    if (joinedContent.includes(forbidden)) {
      throw new Error(`combined content contains forbidden text ${forbidden}`);
    }
  }
}

function validatePath(task: LlmEvalTask, path: string) {
  if (path.startsWith("/") || path.includes("../") || path.includes("..\\")) {
    throw new Error(`${path}: paths must stay inside target`);
  }
  for (const forbidden of task.forbiddenPaths) {
    if (forbidden === "/" && path.startsWith("/")) {
      throw new Error(`${path}: forbidden absolute path`);
    }
    if (forbidden !== "/" && path.includes(forbidden)) {
      throw new Error(`${path}: forbidden path fragment ${forbidden}`);
    }
  }
}

function taskPrompt(task: LlmEvalTask) {
  return [
    "You are evaluating a local refactoring assistant.",
    "Return only valid JSON. Do not use Markdown fences. Do not explain outside JSON.",
    "The response must match patch-plan-v1 exactly:",
    "{",
    "  \"summary\": \"short summary\",",
    "  \"files\": [",
    "    { \"path\": \"src/file.ts\", \"action\": \"update\", \"content\": \"complete TypeScript source\" }",
    "  ],",
    "  \"preservedExports\": [\"ExportName\"],",
    "  \"validationCommand\": \"tsc --noEmit\"",
    "}",
    `Task id: ${task.id}`,
    `Required files: ${task.requiredFiles.join(", ")}`,
    `Required preserved exports: ${task.requiredExports.join(", ")}`,
    `Required content snippets: ${task.requiredText.join(" | ")}`,
    `Forbidden text: ${task.forbiddenText.join(" | ")}`,
    `Validation command: ${task.validationCommand}`,
    task.prompt,
  ].join("\n\n");
}

async function fetchJson<T>(
  path: string,
  options: RequestInit & { failureMessage: string },
): Promise<T> {
  const { failureMessage, ...requestOptions } = options;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(`${baseUrl}${path}`, {
      ...requestOptions,
      signal: controller.signal,
    });
    if (!response.ok) {
      fail(`${failureMessage}: HTTP ${response.status} ${await response.text()}`);
    }
    return (await response.json()) as T;
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      fail(`${failureMessage}: timeout exceeded after ${timeoutMs}ms`);
    }
    fail(`${failureMessage}: ${error instanceof Error ? error.message : String(error)}`);
  } finally {
    clearTimeout(timeout);
  }
}

function argValue(name: string) {
  const index = process.argv.indexOf(name);
  if (index === -1) return null;
  return process.argv[index + 1] ?? null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function fail(message: string): never {
  console.error(`Result: failed`);
  console.error(message);
  process.exit(1);
}

if (!existsSync(tasksRoot)) {
  fail(`LLM eval task directory is missing: ${tasksRoot}`);
}

await main();
