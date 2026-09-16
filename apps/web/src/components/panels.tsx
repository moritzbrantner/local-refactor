import type { FormEvent, ReactNode } from "react";
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
  RotateCcw,
  Save,
  Shield,
  Trash2,
} from "lucide-react";
import type {
  CandidateFilePreviewResponse,
  ConventionSettings,
  FolderEntry,
  ModelSummary,
  RepositoryConventionsResponse,
  RepositoryRecord,
  Rule,
  RuleSelectionPlan,
  RunDraft,
  RunEvent,
  RunRecord,
  RunReviewResponse,
} from "../types";
import {
  formatToken,
  languageLabel,
  repositoryLabel,
  ROOT_PATH,
  ruleName,
  ruleRecords,
  runTargetLabel,
} from "../view-helpers";
import { CandidateFilePreview } from "./previews";

export type ConventionsPageProps = {
  selectedRepository: RepositoryRecord | null;
  conventions: RepositoryConventionsResponse | null;
  draft: ConventionSettings | null;
  loading: boolean;
  saving: boolean;
  error: string;
  onChangeDraft: (draft: ConventionSettings) => void;
  onSave: () => void;
  onReset: () => void;
};

export function ConventionsPage({
  selectedRepository,
  conventions,
  draft,
  loading,
  saving,
  error,
  onChangeDraft,
  onSave,
  onReset,
}: ConventionsPageProps) {
  if (!selectedRepository) {
    return (
      <section className="panel conventions-page">
        <div className="panel-title">
          <Shield size={18} />
          <h2>Conventions</h2>
        </div>
        <p className="empty">Select a repository.</p>
      </section>
    );
  }

  const settings = draft ?? conventions?.effective ?? null;

  return (
    <section className="panel conventions-page">
      <div className="panel-title">
        <Shield size={18} />
        <h2>Conventions</h2>
      </div>
      <div className="selected-source">
        <strong>{selectedRepository.label}</strong>
        <span>{selectedRepository.rootPath}</span>
      </div>
      {loading && <p className="empty">Loading conventions.</p>}
      {error && <small className="field-error">{error}</small>}
      {settings && (
        <>
          <div className="convention-actions">
            <button type="button" className="primary" onClick={onSave} disabled={saving}>
              <Save size={16} />
              {saving ? "Saving" : "Save override"}
            </button>
            <button type="button" className="secondary" onClick={onReset} disabled={saving}>
              <RotateCcw size={16} />
              Reset override
            </button>
          </div>

          {conventions && conventions.diagnostics.length > 0 && (
            <div className="diagnostics">
              {conventions.diagnostics.map((diagnostic) => (
                <p key={diagnostic}>{diagnostic}</p>
              ))}
            </div>
          )}

          <div className="convention-grid">
            <fieldset>
              <legend>Profile</legend>
              <select
                value={settings.profile}
                onChange={(event) =>
                  onChangeDraft({
                    ...settings,
                    profile: event.target.value as ConventionSettings["profile"],
                  })
                }
              >
                <option value="standard">standard</option>
                <option value="minimal">minimal</option>
                <option value="custom">custom</option>
              </select>
            </fieldset>

            <ConventionGroup title="TypeScript">
              <Checkbox
                label="Formatter"
                checked={settings.typescript.formatter.enabled}
                onChange={(enabled) =>
                  onChangeDraft({
                    ...settings,
                    typescript: {
                      ...settings.typescript,
                      formatter: { ...settings.typescript.formatter, enabled },
                    },
                  })
                }
              />
              <Checkbox
                label="Require formatter config"
                checked={settings.typescript.formatter.requireConfig}
                onChange={(requireConfig) =>
                  onChangeDraft({
                    ...settings,
                    typescript: {
                      ...settings.typescript,
                      formatter: { ...settings.typescript.formatter, requireConfig },
                    },
                  })
                }
              />
              <Checkbox
                label="Import ordering"
                checked={settings.typescript.ordering.imports}
                onChange={(imports) =>
                  onChangeDraft({
                    ...settings,
                    typescript: {
                      ...settings.typescript,
                      ordering: { ...settings.typescript.ordering, imports },
                    },
                  })
                }
              />
              <Checkbox
                label="Class member ordering"
                checked={settings.typescript.ordering.classMembers}
                onChange={(classMembers) =>
                  onChangeDraft({
                    ...settings,
                    typescript: {
                      ...settings.typescript,
                      ordering: { ...settings.typescript.ordering, classMembers },
                    },
                  })
                }
              />
              <MemberGroups
                value={settings.typescript.ordering.memberGroups}
                onChange={(memberGroups) =>
                  onChangeDraft({
                    ...settings,
                    typescript: {
                      ...settings.typescript,
                      ordering: { ...settings.typescript.ordering, memberGroups },
                    },
                  })
                }
              />
            </ConventionGroup>

            <ConventionGroup title="Rust">
              <Checkbox
                label="Formatter"
                checked={settings.rust.formatter.enabled}
                onChange={(enabled) =>
                  onChangeDraft({
                    ...settings,
                    rust: {
                      ...settings.rust,
                      formatter: { ...settings.rust.formatter, enabled },
                    },
                  })
                }
              />
              <Checkbox
                label="Require formatter config"
                checked={settings.rust.formatter.requireConfig}
                onChange={(requireConfig) =>
                  onChangeDraft({
                    ...settings,
                    rust: {
                      ...settings.rust,
                      formatter: { ...settings.rust.formatter, requireConfig },
                    },
                  })
                }
              />
              <Checkbox
                label="Use item ordering"
                checked={settings.rust.ordering.useItems}
                onChange={(useItems) =>
                  onChangeDraft({
                    ...settings,
                    rust: {
                      ...settings.rust,
                      ordering: { ...settings.rust.ordering, useItems },
                    },
                  })
                }
              />
              <Checkbox
                label="Impl member ordering"
                checked={settings.rust.ordering.implMembers}
                onChange={(implMembers) =>
                  onChangeDraft({
                    ...settings,
                    rust: {
                      ...settings.rust,
                      ordering: { ...settings.rust.ordering, implMembers },
                    },
                  })
                }
              />
              <MemberGroups
                value={settings.rust.ordering.memberGroups}
                onChange={(memberGroups) =>
                  onChangeDraft({
                    ...settings,
                    rust: {
                      ...settings.rust,
                      ordering: { ...settings.rust.ordering, memberGroups },
                    },
                  })
                }
              />
            </ConventionGroup>
          </div>

          <div className="convention-layers">
            <LayerSummary title="Project Config" value={conventions?.projectConfig ?? null} />
            <LayerSummary title="Local Override" value={conventions?.localOverride ?? null} />
            <LayerSummary title="Effective" value={conventions?.effective ?? settings} />
          </div>
        </>
      )}
    </section>
  );
}

function ConventionGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <fieldset>
      <legend>{title}</legend>
      <div className="convention-controls">{children}</div>
    </fieldset>
  );
}

function Checkbox({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="checkbox-row">
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
      <span>{label}</span>
    </label>
  );
}

function MemberGroups({ value, onChange }: { value: string[]; onChange: (value: string[]) => void }) {
  return (
    <label className="field-stack">
      <span>Member groups</span>
      <input
        value={value.join(", ")}
        onChange={(event) =>
          onChange(
            event.target.value
              .split(",")
              .map((item) => item.trim())
              .filter(Boolean),
          )
        }
      />
    </label>
  );
}

function LayerSummary({ title, value }: { title: string; value: unknown }) {
  return (
    <details>
      <summary>{title}</summary>
      <pre>{value ? JSON.stringify(value, null, 2) : "null"}</pre>
    </details>
  );
}

export type RepositoriesPanelProps = {
  collapsed: boolean;
  repositories: RepositoryRecord[];
  selectedRepositoryId: string | null;
  editedLabels: Record<string, string>;
  isPickingRepository: boolean;
  onToggleCollapsed: () => void;
  onAddRepository: () => void;
  onSelectRepository: (repositoryId: string) => void;
  onEditLabel: (repositoryId: string, label: string) => void;
  onRenameRepository: (repository: RepositoryRecord) => void;
  onRemoveRepository: (repositoryId: string) => void;
};

