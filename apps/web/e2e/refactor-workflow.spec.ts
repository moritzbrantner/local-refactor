import { expect, test } from "@playwright/test";
import {
  createFixtureApi,
  defaultRepository,
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

test("user can configure and start a deterministic refactor run", async ({ page }) => {
  const repository = defaultRepository();
  const fixture = createFixtureApi({ repositories: [repository] });
  await fixture.install(page);

  await page.goto("/");
  await expect(page.locator("input").first()).toHaveValue("Fixture Repo");

  await page.getByLabel("Validation commands").fill("bun test\nbun run typecheck");
  await page.getByLabel("Protected paths").fill("src/generated/**\ndist/**");
  await page.getByRole("button", { name: /Start run/ }).click();

  expect(fixture.lastRunRequest).toMatchObject({
    repositoryId: "repo-1",
    targetRelativePath: ".",
    rules: ["simplify-conditional"],
    model: "qwen2.5-coder:7b",
    testFileMode: "readOnly",
    validationCommands: ["bun test", "bun run typecheck"],
    protectedPaths: ["src/generated/**", "dist/**"],
  });

  await expect(page.locator(".status.succeeded").first()).toBeVisible();
  await expect(page.getByText("simplify-conditional")).toBeVisible();
  await expect(page.getByText("qwen2.5-coder:7b").first()).toBeVisible();
  await expect(page.getByText("--- /tmp/local-refactor-fixture/src/sample.ts")).toBeVisible();
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
