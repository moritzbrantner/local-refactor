import type {
  AnalyzerEdit,
  CandidateFilePreviewResponse,
  DeterministicPreviewResponse,
  FolderEntry,
  ModelSummary,
  RepositoryFilePreviewResponse,
  RepositoryRecord,
  Rule,
  RuleSelectionPlan,
  RunDraft,
  RunEvent,
  RunRecord,
  RunReviewResponse,
} from "../types";

export const now = "2026-06-30T12:00:00.000Z";

export function repository(overrides: Partial<RepositoryRecord> = {}): RepositoryRecord {
  return {
    id: "repo-1",
    label: "Fixture Repo",
    rootPath: "/tmp/local-refactor-fixture",
    available: true,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

export const rules: Rule[] = [
  {
    id: "simplify-conditional",
    language: "typescript",
    name: "Simplify Conditional",
    description: "Rewrites simple boolean-return conditionals into direct return expressions.",
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
    name: "Add Documentation Comments",
    description: "Adds Rustdoc comments without changing runtime code.",
    executionKind: "modelPlanned",
    allowedWrites: "single-file",
    category: "documentation",
    safetyLevel: "syntax-only",
    preserves: ["runtime-behavior", "public-api", "typecheck"],
    requiresTypeInformation: false,
    requiresImportGraph: false,
    planningProfile: "documentation-only",
  },
];

export const ruleSelectionPlan: RuleSelectionPlan = {
  targetRelativePath: ".",
  segments: [
    {
      relativePath: "src",
      rules: ["simplify-conditional"],
      reasons: [
        {
          ruleId: "simplify-conditional",
          source: "fallback",
          message: "TypeScript source files are eligible for local transformations",
        },
      ],
    },
  ],
};

export const folders: Record<string, FolderEntry[]> = {
  ".": [{ name: "src", relativePath: "src" }],
  src: [{ name: "components", relativePath: "src/components" }],
  "src/components": [],
};

export const models: ModelSummary[] = [
  {
    name: "qwen2.5-coder:7b",
    label: "Qwen2.5 Coder 7B",
    description: "Ready test model",
    downloaded: true,
  },
];

export function candidatePreview(
  overrides: Partial<CandidateFilePreviewResponse> = {},
): CandidateFilePreviewResponse {
  return {
    targetRelativePath: ".",
    totalCandidateFiles: 3,
    limitPerGroup: 2,
    groups: [
      {
        id: "segment:src:rule:simplify-conditional",
        label: "src - Simplify Conditional",
        segmentRelativePath: "src",
        ruleId: "simplify-conditional",
        ruleName: "Simplify Conditional",
        language: "typescript",
        totalFiles: 3,
        hiddenFiles: 1,
        files: [{ relativePath: "src/sample.ts" }, { relativePath: "src/other.ts" }],
      },
      {
        id: "segment:src:rule:rust-add-documentation-comments",
        label: "src - Add Documentation Comments",
        segmentRelativePath: "src",
        ruleId: "rust-add-documentation-comments",
        ruleName: "Add Documentation Comments",
        language: "rust",
        totalFiles: 1,
        hiddenFiles: 0,
        files: [{ relativePath: "src/lib.rs" }],
      },
    ],
    ...overrides,
  };
}

export const filePreview: RepositoryFilePreviewResponse = {
  repositoryId: "repo-1",
  relativePath: "src/sample.ts",
  language: "typescript",
  content: "export function isReady(value: boolean) {\n  if (value) {\n    return true;\n  }\n  return false;\n}\n",
  sizeBytes: 94,
};

export const analyzerEdit: AnalyzerEdit = {
  filePath: "/tmp/local-refactor-fixture/src/sample.ts",
  originalContent: filePreview.content,
  newContent: "export function isReady(value: boolean) {\n  return value;\n}\n",
  ruleId: "simplify-conditional",
  summary: "Replaced boolean conditional with direct return.",
};

export function deterministicPreview(
  overrides: Partial<DeterministicPreviewResponse> = {},
): DeterministicPreviewResponse {
  return {
    targetRelativePath: ".",
    rules: ["simplify-conditional"],
    previewFingerprint: "preview-fingerprint",
    diagnostics: ["Analyzed /tmp/local-refactor-fixture/src/sample.ts: 1 function declarations, 0 arrow functions"],
    files: [
      {
        relativePath: "src/sample.ts",
        filePath: "/tmp/local-refactor-fixture/src/sample.ts",
        ruleIds: ["simplify-conditional"],
        summaries: ["Replaced boolean conditional with direct return."],
        originalContentHash: "original-hash",
        newContentHash: "new-hash",
        diff: "--- /tmp/local-refactor-fixture/src/sample.ts\n+++ /tmp/local-refactor-fixture/src/sample.ts\n+  return value;\n",
      },
    ],
    ...overrides,
  };
}

export function run(overrides: Partial<RunRecord> = {}): RunRecord {
  return {
    id: "run-1",
    targetPath: "/tmp/local-refactor-fixture",
    status: "succeeded",
    createdAt: now,
    updatedAt: now,
    rules: ["simplify-conditional"],
    ruleSelectionPlan,
    model: "qwen2.5-coder:7b",
    testFileMode: "readOnly",
    validationCommands: ["bun test"],
    protectedPaths: ["src/generated/**"],
    repositoryId: "repo-1",
    repositoryRootPath: "/tmp/local-refactor-fixture",
    targetRelativePath: ".",
    runKind: "refactoring",
    sourceCoverageRunId: null,
    coverageEvidence: [],
    behaviorClaims: [],
    ...overrides,
  };
}

export const runEvents: RunEvent[] = [
  { id: 1, runId: "run-1", timestamp: now, message: "Run started" },
  { id: 2, runId: "run-1", timestamp: now, message: "Downloading qwen2.5-coder:7b: 45%" },
];

export function runReview(overrides: Partial<RunReviewResponse> = {}): RunReviewResponse {
  const currentRun = run();
  return {
    run: currentRun,
    events: runEvents,
    diff: {
      runId: currentRun.id,
      files: [
        {
          filePath: "/tmp/local-refactor-fixture/src/sample.ts",
          ruleId: "simplify-conditional",
          summary: "Replaced boolean conditional with direct return.",
          diff: "--- /tmp/local-refactor-fixture/src/sample.ts\n+++ /tmp/local-refactor-fixture/src/sample.ts\n+  return value;\n",
        },
      ],
    },
    metrics: {
      totalRunMs: 100,
      modelEnsureAvailableMs: null,
      fileCollectionMs: 10,
      analyzerPlanningMs: 20,
      modelPlanningMs: null,
      patchPlanValidationMs: null,
      editApplicationMs: 15,
      validationMs: 30,
    },
    ...overrides,
  };
}

export function runDraft(overrides: Partial<RunDraft> = {}): RunDraft {
  return {
    repositoryId: "repo-1",
    repositoryLabel: "Fixture Repo",
    targetRelativePath: ".",
    targetLabel: "Repository root",
    rules: ["simplify-conditional"],
    ruleLabels: ["Simplify Conditional"],
    ruleSummaries: [rules[0]],
    model: "qwen2.5-coder:7b",
    modelLabel: "Qwen2.5 Coder 7B",
    testFileMode: "readOnly",
    validationCommands: ["bun test"],
    protectedPaths: ["src/generated/**"],
    usesModelPlannedRules: false,
    mode: "automatic",
    ruleSelectionPlan,
    candidateFilePreview: candidatePreview(),
    ...overrides,
  };
}
