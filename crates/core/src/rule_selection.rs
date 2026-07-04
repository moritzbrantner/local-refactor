use crate::{
    config::{default_protected_paths, load_config_file, TestFileMode},
    path_policy::{is_rust_source, is_test_file, is_typescript_source},
    rules::{rule_by_id, Language},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuleSelectionPlan {
    pub target_relative_path: String,
    pub segments: Vec<RuleSelectionSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuleSelectionSegment {
    pub relative_path: String,
    pub rules: Vec<String>,
    pub reasons: Vec<RuleSelectionReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct RuleSelectionReason {
    pub rule_id: String,
    pub source: RuleSelectionReasonSource,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum RuleSelectionReasonSource {
    Config,
    Content,
    Fallback,
}

#[derive(Debug, Clone)]
pub struct RuleSelectionInput {
    pub repository_root: Option<PathBuf>,
    pub target_path: PathBuf,
    pub protected_paths: Vec<String>,
    pub test_file_mode: TestFileMode,
}

#[derive(Debug, Clone, Default)]
struct SegmentEvidence {
    files: usize,
    languages: BTreeSet<Language>,
    rules: BTreeMap<String, BTreeSet<RuleSelectionReason>>,
}

pub fn select_rules(input: RuleSelectionInput) -> Result<RuleSelectionPlan> {
    let target_path = std::fs::canonicalize(&input.target_path).with_context(|| {
        format!(
            "target path does not exist: {}",
            input.target_path.display()
        )
    })?;
    let target_root = if target_path.is_file() {
        target_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("target file has no parent directory"))?
            .to_path_buf()
    } else {
        target_path
    };
    let repository_root = input
        .repository_root
        .and_then(|root| std::fs::canonicalize(root).ok())
        .unwrap_or_else(|| infer_context_root(&target_root));
    let target_relative_path = relative_path(&repository_root, &target_root);

    let protected_paths = if input.protected_paths.is_empty() {
        default_protected_paths()
    } else {
        input.protected_paths
    };
    let protected = globset_for(&protected_paths)?;
    let inherited_config_rules = inherited_config_rules(&repository_root, &target_root)?;

    let mut segments = BTreeMap::<PathBuf, SegmentEvidence>::new();
    let target_segment = semantic_segment_root(&target_root, &target_root);
    add_rules(
        segments.entry(target_segment).or_default(),
        inherited_config_rules.iter().cloned(),
        RuleSelectionReasonSource::Config,
        "Inherited from applicable refactor-rules.toml",
    );

    for entry in WalkDir::new(&target_root)
        .into_iter()
        .filter_entry(should_visit_entry)
    {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_dir() {
            if path != target_root && path.join("refactor-rules.toml").exists() {
                let segment = semantic_segment_root(&target_root, path);
                let layer = load_config_file(&path.join("refactor-rules.toml"))?;
                add_rules(
                    segments.entry(segment).or_default(),
                    layer.rules.unwrap_or_default(),
                    RuleSelectionReasonSource::Config,
                    "Configured by refactor-rules.toml in this subtree",
                );
            }
            continue;
        }

        if !entry.file_type().is_file()
            || !is_mutable_candidate(&target_root, path, &protected, input.test_file_mode)
        {
            continue;
        }
        if !is_typescript_source(path) && !is_rust_source(path) {
            continue;
        }

        let segment_path =
            semantic_segment_root(&target_root, path.parent().unwrap_or(&target_root));
        let relative = relative_path(&target_root, path);
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let evidence = segments.entry(segment_path).or_default();
        evidence.files += 1;

        if is_typescript_source(path) {
            evidence.languages.insert(Language::TypeScript);
            collect_typescript_evidence(evidence, &relative, &content);
        }
        if is_rust_source(path) {
            evidence.languages.insert(Language::Rust);
            collect_rust_evidence(evidence, &relative, &content);
        }
    }

    for evidence in segments.values_mut() {
        apply_language_fallbacks(evidence);
        remove_rules_for_absent_languages(evidence);
    }

    let mut segments = segments
        .into_iter()
        .filter_map(|(path, evidence)| {
            if evidence.files == 0 && evidence.rules.is_empty() {
                return None;
            }
            let rules = evidence.rules.keys().cloned().collect::<Vec<_>>();
            if rules.is_empty() {
                return None;
            }
            let reasons = evidence
                .rules
                .into_values()
                .flat_map(|reasons| reasons.into_iter())
                .collect::<Vec<_>>();
            Some(RuleSelectionSegment {
                relative_path: relative_path(&repository_root, &path),
                rules,
                reasons,
            })
        })
        .collect::<Vec<_>>();

    collapse_identical_segments(&mut segments);
    Ok(RuleSelectionPlan {
        target_relative_path,
        segments,
    })
}

fn inherited_config_rules(repository_root: &Path, target_root: &Path) -> Result<Vec<String>> {
    let mut rules = BTreeSet::new();
    let mut current = Some(target_root);
    let mut dirs = Vec::new();
    while let Some(dir) = current {
        dirs.push(dir.to_path_buf());
        if dir == repository_root {
            break;
        }
        current = dir.parent();
    }
    dirs.reverse();

    for dir in dirs {
        let config = dir.join("refactor-rules.toml");
        if !config.exists() {
            continue;
        }
        let layer = load_config_file(&config)?;
        rules.extend(layer.rules.unwrap_or_default());
    }
    Ok(rules.into_iter().collect())
}

fn add_rules(
    evidence: &mut SegmentEvidence,
    rules: impl IntoIterator<Item = String>,
    source: RuleSelectionReasonSource,
    message: &str,
) {
    for rule_id in rules {
        if rule_by_id(&rule_id).is_none() {
            continue;
        }
        evidence
            .rules
            .entry(rule_id.clone())
            .or_default()
            .insert(RuleSelectionReason {
                rule_id,
                source,
                message: message.to_string(),
            });
    }
}

fn collect_typescript_evidence(evidence: &mut SegmentEvidence, relative: &str, content: &str) {
    if max_brace_depth_for_if(content) >= 3 {
        add_rule_reason(
            evidence,
            "convert-nested-if-to-guard-clause",
            RuleSelectionReasonSource::Content,
            &format!("{relative} contains deeply nested conditional logic"),
        );
    }
    if content.contains("export function") && content.contains(": {") {
        add_rule_reason(
            evidence,
            "extract-type-definition",
            RuleSelectionReasonSource::Content,
            &format!("{relative} has exported APIs with inline object types"),
        );
    }
    if max_function_lines(content) >= 60
        || (nonblank_lines(content) >= 300 && function_like_count(content) >= 3)
    {
        add_rule_reason(
            evidence,
            "split-oversized-function",
            RuleSelectionReasonSource::Content,
            &format!("{relative} contains oversized function-like code"),
        );
    }
    if has_duplicate_window(content, 6) {
        add_rule_reason(
            evidence,
            "extract-duplicate-block",
            RuleSelectionReasonSource::Content,
            &format!("{relative} contains repeated local blocks"),
        );
    }
    if max_function_lines(content) >= 80 && !contains_obvious_io(content) {
        add_rule_reason(
            evidence,
            "isolate-side-effect-free-helper",
            RuleSelectionReasonSource::Content,
            &format!("{relative} contains long computation-oriented helper logic"),
        );
    }
}

fn collect_rust_evidence(evidence: &mut SegmentEvidence, relative: &str, content: &str) {
    if max_function_lines(content) >= 80 {
        add_rule_reason(
            evidence,
            "rust-extract-helper-function",
            RuleSelectionReasonSource::Content,
            &format!("{relative} contains an oversized Rust function"),
        );
    }
}

fn add_rule_reason(
    evidence: &mut SegmentEvidence,
    rule_id: &str,
    source: RuleSelectionReasonSource,
    message: &str,
) {
    evidence
        .rules
        .entry(rule_id.to_string())
        .or_default()
        .insert(RuleSelectionReason {
            rule_id: rule_id.to_string(),
            source,
            message: message.to_string(),
        });
}

fn apply_language_fallbacks(evidence: &mut SegmentEvidence) {
    if evidence.languages.contains(&Language::TypeScript) {
        add_rule_reason(
            evidence,
            "simplify-conditional",
            RuleSelectionReasonSource::Fallback,
            "TypeScript source files are present in this segment",
        );
        add_rule_reason(
            evidence,
            "normalize-imports",
            RuleSelectionReasonSource::Fallback,
            "TypeScript source files are present in this segment",
        );
    }
}

fn remove_rules_for_absent_languages(evidence: &mut SegmentEvidence) {
    let languages = evidence.languages.clone();
    evidence.rules.retain(|rule_id, _| {
        let Some(rule) = rule_by_id(rule_id) else {
            return false;
        };
        languages.is_empty() || languages.contains(&rule.language)
    });
}

fn semantic_segment_root(target_root: &Path, path: &Path) -> PathBuf {
    let mut current = if path.is_file() {
        path.parent().unwrap_or(target_root)
    } else {
        path
    };
    let mut best = target_root.to_path_buf();
    while current.starts_with(target_root) {
        if current != target_root && has_segment_marker(current) {
            best = current.to_path_buf();
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent;
    }
    best
}

fn has_segment_marker(path: &Path) -> bool {
    path.join("refactor-rules.toml").exists()
        || path.join("package.json").exists()
        || path.join("Cargo.toml").exists()
        || path.join("tsconfig.json").exists()
        || path.join("vite.config.ts").exists()
        || path.join("next.config.js").exists()
}

fn collapse_identical_segments(segments: &mut Vec<RuleSelectionSegment>) {
    segments.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut collapsed = Vec::<RuleSelectionSegment>::new();
    for segment in segments.drain(..) {
        if let Some(parent) = collapsed.iter().position(|existing| {
            segment
                .relative_path
                .starts_with(&(existing.relative_path.clone() + "/"))
                && existing.rules == segment.rules
                && reason_sources(&existing.reasons) == reason_sources(&segment.reasons)
        }) {
            collapsed[parent].reasons.extend(segment.reasons);
            collapsed[parent].reasons.sort();
            collapsed[parent].reasons.dedup();
        } else {
            collapsed.push(segment);
        }
    }
    *segments = collapsed;
}

fn reason_sources(reasons: &[RuleSelectionReason]) -> Vec<(&str, RuleSelectionReasonSource)> {
    reasons
        .iter()
        .map(|reason| (reason.rule_id.as_str(), reason.source))
        .collect()
}

fn infer_context_root(target_root: &Path) -> PathBuf {
    let mut current = target_root;
    while let Some(parent) = current.parent() {
        if current.join(".git").exists()
            || current.join("Cargo.toml").exists()
            || current.join("package.json").exists()
        {
            return current.to_path_buf();
        }
        current = parent;
    }
    target_root.to_path_buf()
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

fn is_mutable_candidate(
    target_root: &Path,
    path: &Path,
    protected: &globset::GlobSet,
    test_file_mode: TestFileMode,
) -> bool {
    if test_file_mode == TestFileMode::ReadOnly && is_test_file(path) {
        return false;
    }
    let relative = path.strip_prefix(target_root).unwrap_or(path);
    !protected.is_match(relative)
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

fn should_visit_entry(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return true;
    }
    !matches!(
        entry.path().file_name().and_then(|value| value.to_str()),
        Some("node_modules" | "dist" | "build" | ".next" | "coverage" | ".git" | "target")
    )
}

fn max_brace_depth_for_if(content: &str) -> usize {
    let mut depth = 0usize;
    let mut max_if_depth = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("if ") || trimmed.starts_with("if(") || trimmed.contains(" if (") {
            max_if_depth = max_if_depth.max(depth + 1);
        }
        depth += line.matches('{').count();
        depth = depth.saturating_sub(line.matches('}').count());
    }
    max_if_depth
}

fn max_function_lines(content: &str) -> usize {
    let mut max_lines = 0usize;
    let mut in_function = false;
    let mut depth = 0isize;
    let mut lines = 0usize;
    for line in content.lines() {
        let starts_function = line.contains("function ")
            || line.trim_start().starts_with("fn ")
            || line.contains("=>");
        if !in_function && starts_function && line.contains('{') {
            in_function = true;
            depth = 0;
            lines = 0;
        }
        if in_function {
            lines += usize::from(!line.trim().is_empty());
            depth += line.matches('{').count() as isize;
            depth -= line.matches('}').count() as isize;
            if depth <= 0 {
                max_lines = max_lines.max(lines);
                in_function = false;
            }
        }
    }
    max_lines
}

fn nonblank_lines(content: &str) -> usize {
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

fn function_like_count(content: &str) -> usize {
    content.matches("function ").count()
        + content.matches("=>").count()
        + content.matches("\nfn ").count()
}

fn has_duplicate_window(content: &str, size: usize) -> bool {
    let lines = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect::<Vec<_>>();
    if lines.len() < size * 2 {
        return false;
    }
    let mut windows = BTreeSet::new();
    for window in lines.windows(size) {
        let normalized = window
            .iter()
            .map(|line| line.replace(char::is_numeric, "0"))
            .collect::<Vec<_>>()
            .join("\n");
        if !windows.insert(normalized) {
            return true;
        }
    }
    false
}

fn contains_obvious_io(content: &str) -> bool {
    [
        "fetch(",
        "readFile",
        "writeFile",
        "logger.",
        "console.",
        "save",
        "request(",
    ]
    .iter()
    .any(|needle| content.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn selects_typescript_fallback_rules() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("src/app.ts"),
            "export const value = true;\n",
        );

        let plan = select_rules(RuleSelectionInput {
            repository_root: Some(dir.path().to_path_buf()),
            target_path: dir.path().join("src"),
            protected_paths: Vec::new(),
            test_file_mode: TestFileMode::ReadOnly,
        })
        .unwrap();

        assert_eq!(
            plan.segments[0].rules,
            vec!["normalize-imports", "simplify-conditional"]
        );
    }

    #[test]
    fn does_not_select_rust_fallback_rules() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("src/lib.rs"),
            "pub fn value() -> bool { true }\n",
        );

        let plan = select_rules(RuleSelectionInput {
            repository_root: Some(dir.path().to_path_buf()),
            target_path: dir.path().join("src"),
            protected_paths: Vec::new(),
            test_file_mode: TestFileMode::ReadOnly,
        })
        .unwrap();

        assert!(plan.segments.is_empty());
    }

    #[test]
    fn inherits_config_and_adds_content_evidence() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("refactor-rules.toml"),
            "rules = [\"add-documentation-comments\"]\n",
        );
        write(
            &dir.path().join("src/report.ts"),
            "export function render(input: { title: string }) {\n  return input.title;\n}\n",
        );

        let plan = select_rules(RuleSelectionInput {
            repository_root: Some(dir.path().to_path_buf()),
            target_path: dir.path().join("src"),
            protected_paths: Vec::new(),
            test_file_mode: TestFileMode::ReadOnly,
        })
        .unwrap();

        assert!(plan.segments[0]
            .rules
            .contains(&"add-documentation-comments".to_string()));
        assert!(plan.segments[0]
            .rules
            .contains(&"extract-type-definition".to_string()));
    }

    #[test]
    fn excludes_protected_and_test_files() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("src/app.test.ts"),
            "export const value = true;\n",
        );
        write(
            &dir.path().join("src/generated/app.ts"),
            "export const value = true;\n",
        );

        let plan = select_rules(RuleSelectionInput {
            repository_root: Some(dir.path().to_path_buf()),
            target_path: dir.path().join("src"),
            protected_paths: vec!["generated/**".to_string()],
            test_file_mode: TestFileMode::ReadOnly,
        })
        .unwrap();

        assert!(plan.segments.is_empty());
    }
}
