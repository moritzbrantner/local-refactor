import { expect, test } from "@playwright/test";
import {
  createFixtureApi,
  defaultRepository,
  missingModel,
  readyModel,
  succeededRun,
} from "./fixture-api";

test("user can choose a repository source and see folders", async ({ page }) => {
  const fixture = createFixtureApi();
  await fixture.install(page);

  await page.goto("/");
  await expect(page.getByText("No repositories added.")).toBeVisible();

  await page.getByRole("button", { name: /Choose root folder/ }).click();

  await expect(page.locator("input").first()).toHaveValue("Fixture Repo");
  await expect(page.getByText("/tmp/local-refactor-fixture").first()).toBeVisible();
  await expect(page.getByRole("button", { name: /src/ })).toBeVisible();
});

test("user can minimize and restore repository and folder sections", async ({ page }) => {
  const repository = defaultRepository();
  const fixture = createFixtureApi({ repositories: [repository] });
  await fixture.install(page);

  await page.goto("/");
  await expect(page.getByRole("button", { name: /Choose root folder/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /src/ })).toBeVisible();

  await page.getByRole("button", { name: "Minimize repositories" }).click();
  await expect(page.getByRole("button", { name: /Choose root folder/ })).not.toBeVisible();
  await expect(page.getByRole("button", { name: "Restore repositories" })).toBeVisible();

  await page.getByRole("button", { name: "Minimize folders" }).click();
  await expect(page.getByRole("button", { name: /src/ })).not.toBeVisible();
  await expect(page.getByRole("button", { name: "Restore folders" })).toBeVisible();

  await page.getByRole("button", { name: "Restore repositories" }).click();
  await page.getByRole("button", { name: "Restore folders" }).click();
  await expect(page.getByRole("button", { name: /Choose root folder/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /src/ })).toBeVisible();
});

test("user can configure and start a deterministic refactor run", async ({ page }) => {
  const repository = defaultRepository();
  const fixture = createFixtureApi({ repositories: [repository] });
  await fixture.install(page);

  await page.goto("/");
  await expect(page.locator("input").first()).toHaveValue("Fixture Repo");
  await expect(page.getByText("Test Required").first()).toBeVisible();
  await expect(page.getByText("Single File").first()).toBeVisible();
  await expect(page.getByText("Candidate files").first()).toBeVisible();
  await expect(page.getByText("src/sample.ts").first()).toBeVisible();
  await expect(page.getByText("2 total").first()).toBeVisible();

  await page.getByLabel("Validation commands").fill("bun test\nbun run typecheck");
  await page.getByLabel("Protected paths").fill("src/generated/**\ndist/**");
  await page.getByRole("button", { name: /Start run/ }).click();
  await expect(page.getByRole("heading", { name: "Review run" })).toBeVisible();
  await expect(page.locator(".run-review").getByText("Candidate files")).toBeVisible();
  await expect(page.locator(".run-review").getByText("src/other.ts")).toBeVisible();
  await expect(page.locator(".run-review").getByText("Preserves Runtime Behavior, Typecheck")).toBeVisible();
  await expect(page.locator(".run-review").getByText("bun run typecheck")).toBeVisible();
  await page.getByRole("button", { name: /Confirm and start run/ }).click();

  expect(fixture.lastRunRequest).toMatchObject({
    repositoryId: "repo-1",
    targetRelativePath: ".",
    rules: [],
    ruleSelectionPlan: {
      targetRelativePath: ".",
      segments: [
        {
          relativePath: "src",
          rules: ["simplify-conditional"],
        },
      ],
    },
    model: "qwen2.5-coder:7b",
    testFileMode: "readOnly",
    validationCommands: ["bun test", "bun run typecheck"],
    protectedPaths: ["src/generated/**", "dist/**"],
  });
  expect(fixture.lastRunRequest).not.toHaveProperty("candidateFilePreview");

  await expect(page.locator(".status.succeeded").first()).toBeVisible();
  await expect(page.getByText("simplify-conditional").first()).toBeVisible();
  await expect(page.getByText("qwen2.5-coder:7b").first()).toBeVisible();
  await expect(page.getByText("--- /tmp/local-refactor-fixture/src/sample.ts")).toBeVisible();
  await expect(page.getByText("+  return value;")).toBeVisible();
});

test("candidate file preview updates with manual rules and test file mode", async ({ page }) => {
  const repository = defaultRepository();
  const fixture = createFixtureApi({ repositories: [repository] });
  await fixture.install(page);

  await page.goto("/");
  await expect(page.getByText("src/sample.ts").first()).toBeVisible();

  await page.getByRole("button", { name: "Manual" }).click();
  await page.getByLabel("Normalize Imports").check();
  await expect(page.getByText("src - Normalize Imports")).toBeVisible();

  await page.getByRole("button", { name: /Tests mutable/ }).click();
  await expect(page.getByText("src/sample.test.ts").first()).toBeVisible();
  await expect(page.getByText("3 total").first()).toBeVisible();
});

