import type { DeterministicRule } from "./types";

export const extractTypeDefinition: DeterministicRule = ({ content }) => {
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
};
