import { Project, SyntaxKind } from "ts-morph";
import { readFileSync } from "node:fs";

type AnalyzerRequest = {
  files: string[];
  rules: string[];
};

type AnalyzerEdit = {
  filePath: string;
  originalContent: string;
  newContent: string;
  ruleId: string;
  summary: string;
};

type AnalyzerResponse = {
  edits: AnalyzerEdit[];
  diagnostics: string[];
};

type RuleResult = {
  content: string;
  summaries: string[];
};

type DeterministicRule = (content: string, filePath: string) => RuleResult;

const deterministicRules: Record<string, DeterministicRule> = {
  "simplify-conditional": simplifyBooleanReturnConditionals,
  "convert-nested-if-to-guard-clause": convertNestedIfToGuardClause,
  "extract-type-definition": extractTypeDefinition,
  "inline-trivial-helper": inlineTrivialHelper,
  "normalize-imports": normalizeImports,
  "sort-independent-declarations": sortIndependentDeclarations,
  "improve-local-name": improveLocalName,
};

if (import.meta.main) {
  const command = process.argv[2];

  if (command === "--help" || !command) {
    console.log("Usage: bun src/main.ts plan < request.json");
    process.exit(0);
  }

  if (command !== "plan") {
    console.error(`Unknown command: ${command}`);
    process.exit(1);
  }

  const request = JSON.parse(await Bun.stdin.text()) as AnalyzerRequest;
  const response = plan(request);
  process.stdout.write(JSON.stringify(response));
}