test("candidate file preview shows capped hidden counts", async ({ page }) => {
  const repository = defaultRepository();
  const fixture = createFixtureApi({
    repositories: [repository],
    candidatePreview: {
      targetRelativePath: ".",
      totalCandidateFiles: 4,
      limitPerGroup: 1,
      groups: [
        {
          id: "segment:src:rule:simplify-conditional",
          label: "src - Simplify Conditional",
          segmentRelativePath: "src",
          ruleId: "simplify-conditional",
          ruleName: "Simplify Conditional",
          language: "typescript",
          totalFiles: 4,
          hiddenFiles: 3,
          files: [{ relativePath: "src/a.ts" }],
        },
      ],
    },
  });
  await fixture.install(page);

  await page.goto("/");

  await expect(page.getByText("src/a.ts")).toBeVisible();
  await expect(page.getByText("3 more hidden")).toBeVisible();
});

test("candidate file preview errors block starting a run", async ({ page }) => {
  const repository = defaultRepository();
  const fixture = createFixtureApi({
    repositories: [repository],
    candidatePreviewFails: true,
  });
  await fixture.install(page);

  await page.goto("/");

  await expect(page.getByText("candidate preview failed")).toBeVisible();
  await expect(page.getByRole("button", { name: /Start run/ })).toBeDisabled();
});

test("user sees model availability and run safety settings before starting", async ({ page }) => {
  const repository = defaultRepository();
  const fixture = createFixtureApi({
    repositories: [repository],
    models: [readyModel(), missingModel()],
  });
  await fixture.install(page);

  await page.goto("/");
  await expect(page.getByRole("option", { name: /Qwen2.5 Coder 7B downloaded/ })).toHaveCount(1);
  await expect(page.getByRole("option", { name: /DeepSeek Coder 6.7B not downloaded/ })).toHaveCount(1);
  await expect(page.getByRole("button", { name: /Tests read-only/ })).toHaveClass(/active/);

  await page.getByLabel("Validation commands").fill("bun test\nbun run typecheck");
  await page.getByLabel("Protected paths").fill("src/generated/**\ndist/**");
  await page.getByRole("button", { name: /Start run/ }).click();
  await expect(page.getByText("validation runs on this machine through the local shell")).toBeVisible();
  await page.getByRole("button", { name: /Confirm and start run/ }).click();

  expect(fixture.lastRunRequest).toMatchObject({
    repositoryId: "repo-1",
    targetRelativePath: ".",
    model: "qwen2.5-coder:7b",
    testFileMode: "readOnly",
    validationCommands: ["bun test", "bun run typecheck"],
    protectedPaths: ["src/generated/**", "dist/**"],
  });

  await expect(page.getByText("qwen2.5-coder:7b").first()).toBeVisible();
  await expect(page.getByText("+  return value;")).toBeVisible();
});

test("user can inspect and revert a run", async ({ page }) => {
  const repository = defaultRepository();
  const run = succeededRun({ id: "run-existing", repository });
  const fixture = createFixtureApi({ repositories: [repository], runs: [run] });
  await fixture.install(page);

  await page.goto("/");
  await expect(page.locator(".status.succeeded").first()).toBeVisible();
  await expect(page.getByText("+  return value;")).toBeVisible();

  await page.getByRole("button", { name: /Revert/ }).click();

  expect(fixture.revertCalls).toEqual(["run-existing"]);
  await expect(page.locator(".status.reverted").first()).toBeVisible();
});

test("user can browse all persisted runs across repositories", async ({ page }) => {
  const repository = defaultRepository();
  const archivedRepository = {
    ...defaultRepository(),
    id: "repo-2",
    label: "Archived Repo",
    rootPath: "/tmp/local-refactor-archived",
  };
  const fixture = createFixtureApi({
    repositories: [repository, archivedRepository],
    runs: [
      succeededRun({ id: "run-current", repository, targetRelativePath: "src" }),
      succeededRun({
        id: "run-archived",
        repository: archivedRepository,
        targetRelativePath: "legacy",
      }),
    ],
  });
  await fixture.install(page);

  await page.goto("/");
  await expect(page.getByText("legacy")).not.toBeVisible();

  await page.getByRole("button", { name: "All" }).click();

  await expect(page.getByText("legacy")).toBeVisible();
  await page.getByRole("button", { name: "legacy" }).click();
  await expect(page.getByText("Archived Repo").last()).toBeVisible();
  await expect(page.getByText("Run completed successfully").first()).toBeVisible();
});
