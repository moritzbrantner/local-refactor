#!/usr/bin/env bun

import {
  existsSync,
  readdirSync,
  readFileSync,
  statSync,
} from "node:fs";
import { join } from "node:path";

type Catalog = {
  language: Language;
  entries: CatalogEntry[];
};

type CatalogEntry = {
  id: string;
  language: Language;
  name: string;
  category: string;
  status: string;
  safetyLevel: string;
  preserves: string[];
  allowedWrites: string;
  requiresTypeInformation: boolean;
  requiresImportGraph: boolean;
  fixtureDirectory: string;
  llmEvalDirectory?: string;
  description: string;
};

const root = new URL("..", import.meta.url).pathname;
type Language = "typescript" | "rust";

const catalogPaths = [
  join(root, "refactoring-catalog/typescript.json"),
  join(root, "refactoring-catalog/rust.json"),
];
const schemaPath = join(root, "refactoring-catalog/schema.json");
const rulesPath = join(root, "crates/core/src/rules.rs");
const runLifecyclePath = join(root, "crates/service/tests/run_lifecycle.rs");

const categories = new Set([
  "control-flow",
  "naming",
  "extraction",
  "deduplication",
  "module-organization",
  "type-structure",
  "declaration-organization",
]);
const statuses = new Set([
  "cataloged",
  "llm-eval-only",
  "deterministic-rule",
  "run-supported",
]);
const safetyLevels = new Set([
  "syntax-only",
  "typecheck-required",
  "test-required",
]);
const preserves = new Set([
  "runtime-behavior",
  "exports",
  "public-api",
  "typecheck",
  "comments",
  "formatting-intent",
]);
const allowedWrites = new Set(["single-file", "multi-file-within-target"]);
const languages = new Set(["typescript", "rust"]);

const errors: string[] = [];

function main() {
  requireFile(schemaPath, "schema");
  const catalogs = catalogPaths.map((catalogPath) => readJson<Catalog>(catalogPath));
  for (const catalog of catalogs) {
    validateCatalog(catalog);
  }
  validatePublicRules(catalogs);
  validateRunSupportedMarkers(catalogs);

  if (errors.length > 0) {
    for (const error of errors) {
      console.error(`catalog-check: ${error}`);
    }
    process.exit(1);
  }

  console.log(
    `catalog-check: ${catalogs.reduce((sum, catalog) => sum + catalog.entries.length, 0)} entries valid across ${catalogs.length} languages`,
  );
}

function validateCatalog(catalog: Catalog) {
  if (!isRecord(catalog)) {
    add("catalog root must be an object");
    return;
  }
  if (!languages.has(catalog.language)) {
    add(`catalog language must be one of ${[...languages].join(", ")}`);
  }
  if (!Array.isArray(catalog.entries) || catalog.entries.length === 0) {
    add("catalog entries must be a non-empty array");
    return;
  }

  const seen = new Set<string>();
  for (const entry of catalog.entries) {
    validateEntry(entry, seen);
  }
}

