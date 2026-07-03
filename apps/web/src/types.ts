export type Rule = {
  id: string;
  language: "typescript" | "rust";
  name: string;
  description: string;
  executionKind?: "deterministic" | "modelPlanned";
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
  model: string;
  modelLabel: string;
  testFileMode: "readOnly" | "mutable";
  validationCommands: string[];
  protectedPaths: string[];
  usesModelPlannedRules: boolean;
};
