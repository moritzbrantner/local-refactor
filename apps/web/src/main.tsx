import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  Activity,
  ChevronDown,
  ChevronRight,
  Cpu,
  FileDiff,
  Folder,
  FolderOpen,
  GitBranch,
  Maximize2,
  Minimize2,
  Play,
  Plus,
  RefreshCcw,
  RotateCcw,
  Shield,
  Trash2,
} from "lucide-react";
import { API_BASE_URL, api } from "./api";
import {
  ConventionsPage,
  FoldersPanel,
  RepositoriesPanel,
  RuleSelectionPanel,
  RunConfigurationForm,
  RunDetail,
  RunHistory,
  RunReviewPanel,
  RunsPanelShell,
} from "./components/panels";
import { CandidateFilePreview, DeterministicPreviewPanel } from "./components/previews";
import "./styles.css";
import type {
  AnalyzerEdit,
  AnalyzerResponse,
  CandidateFilePreviewResponse,
  ConventionSettings,
  DeterministicPreviewResponse,
  FolderChildrenResponse,
  FolderEntry,
  ModelsResponse,
  ModelSummary,
  RepositoryPickResponse,
  RepositoryRecord,
  RepositoryFilePreviewResponse,
  RepositoryConventionsResponse,
  Rule,
  RuleSelectionPlan,
  RuleSelectionPlanResponse,
  RunDraft,
  RunEvent,
  RunRecord,
  RunReviewResponse,
} from "./types";
import {
  appendUniqueRunEvent,
  changedLineNumbers,
  flattenedPlanRules,
  formatToken,
  languageLabel,
  latestDownloadProgress,
  lines,
  pathsEndWithSameFile,
  repositoryFilePath,
  repositoryLabel,
  ROOT_PATH,
  ruleName,
  ruleRecords,
  runTargetLabel,
} from "./view-helpers";

const DEFAULT_MODEL = "qwen2.5-coder:7b";

