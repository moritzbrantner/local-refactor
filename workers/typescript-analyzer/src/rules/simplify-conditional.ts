import { Node, SyntaxKind } from "ts-morph";
import type { DeterministicRule, RuleResult } from "./types";

export const simplifyBooleanReturnConditionals: DeterministicRule = ({
  content,
  sourceFile,
}) => {
  const replacements: Array<{
    start: number;
    end: number;
    text: string;
    summary: string;
  }> = [];

  for (const ifStatement of sourceFile.getDescendantsOfKind(SyntaxKind.IfStatement)) {
    if (ifStatement.getElseStatement()) continue;

    const parent = ifStatement.getParent();
    if (!Node.isBlock(parent) && !Node.isSourceFile(parent)) continue;

    const statements = parent.getStatements();
    const statementIndex = statements.findIndex((statement) => statement === ifStatement);
    const nextStatement = statements[statementIndex + 1];
    if (!Node.isReturnStatement(nextStatement)) continue;

    const thenStatement = ifStatement.getThenStatement();
    if (!Node.isBlock(thenStatement)) continue;

    const thenStatements = thenStatement.getStatements();
    if (thenStatements.length !== 1 || !Node.isReturnStatement(thenStatements[0])) {
      continue;
    }

    const condition = ifStatement.getExpression().getText(sourceFile).trim();
    if (/[()\n;{}]/.test(condition)) continue;

    const thenValue = booleanReturnValue(thenStatements[0]);
    const nextValue = booleanReturnValue(nextStatement);
    if (thenValue === undefined || nextValue === undefined || thenValue === nextValue) {
      continue;
    }

    replacements.push({
      start: ifStatement.getStart(sourceFile),
      end: nextStatement.getEnd(),
      text: thenValue ? `return ${condition};` : `return !(${condition});`,
      summary: thenValue
        ? `Replaced if/return true/false with return ${condition}`
        : `Replaced if/return false/true with return !(${condition})`,
    });
  }

  return applyReplacements(content, replacements);
};

function booleanReturnValue(returnStatement: import("ts-morph").ReturnStatement) {
  const expression = returnStatement.getExpression();
  const text = expression?.getText(returnStatement.getSourceFile()).trim();
  if (text === "true") return true;
  if (text === "false") return false;
  return undefined;
}

function applyReplacements(
  content: string,
  replacements: Array<{ start: number; end: number; text: string; summary: string }>,
): RuleResult {
  if (replacements.length === 0) return { content, summaries: [] };

  let next = content;
  const summaries: string[] = [];
  for (const replacement of replacements.sort((left, right) => right.start - left.start)) {
    next = `${next.slice(0, replacement.start)}${replacement.text}${next.slice(
      replacement.end,
    )}`;
    summaries.push(replacement.summary);
  }
  summaries.reverse();
  return { content: next, summaries };
}
