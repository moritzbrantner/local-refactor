use crate::config::TestFileMode;
use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PathDecision {
    Mutable,
    ReadOnly,
    Protected,
}

#[derive(Debug, Clone)]
pub struct PathPolicy {
    target_root: PathBuf,
    protected: GlobSet,
    test_file_mode: TestFileMode,
}

impl PathPolicy {
    pub fn new(
        target_root: impl Into<PathBuf>,
        protected_globs: &[String],
        test_file_mode: TestFileMode,
    ) -> Result<Self> {
        let target_root = target_root.into();
        let mut builder = GlobSetBuilder::new();
        for pattern in protected_globs {
            builder.add(
                Glob::new(pattern)
                    .with_context(|| format!("invalid protected path glob `{pattern}`"))?,
            );
        }

        Ok(Self {
            target_root,
            protected: builder.build()?,
            test_file_mode,
        })
    }

    pub fn target_root(&self) -> &Path {
        &self.target_root
    }

    pub fn decision_for(&self, path: &Path) -> PathDecision {
        if !path.starts_with(&self.target_root) {
            return PathDecision::ReadOnly;
        }

        let relative = path.strip_prefix(&self.target_root).unwrap_or(path);
        if self.protected.is_match(relative) {
            return PathDecision::Protected;
        }

        if self.test_file_mode == TestFileMode::ReadOnly && is_test_file(path) {
            return PathDecision::ReadOnly;
        }

        PathDecision::Mutable
    }

    pub fn mutable_source_files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in WalkDir::new(&self.target_root)
            .into_iter()
            .filter_entry(|entry| !is_skipped_dir(entry.path()))
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            if !is_typescript_source(path) {
                continue;
            }

            if self.decision_for(path) == PathDecision::Mutable {
                files.push(path.to_path_buf());
            }
        }
        files.sort();
        Ok(files)
    }
}

pub fn is_test_file(path: &Path) -> bool {
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

pub fn is_typescript_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("ts" | "tsx" | "js" | "jsx")
    )
}

fn is_skipped_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|value| value.to_str()),
        Some("node_modules" | "dist" | "build" | ".next" | "coverage" | ".git")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_files_are_read_only_by_default() {
        let policy = PathPolicy::new(
            PathBuf::from("/repo/src"),
            &["generated/**".to_string()],
            TestFileMode::ReadOnly,
        )
        .unwrap();

        assert_eq!(
            policy.decision_for(Path::new("/repo/src/foo.test.ts")),
            PathDecision::ReadOnly
        );
    }

    #[test]
    fn protected_paths_win_over_mutable_scope() {
        let policy = PathPolicy::new(
            PathBuf::from("/repo/src"),
            &["generated/**".to_string()],
            TestFileMode::Mutable,
        )
        .unwrap();

        assert_eq!(
            policy.decision_for(Path::new("/repo/src/generated/client.ts")),
            PathDecision::Protected
        );
    }

    #[test]
    fn sibling_paths_are_read_only() {
        let policy =
            PathPolicy::new(PathBuf::from("/repo/src"), &[], TestFileMode::Mutable).unwrap();

        assert_eq!(
            policy.decision_for(Path::new("/repo/other/file.ts")),
            PathDecision::ReadOnly
        );
    }
}
