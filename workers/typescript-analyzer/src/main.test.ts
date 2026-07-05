import { expect, test } from "bun:test";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { plan } from "./main";

type RuleFixtureManifest = {
  ruleId: string;
  language: "typescript";
  positiveCases: string[];
  noEditCases: string[];
  invalidCases: string[];
  requiredDiagnostics: string[];
};

const FIXTURE_ROOT = join(import.meta.dir, "../test-fixtures");

for (const manifest of fixtureManifests()) {
  test(`${manifest.ruleId} positive fixtures produce expected edits`, () => {
    for (const caseName of manifest.positiveCases) {
      const response = runManifestCase(manifest, "positive", caseName, [
        manifest.ruleId,
      ]);
      const expected = readFixture(
        manifest,
        "positive",
        `${caseName}.expected.ts`,
      );

      expect(response.edits, caseName).toHaveLength(1);
      expect(response.edits[0].ruleId).toBe(manifest.ruleId);
      expect(response.edits[0].newContent).toBe(expected);
      expectDiagnostics(response.diagnostics, manifest.requiredDiagnostics);
    }
  });

  test(`${manifest.ruleId} no-edit fixtures produce no edits`, () => {
    for (const caseName of manifest.noEditCases) {
      const response = runManifestCase(manifest, "no-edit", caseName, [
        manifest.ruleId,
      ]);

      expect(response.edits, caseName).toHaveLength(0);
      expectDiagnostics(response.diagnostics, manifest.requiredDiagnostics);
    }
  });

  test(`${manifest.ruleId} invalid fixtures do not crash analyzer`, () => {
    for (const caseName of manifest.invalidCases) {
      const response = runManifestCase(manifest, "invalid", caseName, [
        manifest.ruleId,
      ]);

      expect(response.diagnostics.length, caseName).toBeGreaterThan(0);
    }
  });

  test(`${manifest.ruleId} disabled rule produces no edits`, () => {
    for (const caseName of manifest.positiveCases) {
      const response = runManifestCase(manifest, "positive", caseName, []);

      expect(response.edits, caseName).toHaveLength(0);
    }
  });
}

test("reports diagnostics for every requested file", () => {
  const first = "export function first() {\n  return true;\n}\n";
  const second = "export const second = () => false;\n";

  const response = withTempSources(
    [
      ["first.ts", first],
      ["second.ts", second],
    ],
    (files) => plan({ files, rules: ["simplify-conditional"] }),
  );

  const analyzed = response.diagnostics.filter((diagnostic) =>
    diagnostic.startsWith("Analyzed "),
  );
  expect(analyzed).toHaveLength(2);
  expect(analyzed[0]).toContain("1 function declarations");
  expect(analyzed[1]).toContain("1 arrow functions");
});

test("skips unreadable or missing files without failing whole plan", () => {
  const manifest = fixtureManifests().find(
    (candidate) => candidate.ruleId === "simplify-conditional",
  );
  if (!manifest) throw new Error("simplify-conditional manifest missing");
  const input = readFixture(manifest, "positive", "true-false.input.ts");

  const response = withTempSources([["valid.ts", input]], (files) =>
    plan({
      files: [...files, join(tmpdir(), "local-refactor-missing-file.ts")],
      rules: ["simplify-conditional"],
    }),
  );

  expect(response.edits).toHaveLength(1);
  expect(
    response.diagnostics.some((diagnostic) => diagnostic.startsWith("Skipped ")),
  ).toBe(true);
});

test("applies multiple enabled rules in deterministic rule order", () => {
  const source = [
    'import { beta } from "./tools";',
    'import { alpha } from "./tools";',
    "",
    "export function isReady(value: boolean) {",
    "  if (value) {",
    "    return true;",
    "  }",
    "  return false;",
    "}",
    "",
  ].join("\n");

  const response = withTempSource("multi-rule", source, (file) =>
    plan({ files: [file], rules: ["normalize-imports", "simplify-conditional"] }),
  );

  expect(response.edits).toHaveLength(1);
  expect(response.edits[0].ruleId).toBe("simplify-conditional");
  expect(response.edits[0].newContent).toContain(
    'import { alpha, beta } from "./tools";',
  );
  expect(response.edits[0].newContent).toContain("return value;");
  expect(response.edits[0].summary).toContain(
    "Replaced if/return true/false with return value",
  );
  expect(response.edits[0].summary).toContain("Normalized duplicate named imports");
});

function fixtureManifests(): RuleFixtureManifest[] {
  return readdirSync(FIXTURE_ROOT)
    .map((directory) => join(FIXTURE_ROOT, directory, "manifest.json"))
    .filter((manifestPath) => existsSync(manifestPath))
    .map((manifestPath) => {
      const manifest = JSON.parse(
        readFileSync(manifestPath, "utf8"),
      ) as RuleFixtureManifest;
      validateManifest(manifest, manifestPath);
      return manifest;
    })
    .sort((left, right) => left.ruleId.localeCompare(right.ruleId));
}

function validateManifest(manifest: RuleFixtureManifest, manifestPath: string) {
  expect(manifest.language, manifestPath).toBe("typescript");
  expect(manifest.ruleId, manifestPath).toMatch(/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/);
  for (const [kind, cases] of [
    ["positive", manifest.positiveCases],
    ["no-edit", manifest.noEditCases],
    ["invalid", manifest.invalidCases],
  ] as const) {
    expect(Array.isArray(cases), `${manifest.ruleId} ${kind}`).toBe(true);
    for (const caseName of cases) {
      const input = fixturePath(manifest, kind, `${caseName}.input.ts`);
      expect(existsSync(input), input).toBe(true);
      if (kind === "positive") {
        const expected = fixturePath(manifest, kind, `${caseName}.expected.ts`);
        expect(existsSync(expected), expected).toBe(true);
      }
    }
  }
}

function runManifestCase(
  manifest: RuleFixtureManifest,
  kind: "positive" | "no-edit" | "invalid",
  caseName: string,
  rules: string[],
) {
  const input = readFixture(manifest, kind, `${caseName}.input.ts`);
  return withTempSource(caseName, input, (file) => plan({ files: [file], rules }));
}

function readFixture(
  manifest: RuleFixtureManifest,
  kind: "positive" | "no-edit" | "invalid",
  name: string,
): string {
  return readFileSync(fixturePath(manifest, kind, name), "utf8");
}

function fixturePath(
  manifest: RuleFixtureManifest,
  kind: "positive" | "no-edit" | "invalid",
  name: string,
) {
  return join(FIXTURE_ROOT, manifest.ruleId, kind, name);
}

function expectDiagnostics(diagnostics: string[], required: string[]) {
  for (const expected of required) {
    expect(
      diagnostics.some((diagnostic) => diagnostic.includes(expected)),
      `missing diagnostic containing ${expected}`,
    ).toBe(true);
  }
}

function withTempSource<T>(
  fixtureName: string,
  contents: string,
  run: (file: string) => T,
): T {
  return withTempSources([[`${fixtureName}.ts`, contents]], ([file]) => run(file));
}

function withTempSources<T>(
  sources: Array<[name: string, contents: string]>,
  run: (files: string[]) => T,
): T {
  const dir = mkdtempSync(join(tmpdir(), "local-refactor-analyzer-"));
  try {
    const files = sources.map(([name, contents]) => {
      const file = join(dir, name);
      writeFileSync(file, contents);
      return file;
    });
    return run(files);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
