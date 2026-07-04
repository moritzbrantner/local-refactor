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
import "./styles.css";
import type {
  FolderChildrenResponse,
  FolderEntry,
  ModelsResponse,
  ModelSummary,
  RepositoryPickResponse,
  RepositoryRecord,
  Rule,
  RuleSelectionPlan,
  RuleSelectionPlanResponse,
  RunDraft,
  RunEvent,
  RunRecord,
  RunReviewResponse,
} from "./types";

const ROOT_PATH = ".";
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
  const [selectedModel, setSelectedModel] = useState(DEFAULT_MODEL);
  const [testFileMode, setTestFileMode] = useState<"readOnly" | "mutable">("readOnly");
  const [validationCommands, setValidationCommands] = useState("");
  const [protectedPaths, setProtectedPaths] = useState("src/generated/**");
  const [pendingRunDraft, setPendingRunDraft] = useState<RunDraft | null>(null);
  const [message, setMessage] = useState("");
  const [repositoriesCollapsed, setRepositoriesCollapsed] = useState(false);
  const [foldersCollapsed, setFoldersCollapsed] = useState(false);

  const selectedRepository = useMemo(
    () => repositories.find((repository) => repository.id === selectedRepositoryId) ?? null,
    [repositories, selectedRepositoryId],
  );

  const selectedRun = useMemo(
    () => {
      const listedRun = runs.find((run) => run.id === selectedRunId) ?? runs[0] ?? null;
      if (
        selectedRunReview &&
        selectedRunReview.run.id === (selectedRunId ?? listedRun?.id)
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
    const modelsResponse = await api.get<ModelsResponse>("/api/models");

    setRules(rulesResponse.rules);
    setModels(modelsResponse.models);
    setModelsError(modelsResponse.error ?? "");
    setSelectedModel((current) =>
      modelsResponse.models.some((model) => model.name === current)
        ? current
        : modelsResponse.models[0]?.name ?? DEFAULT_MODEL,
    );
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

  async function loadFolder(repositoryId: string, relativePath: string) {
    const response = await api.get<FolderChildrenResponse>(
      `/api/repositories/${repositoryId}/folders?path=${encodeURIComponent(relativePath)}`,
    );
    setFolderChildren((current) => ({
      ...current,
      [response.path]: response.entries,
    }));
  }

  useEffect(() => {
    refresh().catch((error) => setMessage(error.message));
    const timer = window.setInterval(() => {
      refresh().catch(() => undefined);
    }, 2500);
    return () => window.clearInterval(timer);
  }, [selectedRepositoryId, runHistoryScope]);

  useEffect(() => {
    setFolderChildren({});
    setExpandedFolders(new Set([ROOT_PATH]));
    setSelectedTargetRelativePath(ROOT_PATH);
    setRuleSelectionPlan(null);
    if (!selectedRepositoryId) return;
    loadFolder(selectedRepositoryId, ROOT_PATH).catch((error) => setMessage(error.message));
  }, [selectedRepositoryId]);

  useEffect(() => {
    if (!selectedRepositoryId) {
      setRuleSelectionPlan(null);
      return;
    }
    let ignore = false;
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
    const draft = buildRunDraft();
    if (!draft) return;
    setMessage("");
    setPendingRunDraft(draft);
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

  function buildRunDraft(): RunDraft | null {
    if (!selectedRepositoryId || !selectedRepository) return null;
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

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <h1>local-refactor</h1>
          <p>Local service for scoped, behavior-preserving refactors.</p>
        </div>
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
        <section className={repositoriesCollapsed ? "panel repositories collapsed" : "panel repositories"}>
          <div className="panel-title">
            <GitBranch size={18} />
            <h2>Repositories</h2>
            <button
              type="button"
              className="icon-button panel-toggle"
              aria-controls="repositories-panel-body"
              aria-expanded={!repositoriesCollapsed}
              aria-label={repositoriesCollapsed ? "Restore repositories" : "Minimize repositories"}
              title={repositoriesCollapsed ? "Restore repositories" : "Minimize repositories"}
              onClick={() => setRepositoriesCollapsed((current) => !current)}
            >
              {repositoriesCollapsed ? <Maximize2 size={16} /> : <Minimize2 size={16} />}
            </button>
          </div>

          <div id="repositories-panel-body" className="panel-body" hidden={repositoriesCollapsed}>
            <div className="add-repository">
              <button
                className="primary"
                type="button"
                onClick={() => addRepository().catch((error) => setMessage(error.message))}
                disabled={isPickingRepository}
              >
                <Plus size={18} />
                {isPickingRepository ? "Choosing folder" : "Choose root folder"}
              </button>
            </div>

            <div className="repository-list">
              {repositories.map((repository) => (
                <article
                  className={
                    repository.id === selectedRepositoryId
                      ? "repository-row selected"
                      : "repository-row"
                  }
                  key={repository.id}
                >
                  <div className="repository-select">
                    <input
                      value={editedLabels[repository.id] ?? repository.label}
                      onChange={(event) =>
                        setEditedLabels((current) => ({
                          ...current,
                          [repository.id]: event.target.value,
                        }))
                      }
                      onBlur={() =>
                        renameRepository(repository).catch((error) => setMessage(error.message))
                      }
                    />
                    <button
                      type="button"
                      className="repository-path"
                      onClick={() => setSelectedRepositoryId(repository.id)}
                    >
                      <span className="path">{repository.rootPath}</span>
                      {!repository.available && <span className="unavailable">Unavailable</span>}
                    </button>
                  </div>
                  <button
                    type="button"
                    className="icon-button"
                    onClick={() =>
                      removeRepository(repository.id).catch((error) => setMessage(error.message))
                    }
                    title="Remove"
                  >
                    <Trash2 size={16} />
                  </button>
                </article>
              ))}
              {repositories.length === 0 && <p className="empty">No repositories added.</p>}
            </div>
          </div>
        </section>

        <section className={foldersCollapsed ? "panel folders collapsed" : "panel folders"}>
          <div className="panel-title">
            <Folder size={18} />
            <h2>Folders</h2>
            <button
              type="button"
              className="icon-button panel-toggle"
              aria-controls="folders-panel-body"
              aria-expanded={!foldersCollapsed}
              aria-label={foldersCollapsed ? "Restore folders" : "Minimize folders"}
              title={foldersCollapsed ? "Restore folders" : "Minimize folders"}
              onClick={() => setFoldersCollapsed((current) => !current)}
            >
              {foldersCollapsed ? <Maximize2 size={16} /> : <Minimize2 size={16} />}
            </button>
          </div>
          <div id="folders-panel-body" className="panel-body" hidden={foldersCollapsed}>
            {selectedRepository ? (
              <>
                <div className="selected-source">
                  <strong>{selectedRepository.label}</strong>
                  <span>{selectedRepository.rootPath}</span>
                </div>
                <div className="folder-tree">
                  {renderFolder(ROOT_PATH, selectedRepository.label)}
                </div>
              </>
            ) : (
              <p className="empty">Select a repository.</p>
            )}
          </div>
        </section>

        <section className="panel runs-panel">
          <div className="panel-title">
            <Activity size={18} />
            <h2>Runs</h2>
          </div>

          <form className="run-form" onSubmit={startRun}>
            <div className="target-summary">
              <span>Target</span>
              <strong>{selectedTargetRelativePath === ROOT_PATH ? "Repository root" : selectedTargetRelativePath}</strong>
            </div>

            <div className="field-group">
              <span>Model</span>
              <label className="model-select">
                <Cpu size={16} />
                <select
                  value={selectedModel}
                  onChange={(event) => setSelectedModel(event.target.value)}
                >
                  {models.map((model) => (
                    <option key={model.name} value={model.name}>
                      {model.label} {model.downloaded ? "downloaded" : "not downloaded"}
                    </option>
                  ))}
                </select>
              </label>
              <small className="field-note">
                {models.find((model) => model.name === selectedModel)?.description ??
                  "Selected model is downloaded automatically before the run."}
              </small>
              {modelsError && <small className="field-error">{modelsError}</small>}
            </div>

            <div className="field-group">
              <span>Rules</span>
              <div className="segmented" aria-label="Rule selection mode">
                <button
                  type="button"
                  className={ruleMode === "automatic" ? "active" : ""}
                  onClick={() => setRuleMode("automatic")}
                >
                  Automatic
                </button>
                <button
                  type="button"
                  className={ruleMode === "manual" ? "active" : ""}
                  onClick={() => setRuleMode("manual")}
                >
                  Manual
                </button>
              </div>
              {ruleMode === "automatic" ? (
                <div className="rule-plan">
                  {ruleSelectionError && (
                    <small className="field-error">{ruleSelectionError}</small>
                  )}
                  {ruleSelectionPlan?.segments.map((segment) => (
                    <article className="rule-segment" key={segment.relativePath || "."}>
                      <div className="rule-segment-title">
                        <strong>
                          {segment.relativePath === "" ? "Repository root" : segment.relativePath}
                        </strong>
                        <span>{segment.rules.length} rules</span>
                      </div>
                      <ul className="rule-summary-list">
                        {ruleRecords(segment.rules, rules).map((rule) => (
                          <li key={rule.id}>
                            <strong>{rule.name}</strong>
                            <span className="rule-metadata">
                              <span>{languageLabel(rule.language)}</span>
                              <span>
                                {rule.executionKind === "modelPlanned"
                                  ? "model planned"
                                  : "deterministic"}
                              </span>
                              <span>{formatToken(rule.safetyLevel)}</span>
                              <span>{formatToken(rule.allowedWrites)}</span>
                            </span>
                          </li>
                        ))}
                      </ul>
                      <ul className="reason-list">
                        {segment.reasons.slice(0, 4).map((reason) => (
                          <li key={`${reason.ruleId}-${reason.source}-${reason.message}`}>
                            <span>{formatToken(reason.source)}</span>
                            {reason.message}
                          </li>
                        ))}
                      </ul>
                    </article>
                  ))}
                  {ruleSelectionPlan && ruleSelectionPlan.segments.length === 0 && (
                    <p className="empty">No automatic rules matched this target.</p>
                  )}
                  {!ruleSelectionPlan && !ruleSelectionError && (
                    <p className="empty">Detecting rules for this target.</p>
                  )}
                </div>
              ) : (
                <div className="rule-list">
                  {rules.map((rule) => (
                    <label className="checkbox-row" key={rule.id}>
                      <input
                        type="checkbox"
                        checked={selectedRules.includes(rule.id)}
                        onChange={() => toggleRule(rule.id)}
                      />
                      <span>
                        <span className="rule-heading">
                          <strong>{rule.name}</strong>
                          <span className="language-badge">{languageLabel(rule.language)}</span>
                        </span>
                        <span className="rule-metadata">
                          <span>{rule.executionKind === "modelPlanned" ? "model planned" : "deterministic"}</span>
                          <span>{formatToken(rule.safetyLevel)}</span>
                          <span>{formatToken(rule.allowedWrites)}</span>
                        </span>
                        <small>{rule.description}</small>
                      </span>
                    </label>
                  ))}
                </div>
              )}
            </div>

            <div className="segmented" aria-label="Test file mode">
              <button
                type="button"
                className={testFileMode === "readOnly" ? "active" : ""}
                onClick={() => setTestFileMode("readOnly")}
              >
                <Shield size={16} />
                Tests read-only
              </button>
              <button
                type="button"
                className={testFileMode === "mutable" ? "active" : ""}
                onClick={() => setTestFileMode("mutable")}
              >
                <FileDiff size={16} />
                Tests mutable
              </button>
            </div>

            <label>
              Validation commands
              <textarea
                value={validationCommands}
                onChange={(event) => setValidationCommands(event.target.value)}
                placeholder="bun test&#10;bun run typecheck&#10;cargo check --all-targets&#10;cargo clippy --all-targets -- -D warnings"
              />
            </label>

            <label>
              Protected paths
              <textarea
                value={protectedPaths}
                onChange={(event) => setProtectedPaths(event.target.value)}
              />
            </label>

            <button className="primary" type="submit" disabled={!selectedRepositoryId}>
              <Play size={18} />
              Start run
            </button>
          </form>

          {pendingRunDraft && (
            <section className="run-review">
              <div className="run-review-title">
                <Shield size={17} />
                <h3>Review run</h3>
              </div>
              <dl className="metadata compact-metadata">
                <div>
                  <dt>Repository</dt>
                  <dd>{pendingRunDraft.repositoryLabel}</dd>
                </div>
                <div>
                  <dt>Mutable scope</dt>
                  <dd>{pendingRunDraft.targetLabel}</dd>
                </div>
                <div>
                  <dt>Model</dt>
                  <dd>{pendingRunDraft.modelLabel}</dd>
                </div>
                <div>
                  <dt>Tests</dt>
                  <dd>{pendingRunDraft.testFileMode}</dd>
                </div>
              </dl>
              <div className="settings-grid">
                <section>
                  <h4>{pendingRunDraft.mode === "automatic" ? "Rule selection plan" : "Rules"}</h4>
                  {pendingRunDraft.mode === "automatic" && pendingRunDraft.ruleSelectionPlan ? (
                    <div className="rule-plan compact-plan">
                      {pendingRunDraft.ruleSelectionPlan.segments.map((segment) => (
                        <article className="rule-segment" key={segment.relativePath || "."}>
                          <div className="rule-segment-title">
                            <strong>
                              {segment.relativePath === ""
                                ? "Repository root"
                                : segment.relativePath}
                            </strong>
                            <span>{segment.rules.length} rules</span>
                          </div>
                          <p>{segment.rules.map((ruleId) => ruleName(ruleId, rules)).join(", ")}</p>
                          {ruleRecords(segment.rules, rules).map((rule) => (
                            <span className="rule-preserves" key={rule.id}>
                              Preserves {rule.preserves.map(formatToken).join(", ")}
                            </span>
                          ))}
                        </article>
                      ))}
                    </div>
                  ) : (
                    <ul className="rule-summary-list">
                      {pendingRunDraft.ruleSummaries.map((rule) => (
                        <li key={rule.id}>
                          <strong>{rule.name}</strong>
                          <span className="rule-metadata">
                            <span>{languageLabel(rule.language)}</span>
                            <span>
                              {rule.executionKind === "modelPlanned"
                                ? "model planned"
                                : "deterministic"}
                            </span>
                            <span>{formatToken(rule.safetyLevel)}</span>
                            <span>{formatToken(rule.allowedWrites)}</span>
                          </span>
                          <span className="rule-preserves">
                            Preserves {rule.preserves.map(formatToken).join(", ")}
                          </span>
                        </li>
                      ))}
                    </ul>
                  )}
                </section>
                <section>
                  <h4>Protected paths</h4>
                  {pendingRunDraft.protectedPaths.length > 0 ? (
                    <ul>
                      {pendingRunDraft.protectedPaths.map((path) => (
                        <li key={path}>{path}</li>
                      ))}
                    </ul>
                  ) : (
                    <p className="empty">No custom protected paths.</p>
                  )}
                </section>
              </div>
              <section className="trust-check">
                <h4>Local execution</h4>
                {pendingRunDraft.validationCommands.length > 0 ? (
                  <ul>
                    {pendingRunDraft.validationCommands.map((command) => (
                      <li key={command}>{command}</li>
                    ))}
                  </ul>
                ) : (
                  <p>No custom validation commands were entered.</p>
                )}
                {(pendingRunDraft.validationCommands.length > 0 ||
                  pendingRunDraft.usesModelPlannedRules) && (
                  <p>
                    Confirm only for repositories and commands you trust; validation runs on this
                    machine through the local shell.
                  </p>
                )}
              </section>
              <div className="review-actions">
                <button
                  className="secondary"
                  type="button"
                  onClick={() => setPendingRunDraft(null)}
                >
                  Cancel
                </button>
                <button
                  className="primary"
                  type="button"
                  onClick={() => confirmPendingRun().catch((error) => setMessage(error.message))}
                >
                  <Play size={18} />
                  Confirm and start run
                </button>
              </div>
            </section>
          )}

          <div className="runs-layout">
            <section className="history">
              <div className="history-title">
                <h3>History</h3>
                <div className="segmented compact" aria-label="Run history scope">
                  <button
                    type="button"
                    className={runHistoryScope === "repository" ? "active" : ""}
                    onClick={() => setRunHistoryScope("repository")}
                    disabled={!selectedRepositoryId}
                  >
                    Repository
                  </button>
                  <button
                    type="button"
                    className={runHistoryScope === "all" ? "active" : ""}
                    onClick={() => setRunHistoryScope("all")}
                  >
                    All
                  </button>
                </div>
              </div>
              <div className="run-list">
                {runs.map((run) => (
                  <button
                    key={run.id}
                    className={run.id === selectedRun?.id ? "run-row selected" : "run-row"}
                    onClick={() => setSelectedRunId(run.id)}
                  >
                    <span className={`status ${run.status}`}>{run.status}</span>
                    <span className="path">{runTargetLabel(run)}</span>
                    {run.model && <span className="run-model">{run.model}</span>}
                    <span className="run-repository">{repositoryLabel(run, repositories)}</span>
                    <span className="time">{new Date(run.updatedAt).toLocaleString()}</span>
                  </button>
                ))}
                {runs.length === 0 && (
                  <p className="empty">
                    {runHistoryScope === "repository"
                      ? "No runs for this repository."
                      : "No stored runs."}
                  </p>
                )}
              </div>
            </section>

            <section className="detail">
              <div className="detail-title">
                <h3>Detail</h3>
                {selectedRun && (
                  <button className="secondary" onClick={revertSelectedRun}>
                    <RotateCcw size={16} />
                    Revert
                  </button>
                )}
              </div>

              {selectedRun ? (
                <>
                  <dl className="metadata">
                    <div>
                      <dt>Status</dt>
                      <dd>{selectedRun.status}</dd>
                    </div>
                    <div>
                      <dt>Rules</dt>
                      <dd>{selectedRun.rules.join(", ") || "default"}</dd>
                    </div>
                    <div>
                      <dt>Model</dt>
                      <dd>{selectedRun.model ?? "default"}</dd>
                    </div>
                    <div>
                      <dt>Tests</dt>
                      <dd>{selectedRun.testFileMode}</dd>
                    </div>
                    <div>
                      <dt>Target</dt>
                      <dd>{runTargetLabel(selectedRun)}</dd>
                    </div>
                    <div>
                      <dt>Repository</dt>
                      <dd>{repositoryLabel(selectedRun, repositories)}</dd>
                    </div>
                    <div>
                      <dt>Created</dt>
                      <dd>{new Date(selectedRun.createdAt).toLocaleString()}</dd>
                    </div>
                    <div>
                      <dt>Updated</dt>
                      <dd>{new Date(selectedRun.updatedAt).toLocaleString()}</dd>
                    </div>
                  </dl>

                  <div className="settings-grid">
                    {selectedRun.ruleSelectionPlan && (
                      <section>
                        <h4>Rule selection plan</h4>
                        <div className="rule-plan compact-plan">
                          {selectedRun.ruleSelectionPlan.segments.map((segment) => (
                            <article className="rule-segment" key={segment.relativePath || "."}>
                              <div className="rule-segment-title">
                                <strong>
                                  {segment.relativePath === ""
                                    ? "Repository root"
                                    : segment.relativePath}
                                </strong>
                                <span>{segment.rules.length} rules</span>
                              </div>
                              <p>{segment.rules.map((ruleId) => ruleName(ruleId, rules)).join(", ")}</p>
                            </article>
                          ))}
                        </div>
                      </section>
                    )}
                    <section>
                      <h4>Validation commands</h4>
                      {selectedRun.validationCommands.length > 0 ? (
                        <ul>
                          {selectedRun.validationCommands.map((command) => (
                            <li key={command}>{command}</li>
                          ))}
                        </ul>
                      ) : (
                        <p className="empty">No validation commands recorded.</p>
                      )}
                    </section>
                    <section>
                      <h4>Protected paths</h4>
                      {selectedRun.protectedPaths.length > 0 ? (
                        <ul>
                          {selectedRun.protectedPaths.map((path) => (
                            <li key={path}>{path}</li>
                          ))}
                        </ul>
                      ) : (
                        <p className="empty">No protected paths recorded.</p>
                      )}
                    </section>
                  </div>

                  {selectedRun.validationOutput && (
                    <pre className="log">{selectedRun.validationOutput}</pre>
                  )}
                  {selectedRun.error && <pre className="error">{selectedRun.error}</pre>}

                  {downloadProgress && (
                    <div className="download-progress">
                      <div>
                        <span>{downloadProgress.label}</span>
                        <strong>{downloadProgress.percent}%</strong>
                      </div>
                      <progress value={downloadProgress.percent} max={100} />
                    </div>
                  )}

                  {selectedRunEvents.length > 0 && (
                    <ol className="event-log">
                      {selectedRunEvents.map((event) => (
                        <li key={event.id}>
                          <time>{new Date(event.timestamp).toLocaleTimeString()}</time>
                          <span>{event.message}</span>
                        </li>
                      ))}
                    </ol>
                  )}

                  <div className="diff-stack">
                    {selectedRunReview?.diff.files.map((file) => (
                      <article className="diff-file" key={file.filePath}>
                        <h3>{file.filePath}</h3>
                        {(file.ruleId || file.summary) && (
                          <div className="diff-summary">
                            {file.ruleId && <span>{file.ruleId}</span>}
                            {file.summary && <p>{file.summary}</p>}
                          </div>
                        )}
                        <pre>{file.diff}</pre>
                      </article>
                    ))}
                    {selectedRunReview?.diff.files.length === 0 && (
                      <p className="empty">No file changes recorded.</p>
                    )}
                  </div>
                </>
              ) : (
                <p className="empty">Select a run to inspect.</p>
              )}
            </section>
          </div>
        </section>
      </section>
    </main>
  );
}

function lines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

function languageLabel(language: Rule["language"]): string {
  return language === "rust" ? "Rust" : "TypeScript";
}

function flattenedPlanRules(plan: RuleSelectionPlan): string[] {
  return [...new Set(plan.segments.flatMap((segment) => segment.rules))].sort();
}

function ruleRecords(ruleIds: string[], rules: Rule[]): Rule[] {
  return ruleIds
    .map((ruleId) => rules.find((rule) => rule.id === ruleId))
    .filter((rule): rule is Rule => Boolean(rule));
}

function ruleName(ruleId: string, rules: Rule[]): string {
  return rules.find((rule) => rule.id === ruleId)?.name ?? ruleId;
}

function formatToken(value: string): string {
  return value
    .split("-")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function runTargetLabel(run: RunRecord): string {
  if (run.targetRelativePath && run.targetRelativePath !== ROOT_PATH) {
    return run.targetRelativePath;
  }
  if (run.targetRelativePath === ROOT_PATH) {
    return "Repository root";
  }
  return run.targetPath;
}

function repositoryLabel(run: RunRecord, repositories: RepositoryRecord[]): string {
  const repository = repositories.find((item) => item.id === run.repositoryId);
  if (repository) return repository.label;
  return run.repositoryRootPath ?? run.targetPath;
}

function appendUniqueRunEvent(events: RunEvent[], event: RunEvent): RunEvent[] {
  if (events.some((existing) => existing.id === event.id)) return events;
  return [...events, event].sort((left, right) => left.id - right.id);
}

function latestDownloadProgress(events: RunEvent[]): { label: string; percent: number } | null {
  for (const event of [...events].reverse()) {
    const match = event.message.match(/^(Downloading .+): (\d+)%/);
    if (!match) continue;
    return {
      label: match[1],
      percent: Math.min(100, Number(match[2])),
    };
  }
  return null;
}

createRoot(document.getElementById("root")!).render(<App />);