function App() {
  const [rules, setRules] = useState<Rule[]>([]);
  const [models, setModels] = useState<ModelSummary[]>([]);
  const [modelsError, setModelsError] = useState("");
  const [repositories, setRepositories] = useState<RepositoryRecord[]>([]);
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [selectedRepositoryId, setSelectedRepositoryId] = useState<string | null>(null);
  const [selectedTargetRelativePath, setSelectedTargetRelativePath] = useState(ROOT_PATH);
  const [folderChildren, setFolderChildren] = useState<Record<string, FolderEntry[]>>({});
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(
    () => new Set([ROOT_PATH]),
  );
  const [runHistoryScope, setRunHistoryScope] = useState<"repository" | "all">("repository");
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [selectedRunReview, setSelectedRunReview] = useState<RunReviewResponse | null>(null);
  const [selectedRunEvents, setSelectedRunEvents] = useState<RunEvent[]>([]);
  const [isPickingRepository, setIsPickingRepository] = useState(false);
  const [editedLabels, setEditedLabels] = useState<Record<string, string>>({});
  const [selectedRules, setSelectedRules] = useState<string[]>(["simplify-conditional"]);
  const [ruleSelectionPlan, setRuleSelectionPlan] = useState<RuleSelectionPlan | null>(null);
  const [ruleSelectionError, setRuleSelectionError] = useState("");
  const [ruleMode, setRuleMode] = useState<"automatic" | "manual">("automatic");
  const [rulesSectionCollapsed, setRulesSectionCollapsed] = useState(false);
  const [expandedRuleItems, setExpandedRuleItems] = useState<Set<string>>(() => new Set());
  const [candidateFilePreview, setCandidateFilePreview] =
    useState<CandidateFilePreviewResponse | null>(null);
  const [candidateFilePreviewError, setCandidateFilePreviewError] = useState("");
  const [candidateFilePreviewLoading, setCandidateFilePreviewLoading] = useState(false);
  const [selectedCandidateFilePath, setSelectedCandidateFilePath] = useState<string | null>(null);
  const [filePreview, setFilePreview] = useState<RepositoryFilePreviewResponse | null>(null);
  const [filePreviewLoading, setFilePreviewLoading] = useState(false);
  const [filePreviewError, setFilePreviewError] = useState("");
  const [fileChangePreview, setFileChangePreview] = useState<AnalyzerEdit | null>(null);
  const [fileChangePreviewLoading, setFileChangePreviewLoading] = useState(false);
  const [fileChangePreviewError, setFileChangePreviewError] = useState("");
  const [fileChangePreviewUnavailable, setFileChangePreviewUnavailable] = useState(false);
  const [selectedModel, setSelectedModel] = useState(DEFAULT_MODEL);
  const [testFileMode, setTestFileMode] = useState<"readOnly" | "mutable">("readOnly");
  const [validationCommands, setValidationCommands] = useState("");
  const [protectedPaths, setProtectedPaths] = useState("src/generated/**");
  const [pendingRunDraft, setPendingRunDraft] = useState<RunDraft | null>(null);
  const [deterministicPreview, setDeterministicPreview] =
    useState<DeterministicPreviewResponse | null>(null);
  const [deterministicPreviewLoading, setDeterministicPreviewLoading] = useState(false);
  const [deterministicPreviewApplying, setDeterministicPreviewApplying] = useState(false);
  const [deterministicPreviewError, setDeterministicPreviewError] = useState("");
  const [message, setMessage] = useState("");
  const [repositoriesCollapsed, setRepositoriesCollapsed] = useState(false);
  const [foldersCollapsed, setFoldersCollapsed] = useState(false);
  const [activePage, setActivePage] = useState<"runs" | "conventions">("runs");
  const [repositoryConventions, setRepositoryConventions] =
    useState<RepositoryConventionsResponse | null>(null);
  const [conventionDraft, setConventionDraft] = useState<ConventionSettings | null>(null);
  const [conventionsLoading, setConventionsLoading] = useState(false);
  const [conventionsSaving, setConventionsSaving] = useState(false);
  const [conventionsError, setConventionsError] = useState("");

  const selectedRepository = useMemo(
    () => repositories.find((repository) => repository.id === selectedRepositoryId) ?? null,
    [repositories, selectedRepositoryId],
  );

  const selectedRun = useMemo(
    () => {
      const listedRun = runs.find((run) => run.id === selectedRunId) ?? runs[0] ?? null;
      if (
        selectedRunReview &&
        selectedRunReview.run.id === (selectedRunId ?? listedRun?.id) &&
        (!listedRun || selectedRunReview.run.updatedAt === listedRun.updatedAt)
      ) {
        return selectedRunReview.run;
      }
      return listedRun;
    },
    [runs, selectedRunId, selectedRunReview],
  );

  const downloadProgress = useMemo(
    () => latestDownloadProgress(selectedRunEvents),
    [selectedRunEvents],
  );

  const effectiveRuleIds = useMemo(() => {
    if (ruleMode === "manual") return selectedRules;
    return ruleSelectionPlan ? flattenedPlanRules(ruleSelectionPlan) : [];
  }, [ruleMode, selectedRules, ruleSelectionPlan]);

  const effectiveRuleRecords = useMemo(
    () => ruleRecords(effectiveRuleIds, rules),
    [effectiveRuleIds, rules],
  );

  const usesModelPlannedRules = effectiveRuleRecords.some(
    (rule) => rule.executionKind === "modelPlanned",
  );
  const canUseDeterministicPreview =
    effectiveRuleIds.length > 0 &&
    effectiveRuleRecords.length === effectiveRuleIds.length &&
    !usesModelPlannedRules;

  const ruleSelectionLoading =
    ruleMode === "automatic" && !ruleSelectionPlan && !ruleSelectionError;
  const startRunDisabled =
    !selectedRepositoryId ||
    ruleSelectionLoading ||
    (ruleMode === "automatic" && Boolean(ruleSelectionError)) ||
    candidateFilePreviewLoading ||
    Boolean(candidateFilePreviewError) ||
    !candidateFilePreview ||
    deterministicPreviewLoading ||
    deterministicPreviewApplying;

  async function refresh(
    repositoryId = selectedRepositoryId,
    historyScope = runHistoryScope,
  ) {
    const runsPath = historyScope === "repository" && repositoryId
      ? `/api/runs?repositoryId=${encodeURIComponent(repositoryId)}`
      : "/api/runs";
    const [rulesResponse, repositoriesResponse, runsResponse] = await Promise.all([
      api.get<{ rules: Rule[] }>("/api/rules"),
      api.get<RepositoryRecord[]>("/api/repositories"),
      api.get<RunRecord[]>(runsPath),
    ]);

    setRules(rulesResponse.rules);
    setRepositories(repositoriesResponse);
    setEditedLabels((current) => {
      const next = { ...current };
      for (const repository of repositoriesResponse) {
        if (next[repository.id] === undefined) next[repository.id] = repository.label;
      }
      return next;
    });
    setRuns(runsResponse);
    setSelectedRunId((current) =>
      current && runsResponse.some((run) => run.id === current)
        ? current
        : runsResponse[0]?.id ?? null,
    );

    if (!selectedRepositoryId && repositoriesResponse[0]) {
      setSelectedRepositoryId(repositoriesResponse[0].id);
    }
  }

  async function loadModels() {
    const modelsResponse = await api.get<ModelsResponse>("/api/models");
    setModels(modelsResponse.models);
    setModelsError(modelsResponse.error ?? "");
    setSelectedModel((current) =>
      modelsResponse.models.some((model) => model.name === current)
        ? current
        : modelsResponse.models[0]?.name ?? DEFAULT_MODEL,
    );
  }

  async function loadFolder(repositoryId: string, relativePath: string) {
    const response = await api.get<FolderChildrenResponse>(
      `/api/repositories/${repositoryId}/folders?path=${encodeURIComponent(relativePath)}`,
    );
    setFolderChildren((current) => ({
      ...current,
      [response.path]: response.entries,
    }));
  }

  async function loadConventions(repositoryId: string) {
    setConventionsLoading(true);
    setConventionsError("");
    try {
      const response = await api.get<RepositoryConventionsResponse>(
        `/api/repositories/${repositoryId}/conventions`,
      );
      setRepositoryConventions(response);
      setConventionDraft(response.effective);
    } catch (error) {
      setRepositoryConventions(null);
      setConventionDraft(null);
      setConventionsError(error instanceof Error ? error.message : String(error));
    } finally {
      setConventionsLoading(false);
    }
  }

  useEffect(() => {
    refresh().catch((error) => setMessage(error.message));
    const timer = window.setInterval(() => {
      refresh().catch(() => undefined);
    }, 2500);
    return () => window.clearInterval(timer);
  }, [selectedRepositoryId, runHistoryScope]);

  useEffect(() => {
    if (!usesModelPlannedRules) {
      setModelsError("");
      return;
    }
    loadModels().catch((error) => setModelsError(error.message));
  }, [usesModelPlannedRules]);

  useEffect(() => {
    setFolderChildren({});
    setExpandedFolders(new Set([ROOT_PATH]));
    setSelectedTargetRelativePath(ROOT_PATH);
    setRuleSelectionPlan(null);
    setRepositoryConventions(null);
    setConventionDraft(null);
    setConventionsError("");
    if (!selectedRepositoryId) return;
    loadFolder(selectedRepositoryId, ROOT_PATH).catch((error) => setMessage(error.message));
    loadConventions(selectedRepositoryId).catch((error) => setConventionsError(error.message));
  }, [selectedRepositoryId]);

  useEffect(() => {
    if (!selectedRepositoryId) {
      setRuleSelectionPlan(null);
      return;
    }
    let ignore = false;
    setRuleSelectionPlan(null);
    setRuleSelectionError("");
    api
      .post<RuleSelectionPlanResponse>("/api/rule-selection/plan", {
        repositoryId: selectedRepositoryId,
        targetRelativePath: selectedTargetRelativePath,
        testFileMode,
        protectedPaths: lines(protectedPaths),
      })
      .then((response) => {
        if (!ignore) setRuleSelectionPlan(response.plan);
      })
      .catch((error) => {
        if (!ignore) {
          setRuleSelectionPlan(null);
          setRuleSelectionError(error.message);
        }
      });
    return () => {
      ignore = true;
    };
  }, [selectedRepositoryId, selectedTargetRelativePath, testFileMode, protectedPaths]);

  useEffect(() => {
    setPendingRunDraft(null);
    setDeterministicPreview(null);
    setDeterministicPreviewError("");
    setDeterministicPreviewLoading(false);
    setDeterministicPreviewApplying(false);
  }, [
    selectedRepositoryId,
    selectedTargetRelativePath,
    ruleMode,
    selectedRules,
    ruleSelectionPlan,
    testFileMode,
    protectedPaths,
    selectedModel,
  ]);

  useEffect(() => {
    setSelectedCandidateFilePath(null);
    setFilePreview(null);
    setFilePreviewError("");
    setFilePreviewLoading(false);
    setFileChangePreview(null);
    setFileChangePreviewError("");
    setFileChangePreviewLoading(false);
    setFileChangePreviewUnavailable(false);
  }, [
    selectedRepositoryId,
    selectedTargetRelativePath,
    ruleMode,
    selectedRules,
    ruleSelectionPlan,
    testFileMode,
    protectedPaths,
  ]);

  useEffect(() => {
    if (!candidateFilePreview || !selectedCandidateFilePath) return;
    const visiblePaths = new Set(
      candidateFilePreview.groups.flatMap((group) =>
        group.files.map((file) => file.relativePath),
      ),
    );
    if (!visiblePaths.has(selectedCandidateFilePath)) {
      setSelectedCandidateFilePath(null);
      setFilePreview(null);
      setFilePreviewError("");
      setFilePreviewLoading(false);
      setFileChangePreview(null);
      setFileChangePreviewError("");
      setFileChangePreviewLoading(false);
      setFileChangePreviewUnavailable(false);
    }
  }, [candidateFilePreview, selectedCandidateFilePath]);

  useEffect(() => {
    if (!selectedRepositoryId) {
      setCandidateFilePreview(null);
      setCandidateFilePreviewError("");
      setCandidateFilePreviewLoading(false);
      return;
    }
    if (ruleMode === "manual" && selectedRules.length === 0) {
      setCandidateFilePreview(null);
      setCandidateFilePreviewError("Select at least one rule to preview candidate files.");
      setCandidateFilePreviewLoading(false);
      return;
    }
    if (ruleMode === "automatic") {
      if (ruleSelectionError) {
        setCandidateFilePreview(null);
        setCandidateFilePreviewError(ruleSelectionError);
        setCandidateFilePreviewLoading(false);
        return;
      }
      const planTargetRelativePath =
        ruleSelectionPlan?.targetRelativePath === "" ? ROOT_PATH : ruleSelectionPlan?.targetRelativePath;
      if (!ruleSelectionPlan || planTargetRelativePath !== selectedTargetRelativePath) {
        setCandidateFilePreview(null);
        setCandidateFilePreviewError("");
        setCandidateFilePreviewLoading(true);
        return;
      }
    }

    let ignore = false;
    setCandidateFilePreviewLoading(true);
    setCandidateFilePreviewError("");
    const timer = window.setTimeout(() => {
      api
        .post<CandidateFilePreviewResponse>("/api/runs/candidate-file-preview", {
          repositoryId: selectedRepositoryId,
          targetRelativePath: selectedTargetRelativePath,
          rules: ruleMode === "manual" ? selectedRules : [],
          ruleSelectionPlan: ruleMode === "automatic" ? ruleSelectionPlan : undefined,
          model: selectedModel,
          testFileMode,
          protectedPaths: lines(protectedPaths),
          limitPerGroup: 50,
        })
        .then((preview) => {
          if (!ignore) {
            setCandidateFilePreview(preview);
            setCandidateFilePreviewError("");
          }
        })
        .catch((error) => {
          if (!ignore) {
            setCandidateFilePreview(null);
            setCandidateFilePreviewError(error.message);
          }
        })
        .finally(() => {
          if (!ignore) setCandidateFilePreviewLoading(false);
        });
    }, 300);

    return () => {
      ignore = true;
      window.clearTimeout(timer);
    };
  }, [
    selectedRepositoryId,
    selectedTargetRelativePath,
    ruleMode,
    selectedRules,
    ruleSelectionPlan,
    ruleSelectionError,
    testFileMode,
    protectedPaths,
    selectedModel,
  ]);

  useEffect(() => {
    if (!selectedRun?.id) {
      setSelectedRunReview(null);
      return;
    }
    let ignore = false;
    api
      .get<RunReviewResponse>(`/api/runs/${selectedRun.id}/review`)
      .then((review) => {
        if (!ignore) {
          setSelectedRunReview(review);
          setSelectedRunEvents(review.events);
        }
      })
      .catch(() => {
        if (!ignore) setSelectedRunReview(null);
      });
    return () => {
      ignore = true;
    };
  }, [selectedRun?.id, selectedRun?.updatedAt]);

  useEffect(() => {
    if (!selectedRun?.id) {
      setSelectedRunEvents([]);
      return;
    }

    setSelectedRunEvents([]);
    const source = new EventSource(`${API_BASE_URL}/api/runs/${selectedRun.id}/events`);
    source.addEventListener("run-event", (event) => {
      const runEvent = JSON.parse((event as MessageEvent).data) as RunEvent;
      setSelectedRunEvents((current) => appendUniqueRunEvent(current, runEvent));
      setSelectedRunReview((current) =>
        current && current.run.id === runEvent.runId
          ? { ...current, events: appendUniqueRunEvent(current.events, runEvent) }
          : current,
      );
    });
    source.onerror = () => source.close();
    return () => source.close();
  }, [selectedRun?.id]);

  async function addRepository() {
    setMessage("Choose a Git repository root in the system folder dialog.");
    setIsPickingRepository(true);
    try {
      const response = await api.post<RepositoryPickResponse>("/api/repositories/pick");
      if (!response.repository) {
        setMessage("No folder selected.");
        return;
      }
      setSelectedRepositoryId(response.repository.id);
      setRunHistoryScope("repository");
      await refresh(response.repository.id, "repository");
      setMessage("");
    } finally {
      setIsPickingRepository(false);
    }
  }

  async function renameRepository(repository: RepositoryRecord) {
    const label = editedLabels[repository.id]?.trim() ?? "";
    if (!label || label === repository.label) return;
    const updated = await api.patch<RepositoryRecord>(`/api/repositories/${repository.id}`, {
      label,
    });
    setRepositories((current) =>
      current.map((item) => (item.id === updated.id ? updated : item)),
    );
    setEditedLabels((current) => ({ ...current, [updated.id]: updated.label }));
  }

  async function removeRepository(repositoryId: string) {
    await api.delete(`/api/repositories/${repositoryId}`);
    const nextRepositoryId =
      repositories.find((repository) => repository.id !== repositoryId)?.id ?? null;
    if (selectedRepositoryId === repositoryId) {
      setSelectedRepositoryId(nextRepositoryId);
    }
    await refresh(selectedRepositoryId === repositoryId ? nextRepositoryId : selectedRepositoryId);
  }

  async function saveConventionOverride() {
    if (!selectedRepositoryId || !conventionDraft) return;
    setConventionsSaving(true);
    setConventionsError("");
    try {
      const response = await api.patch<RepositoryConventionsResponse>(
        `/api/repositories/${selectedRepositoryId}/conventions/local-override`,
        conventionDraft,
      );
      setRepositoryConventions(response);
      setConventionDraft(response.effective);
    } catch (error) {
      setConventionsError(error instanceof Error ? error.message : String(error));
    } finally {
      setConventionsSaving(false);
    }
  }

  async function resetConventionOverride() {
    if (!selectedRepositoryId) return;
    setConventionsSaving(true);
    setConventionsError("");
    try {
      const response = await api.patch<RepositoryConventionsResponse>(
        `/api/repositories/${selectedRepositoryId}/conventions/local-override`,
        {},
      );
      setRepositoryConventions(response);
      setConventionDraft(response.effective);
    } catch (error) {
      setConventionsError(error instanceof Error ? error.message : String(error));
    } finally {
      setConventionsSaving(false);
    }
  }

  async function toggleFolder(relativePath: string) {
    if (!selectedRepositoryId) return;
    const isExpanded = expandedFolders.has(relativePath);
    setExpandedFolders((current) => {
      const next = new Set(current);
      if (isExpanded) next.delete(relativePath);
      else next.add(relativePath);
      return next;
    });
    if (!isExpanded && folderChildren[relativePath] === undefined) {
      await loadFolder(selectedRepositoryId, relativePath);
    }
  }

  async function startRun(event: React.FormEvent) {
    event.preventDefault();
    if (canUseDeterministicPreview) {
      await previewDeterministicChanges();
      return;
    }
    const draft = buildRunDraft();
    if (!draft) return;
    setMessage("");
    setPendingRunDraft(draft);
  }

  function buildRunRequestFromCurrentConfig() {
    return {
      repositoryId: selectedRepositoryId,
      targetRelativePath: selectedTargetRelativePath,
      rules: ruleMode === "manual" ? selectedRules : [],
      ruleSelectionPlan: ruleMode === "automatic" ? ruleSelectionPlan : undefined,
      model: usesModelPlannedRules ? selectedModel : undefined,
      testFileMode,
      validationCommands: lines(validationCommands),
      protectedPaths: lines(protectedPaths),
      repairBudget: 2,
    };
  }

  async function previewDeterministicChanges() {
    if (!selectedRepositoryId || !candidateFilePreview || !canUseDeterministicPreview) return;
    setMessage("");
    setDeterministicPreviewLoading(true);
    setDeterministicPreviewError("");
    setDeterministicPreview(null);
    try {
      const preview = await api.post<DeterministicPreviewResponse>(
        "/api/runs/deterministic-preview",
        buildRunRequestFromCurrentConfig(),
      );
      setDeterministicPreview(preview);
    } catch (error) {
      setDeterministicPreviewError(error instanceof Error ? error.message : String(error));
    } finally {
      setDeterministicPreviewLoading(false);
    }
  }

  async function applyDeterministicPreview() {
    if (!deterministicPreview) return;
    setDeterministicPreviewApplying(true);
    setDeterministicPreviewError("");
    try {
      const response = await api.post<{ id: string }>(
        "/api/runs/deterministic-preview/apply",
        {
          run: buildRunRequestFromCurrentConfig(),
          previewFingerprint: deterministicPreview.previewFingerprint,
        },
      );
      setDeterministicPreview(null);
      setSelectedRunId(response.id);
      setRunHistoryScope("repository");
      await refresh();
    } catch (error) {
      setDeterministicPreviewError(error instanceof Error ? error.message : String(error));
    } finally {
      setDeterministicPreviewApplying(false);
    }
  }

  async function confirmPendingRun() {
    if (!pendingRunDraft) return;
    const response = await api.post<{ id: string }>("/api/runs", {
      repositoryId: pendingRunDraft.repositoryId,
      targetRelativePath: pendingRunDraft.targetRelativePath,
      rules: pendingRunDraft.mode === "manual" ? pendingRunDraft.rules : [],
      ruleSelectionPlan:
        pendingRunDraft.mode === "automatic" ? pendingRunDraft.ruleSelectionPlan : undefined,
      model: pendingRunDraft.model,
      testFileMode: pendingRunDraft.testFileMode,
      validationCommands: pendingRunDraft.validationCommands,
      protectedPaths: pendingRunDraft.protectedPaths,
      repairBudget: 2,
    });
    setPendingRunDraft(null);
    setSelectedRunId(response.id);
    setRunHistoryScope("repository");
    await refresh();
  }

  async function revertSelectedRun() {
    if (!selectedRun) return;
    await api.post(`/api/runs/${selectedRun.id}/revert`);
    await refresh();
  }

  function toggleRule(ruleId: string) {
    setSelectedRules((current) =>
      current.includes(ruleId)
        ? current.filter((id) => id !== ruleId)
        : [...current, ruleId],
    );
  }

  function toggleRuleItem(itemId: string) {
    setExpandedRuleItems((current) => {
      const next = new Set(current);
      if (next.has(itemId)) next.delete(itemId);
      else next.add(itemId);
      return next;
    });
  }

  async function loadFilePreview(relativePath: string) {
    if (!selectedRepositoryId) return;
    setSelectedCandidateFilePath(relativePath);
    setFilePreview(null);
    setFilePreviewError("");
    setFilePreviewLoading(true);
    setFileChangePreview(null);
    setFileChangePreviewError("");
    setFileChangePreviewLoading(false);
    setFileChangePreviewUnavailable(false);
    try {
      const preview = await api.get<RepositoryFilePreviewResponse>(
        `/api/repositories/${selectedRepositoryId}/file-preview?path=${encodeURIComponent(
          relativePath,
        )}`,
      );
      setFilePreview(preview);
      await loadFileChangePreview(relativePath);
    } catch (error) {
      setFilePreviewError(error instanceof Error ? error.message : String(error));
    } finally {
      setFilePreviewLoading(false);
    }
  }

  async function loadFileChangePreview(relativePath: string) {
    if (!selectedRepository) return;
    const previewRuleIds = deterministicPreviewRuleIds(relativePath);
    if (previewRuleIds.length === 0) {
      setFileChangePreviewUnavailable(true);
      return;
    }

    setFileChangePreviewLoading(true);
    setFileChangePreviewError("");
    setFileChangePreviewUnavailable(false);
    try {
      const response = await api.post<AnalyzerResponse>("/api/analyze", {
        targetPath: repositoryFilePath(selectedRepository.rootPath, relativePath),
        rules: previewRuleIds,
        testFileMode,
        protectedPaths: lines(protectedPaths),
      });
      setFileChangePreview(
        response.edits.find((edit) => pathsEndWithSameFile(edit.filePath, relativePath)) ??
          null,
      );
    } catch (error) {
      setFileChangePreviewError(error instanceof Error ? error.message : String(error));
    } finally {
      setFileChangePreviewLoading(false);
    }
  }

  function deterministicPreviewRuleIds(relativePath: string): string[] {
    if (!candidateFilePreview) return [];
    const ruleIds = candidateFilePreview.groups
      .filter(
        (group) =>
          group.language === "typescript" &&
          group.files.some((file) => file.relativePath === relativePath),
      )
      .map((group) => group.ruleId)
      .filter((ruleId) => {
        const rule = rules.find((candidate) => candidate.id === ruleId);
        return rule?.language === "typescript" && rule.executionKind === "deterministic";
      });
    return [...new Set(ruleIds)].sort();
  }

  function buildRunDraft(): RunDraft | null {
    if (!selectedRepositoryId || !selectedRepository) return null;
    if (!candidateFilePreview || candidateFilePreviewLoading || candidateFilePreviewError) return null;
    const automaticRules =
      ruleSelectionPlan && ruleMode === "automatic" ? flattenedPlanRules(ruleSelectionPlan) : [];
    const draftRuleIds = ruleMode === "automatic" ? automaticRules : selectedRules;
    const selectedRuleRecords = ruleRecords(draftRuleIds, rules);
    const model = models.find((candidate) => candidate.name === selectedModel);
    return {
      repositoryId: selectedRepositoryId,
      repositoryLabel: selectedRepository.label,
      targetRelativePath: selectedTargetRelativePath,
      targetLabel:
        selectedTargetRelativePath === ROOT_PATH ? "Repository root" : selectedTargetRelativePath,
      rules: draftRuleIds,
      ruleLabels:
        selectedRuleRecords.length > 0
          ? selectedRuleRecords.map((rule) => rule.name)
          : draftRuleIds,
      ruleSummaries: selectedRuleRecords,
      model: selectedModel,
      modelLabel: model?.label ?? selectedModel,
      testFileMode,
      validationCommands: lines(validationCommands),
      protectedPaths: lines(protectedPaths),
      usesModelPlannedRules: selectedRuleRecords.some(
        (rule) => rule.executionKind === "modelPlanned",
      ),
      mode: ruleMode,
      ruleSelectionPlan: ruleMode === "automatic" ? ruleSelectionPlan ?? undefined : undefined,
      candidateFilePreview,
    };
  }

  function renderFolder(relativePath: string, name: string, depth = 0) {
    const isExpanded = expandedFolders.has(relativePath);
    const isSelected = selectedTargetRelativePath === relativePath;
    const children = folderChildren[relativePath] ?? [];

    return (
      <div className="folder-node" key={relativePath}>
        <div className="folder-row" style={{ paddingLeft: 8 + depth * 16 }}>
          <button
            type="button"
            className="tree-toggle"
            onClick={() => toggleFolder(relativePath).catch((error) => setMessage(error.message))}
            title={isExpanded ? "Collapse" : "Expand"}
          >
            {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
          </button>
          <button
            type="button"
            className={isSelected ? "folder-select selected" : "folder-select"}
            onClick={() => setSelectedTargetRelativePath(relativePath)}
          >
            {isExpanded ? <FolderOpen size={16} /> : <Folder size={16} />}
            <span>{name}</span>
          </button>
        </div>
        {isExpanded && (
          <div>
            {children.map((child) =>
              renderFolder(child.relativePath, child.name, depth + 1),
            )}
          </div>
        )}
      </div>
    );
  }

  function rulesSummaryText() {
    if (ruleMode === "manual") {
      return `${selectedRules.length} selected rules`;
    }
    if (ruleSelectionError) {
      return "Automatic selection error";
    }
    if (!ruleSelectionPlan) {
      return "Detecting rules";
    }
    return `${ruleSelectionPlan.segments.length} segments, ${
      flattenedPlanRules(ruleSelectionPlan).length
    } rules`;
  }

  function selectedConventionRules() {
    return effectiveRuleRecords.filter((rule) =>
      [
        "format-typescript",
        "format-rust",
        "sort-typescript-class-members",
        "sort-rust-use-items",
        "sort-rust-impl-members",
        "normalize-imports",
        "sort-independent-declarations",
      ].includes(rule.id),
    );
  }

  function renderCandidateFilePreview(
    preview: CandidateFilePreviewResponse,
    options: { interactive?: boolean } = {},
  ) {
    const interactive = options.interactive ?? true;
    return (
      <CandidateFilePreview
        preview={preview}
        interactive={interactive}
        selectedCandidateFilePath={selectedCandidateFilePath}
        filePreview={filePreview}
        filePreviewLoading={filePreviewLoading}
        filePreviewError={filePreviewError}
        fileChangePreview={fileChangePreview}
        fileChangePreviewLoading={fileChangePreviewLoading}
        fileChangePreviewError={fileChangePreviewError}
        fileChangePreviewUnavailable={fileChangePreviewUnavailable}
        onSelectFile={(relativePath) =>
          loadFilePreview(relativePath).catch((error) => setMessage(error.message))
        }
      />
    );
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <h1>local-refactor</h1>
          <p>Local service for scoped, behavior-preserving refactors.</p>
        </div>
        <nav className="topnav" aria-label="Primary">
          <button
            type="button"
            className={activePage === "runs" ? "selected" : ""}
            onClick={() => setActivePage("runs")}
          >
            <Activity size={16} />
            Runs
          </button>
          <button
            type="button"
            className={activePage === "conventions" ? "selected" : ""}
            onClick={() => setActivePage("conventions")}
          >
            <Shield size={16} />
            Conventions
          </button>
        </nav>
        <button className="icon-button" onClick={() => refresh()} title="Refresh">
          <RefreshCcw size={18} />
        </button>
      </header>

      {message && <div className="notice">{message}</div>}

      <section
        className={[
          "workspace",
          repositoriesCollapsed ? "repositories-collapsed" : "",
          foldersCollapsed ? "folders-collapsed" : "",
        ]
          .filter(Boolean)
          .join(" ")}
      >
        <RepositoriesPanel
          collapsed={repositoriesCollapsed}
          repositories={repositories}
          selectedRepositoryId={selectedRepositoryId}
          editedLabels={editedLabels}
          isPickingRepository={isPickingRepository}
          onToggleCollapsed={() => setRepositoriesCollapsed((current) => !current)}
          onAddRepository={() => addRepository().catch((error) => setMessage(error.message))}
          onSelectRepository={setSelectedRepositoryId}
          onEditLabel={(repositoryId, label) =>
            setEditedLabels((current) => ({ ...current, [repositoryId]: label }))
          }
          onRenameRepository={(repository) =>
            renameRepository(repository).catch((error) => setMessage(error.message))
          }
          onRemoveRepository={(repositoryId) =>
            removeRepository(repositoryId).catch((error) => setMessage(error.message))
          }
        />

        <FoldersPanel
          collapsed={foldersCollapsed}
          selectedRepository={selectedRepository}
          selectedTargetRelativePath={selectedTargetRelativePath}
          expandedFolders={expandedFolders}
          folderChildren={folderChildren}
          onToggleCollapsed={() => setFoldersCollapsed((current) => !current)}
          onToggleFolder={(relativePath) =>
            toggleFolder(relativePath).catch((error) => setMessage(error.message))
          }
          onSelectTarget={setSelectedTargetRelativePath}
        />

        {activePage === "conventions" ? (
          <ConventionsPage
            selectedRepository={selectedRepository}
            conventions={repositoryConventions}
            draft={conventionDraft}
            loading={conventionsLoading}
            saving={conventionsSaving}
            error={conventionsError}
            onChangeDraft={setConventionDraft}
            onSave={() => saveConventionOverride().catch((error) => setMessage(error.message))}
            onReset={() => resetConventionOverride().catch((error) => setMessage(error.message))}
          />
        ) : (
        <RunsPanelShell>
          {selectedConventionRules().length > 0 && repositoryConventions && (
            <section className="convention-run-summary">
              <h3>Convention Settings</h3>
              <p>
                {repositoryConventions.effective.profile} profile, TypeScript formatter{" "}
                {repositoryConventions.effective.typescript.formatter.enabled ? "on" : "off"}, Rust formatter{" "}
                {repositoryConventions.effective.rust.formatter.enabled ? "on" : "off"}.
              </p>
            </section>
          )}
          <RunConfigurationForm
            selectedTargetRelativePath={selectedTargetRelativePath}
            selectedModel={selectedModel}
            models={models}
            modelsError={modelsError}
            testFileMode={testFileMode}
            validationCommands={validationCommands}
            protectedPaths={protectedPaths}
            startRunDisabled={startRunDisabled}
            usesModelPlannedRules={usesModelPlannedRules}
            primaryActionLabel={canUseDeterministicPreview ? "Preview changes" : "Start run"}
            onSubmit={startRun}
            onSelectModel={setSelectedModel}
            onSetTestFileMode={setTestFileMode}
            onSetValidationCommands={setValidationCommands}
            onSetProtectedPaths={setProtectedPaths}
            ruleSelection={
              <RuleSelectionPanel
                rules={rules}
                ruleMode={ruleMode}
                selectedRules={selectedRules}
                ruleSelectionPlan={ruleSelectionPlan}
                ruleSelectionError={ruleSelectionError}
                rulesSectionCollapsed={rulesSectionCollapsed}
                expandedRuleItems={expandedRuleItems}
                rulesSummaryText={rulesSummaryText()}
                onToggleCollapsed={() => setRulesSectionCollapsed((current) => !current)}
                onSetRuleMode={setRuleMode}
                onToggleRule={toggleRule}
                onToggleRuleItem={toggleRuleItem}
              />
            }
            candidatePreviewState={
              <>
                {candidateFilePreviewLoading && (
                  <p className="empty">Previewing candidate files.</p>
                )}
                {candidateFilePreviewError && (
                  <small className="field-error">{candidateFilePreviewError}</small>
                )}
                {candidateFilePreview && renderCandidateFilePreview(candidateFilePreview)}
                {!candidateFilePreviewLoading &&
                  !candidateFilePreviewError &&
                  !candidateFilePreview && (
                    <p className="empty">Candidate files appear after selecting a repository.</p>
                  )}
              </>
            }
          />

          {pendingRunDraft && (
            <RunReviewPanel
              draft={pendingRunDraft}
              rules={rules}
              onCancel={() => setPendingRunDraft(null)}
              onConfirm={() => confirmPendingRun().catch((error) => setMessage(error.message))}
            />
          )}

          {deterministicPreviewLoading && (
            <p className="empty">Generating deterministic preview.</p>
          )}

          {deterministicPreview && (
            <DeterministicPreviewPanel
              preview={deterministicPreview}
              applying={deterministicPreviewApplying}
              error={deterministicPreviewError}
              onCancel={() => {
                setDeterministicPreview(null);
                setDeterministicPreviewError("");
              }}
              onApply={() => applyDeterministicPreview().catch((error) => setMessage(error.message))}
            />
          )}

          {!deterministicPreview && deterministicPreviewError && (
            <small className="field-error">{deterministicPreviewError}</small>
          )}

          <div className="runs-layout">
            <RunHistory
              runs={runs}
              repositories={repositories}
              selectedRun={selectedRun}
              runHistoryScope={runHistoryScope}
              selectedRepositoryId={selectedRepositoryId}
              onSetRunHistoryScope={setRunHistoryScope}
              onSelectRun={setSelectedRunId}
            />

            <RunDetail
              selectedRun={selectedRun}
              repositories={repositories}
              selectedRunReview={selectedRunReview}
              selectedRunEvents={selectedRunEvents}
              downloadProgress={downloadProgress}
              rules={rules}
              onRevert={() => revertSelectedRun().catch((error) => setMessage(error.message))}
            />
          </div>
        </RunsPanelShell>
        )}
      </section>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
