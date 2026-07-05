import { describe, expect, test } from "vitest";
import { repository, ruleSelectionPlan, run, runEvents } from "./test/fixtures";
import {
  appendUniqueRunEvent,
  changedLineNumbers,
  flattenedPlanRules,
  formatBytes,
  formatToken,
  languageLabel,
  latestDownloadProgress,
  lines,
  pathsEndWithSameFile,
  repositoryFilePath,
  repositoryLabel,
  ruleName,
  ruleRecords,
  runTargetLabel,
} from "./view-helpers";
import { rules } from "./test/fixtures";

describe("view helpers", () => {
  test("line parsing trims blank lines", () => {
    expect(lines(" bun test \n\n cargo check \r\n ")).toEqual(["bun test", "cargo check"]);
  });

  test("rule selection flattening deduplicates and sorts", () => {
    expect(
      flattenedPlanRules({
        ...ruleSelectionPlan,
        segments: [
          { relativePath: "b", rules: ["normalize-imports"], reasons: [] },
          { relativePath: "a", rules: ["simplify-conditional", "normalize-imports"], reasons: [] },
        ],
      }),
    ).toEqual(["normalize-imports", "simplify-conditional"]);
  });

  test("rule lookup helpers prefer known rule records and names", () => {
    expect(ruleRecords(["simplify-conditional", "missing"], rules)).toEqual([rules[0]]);
    expect(ruleName("simplify-conditional", rules)).toBe("Simplify Conditional");
    expect(ruleName("missing", rules)).toBe("missing");
  });

  test("tokens and language labels are humanized", () => {
    expect(languageLabel("typescript")).toBe("TypeScript");
    expect(languageLabel("rust")).toBe("Rust");
    expect(formatToken("test-required")).toBe("Test Required");
  });

  test("run labels prefer repository-relative targets", () => {
    expect(runTargetLabel(run({ targetRelativePath: "." }))).toBe("Repository root");
    expect(runTargetLabel(run({ targetRelativePath: "src" }))).toBe("src");
    expect(runTargetLabel(run({ targetRelativePath: undefined, targetPath: "/tmp/repo" }))).toBe(
      "/tmp/repo",
    );
  });

  test("repository labels prefer saved Repository Source labels", () => {
    expect(repositoryLabel(run(), [repository({ label: "Current Repo" })])).toBe("Current Repo");
    expect(repositoryLabel(run({ repositoryId: "missing" }), [])).toBe(
      "/tmp/local-refactor-fixture",
    );
  });

  test("run events are deduplicated and sorted", () => {
    expect(
      appendUniqueRunEvent(
        [runEvents[1]],
        { id: 1, runId: "run-1", timestamp: runEvents[0].timestamp, message: "Run started" },
      ).map((event) => event.id),
    ).toEqual([1, 2]);
    expect(appendUniqueRunEvent(runEvents, runEvents[0])).toBe(runEvents);
  });

  test("latest download progress parses the newest progress event", () => {
    expect(latestDownloadProgress(runEvents)).toEqual({
      label: "Downloading qwen2.5-coder:7b",
      percent: 45,
    });
    expect(
      latestDownloadProgress([
        ...runEvents,
        { id: 3, runId: "run-1", timestamp: runEvents[0].timestamp, message: "Done" },
      ]),
    ).toEqual({
      label: "Downloading qwen2.5-coder:7b",
      percent: 45,
    });
  });

  test("repository file paths and suffix matching handle POSIX and Windows paths", () => {
    expect(repositoryFilePath("/tmp/repo/", "src/file.ts")).toBe("/tmp/repo/src/file.ts");
    expect(pathsEndWithSameFile("/tmp/repo/src/file.ts", "src/file.ts")).toBe(true);
    expect(pathsEndWithSameFile("C:\\repo\\src\\file.ts", "src/file.ts")).toBe(true);
    expect(pathsEndWithSameFile("/tmp/repo/src/other.ts", "src/file.ts")).toBe(false);
  });

  test("changed line detection handles replacements, additions, removals, and unchanged content", () => {
    expect(changedLineNumbers("a\nb\nc", "a\nx\nc")).toEqual([2]);
    expect(changedLineNumbers("a\nb", "a\nb\nc")).toEqual([2]);
    expect(changedLineNumbers("a\nb\nc", "a\nc")).toEqual([2]);
    expect(changedLineNumbers("a\nb", "a\nb")).toEqual([]);
    expect(changedLineNumbers("a", "b")).toEqual([1]);
  });

  test("byte formatting uses bytes and KiB", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KiB");
  });
});
