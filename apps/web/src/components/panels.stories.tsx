import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  candidatePreview,
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
import {
  FoldersPanel,
  RepositoriesPanel,
  RuleSelectionPanel,
  RunConfigurationForm,
  RunDetail,
  RunHistory,
  RunReviewPanel,
} from "./panels";

const noop = () => undefined;

const meta = {
  title: "Components/Panels",
} satisfies Meta;

export default meta;
type Story = StoryObj<typeof meta>;

export const RepositoriesEmpty: Story = {
  render: () => (
    <RepositoriesPanel
      collapsed={false}
      repositories={[]}
      selectedRepositoryId={null}
      editedLabels={{}}
      isPickingRepository={false}
      onToggleCollapsed={noop}
      onAddRepository={noop}
      onSelectRepository={noop}
      onEditLabel={noop}
      onRenameRepository={noop}
      onRemoveRepository={noop}
    />
  ),
};

export const RepositoriesSelected: Story = {
  render: () => (
    <RepositoriesPanel
      collapsed={false}
      repositories={[repository()]}
      selectedRepositoryId="repo-1"
      editedLabels={{ "repo-1": "Fixture Repo" }}
      isPickingRepository={false}
      onToggleCollapsed={noop}
      onAddRepository={noop}
      onSelectRepository={noop}
      onEditLabel={noop}
      onRenameRepository={noop}
      onRemoveRepository={noop}
    />
  ),
};

export const RepositoriesUnavailable: Story = {
  render: () => (
    <RepositoriesPanel
      collapsed={false}
      repositories={[repository({ available: false })]}
      selectedRepositoryId="repo-1"
      editedLabels={{ "repo-1": "Fixture Repo" }}
      isPickingRepository={false}
      onToggleCollapsed={noop}
      onAddRepository={noop}
      onSelectRepository={noop}
      onEditLabel={noop}
      onRenameRepository={noop}
      onRemoveRepository={noop}
    />
  ),
};

export const RepositoriesPickingFolder: Story = {
  render: () => (
    <RepositoriesPanel
      collapsed={false}
      repositories={[repository()]}
      selectedRepositoryId="repo-1"
      editedLabels={{ "repo-1": "Fixture Repo" }}
      isPickingRepository
      onToggleCollapsed={noop}
      onAddRepository={noop}
      onSelectRepository={noop}
      onEditLabel={noop}
      onRenameRepository={noop}
      onRemoveRepository={noop}
    />
  ),
};

export const FoldersNoRepository: Story = {
  render: () => (
    <FoldersPanel
      collapsed={false}
      selectedRepository={null}
      selectedTargetRelativePath="."
      expandedFolders={new Set(["."])}
      folderChildren={{}}
      onToggleCollapsed={noop}
      onToggleFolder={noop}
      onSelectTarget={noop}
    />
  ),
};

export const FoldersRootOnly: Story = {
  render: () => (
    <FoldersPanel
      collapsed={false}
      selectedRepository={repository()}
      selectedTargetRelativePath="."
      expandedFolders={new Set(["."])}
      folderChildren={{ ".": [] }}
      onToggleCollapsed={noop}
      onToggleFolder={noop}
      onSelectTarget={noop}
    />
  ),
};

export const FoldersNestedExpandedTree: Story = {
  render: () => (
    <FoldersPanel
      collapsed={false}
      selectedRepository={repository()}
      selectedTargetRelativePath="src"
      expandedFolders={new Set([".", "src"])}
      folderChildren={folders}
      onToggleCollapsed={noop}
      onToggleFolder={noop}
      onSelectTarget={noop}
    />
  ),
};

export const RuleSelectionAutomaticLoading: Story = {
  render: () => (
    <RuleSelectionPanel
      rules={rules}
      ruleMode="automatic"
      selectedRules={[]}
      ruleSelectionPlan={null}
      ruleSelectionError=""
      rulesSectionCollapsed={false}
      expandedRuleItems={new Set()}
      rulesSummaryText="Detecting rules"
      onToggleCollapsed={noop}
      onSetRuleMode={noop}
      onToggleRule={noop}
      onToggleRuleItem={noop}
    />
  ),
};

export const RuleSelectionAutomaticWithSegmentAndReasons: Story = {
  render: () => (
    <RuleSelectionPanel
      rules={rules}
      ruleMode="automatic"
      selectedRules={[]}
      ruleSelectionPlan={ruleSelectionPlan}
      ruleSelectionError=""
      rulesSectionCollapsed={false}
      expandedRuleItems={new Set(["automatic:src"])}
      rulesSummaryText="1 segments, 1 rules"
      onToggleCollapsed={noop}
      onSetRuleMode={noop}
      onToggleRule={noop}
      onToggleRuleItem={noop}
    />
  ),
};

export const RuleSelectionAutomaticError: Story = {
  render: () => (
    <RuleSelectionPanel
      rules={rules}
      ruleMode="automatic"
      selectedRules={[]}
      ruleSelectionPlan={null}
      ruleSelectionError="Rule selection failed."
      rulesSectionCollapsed={false}
      expandedRuleItems={new Set()}
      rulesSummaryText="Automatic selection error"
      onToggleCollapsed={noop}
      onSetRuleMode={noop}
      onToggleRule={noop}
      onToggleRuleItem={noop}
    />
  ),
};

