import type { DeterministicRule } from "./types";

export const improveLocalName: DeterministicRule = ({ content }) => {
  const summaries: string[] = [];
  const next = content.replace(
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
};
