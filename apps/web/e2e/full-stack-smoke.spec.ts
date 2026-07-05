import { expect, test } from "@playwright/test";
import { execFileSync, spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import {
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
  mkdirSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const serviceUrl = "http://127.0.0.1:7373";
const repoRoot = resolve(import.meta.dirname, "../../..");
const analyzerPath = join(repoRoot, "workers/typescript-analyzer/src/main.ts");

test("full stack deterministic run can be started, reviewed, and reverted", async ({
  page,
  request,
}) => {
  const workspace = mkdtempSync(join(tmpdir(), "local-refactor-e2e-"));
  const fixtureRepo = join(workspace, "repo");
  const dbPath = join(workspace, "local-refactor.sqlite");
  let service: ChildProcessWithoutNullStreams | null = null;

  try {
    mkdirSync(join(fixtureRepo, "src"), { recursive: true });
    execFileSync("git", ["init", fixtureRepo], { stdio: "ignore" });
    const sample = join(fixtureRepo, "src/sample.ts");
    writeFileSync(
      sample,
      [
        "export function isReady(value: boolean) {",
        "  if (value) {",
        "    return true;",
        "  }",
        "  return false;",
        "}",
        "",
      ].join("\n"),
    );

    service = spawn("cargo", ["run", "-p", "local-refactor-service"], {
      cwd: repoRoot,
      env: {
        ...process.env,
        LOCAL_REFACTOR_DB: dbPath,
        LOCAL_REFACTOR_PORT: "7373",
        LOCAL_REFACTOR_ANALYZER: analyzerPath,
      },
    });
    await waitForService(service);

    const created = await request.post(`${serviceUrl}/api/repositories`, {
      data: {
        path: fixtureRepo,
        label: "Full Stack Repo",
      },
    });
    expect(created.ok()).toBe(true);

    await page.goto("/");
    await expect(page.locator("input").first()).toHaveValue("Full Stack Repo");
    await expect(page.getByText("src/sample.ts").first()).toBeVisible({
      timeout: 30_000,
    });

    await page.getByRole("button", { name: /Start run/ }).click();
    await expect(page.getByRole("heading", { name: "Review run" })).toBeVisible();
    await page.getByRole("button", { name: /Confirm and start run/ }).click();

    await expect(page.locator(".status.succeeded").first()).toBeVisible({
      timeout: 30_000,
    });
    await page.getByRole("button", { name: "Refresh" }).click();
    await expect(page.locator(".detail").getByText("succeeded")).toBeVisible({
      timeout: 30_000,
    });
    await expect(page.locator(".diff-stack").getByText(/return value/).first()).toBeVisible({
      timeout: 30_000,
    });
    expect(readFileSync(sample, "utf8")).toContain("return value;");

    await page.getByRole("button", { name: /Revert/ }).click();
    await expect(page.locator(".status.reverted").first()).toBeVisible({
      timeout: 30_000,
    });
    expect(readFileSync(sample, "utf8")).toContain("return true;");
  } finally {
    if (service && service.exitCode === null) {
      service.kill();
      await new Promise((resolveExit) => service?.once("exit", resolveExit));
    }
    rmSync(workspace, { recursive: true, force: true });
  }
});

async function waitForService(service: ChildProcessWithoutNullStreams) {
  let stderr = "";
  service.stdout.on("data", () => undefined);
  service.stderr.on("data", (chunk) => {
    stderr += chunk.toString();
  });

  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (service.exitCode !== null) {
      throw new Error(`service exited early with ${service.exitCode}\n${stderr}`);
    }
    try {
      const response = await fetch(`${serviceUrl}/api/health`);
      if (response.ok) return;
    } catch {
      await new Promise((resolveWait) => setTimeout(resolveWait, 250));
    }
  }
  throw new Error(`service did not become healthy\n${stderr}`);
}