export const RuleSelectionManualSelectedRules: Story = {
  render: () => (
    <RuleSelectionPanel
      rules={rules}
      ruleMode="manual"
      selectedRules={["simplify-conditional"]}
      ruleSelectionPlan={null}
      ruleSelectionError=""
      rulesSectionCollapsed={false}
      expandedRuleItems={new Set()}
      rulesSummaryText="1 selected rules"
      onToggleCollapsed={noop}
      onSetRuleMode={noop}
      onToggleRule={noop}
      onToggleRuleItem={noop}
    />
  ),
};

export const RuleSelectionManualExpandedRuleDetail: Story = {
  render: () => (
    <RuleSelectionPanel
      rules={rules}
      ruleMode="manual"
      selectedRules={["normalize-imports"]}
      ruleSelectionPlan={null}
      ruleSelectionError=""
      rulesSectionCollapsed={false}
      expandedRuleItems={new Set(["manual:normalize-imports"])}
      rulesSummaryText="1 selected rules"
      onToggleCollapsed={noop}
      onSetRuleMode={noop}
      onToggleRule={noop}
      onToggleRuleItem={noop}
    />
  ),
};

export const RunConfigurationWithCandidatePreview: Story = {
  render: () => (
    <RunConfigurationForm
      selectedTargetRelativePath="."
      selectedModel="qwen2.5-coder:7b"
      models={models}
      modelsError=""
      testFileMode="readOnly"
      validationCommands="bun test"
      protectedPaths="src/generated/**"
      startRunDisabled={false}
      usesModelPlannedRules={false}
      primaryActionLabel="Preview changes"
      ruleSelection={<p className="empty">Rules slot</p>}
      candidatePreviewState={<p className="empty">{candidatePreview().totalCandidateFiles} candidates ready.</p>}
      onSubmit={(event) => event.preventDefault()}
      onSelectModel={noop}
      onSetTestFileMode={noop}
      onSetValidationCommands={noop}
      onSetProtectedPaths={noop}
    />
  ),
};

export const RunReviewAutomaticPlan: Story = {
  render: () => <RunReviewPanel draft={runDraft()} rules={rules} onCancel={noop} onConfirm={noop} />,
};

export const RunReviewManualRules: Story = {
  render: () => (
    <RunReviewPanel
      draft={runDraft({ mode: "manual", ruleSelectionPlan: undefined })}
      rules={rules}
      onCancel={noop}
      onConfirm={noop}
    />
  ),
};

export const RunReviewWithModelPlannedWarning: Story = {
  render: () => (
    <RunReviewPanel
      draft={runDraft({
        usesModelPlannedRules: true,
        validationCommands: [],
        ruleSummaries: [rules[2]],
        rules: ["rust-add-documentation-comments"],
        ruleLabels: ["Add Documentation Comments"],
      })}
      rules={rules}
      onCancel={noop}
      onConfirm={noop}
    />
  ),
};

export const RunHistoryEmptyRepositoryScope: Story = {
  render: () => (
    <RunHistory
      runs={[]}
      repositories={[repository()]}
      selectedRun={null}
      runHistoryScope="repository"
      selectedRepositoryId="repo-1"
      onSetRunHistoryScope={noop}
      onSelectRun={noop}
    />
  ),
};

export const RunHistoryEmptyAllScope: Story = {
  render: () => (
    <RunHistory
      runs={[]}
      repositories={[repository()]}
      selectedRun={null}
      runHistoryScope="all"
      selectedRepositoryId="repo-1"
      onSetRunHistoryScope={noop}
      onSelectRun={noop}
    />
  ),
};

export const RunHistoryMultipleRuns: Story = {
  render: () => (
    <RunHistory
      runs={[run(), run({ id: "run-2", status: "failed", targetRelativePath: "legacy" })]}
      repositories={[repository()]}
      selectedRun={run()}
      runHistoryScope="all"
      selectedRepositoryId="repo-1"
      onSetRunHistoryScope={noop}
      onSelectRun={noop}
    />
  ),
};

export const RunDetailNoSelectedRun: Story = {
  render: () => (
    <RunDetail
      selectedRun={null}
      repositories={[repository()]}
      selectedRunReview={null}
      selectedRunEvents={[]}
      downloadProgress={null}
      rules={rules}
      onRevert={noop}
    />
  ),
};

export const RunDetailSucceededWithDiff: Story = {
  render: () => (
    <RunDetail
      selectedRun={run()}
      repositories={[repository()]}
      selectedRunReview={runReview()}
      selectedRunEvents={runEvents}
      downloadProgress={null}
      rules={rules}
      onRevert={noop}
    />
  ),
};

export const RunDetailFailedWithError: Story = {
  render: () => (
    <RunDetail
      selectedRun={run({ status: "failed", error: "Validation failed." })}
      repositories={[repository()]}
      selectedRunReview={runReview({ diff: { runId: "run-1", files: [] } })}
      selectedRunEvents={[]}
      downloadProgress={null}
      rules={rules}
      onRevert={noop}
    />
  ),
};

export const RunDetailReverted: Story = {
  render: () => (
    <RunDetail
      selectedRun={run({ status: "reverted" })}
      repositories={[repository()]}
      selectedRunReview={runReview()}
      selectedRunEvents={[]}
      downloadProgress={null}
      rules={rules}
      onRevert={noop}
    />
  ),
};

export const RunDetailDownloadProgressAndEvents: Story = {
  render: () => (
    <RunDetail
      selectedRun={run()}
      repositories={[repository()]}
      selectedRunReview={runReview()}
      selectedRunEvents={runEvents}
      downloadProgress={{ label: "Downloading qwen2.5-coder:7b", percent: 45 }}
      rules={rules}
      onRevert={noop}
    />
  ),
};