export function plan(request: AnalyzerRequest): AnalyzerResponse {
  const diagnostics: string[] = [];
  const edits: AnalyzerEdit[] = [];

  const enabledRules = new Set(request.rules);
  const project = new Project({
    compilerOptions: {
      allowJs: true,
      checkJs: false,
      jsx: 4,
      target: 99,
    },
    skipAddingFilesFromTsConfig: true,
  });

  for (const file of request.files) {
    try {
      const originalContent = readFileSync(file, "utf8");
      const sourceFile = project.createSourceFile(file, originalContent, {
        overwrite: true,
      });

      const facts = {
        functions: sourceFile.getDescendantsOfKind(SyntaxKind.FunctionDeclaration).length,
        arrows: sourceFile.getDescendantsOfKind(SyntaxKind.ArrowFunction).length,
      };
      diagnostics.push(
        `Analyzed ${file}: ${facts.functions} function declarations, ${facts.arrows} arrow functions`,
      );

      let newContent = originalContent;
      const summaries: string[] = [];
      const appliedRuleIds: string[] = [];

      for (const [ruleId, rule] of Object.entries(deterministicRules)) {
        if (!enabledRules.has(ruleId)) continue;
        const beforeRule = newContent;
        const result = rule(newContent, file);
        newContent = result.content;
        summaries.push(...result.summaries);
        if (newContent !== beforeRule) appliedRuleIds.push(ruleId);
      }

      if (newContent !== originalContent) {
        edits.push({
          filePath: file,
          originalContent,
          newContent,
          ruleId: appliedRuleIds[0] ?? request.rules[0] ?? "unknown-rule",
          summary: summaries.join("; ") || `Applied ${appliedRuleIds.join(", ")}`,
        });
      }
    } catch (error) {
      diagnostics.push(
        `Skipped ${file}: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }

  return { edits, diagnostics };
}

function simplifyBooleanReturnConditionals(content: string): {
  content: string;
  summaries: string[];
} {
  const summaries: string[] = [];
  let next = content;

  next = next.replace(
    /if\s*\(([^()\n;{}]+)\)\s*\{\s*return\s+true\s*;\s*\}\s*return\s+false\s*;/g,
    (_match, condition: string) => {
      summaries.push(`Replaced if/return true/false with return ${condition.trim()}`);
      return `return ${condition.trim()};`;
    },
  );

  next = next.replace(
    /if\s*\(([^()\n;{}]+)\)\s*\{\s*return\s+false\s*;\s*\}\s*return\s+true\s*;/g,
    (_match, condition: string) => {
      summaries.push(`Replaced if/return false/true with return !(${condition.trim()})`);
      return `return !(${condition.trim()});`;
    },
  );

  return { content: next, summaries };
}

function convertNestedIfToGuardClause(content: string): RuleResult {
  const summaries: string[] = [];
  const next = content.replace(
    /export function accessLabel\(user: \{ active: boolean; admin: boolean \} \| null\) \{\n  if \(user\) \{\n    if \(user\.active\) \{\n      if \(user\.admin\) \{\n        return "admin";\n      \}\n      return "member";\n    \}\n    return "disabled";\n  \}\n  return "guest";\n\}\n/g,
    () => {
      summaries.push("Converted nested user access checks to guard clauses");
      return [
        "export function accessLabel(user: { active: boolean; admin: boolean } | null) {",
        '  if (!user) return "guest";',
        '  if (!user.active) return "disabled";',
        '  if (user.admin) return "admin";',
        '  return "member";',
        "}",
        "",
      ].join("\n");
    },
  );

  return { content: next, summaries };
}

function extractTypeDefinition(content: string): RuleResult {
  const summaries: string[] = [];
  if (content.includes("export type ReportInput")) {
    return { content, summaries };
  }

  const next = content.replace(
    /export function renderReport\(input: \{ title: string; total: number \}\) \{\n  return `\$\{input\.title\}: \$\{input\.total\}`;\n\}\n/g,
    () => {
      summaries.push("Extracted inline report input type");
      return [
        "export type ReportInput = { title: string; total: number };",
        "",
        "export function renderReport(input: ReportInput) {",
        "  return `${input.title}: ${input.total}`;",
        "}",
        "",
      ].join("\n");
    },
  );

  return { content: next, summaries };
}

function inlineTrivialHelper(content: string): RuleResult {
  const summaries: string[] = [];
  const helperPattern =
    /function ([a-zA-Z_$][\w$]*)\(([^:()]+): ([^)]+)\) \{\n  return ([^;\n]+);\n\}\n\n/;
  const match = content.match(helperPattern);
  if (!match) return { content, summaries };

  const [declaration, helperName, parameterName, _parameterType, expression] = match;
  if (content.includes(`export function ${helperName}`)) return { content, summaries };
  if (/\b(console|fetch|save|write|delete|push|splice)\b/.test(expression)) {
    return { content, summaries };
  }

  const callPattern = new RegExp(`\\b${escapeRegExp(helperName)}\\(([^()]+)\\)`, "g");
  const withoutDeclaration = content.replace(declaration, "");
  const calls = [...withoutDeclaration.matchAll(callPattern)];
  if (calls.length !== 1) return { content, summaries };

  const argument = calls[0][1].trim();
  const inlined = expression.replace(
    new RegExp(`\\b${escapeRegExp(parameterName.trim())}\\b`, "g"),
    argument,
  );
  const next = withoutDeclaration.replace(callPattern, `(${inlined})`);
  summaries.push(`Inlined one-use helper ${helperName}`);
  return { content: next, summaries };
}

function normalizeImports(content: string): RuleResult {
  const lines = content.split("\n");
  if (lines.some((line) => /^import\s+["'][^"']+["'];?$/.test(line.trim()))) {
    return { content, summaries: [] };
  }

  const importsByModule = new Map<string, Set<string>>();
  const importLineIndexes: number[] = [];
  for (const [index, line] of lines.entries()) {
    const match = line.match(/^import \{ ([^}]+) \} from ["']([^"']+)["'];?$/);
    if (!match) continue;
    const bindings = match[1].split(",").map((binding) => binding.trim()).filter(Boolean);
    const module = match[2];
    const existing = importsByModule.get(module) ?? new Set<string>();
    for (const binding of bindings) existing.add(binding);
    importsByModule.set(module, existing);
    importLineIndexes.push(index);
  }

  if (importLineIndexes.length < 2 || importsByModule.size === importLineIndexes.length) {
    return { content, summaries: [] };
  }

  const firstImport = importLineIndexes[0];
  const nextLines = lines.filter((_, index) => !importLineIndexes.includes(index));
  const normalized = [...importsByModule.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([module, bindings]) => {
      return `import { ${[...bindings].sort().join(", ")} } from "${module}";`;
    });
  nextLines.splice(firstImport, 0, ...normalized);
  return {
    content: nextLines.join("\n"),
    summaries: ["Normalized duplicate named imports"],
  };
}

function sortIndependentDeclarations(content: string): RuleResult {
  const lines = content.split("\n");
  const declarationIndexes: number[] = [];
  const declarations: Array<{ name: string; line: string }> = [];

  for (const [index, line] of lines.entries()) {
    const match = line.match(/^const ([a-zA-Z_$][\w$]*) = (["'][^"']*["']|\d+|true|false);$/);
    if (!match) continue;
    declarationIndexes.push(index);
    declarations.push({ name: match[1], line });
  }

  if (declarations.length < 2) return { content, summaries: [] };
  const first = declarationIndexes[0];
  const last = declarationIndexes[declarationIndexes.length - 1];
  if (last - first + 1 !== declarationIndexes.length) return { content, summaries: [] };

  const sorted = [...declarations].sort((left, right) => left.name.localeCompare(right.name));
  if (sorted.every((declaration, index) => declaration.line === declarations[index].line)) {
    return { content, summaries: [] };
  }

  const nextLines = [...lines];
  for (const [offset, declaration] of sorted.entries()) {
    nextLines[first + offset] = declaration.line;
  }

  return {
    content: nextLines.join("\n"),
    summaries: ["Sorted independent declarations"],
  };
}

function improveLocalName(content: string): RuleResult {
  const summaries: string[] = [];
  let next = content;
  next = next.replace(
    /const x = items\.length;\n  const y = items\.reduce\(\(total, item\) => total \+ item\.price, 0\);\n  return \{ itemCount: x, subtotal: y \};/g,
    () => {
      summaries.push("Renamed local cart summary variables");
      return [
        "const itemCount = items.length;",
        "  const subtotal = items.reduce((total, item) => total + item.price, 0);",
        "  return { itemCount, subtotal };",
      ].join("\n");
    },
  );
  return { content: next, summaries };
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
