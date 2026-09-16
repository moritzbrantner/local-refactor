import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { rules, runDraft } from "../test/fixtures";
import { RunReviewPanel } from "./panels";

describe("RunReviewPanel automatic validation trust", () => {
  test("warns before deterministic automatic validation executes repository tooling", () => {
    render(
      <RunReviewPanel
        draft={runDraft({
          validationCommands: [],
          usesModelPlannedRules: false,
        })}
        rules={rules}
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );

    expect(screen.getByText("coding-tooling")).toBeInTheDocument();
    expect(screen.getByText(/repository-discovered checks on this machine/)).toBeInTheDocument();
    expect(
      screen.getByText(/Confirm only for repositories and validation tooling you trust/),
    ).toBeInTheDocument();
  });
});
