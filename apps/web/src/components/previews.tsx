import { javascript } from "@codemirror/lang-javascript";
import { rust } from "@codemirror/lang-rust";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { EditorState } from "@codemirror/state";
import { Decoration, EditorView, lineNumbers } from "@codemirror/view";
import { useEffect, useRef } from "react";
import type {
  AnalyzerEdit,
  CandidateFilePreviewResponse,
  DeterministicPreviewResponse,
  RepositoryFilePreviewResponse,
} from "../types";
import { changedLineNumbers, formatBytes, languageLabel } from "../view-helpers";

export type CandidateFilePreviewProps = {
  preview: CandidateFilePreviewResponse;
  interactive?: boolean;
  selectedCandidateFilePath?: string | null;
  filePreview?: RepositoryFilePreviewResponse | null;
  filePreviewLoading?: boolean;
  filePreviewError?: string;
  fileChangePreview?: AnalyzerEdit | null;
  fileChangePreviewLoading?: boolean;
  fileChangePreviewError?: string;
  fileChangePreviewUnavailable?: boolean;
  onSelectFile?: (relativePath: string) => void;
};

export function CandidateFilePreview({
  preview,
  interactive = true,
  selectedCandidateFilePath = null,
  filePreview = null,
  filePreviewLoading = false,
  filePreviewError = "",
  fileChangePreview = null,
  fileChangePreviewLoading = false,
  fileChangePreviewError = "",
  fileChangePreviewUnavailable = false,
  onSelectFile,
}: CandidateFilePreviewProps) {
  return (
    <section className="candidate-preview">
      <div className="candidate-preview-title">
        <h4>Candidate File Preview</h4>
        <span>{preview.totalCandidateFiles} total</span>
      </div>
      <div
        className={interactive ? "candidate-preview-layout" : "candidate-preview-layout summary-only"}
      >
        <div className="candidate-groups">
          {preview.groups.map((group) => (
            <article className="candidate-group" key={group.id}>
              <div className="candidate-group-title">
                <strong>{group.label}</strong>
                <span>{group.totalFiles} files</span>
              </div>
              <div className="rule-metadata">
                <span>{languageLabel(group.language)}</span>
                <span>{group.ruleName}</span>
                {group.segmentRelativePath !== undefined && (
                  <span>
                    {group.segmentRelativePath === "" ? "Repository root" : group.segmentRelativePath}
                  </span>
                )}
              </div>
              {group.files.length > 0 ? (
                <ul className="candidate-file-list">
                  {group.files.map((file) => (
                    <li key={file.relativePath}>
                      {interactive ? (
                        <button
                          type="button"
                          className={
                            selectedCandidateFilePath === file.relativePath
                              ? "candidate-file-button selected"
                              : "candidate-file-button"
                          }
                          onClick={() => onSelectFile?.(file.relativePath)}
                        >
                          {file.relativePath}
                        </button>
                      ) : (
                        file.relativePath
                      )}
                    </li>
                  ))}
                  {group.hiddenFiles > 0 && (
                    <li className="candidate-hidden">{group.hiddenFiles} more hidden</li>
                  )}
                </ul>
              ) : (
                <p className="empty">No mutable source files matched this group.</p>
              )}
            </article>
          ))}
          {preview.groups.length === 0 && (
            <p className="empty">No candidate files matched this run configuration.</p>
          )}
        </div>
        {interactive && (
          <FilePreviewPanel
            filePreview={filePreview}
            filePreviewLoading={filePreviewLoading}
            filePreviewError={filePreviewError}
            fileChangePreview={fileChangePreview}
            fileChangePreviewLoading={fileChangePreviewLoading}
            fileChangePreviewError={fileChangePreviewError}
            fileChangePreviewUnavailable={fileChangePreviewUnavailable}
          />
        )}
      </div>
    </section>
  );
}

export type FilePreviewPanelProps = {
  filePreview: RepositoryFilePreviewResponse | null;
  filePreviewLoading: boolean;
  filePreviewError: string;
  fileChangePreview: AnalyzerEdit | null;
  fileChangePreviewLoading: boolean;
  fileChangePreviewError: string;
  fileChangePreviewUnavailable: boolean;
};