function validateEntry(entry: CatalogEntry, seen: Set<string>) {
  if (!isRecord(entry)) {
    add("catalog entry must be an object");
    return;
  }

  if (!/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(entry.id)) {
    add(`invalid id ${entry.id}`);
  }
  if (seen.has(entry.id)) {
    add(`duplicate id ${entry.id}`);
  }
  seen.add(entry.id);

  requireString(entry, "name");
  requireString(entry, "description");
  requireString(entry, "fixtureDirectory");
  requireEnum(entry.category, categories, entry.id, "category");
  requireEnum(entry.status, statuses, entry.id, "status");
  requireEnum(entry.safetyLevel, safetyLevels, entry.id, "safetyLevel");
  requireEnum(entry.allowedWrites, allowedWrites, entry.id, "allowedWrites");

  if (!languages.has(entry.language)) {
    add(`${entry.id}: language must be one of ${[...languages].join(", ")}`);
  }
  if (typeof entry.requiresTypeInformation !== "boolean") {
    add(`${entry.id}: requiresTypeInformation must be boolean`);
  }
  if (typeof entry.requiresImportGraph !== "boolean") {
    add(`${entry.id}: requiresImportGraph must be boolean`);
  }
  if (!Array.isArray(entry.preserves) || entry.preserves.length === 0) {
    add(`${entry.id}: preserves must be a non-empty array`);
  } else {
    for (const value of entry.preserves) {
      requireEnum(value, preserves, entry.id, "preserves");
    }
  }

  const fixtureDirectory = join(root, entry.fixtureDirectory);
  if (!existsSync(fixtureDirectory) || !statSync(fixtureDirectory).isDirectory()) {
    add(`${entry.id}: fixtureDirectory does not exist: ${entry.fixtureDirectory}`);
  }

  if (["deterministic-rule", "run-supported"].includes(entry.status)) {
    const manifest = join(fixtureDirectory, "manifest.json");
    if (!existsSync(manifest)) {
      add(`${entry.id}: ${entry.status} entries require fixture manifest.json`);
    }
  }

  if ((entry.status === "llm-eval-only" || entry.status === "run-supported") && entry.llmEvalDirectory) {
    const evalDirectory = join(root, entry.llmEvalDirectory);
    if (!existsSync(evalDirectory) || !statSync(evalDirectory).isDirectory()) {
      add(`${entry.id}: llmEvalDirectory does not exist: ${entry.llmEvalDirectory}`);
    } else if (!readdirSync(evalDirectory).some((name) => name === `${entry.id}.json`)) {
      add(`${entry.id}: missing LLM eval task ${entry.llmEvalDirectory}/${entry.id}.json`);
    }
  }
}

function validatePublicRules(catalogs: Catalog[]) {
  const rulesSource = readFileSync(rulesPath, "utf8");
  const publicRules = [...rulesSource.matchAll(/id:\s*"([^"]+)"/g)].map(
    (match) => match[1],
  );
  const catalogIds = new Set(catalogs.flatMap((catalog) => catalog.entries.map((entry) => entry.id)));
  for (const rule of publicRules) {
    if (!catalogIds.has(rule)) {
      add(`public rule ${rule} is missing from a refactoring catalog`);
    }
  }
}

function validateRunSupportedMarkers(catalogs: Catalog[]) {
  const runLifecycle = readFileSync(runLifecyclePath, "utf8");
  const entries = catalogs.flatMap((catalog) => catalog.entries);
  for (const entry of entries) {
    if (entry.status !== "run-supported") continue;
    const marker = new RegExp(
      `assert_(?:rust_)?run_supported_rule\\s*\\(\\s*"${escapeRegExp(entry.id)}"`,
    );
    if (!marker.test(runLifecycle)) {
      add(`${entry.id}: run-supported entry is missing service test marker`);
    }
  }
}

function requireString(entry: CatalogEntry, key: keyof CatalogEntry) {
  if (typeof entry[key] !== "string" || String(entry[key]).trim().length === 0) {
    add(`${entry.id}: ${String(key)} must be a non-empty string`);
  }
}

function requireEnum(
  value: unknown,
  allowed: Set<string>,
  id: string,
  key: string,
) {
  if (typeof value !== "string" || !allowed.has(value)) {
    add(`${id}: invalid ${key} ${String(value)}`);
  }
}

function requireFile(path: string, label: string) {
  if (!existsSync(path) || !statSync(path).isFile()) {
    add(`missing ${label} file: ${path}`);
  }
}

function readJson<T>(path: string): T {
  requireFile(path, "JSON");
  try {
    return JSON.parse(readFileSync(path, "utf8")) as T;
  } catch (error) {
    add(`invalid JSON in ${path}: ${error instanceof Error ? error.message : String(error)}`);
    return {} as T;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function add(error: string) {
  errors.push(error);
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

main();
