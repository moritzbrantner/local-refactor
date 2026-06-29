import { expect, test } from "bun:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { plan } from "./main";

test("simplifies boolean-return conditionals", () => {
  const dir = join(import.meta.dir, "../.tmp");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, "sample.ts");
  writeFileSync(
    file,
    `
export function isReady(value: boolean) {
  if (value) {
    return true;
  }
  return false;
}
`,
  );

  const response = plan({
    files: [file],
    rules: ["simplify-conditional"],
  });

  expect(response.edits).toHaveLength(1);
  expect(response.edits[0].newContent).toContain("return value;");
});

