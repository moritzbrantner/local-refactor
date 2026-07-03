import type { SourceFile } from "ts-morph";

export type RuleResult = {
  content: string;
  summaries: string[];
};

export type RuleContext = {
  content: string;
  filePath: string;
  sourceFile: SourceFile;
};

export type DeterministicRule = (context: RuleContext) => RuleResult;
