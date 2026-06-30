import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { plan } from "./main";

const FIXTURE_ROOT = join(import.meta.dir, "../test-fixtures/simplify-conditional");

test("simplifies boolean-return conditionals", () => {
  expectEdit("true-false", "simplify-conditional");
  expectEdit("false-true", "simplify-conditional");
});

test("leaves code unchanged when simplify-conditional does not apply", () => {
  const response = runPlan("no-edit", ["simplify-conditional"]);

  expect(response.edits).toHaveLength(0);
});

function expectEdit(fixtureName: string, ruleId: string) {
  const response = runPlan(fixtureName, [ruleId]);
  const expected = readFixture(`${fixtureName}.expected.ts`);

  expect(response.edits).toHaveLength(1);
  expect(response.edits[0].ruleId).toBe(ruleId);
  expect(response.edits[0].newContent).toBe(expected);
}

function runPlan(fixtureName: string, rules: string[]) {
  const input = readFixture(`${fixtureName}.input.ts`);
  return withTempSource(fixtureName, input, (file) => plan({ files: [file], rules }));
}

function readFixture(name: string): string {
  return readFileSync(join(FIXTURE_ROOT, name), "utf8");
}

function withTempSource<T>(
  fixtureName: string,
  contents: string,
  run: (file: string) => T,
): T {
  const dir = mkdtempSync(join(tmpdir(), "local-refactor-analyzer-"));
  try {
    const file = join(dir, `${fixtureName}.ts`);
    writeFileSync(file, contents);
    return run(file);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
