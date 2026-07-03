import { Node, SyntaxKind } from "ts-morph";
import type { DeterministicRule } from "./types";

export const inlineTrivialHelper: DeterministicRule = ({ content, sourceFile }) => {
  const helper = sourceFile.getFunctions().find((functionDeclaration) => {
    if (functionDeclaration.isExported()) return false;
    if (functionDeclaration.getParameters().length !== 1) return false;

    const body = functionDeclaration.getBody();
    const statements = body?.getStatements() ?? [];
    return statements.length === 1 && Node.isReturnStatement(statements[0]);
  });
  if (!helper) return { content, summaries: [] };

  const helperName = helper.getName();
  const parameter = helper.getParameters()[0];
  const returnStatement = helper
    .getBodyOrThrow()
    .getStatements()[0]
    .asKindOrThrow(SyntaxKind.ReturnStatement);
  const expression = returnStatement.getExpression()?.getText(sourceFile);
  if (!helperName || !parameter || !expression) return { content, summaries: [] };
  if (/\b(console|fetch|save|write|delete|push|splice)\b/.test(expression)) {
    return { content, summaries: [] };
  }

  const callPattern = new RegExp(`\\b${escapeRegExp(helperName)}\\(([^()]+)\\)`, "g");
  const withoutDeclaration = removeDeclaration(content, helper.getFullStart(), helper.getEnd());
  const calls = [...withoutDeclaration.matchAll(callPattern)];
  if (calls.length !== 1) return { content, summaries: [] };

  const argument = calls[0][1].trim();
  const inlined = expression.replace(
    new RegExp(`\\b${escapeRegExp(parameter.getName())}\\b`, "g"),
    argument,
  );
  return {
    content: withoutDeclaration.replace(callPattern, `(${inlined})`),
    summaries: [`Inlined one-use helper ${helperName}`],
  };
};

function removeDeclaration(content: string, fullStart: number, end: number) {
  let removeEnd = end;
  if (content.slice(removeEnd, removeEnd + 2) === "\n\n") {
    removeEnd += 2;
  } else if (content[removeEnd] === "\n") {
    removeEnd += 1;
  }
  return `${content.slice(0, fullStart)}${content.slice(removeEnd)}`;
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
