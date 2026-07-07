import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import {
  candidatePreview,
  deterministicPreview,
  folders,
  models,
  repository,
  ruleSelectionPlan,
  rules,
  run,
  runDraft,
  runEvents,
  runReview,
} from "../test/fixtures";
import type { RunRecord } from "../types";
import {
  CandidateFilePreview,
  ChangePreviewSummary,
  DeterministicPreviewPanel,
  FilePreviewPanel,
} from "./previews";
import {
  FoldersPanel,
  RepositoriesPanel,
  RuleSelectionPanel,
  RunConfigurationForm,
  RunDetail,
  RunHistory,
  RunReviewPanel,
} from "./panels";

describe("presentational panels", () => {
  test("RepositoriesPanel renders empty, selected, unavailable, rename, and remove states", () => {
    const onEditLabel = vi.fn();
    const onRenameRepository = vi.fn();
    const onRemoveRepository = vi.fn();

    const { rerender } = render(
      <RepositoriesPanel
        collapsed={false}
        repositories={[]}
        selectedRepositoryId={null}
        editedLabels={{}}
        isPickingRepository={false}
        onToggleCollapsed={vi.fn()}
        onAddRepository={vi.fn()}
        onSelectRepository={vi.fn()}
        onEditLabel={onEditLabel}
        onRenameRepository={onRenameRepository}
        onRemoveRepository={onRemoveRepository}
      />,
    );
    expect(screen.getByText("No repositories added.")).toBeInTheDocument();

    const unavailable = repository({ id: "repo-2", available: false, label: "Offline Repo" });
    rerender(
      <RepositoriesPanel
        collapsed={false}
        repositories={[repository(), unavailable]}
        selectedRepositoryId="repo-1"
        editedLabels={{ "repo-1": "Fixture Repo" }}
        isPickingRepository
        onToggleCollapsed={vi.fn()}
        onAddRepository={vi.fn()}
        onSelectRepository={vi.fn()}
        onEditLabel={onEditLabel}
        onRenameRepository={onRenameRepository}
        onRemoveRepository={onRemoveRepository}
      />,
    );
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    fireEvent.change(screen.getByDisplayValue("Fixture Repo"), {
      target: { value: "Renamed Repo" },
    });
    fireEvent.blur(screen.getByDisplayValue("Fixture Repo"));
    fireEvent.click(screen.getAllByTitle("Remove")[0]);
    expect(onEditLabel).toHaveBeenCalledWith("repo-1", "Renamed Repo");
    expect(onRenameRepository).toHaveBeenCalledWith(repository());
    expect(onRemoveRepository).toHaveBeenCalledWith("repo-1");
    expect(screen.getByRole("button", { name: /Choosing folder/ })).toBeDisabled();
  });

  test("FoldersPanel renders empty and nested folder states", () => {
    const onToggleFolder = vi.fn();
    const onSelectTarget = vi.fn();
    const { rerender } = render(
      <FoldersPanel
        collapsed={false}
        selectedRepository={null}
        selectedTargetRelativePath="."
        expandedFolders={new Set(["."])}
        folderChildren={{}}
        onToggleCollapsed={vi.fn()}
        onToggleFolder={onToggleFolder}
        onSelectTarget={onSelectTarget}
      />,
    );
    expect(screen.getByText("Select a repository.")).toBeInTheDocument();

    rerender(
      <FoldersPanel
        collapsed={false}
        selectedRepository={repository()}
        selectedTargetRelativePath="src"
        expandedFolders={new Set([".", "src"])}
        folderChildren={folders}
        onToggleCollapsed={vi.fn()}
        onToggleFolder={onToggleFolder}
        onSelectTarget={onSelectTarget}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "src" }));
    fireEvent.click(screen.getAllByTitle("Collapse")[0]);
    expect(screen.getByText("components")).toBeInTheDocument();
    expect(onSelectTarget).toHaveBeenCalledWith("src");
    expect(onToggleFolder).toHaveBeenCalledWith(".");
  });

  test("RuleSelectionPanel renders automatic loading, error, empty, expanded, and manual states", () => {
    const { rerender } = render(
      <RuleSelectionPanel
        rules={rules}
        ruleMode="automatic"
        selectedRules={["simplify-conditional"]}
        ruleSelectionPlan={null}
        ruleSelectionError=""
        rulesSectionCollapsed={false}
        expandedRuleItems={new Set()}
        rulesSummaryText="Detecting rules"
        onToggleCollapsed={vi.fn()}
        onSetRuleMode={vi.fn()}
        onToggleRule={vi.fn()}
        onToggleRuleItem={vi.fn()}
      />,
    );
    expect(screen.getByText("Detecting rules for this target.")).toBeInTheDocument();

    rerender(
      <RuleSelectionPanel
        rules={rules}
        ruleMode="automatic"
        selectedRules={[]}
        ruleSelectionPlan={{ targetRelativePath: ".", segments: [] }}
        ruleSelectionError=""
        rulesSectionCollapsed={false}
        expandedRuleItems={new Set()}
        rulesSummaryText="0 segments, 0 rules"
        onToggleCollapsed={vi.fn()}
        onSetRuleMode={vi.fn()}
        onToggleRule={vi.fn()}
        onToggleRuleItem={vi.fn()}
      />,
    );
    expect(screen.getByText("No automatic rules matched this target.")).toBeInTheDocument();

    rerender(
      <RuleSelectionPanel
        rules={rules}
        ruleMode="automatic"
        selectedRules={[]}
        ruleSelectionPlan={ruleSelectionPlan}
        ruleSelectionError="selection failed"
        rulesSectionCollapsed={false}
        expandedRuleItems={new Set(["automatic:src"])}
        rulesSummaryText="selection error"
        onToggleCollapsed={vi.fn()}
        onSetRuleMode={vi.fn()}
        onToggleRule={vi.fn()}
        onToggleRuleItem={vi.fn()}
      />,
    );
    expect(screen.getByText("selection failed")).toBeInTheDocument();
    expect(screen.getByText("Simplify Conditional")).toBeInTheDocument();
    expect(screen.getByText(/TypeScript source files/)).toBeInTheDocument();

    const onToggleRule = vi.fn();
    rerender(
      <RuleSelectionPanel
        rules={rules}
        ruleMode="manual"
        selectedRules={["normalize-imports"]}
        ruleSelectionPlan={null}
        ruleSelectionError=""
        rulesSectionCollapsed={false}
        expandedRuleItems={new Set(["manual:normalize-imports"])}
        rulesSummaryText="1 selected rules"
        onToggleCollapsed={vi.fn()}
        onSetRuleMode={vi.fn()}
        onToggleRule={onToggleRule}
        onToggleRuleItem={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("checkbox", { name: /Normalize Imports/ }));
    expect(screen.getByText("Sorts and groups local import declarations.")).toBeInTheDocument();
    expect(onToggleRule).toHaveBeenCalledWith("normalize-imports");
  });

  test("RunConfigurationForm renders deterministic action without model controls", () => {
    const onSetTestFileMode = vi.fn();
    render(
      <RunConfigurationForm
        selectedTargetRelativePath="src"
        selectedModel="qwen2.5-coder:7b"
        models={models}
        modelsError=""
        testFileMode="readOnly"
        validationCommands="bun test"
        protectedPaths="src/generated/**"
        startRunDisabled={false}
        usesModelPlannedRules={false}
        primaryActionLabel="Preview changes"
        ruleSelection={<p>Rules slot</p>}
        candidatePreviewState={<p>Candidate slot</p>}
        onSubmit={(event) => event.preventDefault()}
        onSelectModel={vi.fn()}
        onSetTestFileMode={onSetTestFileMode}
        onSetValidationCommands={vi.fn()}
        onSetProtectedPaths={vi.fn()}
      />,
    );
    expect(screen.getByText("src")).toBeInTheDocument();
    expect(screen.getByText("Rules slot")).toBeInTheDocument();
    expect(screen.getByText("Candidate slot")).toBeInTheDocument();
    expect(screen.queryByText("Model")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Preview changes/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Tests mutable/ }));
    expect(onSetTestFileMode).toHaveBeenCalledWith("mutable");
  });

  test("RunConfigurationForm renders model controls for model-planned rules", () => {
    render(
      <RunConfigurationForm
        selectedTargetRelativePath="src"
        selectedModel="qwen2.5-coder:7b"
        models={models}
        modelsError=""
        testFileMode="readOnly"
        validationCommands="bun test"
        protectedPaths="src/generated/**"
        startRunDisabled={false}
        usesModelPlannedRules
        primaryActionLabel="Start run"
        ruleSelection={<p>Rules slot</p>}
        candidatePreviewState={<p>Candidate slot</p>}
        onSubmit={(event) => event.preventDefault()}
        onSelectModel={vi.fn()}
        onSetTestFileMode={vi.fn()}
        onSetValidationCommands={vi.fn()}
        onSetProtectedPaths={vi.fn()}
      />,
    );
    expect(screen.getByText("Model")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Start run/ })).toBeInTheDocument();
  });

  test("CandidateFilePreview renders grouped, empty, hidden, selected, loading, and error states", () => {
    const onSelectFile = vi.fn();
    const { rerender } = render(
      <CandidateFilePreview
        preview={candidatePreview()}
        selectedCandidateFilePath="src/sample.ts"
        filePreview={null}
        filePreviewLoading={false}
        filePreviewError=""
        fileChangePreview={null}
        fileChangePreviewLoading={false}
        fileChangePreviewError=""
        fileChangePreviewUnavailable={false}
        onSelectFile={onSelectFile}
      />,
    );
    expect(screen.getByText("3 total")).toBeInTheDocument();
    expect(screen.getByText("Rust")).toBeInTheDocument();
    expect(screen.getByText("1 more hidden")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "src/other.ts" }));
    expect(onSelectFile).toHaveBeenCalledWith("src/other.ts");

    rerender(<CandidateFilePreview preview={candidatePreview({ groups: [], totalCandidateFiles: 0 })} />);
    expect(screen.getByText("No candidate files matched this run configuration.")).toBeInTheDocument();

    rerender(
      <FilePreviewPanel
        filePreview={null}
        filePreviewLoading
        filePreviewError=""
        fileChangePreview={null}
        fileChangePreviewLoading={false}
        fileChangePreviewError=""
        fileChangePreviewUnavailable={false}
      />,
    );
    expect(screen.getByText("Loading file preview.")).toBeInTheDocument();

    rerender(
      <FilePreviewPanel
        filePreview={null}
        filePreviewLoading={false}
        filePreviewError="preview failed"
        fileChangePreview={null}
        fileChangePreviewLoading={false}
        fileChangePreviewError=""
        fileChangePreviewUnavailable={false}
      />,
    );
    expect(screen.getByText("preview failed")).toBeInTheDocument();
  });

  test("ChangePreviewSummary renders deterministic preview states", () => {
    const { rerender } = render(
      <ChangePreviewSummary edit={null} loading error="" unavailable={false} />,
    );
    expect(screen.getByText("Checking deterministic edits.")).toBeInTheDocument();
    rerender(<ChangePreviewSummary edit={null} loading={false} error="" unavailable />);
    expect(screen.getByText("No deterministic edit preview available.")).toBeInTheDocument();
    rerender(<ChangePreviewSummary edit={null} loading={false} error="" unavailable={false} />);
    expect(screen.getByText("No deterministic edits found.")).toBeInTheDocument();
    rerender(<ChangePreviewSummary edit={null} loading={false} error="analysis failed" unavailable={false} />);
    expect(screen.getByText("analysis failed")).toBeInTheDocument();
  });

  test("DeterministicPreviewPanel renders diff, empty, loading, and error states", () => {
    const onApply = vi.fn();
    const { rerender } = render(
      <DeterministicPreviewPanel
        preview={deterministicPreview()}
        applying={false}
        error=""
        onApply={onApply}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByRole("heading", { name: "Deterministic Preview" })).toBeInTheDocument();
    expect(screen.getByText("src/sample.ts")).toBeInTheDocument();
    expect(screen.getByText(/return value/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    expect(onApply).toHaveBeenCalled();

    rerender(
      <DeterministicPreviewPanel
        preview={deterministicPreview({ files: [] })}
        applying={false}
        error=""
        onApply={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText("No deterministic edits found.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Apply changes" })).toBeDisabled();

    rerender(
      <DeterministicPreviewPanel
        preview={deterministicPreview()}
        applying
        error="Preview changed"
        onApply={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(screen.getByText("Preview changed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Apply changes" })).toBeDisabled();
  });

  test("RunReviewPanel renders automatic and manual review states", () => {
    const { rerender } = render(
      <RunReviewPanel draft={runDraft()} rules={rules} onCancel={vi.fn()} onConfirm={vi.fn()} />,
    );
    expect(screen.getByRole("heading", { name: "Review run" })).toBeInTheDocument();
    expect(screen.getByText("Rule selection plan")).toBeInTheDocument();
    expect(screen.getByText("src/generated/**")).toBeInTheDocument();
    expect(screen.getByText(/validation runs on this machine/)).toBeInTheDocument();

    rerender(
      <RunReviewPanel
        draft={runDraft({
          mode: "manual",
          ruleSelectionPlan: undefined,
          usesModelPlannedRules: true,
          validationCommands: [],
        })}
        rules={rules}
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );
    expect(screen.getByText("Rules")).toBeInTheDocument();
    expect(screen.getByText("No custom validation commands were entered.")).toBeInTheDocument();
  });

  test("RunHistory renders repository/all empty states, rows, and selected run", () => {
    const { rerender } = render(
      <RunHistory
        runs={[]}
        repositories={[repository()]}
        selectedRun={null}
        runHistoryScope="repository"
        selectedRepositoryId="repo-1"
        onSetRunHistoryScope={vi.fn()}
        onSelectRun={vi.fn()}
      />,
    );
    expect(screen.getByText("No runs for this repository.")).toBeInTheDocument();

    rerender(
      <RunHistory
        runs={[]}
        repositories={[repository()]}
        selectedRun={null}
        runHistoryScope="all"
        selectedRepositoryId="repo-1"
        onSetRunHistoryScope={vi.fn()}
        onSelectRun={vi.fn()}
      />,
    );
    expect(screen.getByText("No stored runs.")).toBeInTheDocument();

    const onSelectRun = vi.fn();
    const selected = run();
    rerender(
      <RunHistory
        runs={[selected, run({ id: "run-2", status: "failed" })]}
        repositories={[repository()]}
        selectedRun={selected}
        runHistoryScope="all"
        selectedRepositoryId="repo-1"
        onSetRunHistoryScope={vi.fn()}
        onSelectRun={onSelectRun}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /failed/ }));
    expect(onSelectRun).toHaveBeenCalledWith("run-2");
  });

  test("RunDetail renders empty, success, no-diff, failed, reverted, progress, and events", () => {
    const review = runReview();
    const { rerender } = render(
      <RunDetail
        selectedRun={null}
        repositories={[repository()]}
        selectedRunReview={null}
        selectedRunEvents={[]}
        downloadProgress={null}
        rules={rules}
        onRevert={vi.fn()}
      />,
    );
    expect(screen.getByText("Select a run to inspect.")).toBeInTheDocument();

    rerender(
      <RunDetail
        selectedRun={run()}
        repositories={[repository()]}
        selectedRunReview={review}
        selectedRunEvents={runEvents}
        downloadProgress={{ label: "Downloading qwen2.5-coder:7b", percent: 45 }}
        rules={rules}
        onRevert={vi.fn()}
      />,
    );
    expect(screen.getByText("succeeded")).toBeInTheDocument();
    expect(screen.getByText("45%")).toBeInTheDocument();
    expect(screen.getByText("Run started")).toBeInTheDocument();
    expect(screen.getByText(/return value/)).toBeInTheDocument();

    rerender(
      <RunDetail
        selectedRun={run({ status: "failed", error: "validation failed" })}
        repositories={[repository()]}
        selectedRunReview={runReview({ diff: { runId: "run-1", files: [] } })}
        selectedRunEvents={[]}
        downloadProgress={null}
        rules={rules}
        onRevert={vi.fn()}
      />,
    );
    expect(screen.getByText("validation failed")).toBeInTheDocument();
    expect(screen.getByText("No file changes recorded.")).toBeInTheDocument();

    const reverted: RunRecord = run({ status: "reverted" });
    rerender(
      <RunDetail
        selectedRun={reverted}
        repositories={[repository()]}
        selectedRunReview={null}
        selectedRunEvents={[]}
        downloadProgress={null}
        rules={rules}
        onRevert={vi.fn()}
      />,
    );
    expect(screen.getByText("reverted")).toBeInTheDocument();
  });
});
