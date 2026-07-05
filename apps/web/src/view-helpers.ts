import type { RepositoryRecord, Rule, RuleSelectionPlan, RunEvent, RunRecord } from "./types";

export const ROOT_PATH = ".";

export function lines(value: string): string[] {
  return value
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function languageLabel(language: Rule["language"]): string {
  return language === "rust" ? "Rust" : "TypeScript";
}

export function flattenedPlanRules(plan: RuleSelectionPlan): string[] {
  return [...new Set(plan.segments.flatMap((segment) => segment.rules))].sort();
}

export function ruleRecords(ruleIds: string[], rules: Rule[]): Rule[] {
  return ruleIds
    .map((ruleId) => rules.find((rule) => rule.id === ruleId))
    .filter((rule): rule is Rule => Boolean(rule));
}

export function ruleName(ruleId: string, rules: Rule[]): string {
  return rules.find((rule) => rule.id === ruleId)?.name ?? ruleId;
}

export function formatToken(value: string): string {
  return value
    .split("-")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

export function runTargetLabel(run: RunRecord): string {
  if (run.targetRelativePath && run.targetRelativePath !== ROOT_PATH) {
    return run.targetRelativePath;
  }
  if (run.targetRelativePath === ROOT_PATH) {
    return "Repository root";
  }
  return run.targetPath;
}

export function repositoryLabel(run: RunRecord, repositories: RepositoryRecord[]): string {
  const repository = repositories.find((item) => item.id === run.repositoryId);
  if (repository) return repository.label;
  return run.repositoryRootPath ?? run.targetPath;
}

export function appendUniqueRunEvent(events: RunEvent[], event: RunEvent): RunEvent[] {
  if (events.some((existing) => existing.id === event.id)) return events;
  return [...events, event].sort((left, right) => left.id - right.id);
}

export function latestDownloadProgress(
  events: RunEvent[],
): { label: string; percent: number } | null {
  for (const event of [...events].reverse()) {
    const match = event.message.match(/^(Downloading .+): (\d+)%/);
    if (!match) continue;
    return {
      label: match[1],
      percent: Math.min(100, Number(match[2])),
    };
  }
  return null;
}

export function repositoryFilePath(repositoryRootPath: string, relativePath: string): string {
  const root = repositoryRootPath.replace(/[\\/]+$/, "");
  return `${root}/${relativePath}`;
}

export function pathsEndWithSameFile(absolutePath: string, relativePath: string): boolean {
  const normalizedAbsolute = absolutePath.replaceAll("\\", "/");
  const normalizedRelative = relativePath.replaceAll("\\", "/");
  return (
    normalizedAbsolute === normalizedRelative ||
    normalizedAbsolute.endsWith(`/${normalizedRelative}`)
  );
}

export function changedLineNumbers(originalContent: string, newContent: string): number[] {
  const originalLines = originalContent.split("\n");
  const newLines = newContent.split("\n");
  const commonSubsequence = longestCommonLineSubsequence(originalLines, newLines);
  const unchangedOriginalLines = new Set(commonSubsequence.map(([originalIndex]) => originalIndex));
  const changedLines: number[] = [];

  for (let index = 0; index < originalLines.length; index += 1) {
    if (!unchangedOriginalLines.has(index)) {
      changedLines.push(index + 1);
    }
  }

  if (changedLines.length === 0 && originalContent !== newContent) {
    return [Math.max(1, originalLines.length)];
  }

  return changedLines;
}

function longestCommonLineSubsequence(
  left: string[],
  right: string[],
): Array<[number, number]> {
  const table = Array.from({ length: left.length + 1 }, () =>
    Array<number>(right.length + 1).fill(0),
  );

  for (let leftIndex = left.length - 1; leftIndex >= 0; leftIndex -= 1) {
    for (let rightIndex = right.length - 1; rightIndex >= 0; rightIndex -= 1) {
      table[leftIndex][rightIndex] =
        left[leftIndex] === right[rightIndex]
          ? table[leftIndex + 1][rightIndex + 1] + 1
          : Math.max(table[leftIndex + 1][rightIndex], table[leftIndex][rightIndex + 1]);
    }
  }

  const pairs: Array<[number, number]> = [];
  let leftIndex = 0;
  let rightIndex = 0;
  while (leftIndex < left.length && rightIndex < right.length) {
    if (left[leftIndex] === right[rightIndex]) {
      pairs.push([leftIndex, rightIndex]);
      leftIndex += 1;
      rightIndex += 1;
    } else if (table[leftIndex + 1][rightIndex] >= table[leftIndex][rightIndex + 1]) {
      leftIndex += 1;
    } else {
      rightIndex += 1;
    }
  }

  return pairs;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  return `${(bytes / 1024).toFixed(1)} KiB`;
}
