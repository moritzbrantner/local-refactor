export type Rule = {
  id: string;
  language: "typescript" | "rust";
  name: string;
  description: string;
  executionKind?: "deterministic" | "modelPlanned";
  allowedWrites: "single-file" | "multi-file-within-target";
  category:
    | "control-flow"
    | "naming"
    | "extraction"
    | "deduplication"
    | "module-organization"
    | "type-structure"
    | "declaration-organization"
    | "documentation";
  safetyLevel: "syntax-only" | "typecheck-required" | "test-required";
  preserves: Array<
    | "runtime-behavior"
    | "exports"
    | "public-api"
    | "typecheck"
    | "comments"
    | "formatting-intent"
  >;
  requiresTypeInformation: boolean;
  requiresImportGraph: boolean;
  planningProfile:
    | "local-transformation"
    | "local-extraction"
    | "module-split"
    | "public-contract-shape"
    | "documentation-only";
};

export type RepositoryRecord = {
  id: string;
  label: string;
  rootPath: string;
  available: boolean;
  createdAt: string;
  updatedAt: string;
};

export type RepositoryPickResponse = {
  repository: RepositoryRecord | null;
};

export type FolderEntry = {
  name: string;
  relativePath: string;
};

export type FolderChildrenResponse = {
  repositoryId: string;
  path: string;
  entries: FolderEntry[];
};

export type RunRecord = {
  id: string;
  targetPath: string;
  status: string;
  createdAt: string;
  updatedAt: string;
  rules: string[];
  ruleSelectionPlan?: RuleSelectionPlan | null;
  model?: string;
  testFileMode: string;
  validationCommands: string[];
  protectedPaths: string[];
  validationOutput?: string;
  error?: string;
  repositoryId?: string;
  repositoryRootPath?: string;
  targetRelativePath?: string;
};

export type RuleSelectionReason = {
  ruleId: string;
  source: "config" | "content" | "fallback";
  message: string;
};

export type RuleSelectionSegment = {
  relativePath: string;
  rules: string[];
  reasons: RuleSelectionReason[];
};

export type RuleSelectionPlan = {
  targetRelativePath: string;
  segments: RuleSelectionSegment[];
};

export type RuleSelectionPlanResponse = {
  plan: RuleSelectionPlan;
  effectiveConfig: {
    rules: string[];
    protectedPaths: string[];
    validationCommands: string[];
    testFileMode: "readOnly" | "mutable";
  };
};

export type RunEvent = {
  id: number;
  runId: string;
  timestamp: string;
  message: string;
};

export type DiffFile = {
  filePath: string;
  ruleId?: string;
  summary?: string;
  diff: string;
};

export type DiffResponse = {
  runId: string;
  files: DiffFile[];
};

export type RunMetrics = {
  totalRunMs: number | null;
  modelEnsureAvailableMs: number | null;
  fileCollectionMs: number | null;
  analyzerPlanningMs: number | null;
  modelPlanningMs: number | null;
  patchPlanValidationMs: number | null;
  editApplicationMs: number | null;
  validationMs: number | null;
};

export type RunReviewResponse = {
  run: RunRecord;
  events: RunEvent[];
  diff: DiffResponse;
  metrics: RunMetrics;
};

export type CandidateFilePreviewFile = {
  relativePath: string;
};

export type CandidateFilePreviewGroup = {
  id: string;
  label: string;
  segmentRelativePath?: string;
  ruleId: string;
  ruleName: string;
  language: "typescript" | "rust";
  totalFiles: number;
  hiddenFiles: number;
  files: CandidateFilePreviewFile[];
};

export type CandidateFilePreviewResponse = {
  targetRelativePath: string;
  totalCandidateFiles: number;
  limitPerGroup: number;
  groups: CandidateFilePreviewGroup[];
};

export type RepositoryFilePreviewResponse = {
  repositoryId: string;
  relativePath: string;
  language: "typescript" | "rust" | "text";
  content: string;
  sizeBytes: number;
};

export type AnalyzerEdit = {
  filePath: string;
  originalContent: string;
  newContent: string;
  ruleId: string;
  summary: string;
};

export type AnalyzerResponse = {
  edits: AnalyzerEdit[];
  diagnostics: string[];
};

export type ModelSummary = {
  name: string;
  label: string;
  description: string;
  downloaded: boolean;
};

export type ModelsResponse = {
  provider: string;
  models: ModelSummary[];
  error?: string;
};

export type RunDraft = {
  repositoryId: string;
  repositoryLabel: string;
  targetRelativePath: string;
  targetLabel: string;
  rules: string[];
  ruleLabels: string[];
  ruleSummaries: Rule[];
  model: string;
  modelLabel: string;
  testFileMode: "readOnly" | "mutable";
  validationCommands: string[];
  protectedPaths: string[];
  usesModelPlannedRules: boolean;
  mode: "automatic" | "manual";
  ruleSelectionPlan?: RuleSelectionPlan;
  candidateFilePreview: CandidateFilePreviewResponse;
};
