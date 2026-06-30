import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  assertCaseCorrectness,
  buildBenchmarkReport,
  markdownReport,
  parseCliArgs,
  writeReportArtifacts,
  type RuleCase,
} from "./benchmark-refactorings";

describe("benchmark refactoring CLI", () => {
  test("parses suite, iterations, model, compare-models, and output options", () => {
    expect(
      parseCliArgs([
        "--suite",
        "validation-realistic",
        "--iterations",
        "3",
        "--model",
        "qwen2.5-coder:7b",
        "--output",
        "/tmp/bench",
      ]),
    ).toMatchObject({
      suite: "validation-realistic",
      iterations: 3,
      models: ["qwen2.5-coder:7b"],
      outputDir: "/tmp/bench",
    });

    expect(
      parseCliArgs([
        "--suite=fixture-smoke",
        "--compare-models=qwen2.5-coder:7b,deepseek-coder:6.7b",
      ]),
    ).toMatchObject({
      suite: "fixture-smoke",
      iterations: 5,
      models: ["qwen2.5-coder:7b", "deepseek-coder:6.7b"],
    });
  });
});

describe("benchmark correctness checks", () => {
  test("fails when a succeeded run is missing required output text", () => {
    const repo = mkdtempSync(join(tmpdir(), "local-refactor-benchmark-test-"));
    mkdirSync(join(repo, "src"), { recursive: true });
    writeFileSync(join(repo, "src/sample.ts"), "export const value = false;\n");

    const rule: RuleCase = {
      id: "simplify-conditional",
      executionKind: "deterministic",
      source: "",
      requiredChangedFiles: ["src/sample.ts"],
      requiredTextSnippets: [{ file: "src/sample.ts", text: "return value;" }],
      expectedDiffFileCount: 1,
    };

    expect(() =>
      assertCaseCorrectness(rule, repo, {
        events: [],
        metrics: {
          totalRunMs: 1,
          modelEnsureAvailableMs: 0,
          fileCollectionMs: 0,
          analyzerPlanningMs: 0,
          modelPlanningMs: null,
          patchPlanValidationMs: null,
          editApplicationMs: 0,
          validationMs: 0,
        },
        diff: {
          files: [
            {
              filePath: join(repo, "src/sample.ts"),
              ruleId: "simplify-conditional",
              summary: "updated",
              diff: "",
            },
          ],
        },
      }),
    ).toThrow(/missing required text/);
  });
});

describe("benchmark reports", () => {
  test("emit JSON, Markdown, and HTML artifacts with stable top-level schema", () => {
    const outputDir = mkdtempSync(join(tmpdir(), "local-refactor-benchmark-artifacts-"));
    const report = buildBenchmarkReport({
      suite: "fixture-smoke",
      iterations: 1,
      models: ["qwen2.5-coder:7b"],
      rawSamples: [
        {
          model: "qwen2.5-coder:7b",
          suite: "fixture-smoke",
          phase: "warm",
          iteration: 1,
          ruleId: "simplify-conditional",
          executionKind: "deterministic",
          status: "succeeded",
          wallMs: 10,
          diffFiles: 1,
          events: 4,
          metrics: {
            totalRunMs: 9,
            modelEnsureAvailableMs: 0,
            fileCollectionMs: 1,
            analyzerPlanningMs: 5,
            modelPlanningMs: null,
            patchPlanValidationMs: null,
            editApplicationMs: 1,
            validationMs: 1,
          },
          servicePeakRssMb: 20,
          ollamaPeakRssMb: null,
          nvidiaPeakTotalVramMb: null,
          nvidiaPeakOllamaVramMb: null,
          samples: [],
        },
      ],
    });

    const artifacts = writeReportArtifacts(report, outputDir);
    const json = JSON.parse(readFileSync(artifacts.jsonPath, "utf8"));
    const markdown = readFileSync(artifacts.markdownPath, "utf8");
    const html = readFileSync(artifacts.htmlPath, "utf8");

    expect(json.schemaVersion).toBe(1);
    expect(json.rawSamples[0].metrics.totalRunMs).toBe(9);
    expect(json.summary.byRule[0].wallMs.median).toBe(10);
    expect(markdownReport(report)).toContain("## Rule Latency");
    expect(markdown).toContain("## Model Comparison");
    expect(html).toContain("Rule Latency By Stage");
  });
});
