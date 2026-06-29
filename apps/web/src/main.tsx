import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  Activity,
  ChevronDown,
  ChevronRight,
  FileDiff,
  Folder,
  FolderOpen,
  GitBranch,
  Play,
  Plus,
  RefreshCcw,
  RotateCcw,
  Shield,
  Trash2,
} from "lucide-react";
import "./styles.css";

type Rule = {
  id: string;
  name: string;
  description: string;
};

type RepositoryRecord = {
  id: string;
  label: string;
  rootPath: string;
  available: boolean;
  createdAt: string;
  updatedAt: string;
};

type FolderEntry = {
  name: string;
  relativePath: string;
};

type FolderChildrenResponse = {
  repositoryId: string;
  path: string;
  entries: FolderEntry[];
};

type RunRecord = {
  id: string;
  targetPath: string;
  status: string;
  createdAt: string;
  updatedAt: string;
  rules: string[];
  testFileMode: string;
  validationCommands: string[];
  protectedPaths: string[];
  validationOutput?: string;
  error?: string;
  repositoryId?: string;
  repositoryRootPath?: string;
  targetRelativePath?: string;
};

type DiffResponse = {
  runId: string;
  files: Array<{ filePath: string; diff: string }>;
};

const ROOT_PATH = ".";

const api = {
  async get<T>(path: string): Promise<T> {
    const response = await fetch(path);
    if (!response.ok) throw new Error(await response.text());
    return response.json() as Promise<T>;
  },
  async post<T>(path: string, body?: unknown): Promise<T> {
    const response = await fetch(path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!response.ok) throw new Error(await response.text());
    if (response.status === 204) return undefined as T;
    return response.json() as Promise<T>;
  },
  async patch<T>(path: string, body: unknown): Promise<T> {
    const response = await fetch(path, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!response.ok) throw new Error(await response.text());
    return response.json() as Promise<T>;
  },
  async delete(path: string): Promise<void> {
    const response = await fetch(path, { method: "DELETE" });
    if (!response.ok) throw new Error(await response.text());
  },
};

