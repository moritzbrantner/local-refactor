import { convertNestedIfToGuardClause } from "./convert-nested-if-to-guard-clause";
import { extractTypeDefinition } from "./extract-type-definition";
import { improveLocalName } from "./improve-local-name";
import { inlineTrivialHelper } from "./inline-trivial-helper";
import { normalizeImports } from "./normalize-imports";
import { simplifyBooleanReturnConditionals } from "./simplify-conditional";
import { sortIndependentDeclarations } from "./sort-independent-declarations";
import { sortTypeScriptClassMembers } from "./sort-typescript-class-members";
import type { DeterministicRule } from "./types";

export const deterministicRules: Record<string, DeterministicRule> = {
  "simplify-conditional": simplifyBooleanReturnConditionals,
  "convert-nested-if-to-guard-clause": convertNestedIfToGuardClause,
  "extract-type-definition": extractTypeDefinition,
  "inline-trivial-helper": inlineTrivialHelper,
  "normalize-imports": normalizeImports,
  "sort-independent-declarations": sortIndependentDeclarations,
  "sort-typescript-class-members": sortTypeScriptClassMembers,
  "improve-local-name": improveLocalName,
};
