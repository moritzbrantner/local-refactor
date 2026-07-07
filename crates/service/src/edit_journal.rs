use crate::{
    analyzer::AnalyzerEdit,
    db::{Database, PatchAction, PatchRecord},
    patch_plan::PatchPlanEdit,
};
use anyhow::{anyhow, Context, Result};
use local_refactor_core::path_policy::{PathDecision, PathPolicy};
use std::{io::ErrorKind, path::PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct JournaledEdit {
    pub file_path: PathBuf,
    pub original_content: String,
    pub new_content: String,
    pub rule_id: String,
    pub summary: String,
    pub action: PatchAction,
}

impl From<AnalyzerEdit> for JournaledEdit {
    fn from(edit: AnalyzerEdit) -> Self {
        Self {
            file_path: PathBuf::from(edit.file_path),
            original_content: edit.original_content,
            new_content: edit.new_content,
            rule_id: edit.rule_id,
            summary: edit.summary,
            action: PatchAction::Update,
        }
    }
}

impl From<PatchPlanEdit> for JournaledEdit {
    fn from(edit: PatchPlanEdit) -> Self {
        Self {
            file_path: PathBuf::from(edit.file_path),
            original_content: edit.original_content,
            new_content: edit.new_content,
            rule_id: edit.rule_id,
            summary: edit.summary,
            action: edit.action,
        }
    }
}

pub(crate) fn apply_edits(
    db: &Database,
    run_id: &str,
    policy: &PathPolicy,
    edits: Vec<JournaledEdit>,
) -> Result<usize> {
    let mut applied = 0;
    for edit in edits {
        if policy.decision_for(&edit.file_path) != PathDecision::Mutable {
            return Err(anyhow!(
                "attempted to edit non-mutable path {}",
                edit.file_path.display()
            ));
        }

        match edit.action {
            PatchAction::Update => {
                let current = std::fs::read_to_string(&edit.file_path)
                    .with_context(|| format!("failed to read {}", edit.file_path.display()))?;
                if current != edit.original_content {
                    return Err(anyhow!(
                        "external modification conflict while editing {}",
                        edit.file_path.display()
                    ));
                }
            }
            PatchAction::Create => {
                if edit.file_path.exists() {
                    return Err(anyhow!(
                        "create target already exists: {}",
                        edit.file_path.display()
                    ));
                }
            }
        }

        db.insert_patch(PatchRecord {
            run_id: run_id.to_string(),
            file_path: edit.file_path.to_string_lossy().to_string(),
            original_content: edit.original_content.clone(),
            new_content: edit.new_content.clone(),
            rule_id: edit.rule_id.clone(),
            summary: edit.summary.clone(),
            action: edit.action,
        })?;
        if let Some(parent) = edit.file_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::write(&edit.file_path, edit.new_content)
            .with_context(|| format!("failed to write {}", edit.file_path.display()))?;
        applied += 1;
    }

    Ok(applied)
}

pub(crate) fn revert_patches(db: &Database, run_id: &str) -> Result<usize> {
    let patches = db.patches_for_run(run_id)?;
    let count = patches.len();
    for patch in patches.into_iter().rev() {
        match patch.action {
            PatchAction::Update => {
                std::fs::write(&patch.file_path, patch.original_content)
                    .with_context(|| format!("failed to revert {}", patch.file_path))?;
            }
            PatchAction::Create => match std::fs::remove_file(&patch.file_path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to remove {}", patch.file_path));
                }
            },
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunCreateRequest;
    use local_refactor_core::config::TestFileMode;
    use std::path::Path;
    use tempfile::{tempdir, TempDir};

    struct Harness {
        root: TempDir,
        _db_dir: TempDir,
        db: Database,
        policy: PathPolicy,
    }

    impl Harness {
        fn new() -> Self {
            let root = tempdir().unwrap();
            let db_dir = tempdir().unwrap();
            let db = Database::open(db_dir.path().join("local-refactor.sqlite")).unwrap();
            insert_run(&db, root.path());
            let policy = PathPolicy::new(root.path(), &[], TestFileMode::Mutable).unwrap();
            Self {
                root,
                _db_dir: db_dir,
                db,
                policy,
            }
        }

        fn with_read_only_tests() -> Self {
            let root = tempdir().unwrap();
            let db_dir = tempdir().unwrap();
            let db = Database::open(db_dir.path().join("local-refactor.sqlite")).unwrap();
            insert_run(&db, root.path());
            let policy = PathPolicy::new(root.path(), &[], TestFileMode::ReadOnly).unwrap();
            Self {
                root,
                _db_dir: db_dir,
                db,
                policy,
            }
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.root.path().join(relative)
        }
    }

    fn insert_run(db: &Database, target_path: &Path) {
        let request = RunCreateRequest {
            target_path: Some(target_path.to_string_lossy().to_string()),
            repository_id: None,
            repository_root_path: None,
            target_relative_path: None,
            rules: vec!["simplify-conditional".to_string()],
            rule_selection_plan: None,
            model: Some("qwen2.5-coder:7b".to_string()),
            test_file_mode: Some(TestFileMode::Mutable),
            validation_commands: vec!["true".to_string()],
            protected_paths: Vec::new(),
            repair_budget: 2,
            expected_deterministic_preview_fingerprint: None,
            convention_snapshot: None,
            run_kind: crate::RunKind::Refactoring,
            source_coverage_run_id: None,
            coverage_evidence: Vec::new(),
            behavior_claims: Vec::new(),
        };
        db.insert_run("run-1", &request).unwrap();
    }

    fn update_edit(path: &Path, original: &str, updated: &str) -> JournaledEdit {
        JournaledEdit {
            file_path: path.to_path_buf(),
            original_content: original.to_string(),
            new_content: updated.to_string(),
            rule_id: "simplify-conditional".to_string(),
            summary: "Simplified conditional".to_string(),
            action: PatchAction::Update,
        }
    }

    fn create_edit(path: &Path, content: &str) -> JournaledEdit {
        JournaledEdit {
            file_path: path.to_path_buf(),
            original_content: String::new(),
            new_content: content.to_string(),
            rule_id: "split-file-by-responsibility".to_string(),
            summary: "Created file".to_string(),
            action: PatchAction::Create,
        }
    }

    #[test]
    fn applies_update_edits_and_records_patch() {
        let harness = Harness::new();
        let sample = harness.path("src/sample.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        std::fs::write(&sample, "old").unwrap();

        let count = apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![update_edit(&sample, "old", "new")],
        )
        .unwrap();

        assert_eq!(count, 1);
        assert_eq!(std::fs::read_to_string(&sample).unwrap(), "new");
        let patches = harness.db.patches_for_run("run-1").unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].original_content, "old");
        assert_eq!(patches[0].new_content, "new");
    }

    #[test]
    fn records_patch_before_filesystem_write_errors_surface() {
        let harness = Harness::new();
        let blocked_parent = harness.path("src");
        std::fs::write(&blocked_parent, "not a directory").unwrap();
        let target = blocked_parent.join("sample.ts");

        let error = apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![create_edit(&target, "new")],
        )
        .unwrap_err();

        assert!(error.to_string().contains("failed to create"));
        let patches = harness.db.patches_for_run("run-1").unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].file_path, target.to_string_lossy().as_ref());
    }

    #[test]
    fn rejects_non_mutable_paths() {
        let harness = Harness::with_read_only_tests();
        let sample = harness.path("src/sample.test.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        std::fs::write(&sample, "old").unwrap();

        let error = apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![update_edit(&sample, "old", "new")],
        )
        .unwrap_err();

        assert!(error.to_string().contains("non-mutable path"));
        assert!(harness.db.patches_for_run("run-1").unwrap().is_empty());
    }

    #[test]
    fn rejects_update_conflicts() {
        let harness = Harness::new();
        let sample = harness.path("src/sample.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        std::fs::write(&sample, "changed").unwrap();

        let error = apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![update_edit(&sample, "old", "new")],
        )
        .unwrap_err();

        assert!(error.to_string().contains("external modification conflict"));
        assert!(harness.db.patches_for_run("run-1").unwrap().is_empty());
    }

    #[test]
    fn rejects_existing_create_targets() {
        let harness = Harness::new();
        let sample = harness.path("src/sample.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        std::fs::write(&sample, "old").unwrap();

        let error = apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![create_edit(&sample, "new")],
        )
        .unwrap_err();

        assert!(error.to_string().contains("create target already exists"));
        assert_eq!(std::fs::read_to_string(&sample).unwrap(), "old");
        assert!(harness.db.patches_for_run("run-1").unwrap().is_empty());
    }

    #[test]
    fn creates_parent_directories_for_create_edits() {
        let harness = Harness::new();
        let sample = harness.path("src/nested/sample.ts");

        let count = apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![create_edit(&sample, "new")],
        )
        .unwrap();

        assert_eq!(count, 1);
        assert_eq!(std::fs::read_to_string(&sample).unwrap(), "new");
    }

    #[test]
    fn reverts_updates_in_reverse_patch_order() {
        let harness = Harness::new();
        let sample = harness.path("src/sample.ts");
        std::fs::create_dir_all(sample.parent().unwrap()).unwrap();
        std::fs::write(&sample, "first").unwrap();
        apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![update_edit(&sample, "first", "second")],
        )
        .unwrap();
        apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![update_edit(&sample, "second", "third")],
        )
        .unwrap();

        let count = revert_patches(&harness.db, "run-1").unwrap();

        assert_eq!(count, 2);
        assert_eq!(std::fs::read_to_string(&sample).unwrap(), "first");
    }

    #[test]
    fn removes_created_files_during_revert() {
        let harness = Harness::new();
        let sample = harness.path("src/sample.ts");
        apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![create_edit(&sample, "new")],
        )
        .unwrap();

        let count = revert_patches(&harness.db, "run-1").unwrap();

        assert_eq!(count, 1);
        assert!(!sample.exists());
    }

    #[test]
    fn ignores_missing_created_files_during_revert() {
        let harness = Harness::new();
        let sample = harness.path("src/sample.ts");
        apply_edits(
            &harness.db,
            "run-1",
            &harness.policy,
            vec![create_edit(&sample, "new")],
        )
        .unwrap();
        std::fs::remove_file(&sample).unwrap();

        let count = revert_patches(&harness.db, "run-1").unwrap();

        assert_eq!(count, 1);
        assert!(!sample.exists());
    }
}
