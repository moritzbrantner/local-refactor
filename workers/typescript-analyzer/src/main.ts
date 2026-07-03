import { Project, SyntaxKind } from "ts-morph";
import { readFileSync } from "node:fs";
import { deterministicRules } from "./rules";

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
        const currentSourceFile = project.createSourceFile(file, newContent, {
          overwrite: true,
        });
        const result = rule({
          content: newContent,
          filePath: file,
          sourceFile: currentSourceFile,
        });
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