function App() {
  const [rules, setRules] = useState<Rule[]>([]);
  const [repositories, setRepositories] = useState<RepositoryRecord[]>([]);
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [selectedRepositoryId, setSelectedRepositoryId] = useState<string | null>(null);
  const [selectedTargetRelativePath, setSelectedTargetRelativePath] = useState(ROOT_PATH);
  const [folderChildren, setFolderChildren] = useState<Record<string, FolderEntry[]>>({});
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(
    () => new Set([ROOT_PATH]),
  );
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [diff, setDiff] = useState<DiffResponse | null>(null);
  const [repositoryPath, setRepositoryPath] = useState("");
  const [repositoryLabel, setRepositoryLabel] = useState("");
  const [editedLabels, setEditedLabels] = useState<Record<string, string>>({});
  const [selectedRules, setSelectedRules] = useState<string[]>(["simplify-conditional"]);
  const [testFileMode, setTestFileMode] = useState<"readOnly" | "mutable">("readOnly");
  const [validationCommands, setValidationCommands] = useState("");
  const [protectedPaths, setProtectedPaths] = useState("src/generated/**");
  const [message, setMessage] = useState("");

  const selectedRepository = useMemo(
    () => repositories.find((repository) => repository.id === selectedRepositoryId) ?? null,
    [repositories, selectedRepositoryId],
  );

  const selectedRun = useMemo(
    () => runs.find((run) => run.id === selectedRunId) ?? runs[0],
    [runs, selectedRunId],
  );

  async function refresh(repositoryId = selectedRepositoryId) {
    const runsPath = repositoryId
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

    if (!repositoryId && repositoriesResponse[0]) {
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
  }, [selectedRepositoryId]);

  useEffect(() => {
    setDiff(null);
    setFolderChildren({});
    setExpandedFolders(new Set([ROOT_PATH]));
    setSelectedTargetRelativePath(ROOT_PATH);
    if (!selectedRepositoryId) return;
    loadFolder(selectedRepositoryId, ROOT_PATH).catch((error) => setMessage(error.message));
  }, [selectedRepositoryId]);

  useEffect(() => {
    if (!selectedRun?.id) {
      setDiff(null);
      return;
    }
    api
      .get<DiffResponse>(`/api/runs/${selectedRun.id}/diff`)
      .then(setDiff)
      .catch(() => setDiff(null));
  }, [selectedRun?.id, selectedRun?.updatedAt]);

  async function addRepository(event: React.FormEvent) {
    event.preventDefault();
    setMessage("");
    const repository = await api.post<RepositoryRecord>("/api/repositories", {
      path: repositoryPath,
      label: repositoryLabel || undefined,
    });
    setRepositoryPath("");
    setRepositoryLabel("");
    setSelectedRepositoryId(repository.id);
    await refresh(repository.id);
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
    if (!selectedRepositoryId) return;
    setMessage("");
    const response = await api.post<{ id: string }>("/api/runs", {
      repositoryId: selectedRepositoryId,
      targetRelativePath: selectedTargetRelativePath,
      rules: selectedRules,
      testFileMode,
      validationCommands: lines(validationCommands),
      protectedPaths: lines(protectedPaths),
      repairBudget: 2,
    });
    setSelectedRunId(response.id);
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

      <section className="workspace">
        <section className="panel repositories">
          <div className="panel-title">
            <GitBranch size={18} />
            <h2>Repositories</h2>
          </div>

          <form className="add-repository" onSubmit={addRepository}>
            <label>
              Local Git path
              <input
                value={repositoryPath}
                onChange={(event) => setRepositoryPath(event.target.value)}
                placeholder="/absolute/path/to/repo-or-subfolder"
                required
              />
            </label>
            <label>
              Label
              <input
                value={repositoryLabel}
                onChange={(event) => setRepositoryLabel(event.target.value)}
                placeholder="Optional"
              />
            </label>
            <button className="primary" type="submit">
              <Plus size={18} />
              Add repository
            </button>
          </form>

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
        </section>

        <section className="panel folders">
          <div className="panel-title">
            <Folder size={18} />
            <h2>Folders</h2>
          </div>
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
              <span>Rules</span>
              <div className="rule-list">
                {rules.map((rule) => (
                  <label className="checkbox-row" key={rule.id}>
                    <input
                      type="checkbox"
                      checked={selectedRules.includes(rule.id)}
                      onChange={() => toggleRule(rule.id)}
                    />
                    <span>
                      <strong>{rule.name}</strong>
                      <small>{rule.description}</small>
                    </span>
                  </label>
                ))}
              </div>
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
                placeholder="bun test&#10;bun run typecheck"
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

          <div className="runs-layout">
            <section className="history">
              <h3>History</h3>
              <div className="run-list">
                {runs.map((run) => (
                  <button
                    key={run.id}
                    className={run.id === selectedRun?.id ? "run-row selected" : "run-row"}
                    onClick={() => setSelectedRunId(run.id)}
                  >
                    <span className={`status ${run.status}`}>{run.status}</span>
                    <span className="path">{run.targetRelativePath ?? run.targetPath}</span>
                    <span className="time">{new Date(run.updatedAt).toLocaleString()}</span>
                  </button>
                ))}
                {runs.length === 0 && <p className="empty">No runs for this repository.</p>}
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
                      <dt>Tests</dt>
                      <dd>{selectedRun.testFileMode}</dd>
                    </div>
                  </dl>

                  {selectedRun.validationOutput && (
                    <pre className="log">{selectedRun.validationOutput}</pre>
                  )}
                  {selectedRun.error && <pre className="error">{selectedRun.error}</pre>}

                  <div className="diff-stack">
                    {diff?.files.map((file) => (
                      <article className="diff-file" key={file.filePath}>
                        <h3>{file.filePath}</h3>
                        <pre>{file.diff}</pre>
                      </article>
                    ))}
                    {diff?.files.length === 0 && (
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

createRoot(document.getElementById("root")!).render(<App />);
