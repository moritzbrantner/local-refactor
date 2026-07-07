import { Node } from "ts-morph";
import type { DeterministicRule } from "./types";

type SortableMember = {
  group: number;
  name: string;
  text: string;
};

export const sortTypeScriptClassMembers: DeterministicRule = ({ content, sourceFile }) => {
  const replacements: Array<{ start: number; end: number; text: string }> = [];

  for (const classDeclaration of sourceFile.getClasses()) {
    if (classDeclaration.getDecorators().length > 0) continue;
    const classText = classDeclaration.getText(sourceFile);
    if (classText.includes("static {")) continue;

    const members = classDeclaration.getMembers();
    if (members.length < 2) continue;

    const sortable = members.map((member): SortableMember | null => {
      if (Node.isPropertyDeclaration(member)) {
        if (member.getDecorators().length > 0 || member.getInitializer()) return null;
        const name = member.getName();
        return {
          group: member.isStatic() ? 0 : 1,
          name,
          text: indentedMemberText(content, member.getStart(), member.getText(sourceFile)),
        };
      }
      if (Node.isConstructorDeclaration(member)) {
        return {
          group: 2,
          name: "constructor",
          text: indentedMemberText(content, member.getStart(), member.getText(sourceFile)),
        };
      }
      if (Node.isMethodDeclaration(member)) {
        if (member.getDecorators().length > 0) return null;
        return {
          group: 3,
          name: member.getName(),
          text: indentedMemberText(content, member.getStart(), member.getText(sourceFile)),
        };
      }
      return null;
    });

    if (sortable.some((member) => member === null)) continue;
    const sortableMembers = sortable as SortableMember[];
    const sorted = [...sortableMembers].sort((left, right) =>
      left.group - right.group || left.name.localeCompare(right.name),
    );
    if (sorted.every((member, index) => member.text === sortableMembers[index].text)) continue;

    replacements.push({
      start: members[0].getFullStart(),
      end: members[members.length - 1].getEnd(),
      text: `\n${sorted.map((member) => member.text).join("\n\n")}`,
    });
  }

  if (replacements.length === 0) return { content, summaries: [] };

  let next = content;
  for (const replacement of replacements.sort((left, right) => right.start - left.start)) {
    next = `${next.slice(0, replacement.start)}${replacement.text}${next.slice(replacement.end)}`;
  }

  return {
    content: next,
    summaries: ["Sorted TypeScript class members"],
  };
};

function indentedMemberText(content: string, start: number, text: string) {
  const lineStart = content.lastIndexOf("\n", start) + 1;
  const indent = content.slice(lineStart, start).match(/^\s*/)?.[0] ?? "";
  return text
    .split("\n")
    .map((line, index) => (index === 0 ? `${indent}${line}` : line))
    .join("\n");
}