export function RepositoriesPanel({
  collapsed,
  repositories,
  selectedRepositoryId,
  editedLabels,
  isPickingRepository,
  onToggleCollapsed,
  onAddRepository,
  onSelectRepository,
  onEditLabel,
  onRenameRepository,
  onRemoveRepository,
}: RepositoriesPanelProps) {
  return (
    <section className={collapsed ? "panel repositories collapsed" : "panel repositories"}>
      <div className="panel-title">
        <GitBranch size={18} />
        <h2>Repositories</h2>
        <button
          type="button"
          className="icon-button panel-toggle"
          aria-controls="repositories-panel-body"
          aria-expanded={!collapsed}
          aria-label={collapsed ? "Restore repositories" : "Minimize repositories"}
          title={collapsed ? "Restore repositories" : "Minimize repositories"}
          onClick={onToggleCollapsed}
        >
          {collapsed ? <Maximize2 size={16} /> : <Minimize2 size={16} />}
        </button>
      </div>

      <div id="repositories-panel-body" className="panel-body" hidden={collapsed}>
        <div className="add-repository">
          <button className="primary" type="button" onClick={onAddRepository} disabled={isPickingRepository}>
            <Plus size={18} />
            {isPickingRepository ? "Choosing folder" : "Choose root folder"}
          </button>
        </div>

        <div className="repository-list">
          {repositories.map((repository) => (
            <article
              className={
                repository.id === selectedRepositoryId ? "repository-row selected" : "repository-row"
              }
              key={repository.id}
            >
              <div className="repository-select">
                <input
                  value={editedLabels[repository.id] ?? repository.label}
                  onChange={(event) => onEditLabel(repository.id, event.target.value)}
                  onBlur={() => onRenameRepository(repository)}
                />
                <button
                  type="button"
                  className="repository-path"
                  onClick={() => onSelectRepository(repository.id)}
                >
                  <span className="path">{repository.rootPath}</span>
                  {!repository.available && <span className="unavailable">Unavailable</span>}
                </button>
              </div>
              <button
                type="button"
                className="icon-button"
                onClick={() => onRemoveRepository(repository.id)}
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
  );
}

export type FoldersPanelProps = {
  collapsed: boolean;
  selectedRepository: RepositoryRecord | null;
  selectedTargetRelativePath: string;
  expandedFolders: Set<string>;
  folderChildren: Record<string, FolderEntry[]>;
  onToggleCollapsed: () => void;
  onToggleFolder: (relativePath: string) => void;
  onSelectTarget: (relativePath: string) => void;
};

export function FoldersPanel({
  collapsed,
  selectedRepository,
  selectedTargetRelativePath,
  expandedFolders,
  folderChildren,
  onToggleCollapsed,
  onToggleFolder,
  onSelectTarget,
}: FoldersPanelProps) {
  function renderFolder(relativePath: string, name: string, depth = 0): ReactNode {
    const isExpanded = expandedFolders.has(relativePath);
    const isSelected = selectedTargetRelativePath === relativePath;
    const children = folderChildren[relativePath] ?? [];

    return (
      <div className="folder-node" key={relativePath}>
        <div className="folder-row" style={{ paddingLeft: 8 + depth * 16 }}>
          <button
            type="button"
            className="tree-toggle"
            onClick={() => onToggleFolder(relativePath)}
            title={isExpanded ? "Collapse" : "Expand"}
          >
            {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
          </button>
          <button
            type="button"
            className={isSelected ? "folder-select selected" : "folder-select"}
            onClick={() => onSelectTarget(relativePath)}
          >
            {isExpanded ? <FolderOpen size={16} /> : <Folder size={16} />}
            <span>{name}</span>
          </button>
        </div>
        {isExpanded && <div>{children.map((child) => renderFolder(child.relativePath, child.name, depth + 1))}</div>}
      </div>
    );
  }

  return (
    <section className={collapsed ? "panel folders collapsed" : "panel folders"}>
      <div className="panel-title">
        <Folder size={18} />
        <h2>Folders</h2>
        <button
          type="button"
          className="icon-button panel-toggle"
          aria-controls="folders-panel-body"
          aria-expanded={!collapsed}
          aria-label={collapsed ? "Restore folders" : "Minimize folders"}
          title={collapsed ? "Restore folders" : "Minimize folders"}
          onClick={onToggleCollapsed}
        >
          {collapsed ? <Maximize2 size={16} /> : <Minimize2 size={16} />}
        </button>
      </div>
      <div id="folders-panel-body" className="panel-body" hidden={collapsed}>
        {selectedRepository ? (
          <>
            <div className="selected-source">
              <strong>{selectedRepository.label}</strong>
              <span>{selectedRepository.rootPath}</span>
            </div>
            <div className="folder-tree">{renderFolder(ROOT_PATH, selectedRepository.label)}</div>
          </>
        ) : (
          <p className="empty">Select a repository.</p>
        )}
      </div>
    </section>
  );
}

export type RuleSelectionPanelProps = {
  rules: Rule[];
  ruleMode: "automatic" | "manual";
  selectedRules: string[];
  ruleSelectionPlan: RuleSelectionPlan | null;
  ruleSelectionError: string;
  rulesSectionCollapsed: boolean;
  expandedRuleItems: Set<string>;
  rulesSummaryText: string;
  onToggleCollapsed: () => void;
  onSetRuleMode: (mode: "automatic" | "manual") => void;
  onToggleRule: (ruleId: string) => void;
  onToggleRuleItem: (itemId: string) => void;
};

export function RuleSelectionPanel({
  rules,
  ruleMode,
  selectedRules,
  ruleSelectionPlan,
  ruleSelectionError,
  rulesSectionCollapsed,
  expandedRuleItems,
  rulesSummaryText,
  onToggleCollapsed,
  onSetRuleMode,
  onToggleRule,
  onToggleRuleItem,
}: RuleSelectionPanelProps) {
  return (
    <div className="field-group collapsible-field">
      <div className="collapsible-title">
        <span>Rules</span>
        <span className="collapse-summary">{rulesSummaryText}</span>
        <button
          type="button"
          className="icon-button"
          aria-controls="rules-panel-body"
          aria-expanded={!rulesSectionCollapsed}
          aria-label={rulesSectionCollapsed ? "Expand rules" : "Collapse rules"}
          title={rulesSectionCollapsed ? "Expand rules" : "Collapse rules"}
          onClick={onToggleCollapsed}
        >
          {rulesSectionCollapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
        </button>
      </div>
      {rulesSectionCollapsed && ruleSelectionError && (
        <small className="field-error">{ruleSelectionError}</small>
      )}
      <div id="rules-panel-body" hidden={rulesSectionCollapsed}>
        <div className="segmented" aria-label="Rule selection mode">
          <button
            type="button"
            className={ruleMode === "automatic" ? "active" : ""}
            onClick={() => onSetRuleMode("automatic")}
          >
            Automatic
          </button>
          <button
            type="button"
            className={ruleMode === "manual" ? "active" : ""}
            onClick={() => onSetRuleMode("manual")}
          >
            Manual
          </button>
        </div>
        {ruleMode === "automatic" ? (
          <div className="rule-plan">
            {ruleSelectionError && <small className="field-error">{ruleSelectionError}</small>}
            {ruleSelectionPlan?.segments.map((segment) => {
              const itemId = `automatic:${segment.relativePath || "."}`;
              const isExpanded = expandedRuleItems.has(itemId);
              return (
                <article className="rule-segment" key={segment.relativePath || "."}>
                  <button
                    type="button"
                    className="rule-item-toggle"
                    aria-expanded={isExpanded}
                    onClick={() => onToggleRuleItem(itemId)}
                  >
                    {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                    <strong>{segment.relativePath === "" ? "Repository root" : segment.relativePath}</strong>
                    <span>{segment.rules.length} rules</span>
                  </button>
                  {isExpanded && (
                    <>
                      <ul className="rule-summary-list">
                        {ruleRecords(segment.rules, rules).map((rule) => (
                          <li key={rule.id}>
                            <strong>{rule.name}</strong>
                            <span className="rule-metadata">
                              <span>{languageLabel(rule.language)}</span>
                              <span>{rule.executionKind === "modelPlanned" ? "model planned" : "deterministic"}</span>
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
                    </>
                  )}
                </article>
              );
            })}
            {ruleSelectionPlan && ruleSelectionPlan.segments.length === 0 && (
              <p className="empty">No automatic rules matched this target.</p>
            )}
            {!ruleSelectionPlan && !ruleSelectionError && (
              <p className="empty">Detecting rules for this target.</p>
            )}
          </div>
        ) : (
          <div className="rule-list">
            {rules.map((rule) => {
              const itemId = `manual:${rule.id}`;
              const isExpanded = expandedRuleItems.has(itemId);
              return (
                <article className="manual-rule-item" key={rule.id}>
                  <div className="manual-rule-header">
                    <label className="manual-rule-check">
                      <input
                        type="checkbox"
                        checked={selectedRules.includes(rule.id)}
                        onChange={() => onToggleRule(rule.id)}
                      />
                      <span className="rule-heading">
                        <strong>{rule.name}</strong>
                        <span className="language-badge">{languageLabel(rule.language)}</span>
                      </span>
                    </label>
                    <button
                      type="button"
                      className="rule-item-toggle compact-toggle"
                      aria-expanded={isExpanded}
                      aria-label={`${isExpanded ? "Collapse" : "Expand"} ${rule.name}`}
                      onClick={() => onToggleRuleItem(itemId)}
                    >
                      {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                    </button>
                  </div>
                  {isExpanded && (
                    <div className="manual-rule-detail">
                      <span className="rule-metadata">
                        <span>{rule.executionKind === "modelPlanned" ? "model planned" : "deterministic"}</span>
                        <span>{formatToken(rule.safetyLevel)}</span>
                        <span>{formatToken(rule.allowedWrites)}</span>
                      </span>
                      <small>{rule.description}</small>
                    </div>
                  )}
                </article>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

export type RunConfigurationFormProps = {
  selectedTargetRelativePath: string;
  selectedModel: string;
  models: ModelSummary[];
  modelsError: string;
  testFileMode: "readOnly" | "mutable";
  validationCommands: string;
  protectedPaths: string;
  startRunDisabled: boolean;
  usesModelPlannedRules: boolean;
  primaryActionLabel: "Preview changes" | "Start run";
  candidatePreviewState: ReactNode;
  ruleSelection: ReactNode;
  onSubmit: (event: FormEvent) => void;
  onSelectModel: (model: string) => void;
  onSetTestFileMode: (mode: "readOnly" | "mutable") => void;
  onSetValidationCommands: (value: string) => void;
  onSetProtectedPaths: (value: string) => void;
};

export function RunConfigurationForm({
  selectedTargetRelativePath,
  selectedModel,
  models,
  modelsError,
  testFileMode,
  validationCommands,
  protectedPaths,
  startRunDisabled,
  usesModelPlannedRules,
  primaryActionLabel,
  candidatePreviewState,
  ruleSelection,
  onSubmit,
  onSelectModel,
  onSetTestFileMode,
  onSetValidationCommands,
  onSetProtectedPaths,
}: RunConfigurationFormProps) {
  return (
    <form className="run-form" onSubmit={onSubmit}>
      <div className="target-summary">
        <span>Target</span>
        <strong>{selectedTargetRelativePath === ROOT_PATH ? "Repository root" : selectedTargetRelativePath}</strong>
      </div>

      {usesModelPlannedRules && (
        <div className="field-group">
          <span>Model</span>
          <label className="model-select">
            <Cpu size={16} />
            <select value={selectedModel} onChange={(event) => onSelectModel(event.target.value)}>
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
      )}

      {ruleSelection}

      <div className="segmented" aria-label="Test file mode">
        <button
          type="button"
          className={testFileMode === "readOnly" ? "active" : ""}
          onClick={() => onSetTestFileMode("readOnly")}
        >
          <Shield size={16} />
          Tests read-only
        </button>
        <button
          type="button"
          className={testFileMode === "mutable" ? "active" : ""}
          onClick={() => onSetTestFileMode("mutable")}
        >
          <FileDiff size={16} />
          Tests mutable
        </button>
      </div>

      <label>
        Validation commands
        <textarea
          value={validationCommands}
          onChange={(event) => onSetValidationCommands(event.target.value)}
          placeholder="bun test&#10;bun run typecheck&#10;cargo check --all-targets&#10;cargo clippy --all-targets -- -D warnings"
        />
      </label>

      <label>
        Protected paths
        <textarea value={protectedPaths} onChange={(event) => onSetProtectedPaths(event.target.value)} />
      </label>

      <section className="candidate-preview-shell">{candidatePreviewState}</section>

      <button className="primary" type="submit" disabled={startRunDisabled}>
        <Play size={18} />
        {primaryActionLabel}
      </button>
    </form>
  );
}

export type RunReviewPanelProps = {
  draft: RunDraft;
  rules: Rule[];
  onCancel: () => void;
  onConfirm: () => void;
};

export function RunReviewPanel({ draft, rules, onCancel, onConfirm }: RunReviewPanelProps) {
  return (
    <section className="run-review">
      <div className="run-review-title">
        <Shield size={17} />
        <h3>Review run</h3>
      </div>
      <dl className="metadata compact-metadata">
        <div>
          <dt>Repository</dt>
          <dd>{draft.repositoryLabel}</dd>
        </div>
        <div>
          <dt>Mutable scope</dt>
          <dd>{draft.targetLabel}</dd>
        </div>
        <div>
          <dt>Model</dt>
          <dd>{draft.modelLabel}</dd>
        </div>
        <div>
          <dt>Tests</dt>
          <dd>{draft.testFileMode}</dd>
        </div>
      </dl>
      <CandidateFilePreview preview={draft.candidateFilePreview} interactive={false} />
      <div className="settings-grid">
        <section>
          <h4>{draft.mode === "automatic" ? "Rule selection plan" : "Rules"}</h4>
          {draft.mode === "automatic" && draft.ruleSelectionPlan ? (
            <div className="rule-plan compact-plan">
              {draft.ruleSelectionPlan.segments.map((segment) => (
                <article className="rule-segment" key={segment.relativePath || "."}>
                  <div className="rule-segment-title">
                    <strong>{segment.relativePath === "" ? "Repository root" : segment.relativePath}</strong>
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
              {draft.ruleSummaries.map((rule) => (
                <li key={rule.id}>
                  <strong>{rule.name}</strong>
                  <span className="rule-metadata">
                    <span>{languageLabel(rule.language)}</span>
                    <span>{rule.executionKind === "modelPlanned" ? "model planned" : "deterministic"}</span>
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
          {draft.protectedPaths.length > 0 ? (
            <ul>{draft.protectedPaths.map((path) => <li key={path}>{path}</li>)}</ul>
          ) : (
            <p className="empty">No custom protected paths.</p>
          )}
        </section>
      </div>
      <section className="trust-check">
        <h4>Local execution</h4>
        {draft.validationCommands.length > 0 ? (
          <ul>{draft.validationCommands.map((command) => <li key={command}>{command}</li>)}</ul>
        ) : (
          <>
            <p>No custom validation commands were entered.</p>
            <p>
              Automatic validation uses the <code>coding-tooling</code> full tier in strict mode and may
              execute repository-discovered checks on this machine.
            </p>
          </>
        )}
        <p>
          Confirm only for repositories and validation tooling you trust; validation runs on this
          machine through the local shell and may execute repository-defined commands.
        </p>
      </section>
      <div className="review-actions">
        <button className="secondary" type="button" onClick={onCancel}>
          Cancel
        </button>
        <button className="primary" type="button" onClick={onConfirm}>
          <Play size={18} />
          Confirm and start run
        </button>
      </div>
    </section>
  );
}

export type RunHistoryProps = {
  runs: RunRecord[];
  repositories: RepositoryRecord[];
  selectedRun: RunRecord | null;
  runHistoryScope: "repository" | "all";
  selectedRepositoryId: string | null;
  onSetRunHistoryScope: (scope: "repository" | "all") => void;
  onSelectRun: (runId: string) => void;
};

export function RunHistory({
  runs,
  repositories,
  selectedRun,
  runHistoryScope,
  selectedRepositoryId,
  onSetRunHistoryScope,
  onSelectRun,
}: RunHistoryProps) {
  return (
    <section className="history">
      <div className="history-title">
        <h3>History</h3>
        <div className="segmented compact" aria-label="Run history scope">
          <button
            type="button"
            className={runHistoryScope === "repository" ? "active" : ""}
            onClick={() => onSetRunHistoryScope("repository")}
            disabled={!selectedRepositoryId}
          >
            Repository
          </button>
          <button
            type="button"
            className={runHistoryScope === "all" ? "active" : ""}
            onClick={() => onSetRunHistoryScope("all")}
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
            onClick={() => onSelectRun(run.id)}
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
            {runHistoryScope === "repository" ? "No runs for this repository." : "No stored runs."}
          </p>
        )}
      </div>
    </section>
  );
}

export type RunDetailProps = {
  selectedRun: RunRecord | null;
  repositories: RepositoryRecord[];
  selectedRunReview: RunReviewResponse | null;
  selectedRunEvents: RunEvent[];
  downloadProgress: { label: string; percent: number } | null;
  rules: Rule[];
  onRevert: () => void;
};

export function RunDetail({
  selectedRun,
  repositories,
  selectedRunReview,
  selectedRunEvents,
  downloadProgress,
  rules,
  onRevert,
}: RunDetailProps) {
  return (
    <section className="detail">
      <div className="detail-title">
        <h3>Detail</h3>
        {selectedRun && (
          <button className="secondary" onClick={onRevert}>
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
                        <strong>{segment.relativePath === "" ? "Repository root" : segment.relativePath}</strong>
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
                <ul>{selectedRun.validationCommands.map((command) => <li key={command}>{command}</li>)}</ul>
              ) : (
                <p className="empty">No validation commands recorded.</p>
              )}
            </section>
            <section>
              <h4>Protected paths</h4>
              {selectedRun.protectedPaths.length > 0 ? (
                <ul>{selectedRun.protectedPaths.map((path) => <li key={path}>{path}</li>)}</ul>
              ) : (
                <p className="empty">No protected paths recorded.</p>
              )}
            </section>
          </div>

          {selectedRun.behaviorClaims && selectedRun.behaviorClaims.length > 0 && (
            <section>
              <h4>Behavior claims</h4>
              <ul>
                {selectedRun.behaviorClaims.map((claim) => (
                  <li key={claim.id}>
                    <strong>{claim.publicEntrypoint}</strong>: {claim.behavior}
                    <span className="muted"> in {claim.testPath}</span>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {selectedRun.validationOutput && <pre className="log">{selectedRun.validationOutput}</pre>}
          {selectedRun.error && (
            <section className="run-error">
              <h4>Run error</h4>
              <pre>{selectedRun.error}</pre>
            </section>
          )}

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
  );
}

export function RunsPanelShell({ children }: { children: ReactNode }) {
  return (
    <section className="panel runs-panel">
      <div className="panel-title">
        <Activity size={18} />
        <h2>Runs</h2>
      </div>
      {children}
    </section>
  );
}
