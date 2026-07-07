use crate::rules::Language;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

pub const DEFAULT_COVERAGE_EVIDENCE_RULES: &[&str] = &[
    "public-entrypoint-without-nearby-test",
    "branch-or-error-path-needs-characterization",
    "boundary-input-needs-characterization",
];

pub const DEFAULT_COVERAGE_SOLIDIFICATION_RULES: &[&str] = &[
    "characterize-public-entrypoint",
    "characterize-branch-and-error-behavior",
    "characterize-boundary-inputs",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoverageEvidenceItem {
    pub id: String,
    pub rule_id: String,
    pub language: Language,
    pub source_path: String,
    pub public_entrypoint: String,
    pub owning_test_layer: String,
    pub nearby_test_paths: Vec<String>,
    pub gap_reason: String,
    pub suggested_solidification_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorClaim {
    pub id: String,
    pub evidence_id: String,
    pub source_paths: Vec<String>,
    pub public_entrypoint: String,
    pub behavior: String,
    pub owning_test_layer: String,
    pub test_path: String,
    pub assertion_summary: String,
    pub existing_coverage_reason: String,
}

pub fn normalize_evidence_rules(rules: &[String]) -> Vec<String> {
    if rules.is_empty() {
        return DEFAULT_COVERAGE_EVIDENCE_RULES
            .iter()
            .map(|rule| (*rule).to_string())
            .collect();
    }

    rules
        .iter()
        .filter(|rule| is_known_evidence_rule(rule))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn is_known_evidence_rule(rule_id: &str) -> bool {
    DEFAULT_COVERAGE_EVIDENCE_RULES.contains(&rule_id)
}

pub fn is_known_solidification_rule(rule_id: &str) -> bool {
    DEFAULT_COVERAGE_SOLIDIFICATION_RULES.contains(&rule_id)
}

pub fn collect_evidence(
    target_root: &Path,
    display_root: &Path,
    evidence_rules: &[String],
    protected_paths: &[String],
) -> Result<Vec<CoverageEvidenceItem>> {
    let protected = globset_for(protected_paths)?;
    let normalized_rules = normalize_evidence_rules(evidence_rules);
    let test_paths = collect_test_paths(target_root, display_root)?;
    let mut evidence = Vec::new();

    for entry in WalkDir::new(target_root)
        .into_iter()
        .filter_entry(|entry| !is_skipped_dir(entry.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let relative_to_target = path.strip_prefix(target_root).unwrap_or(path);
        if protected.is_match(relative_to_target) || is_test_file(path) {
            continue;
        }
        let Some(language) = language_for_path(path) else {
            continue;
        };
        let relative = relative_path(display_root, path);
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let entrypoints = public_entrypoints(language, &content);
        for entrypoint in entrypoints {
            let nearby_tests = nearby_tests_for(&relative, &entrypoint, &test_paths);
            if normalized_rules
                .iter()
                .any(|rule| rule == "public-entrypoint-without-nearby-test")
                && nearby_tests.is_empty()
            {
                evidence.push(evidence_item(
                    "public-entrypoint-without-nearby-test",
                    language,
                    &relative,
                    &entrypoint,
                    Vec::new(),
                    format!("No nearby test appears to cover {entrypoint}."),
                    vec!["characterize-public-entrypoint"],
                ));
            }
            if normalized_rules
                .iter()
                .any(|rule| rule == "branch-or-error-path-needs-characterization")
                && branch_or_error_surface(language, &content)
            {
                evidence.push(evidence_item(
                    "branch-or-error-path-needs-characterization",
                    language,
                    &relative,
                    &entrypoint,
                    nearby_tests.clone(),
                    format!("{entrypoint} contains visible branch or error behavior."),
                    vec!["characterize-branch-and-error-behavior"],
                ));
            }
            if normalized_rules
                .iter()
                .any(|rule| rule == "boundary-input-needs-characterization")
                && boundary_surface(language, &entrypoint, &content)
            {
                evidence.push(evidence_item(
                    "boundary-input-needs-characterization",
                    language,
                    &relative,
                    &entrypoint,
                    nearby_tests,
                    format!(
                        "{entrypoint} appears to parse, validate, normalize, or transform inputs."
                    ),
                    vec!["characterize-boundary-inputs"],
                ));
            }
        }
    }

    evidence.sort_by(|left, right| left.id.cmp(&right.id));
    evidence.dedup_by(|left, right| left.id == right.id);
    Ok(evidence)
}

fn evidence_item(
    rule_id: &str,
    language: Language,
    source_path: &str,
    entrypoint: &str,
    nearby_test_paths: Vec<String>,
    gap_reason: String,
    suggested_rules: Vec<&str>,
) -> CoverageEvidenceItem {
    CoverageEvidenceItem {
        id: format!("{rule_id}:{source_path}:{entrypoint}"),
        rule_id: rule_id.to_string(),
        language,
        source_path: source_path.to_string(),
        public_entrypoint: entrypoint.to_string(),
        owning_test_layer: owning_test_layer(language).to_string(),
        nearby_test_paths,
        gap_reason,
        suggested_solidification_rules: suggested_rules.into_iter().map(str::to_string).collect(),
    }
}

fn public_entrypoints(language: Language, content: &str) -> Vec<String> {
    let mut entrypoints = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        match language {
            Language::TypeScript => {
                if let Some(name) = trimmed.strip_prefix("export function ") {
                    push_identifier(&mut entrypoints, name);
                } else if let Some(name) = trimmed.strip_prefix("export class ") {
                    push_identifier(&mut entrypoints, name);
                } else if let Some(name) = trimmed.strip_prefix("export const ") {
                    if trimmed.contains("=>") || trimmed.contains("function") {
                        push_identifier(&mut entrypoints, name);
                    }
                }
            }
            Language::Rust => {
                if let Some(name) = trimmed.strip_prefix("pub fn ") {
                    push_identifier(&mut entrypoints, name);
                } else if let Some(name) = trimmed.strip_prefix("pub struct ") {
                    push_identifier(&mut entrypoints, name);
                }
            }
        }
    }
    entrypoints.sort();
    entrypoints.dedup();
    entrypoints
}

fn push_identifier(entrypoints: &mut Vec<String>, rest: &str) {
    let name = rest
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .collect::<String>();
    if !name.is_empty() {
        entrypoints.push(name);
    }
}

fn branch_or_error_surface(language: Language, content: &str) -> bool {
    match language {
        Language::TypeScript => ["if ", "if(", "switch", "throw ", "catch", "return"]
            .iter()
            .any(|marker| content.contains(marker)),
        Language::Rust => ["match ", "if ", "?", "Result", "Option", "Err("]
            .iter()
            .any(|marker| content.contains(marker)),
    }
}

fn boundary_surface(language: Language, entrypoint: &str, content: &str) -> bool {
    let lower_entrypoint = entrypoint.to_ascii_lowercase();
    let lower_content = content.to_ascii_lowercase();
    let name_markers = [
        "parse",
        "validate",
        "normalize",
        "filter",
        "sort",
        "transform",
        "serialize",
        "deserialize",
        "convert",
    ];
    if name_markers
        .iter()
        .any(|marker| lower_entrypoint.contains(marker))
    {
        return true;
    }

    match language {
        Language::TypeScript => [".map(", ".filter(", ".reduce(", ".sort(", "json.parse"]
            .iter()
            .any(|marker| lower_content.contains(marker)),
        Language::Rust => [
            ".map(", ".filter(", ".collect", "from_str", "serde", "parse::<",
        ]
        .iter()
        .any(|marker| lower_content.contains(marker)),
    }
}

fn collect_test_paths(target_root: &Path, display_root: &Path) -> Result<Vec<String>> {
    let mut tests = Vec::new();
    for entry in WalkDir::new(target_root)
        .into_iter()
        .filter_entry(|entry| !is_skipped_dir(entry.path()))
    {
        let entry = entry?;
        if entry.file_type().is_file() && is_test_file(entry.path()) {
            tests.push(relative_path(display_root, entry.path()));
        }
    }
    tests.sort();
    Ok(tests)
}

fn nearby_tests_for(source_path: &str, entrypoint: &str, test_paths: &[String]) -> Vec<String> {
    let source_stem = source_path
        .rsplit('/')
        .next()
        .and_then(|name| name.split('.').next())
        .unwrap_or(source_path);
    let source_dir = source_path
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");
    let entrypoint_lower = entrypoint.to_ascii_lowercase();

    test_paths
        .iter()
        .filter(|path| {
            let lower = path.to_ascii_lowercase();
            lower.contains(&source_stem.to_ascii_lowercase())
                || lower.contains(&entrypoint_lower)
                || (!source_dir.is_empty() && path.starts_with(source_dir))
        })
        .cloned()
        .collect()
}

fn owning_test_layer(language: Language) -> &'static str {
    match language {
        Language::TypeScript => "TypeScript unit test",
        Language::Rust => "Rust integration test",
    }
}

fn language_for_path(path: &Path) -> Option<Language> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("ts" | "tsx" | "js" | "jsx") => Some(Language::TypeScript),
        Some("rs") => Some(Language::Rust),
        _ => None,
    }
}

