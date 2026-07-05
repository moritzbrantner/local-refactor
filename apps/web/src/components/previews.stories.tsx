import type { Meta, StoryObj } from "@storybook/react-vite";
import { analyzerEdit, candidatePreview, filePreview } from "../test/fixtures";
import { CandidateFilePreview, ChangePreviewSummary, FilePreviewPanel } from "./previews";

const meta = {
  title: "Components/Candidate File Preview",
  component: CandidateFilePreview,
} satisfies Meta<typeof CandidateFilePreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {
  args: {
    preview: candidatePreview({ groups: [], totalCandidateFiles: 0 }),
  },
};

export const GroupedTypeScriptAndRustFiles: Story = {
  args: {
    preview: candidatePreview(),
  },
};

export const HiddenFiles: Story = {
  args: {
    preview: candidatePreview(),
  },
};

export const SelectedFileWithPreview: Story = {
  args: {
    preview: candidatePreview(),
    selectedCandidateFilePath: "src/sample.ts",
    filePreview,
    filePreviewLoading: false,
    filePreviewError: "",
    fileChangePreview: analyzerEdit,
    fileChangePreviewLoading: false,
    fileChangePreviewError: "",
    fileChangePreviewUnavailable: false,
  },
};

export const PreviewLoading: Story = {
  args: {
    preview: candidatePreview(),
    selectedCandidateFilePath: "src/sample.ts",
    filePreview: null,
    filePreviewLoading: true,
    filePreviewError: "",
  },
};

export const PreviewError: Story = {
  args: {
    preview: candidatePreview(),
    selectedCandidateFilePath: "src/sample.ts",
    filePreview: null,
    filePreviewLoading: false,
    filePreviewError: "File preview failed.",
  },
};

export const DeterministicEditUnavailable: Story = {
  args: {
    preview: candidatePreview(),
  },
  render: () => (
    <FilePreviewPanel
      filePreview={filePreview}
      filePreviewLoading={false}
      filePreviewError=""
      fileChangePreview={null}
      fileChangePreviewLoading={false}
      fileChangePreviewError=""
      fileChangePreviewUnavailable
    />
  ),
};

export const NoDeterministicEdits: Story = {
  args: {
    preview: candidatePreview(),
  },
  render: () => (
    <ChangePreviewSummary edit={null} loading={false} error="" unavailable={false} />
  ),
};
