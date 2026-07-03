import type { DeterministicRule } from "./types";

export const sortIndependentDeclarations: DeterministicRule = ({ content }) => {
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
};
