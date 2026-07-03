import type { DeterministicRule } from "./types";

export const normalizeImports: DeterministicRule = ({ content, sourceFile }) => {
  const importDeclarations = sourceFile.getImportDeclarations();
  if (importDeclarations.some((declaration) => !declaration.getImportClause())) {
    return { content, summaries: [] };
  }

  const namedImports = importDeclarations.filter(
    (declaration) => declaration.getNamedImports().length > 0,
  );
  if (namedImports.length < 2) return { content, summaries: [] };

  const importsByModule = new Map<string, Set<string>>();
  const importLineIndexes: number[] = [];
  const lines = content.split("\n");

  for (const declaration of namedImports) {
    const module = declaration.getModuleSpecifierValue();
    const existing = importsByModule.get(module) ?? new Set<string>();
    for (const namedImport of declaration.getNamedImports()) {
      existing.add(namedImport.getText(sourceFile).trim());
    }
    importsByModule.set(module, existing);

    const lineNumber = declaration.getStartLineNumber() - 1;
    importLineIndexes.push(lineNumber);
  }

  if (importsByModule.size === importLineIndexes.length) {
    return { content, summaries: [] };
  }

  const firstImport = Math.min(...importLineIndexes);
  const importLines = new Set(importLineIndexes);
  const nextLines = lines.filter((_, index) => !importLines.has(index));
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
};