fn is_test_file(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    name.contains(".test.")
        || name.contains(".spec.")
        || normalized.contains("/__tests__/")
        || normalized.contains("/test/")
        || normalized.contains("/tests/")
}

fn is_skipped_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|value| value.to_str()),
        Some("node_modules" | "dist" | "build" | ".next" | "coverage" | ".git" | "target")
    )
}

fn globset_for(patterns: &[String]) -> Result<globset::GlobSet> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            globset::Glob::new(pattern)
                .with_context(|| format!("invalid protected path glob `{pattern}`"))?,
        );
    }
    Ok(builder.build()?)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

pub fn canonical_target_root(target_path: &Path) -> Result<PathBuf> {
    let target_path = std::fs::canonicalize(target_path)
        .with_context(|| format!("target path does not exist: {}", target_path.display()))?;
    if target_path.is_file() {
        return target_path
            .parent()
            .map(Path::to_path_buf)
            .with_context(|| "target file has no parent directory");
    }
    Ok(target_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_typescript_entrypoint_without_nearby_test_emits_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src/calculator.ts");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(
            &source,
            "export function calculateTotal(items: number[]) {\n  return items.reduce((total, item) => total + item, 0);\n}\n",
        )
        .unwrap();

        let evidence = collect_evidence(
            dir.path(),
            dir.path(),
            &["public-entrypoint-without-nearby-test".to_string()],
            &[],
        )
        .unwrap();

        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].public_entrypoint, "calculateTotal");
        assert_eq!(evidence[0].source_path, "src/calculator.ts");
    }

    #[test]
    fn rust_public_function_gets_rust_test_layer() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src/lib.rs");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(
            &source,
            "pub fn parse_total(input: &str) -> Option<u32> { input.parse().ok() }\n",
        )
        .unwrap();

        let evidence = collect_evidence(
            dir.path(),
            dir.path(),
            &["boundary-input-needs-characterization".to_string()],
            &[],
        )
        .unwrap();

        assert_eq!(evidence[0].owning_test_layer, "Rust integration test");
        assert_eq!(evidence[0].public_entrypoint, "parse_total");
    }
}