export function FilePreviewPanel({
  filePreview,
  filePreviewLoading,
  filePreviewError,
  fileChangePreview,
  fileChangePreviewLoading,
  fileChangePreviewError,
  fileChangePreviewUnavailable,
}: FilePreviewPanelProps) {
  return (
    <aside className="file-preview-panel">
      {filePreviewLoading && <p className="empty">Loading file preview.</p>}
      {filePreviewError && <small className="field-error">{filePreviewError}</small>}
      {!filePreviewLoading && !filePreviewError && filePreview && (
        <>
          <div className="file-preview-title">
            <strong>{filePreview.relativePath}</strong>
            <span>{formatBytes(filePreview.sizeBytes)}</span>
          </div>
          <CodePreview
            content={filePreview.content}
            highlightedLines={
              fileChangePreview
                ? changedLineNumbers(fileChangePreview.originalContent, fileChangePreview.newContent)
                : []
            }
            language={filePreview.language}
            path={filePreview.relativePath}
          />
          <ChangePreviewSummary
            edit={fileChangePreview}
            loading={fileChangePreviewLoading}
            error={fileChangePreviewError}
            unavailable={fileChangePreviewUnavailable}
          />
        </>
      )}
      {!filePreviewLoading && !filePreviewError && !filePreview && (
        <p className="empty">Select a candidate file to preview.</p>
      )}
    </aside>
  );
}

export type ChangePreviewSummaryProps = {
  edit: AnalyzerEdit | null;
  loading: boolean;
  error: string;
  unavailable: boolean;
};

export function ChangePreviewSummary({
  edit,
  loading,
  error,
  unavailable,
}: ChangePreviewSummaryProps) {
  if (loading) {
    return <p className="change-preview-note">Checking deterministic edits.</p>;
  }
  if (error) {
    return <p className="change-preview-error">{error}</p>;
  }
  if (unavailable) {
    return <p className="change-preview-note">No deterministic edit preview available.</p>;
  }
  if (!edit) {
    return <p className="change-preview-note">No deterministic edits found.</p>;
  }

  return (
    <p className="change-preview-note strong">
      Would change {changedLineNumbers(edit.originalContent, edit.newContent).length} lines:{" "}
      {edit.summary}
    </p>
  );
}

export type CodePreviewProps = {
  content: string;
  highlightedLines: number[];
  language: RepositoryFilePreviewResponse["language"];
  path: string;
};

export function CodePreview({ content, highlightedLines, language, path }: CodePreviewProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;
    const highlightedLineSet = new Set(highlightedLines);

    const extensions: Extension[] = [
      lineNumbers(),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.lineWrapping,
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      EditorView.decorations.compute([], (state) => {
        const lineDecoration = Decoration.line({ class: "cm-change-preview-line" });
        const ranges = [...highlightedLineSet]
          .filter((lineNumber) => lineNumber >= 1 && lineNumber <= state.doc.lines)
          .map((lineNumber) => lineDecoration.range(state.doc.line(lineNumber).from));
        return Decoration.set(ranges, true);
      }),
    ];

    if (language === "typescript") {
      extensions.push(
        javascript({
          typescript: true,
          jsx: path.endsWith(".tsx") || path.endsWith(".jsx"),
        }),
      );
    }
    if (language === "rust") {
      extensions.push(rust());
    }

    const view = new EditorView({
      parent: containerRef.current,
      state: EditorState.create({
        doc: content,
        extensions,
      }),
    });

    return () => view.destroy();
  }, [content, highlightedLines, language, path]);

  return <div className="code-preview" ref={containerRef} />;
}

export type DeterministicPreviewPanelProps = {
  preview: DeterministicPreviewResponse;
  applying: boolean;
  error: string;
  onApply: () => void;
  onCancel: () => void;
};

export function DeterministicPreviewPanel({
  preview,
  applying,
  error,
  onApply,
  onCancel,
}: DeterministicPreviewPanelProps) {
  return (
    <section className="run-review deterministic-preview-panel">
      <div className="run-review-title">
        <h3>Deterministic Preview</h3>
        <span>{preview.files.length} files</span>
      </div>
      {preview.diagnostics.length > 0 && (
        <div className="event-list compact-events">
          {preview.diagnostics.slice(0, 6).map((diagnostic) => (
            <p key={diagnostic}>{diagnostic}</p>
          ))}
        </div>
      )}
      {preview.files.length === 0 ? (
        <p className="empty">No deterministic edits found.</p>
      ) : (
        <div className="diff-stack">
          {preview.files.map((file) => (
            <article className="diff-file" key={file.filePath}>
              <h3>{file.relativePath}</h3>
              <div className="diff-summary">
                {file.ruleIds.map((ruleId) => (
                  <span key={ruleId}>{ruleId}</span>
                ))}
                <p>{file.summaries.join("; ")}</p>
              </div>
              <pre>{file.diff}</pre>
            </article>
          ))}
        </div>
      )}
      {error && <small className="field-error">{error}</small>}
      <div className="review-actions">
        <button type="button" className="secondary" onClick={onCancel}>
          Cancel
        </button>
        <button
          type="button"
          className="primary"
          disabled={applying || preview.files.length === 0}
          onClick={onApply}
        >
          Apply changes
        </button>
      </div>
    </section>
  );
}
