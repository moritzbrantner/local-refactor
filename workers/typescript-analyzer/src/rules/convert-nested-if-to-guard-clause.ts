import type { DeterministicRule } from "./types";

export const convertNestedIfToGuardClause: DeterministicRule = ({ content }) => {
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
};
