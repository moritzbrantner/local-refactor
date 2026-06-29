import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  Activity,
  FileDiff,
  Play,
  RefreshCcw,
  RotateCcw,
  Shield,
} from "lucide-react";
import "./styles.css";

type Rule = {
  id: string;
  name: string;
  description: string;
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
};

type DiffResponse = {
  runId: string;
  files: Array<{ filePath: string; diff: string }>;
};

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
};

function App() {
  const [rules, setRules] = useState<Rule[]>([]);
  const [runs, setRuns] = useState<RunRecord[]>([]);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [diff, setDiff] = useState<DiffResponse | null>(null);
  const [targetPath, setTargetPath] = useState("");
  const [selectedRules, setSelectedRules] = useState<string[]>(["simplify-conditional"]);
  const [testFileMode, setTestFileMode] = useState<"readOnly" | "mutable">("readOnly");
  const [validationCommands, setValidationCommands] = useState("");
  const [protectedPaths, setProtectedPaths] = useState("src/generated/**");
  const [message, setMessage] = useState("");

  const selectedRun = useMemo(
    () => runs.find((run) => run.id === selectedRunId) ?? runs[0],
    [runs, selectedRunId],
  );

  async function refresh() {
    const [rulesResponse, runsResponse] = await Promise.all([
      api.get<{ rules: Rule[] }>("/api/rules"),
      api.get<RunRecord[]>("/api/runs"),
    ]);
    setRules(rulesResponse.rules);
    setRuns(runsResponse);
    if (!selectedRunId && runsResponse[0]) setSelectedRunId(runsResponse[0].id);
  }

  useEffect(() => {
    refresh().catch((error) => setMessage(error.message));
    const timer = window.setInterval(() => {
      refresh().catch(() => undefined);
    }, 2500);
    return () => window.clearInterval(timer);
  }, []);

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

  async function startRun(event: React.FormEvent) {
    event.preventDefault();
    setMessage("");
    const response = await api.post<{ id: string }>("/api/runs", {
      targetPath,
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
        <form className="panel run-form" onSubmit={startRun}>
          <div className="panel-title">
            <Play size={18} />
            <h2>New Run</h2>
          </div>

          <label>
            Target path
            <input
              value={targetPath}
              onChange={(event) => setTargetPath(event.target.value)}
              placeholder="/absolute/path/to/src"
              required
            />
          </label>

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

          <button className="primary" type="submit">
            <Play size={18} />
            Start run
          </button>
        </form>

        <section className="panel history">
          <div className="panel-title">
            <Activity size={18} />
            <h2>Runs</h2>
          </div>
          <div className="run-list">
            {runs.map((run) => (
              <button
                key={run.id}
                className={run.id === selectedRun?.id ? "run-row selected" : "run-row"}
                onClick={() => setSelectedRunId(run.id)}
              >
                <span className={`status ${run.status}`}>{run.status}</span>
                <span className="path">{run.targetPath}</span>
                <span className="time">{new Date(run.updatedAt).toLocaleString()}</span>
              </button>
            ))}
            {runs.length === 0 && <p className="empty">No runs yet.</p>}
          </div>
        </section>

        <section className="panel detail">
          <div className="panel-title">
            <FileDiff size={18} />
            <h2>Run Detail</h2>
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
                {diff?.files.length === 0 && <p className="empty">No file changes recorded.</p>}
              </div>
            </>
          ) : (
            <p className="empty">Select a run to inspect.</p>
          )}
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

