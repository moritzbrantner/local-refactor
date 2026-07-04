use crate::RunCreateRequest;
use anyhow::{anyhow, Result};
use chrono::Utc;
use local_refactor_core::rule_selection::RuleSelectionPlan;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

pub struct Database {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryRecord {
    pub id: String,
    pub label: String,
    pub root_path: String,
    pub available: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub target_path: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub rules: Vec<String>,
    pub rule_selection_plan: Option<RuleSelectionPlan>,
    pub model: Option<String>,
    pub test_file_mode: String,
    pub validation_commands: Vec<String>,
    pub protected_paths: Vec<String>,
    pub validation_output: Option<String>,
    pub error: Option<String>,
    pub repository_id: Option<String>,
    pub repository_root_path: Option<String>,
    pub target_relative_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMetrics {
    pub total_run_ms: Option<u64>,
    pub model_ensure_available_ms: Option<u64>,
    pub file_collection_ms: Option<u64>,
    pub analyzer_planning_ms: Option<u64>,
    pub model_planning_ms: Option<u64>,
    pub patch_plan_validation_ms: Option<u64>,
    pub edit_application_ms: Option<u64>,
    pub validation_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct PatchRecord {
    pub run_id: String,
    pub file_path: String,
    pub original_content: String,
    pub new_content: String,
    pub rule_id: String,
    pub summary: String,
    pub action: PatchAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchAction {
    Update,
    Create,
}

impl PatchAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Update => "update",
            Self::Create => "create",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "create" => Self::Create,
            _ => Self::Update,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEvent {
    pub id: i64,
    pub run_id: String,
    pub timestamp: String,
    pub message: String,
}

impl Database {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;

            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                target_path TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                rules_json TEXT NOT NULL,
                test_file_mode TEXT NOT NULL,
                validation_json TEXT NOT NULL,
                protected_json TEXT NOT NULL,
                validation_output TEXT,
                error TEXT
            );

            CREATE TABLE IF NOT EXISTS repositories (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL,
                root_path TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS patches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                original_content TEXT NOT NULL,
                new_content TEXT NOT NULL,
                rule_id TEXT NOT NULL,
                summary TEXT NOT NULL,
                action TEXT NOT NULL DEFAULT 'update',
                FOREIGN KEY(run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                message TEXT NOT NULL,
                FOREIGN KEY(run_id) REFERENCES runs(id)
            );
            "#,
        )?;
        ensure_column(&conn, "runs", "repository_id", "TEXT")?;
        ensure_column(&conn, "runs", "repository_root_path", "TEXT")?;
        ensure_column(&conn, "runs", "target_relative_path", "TEXT")?;
        ensure_column(&conn, "runs", "model", "TEXT")?;
        ensure_column(&conn, "runs", "metrics_json", "TEXT")?;
        ensure_column(&conn, "runs", "rule_selection_plan_json", "TEXT")?;
        ensure_column(&conn, "patches", "action", "TEXT NOT NULL DEFAULT 'update'")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_repository(
        &self,
        id: &str,
        label: &str,
        root_path: &str,
    ) -> Result<RepositoryRecord> {
        if let Some(existing) = self.repository_by_root(root_path)? {
            return Ok(existing);
        }

        let now = Utc::now().to_rfc3339();
        self.conn()?.execute(
            r#"
            INSERT INTO repositories (id, label, root_path, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?4)
            "#,
            params![id, label, root_path, now],
        )?;
        self.get_repository(id)?
            .ok_or_else(|| anyhow!("repository was not found after insert"))
    }

    pub fn list_repositories(&self) -> Result<Vec<RepositoryRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, label, root_path, created_at, updated_at
            FROM repositories
            ORDER BY updated_at DESC, label ASC
            "#,
        )?;
        let rows = stmt.query_map([], row_to_repository)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_repository(&self, id: &str) -> Result<Option<RepositoryRecord>> {
        self.conn()?
            .query_row(
                r#"
                SELECT id, label, root_path, created_at, updated_at
                FROM repositories
                WHERE id = ?1
                "#,
                params![id],
                row_to_repository,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn update_repository_label(
        &self,
        id: &str,
        label: &str,
    ) -> Result<Option<RepositoryRecord>> {
        let updated = self.conn()?.execute(
            "UPDATE repositories SET label = ?1, updated_at = ?2 WHERE id = ?3",
            params![label, Utc::now().to_rfc3339(), id],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        self.get_repository(id)
    }

    pub fn delete_repository(&self, id: &str) -> Result<bool> {
        let deleted = self
            .conn()?
            .execute("DELETE FROM repositories WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    fn repository_by_root(&self, root_path: &str) -> Result<Option<RepositoryRecord>> {
        self.conn()?
            .query_row(
                r#"
                SELECT id, label, root_path, created_at, updated_at
                FROM repositories
                WHERE root_path = ?1
                "#,
                params![root_path],
                row_to_repository,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_run(&self, id: &str, request: &RunCreateRequest) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let target_path = request
            .target_path
            .as_deref()
            .ok_or_else(|| anyhow!("run target path was not resolved"))?;
        let conn = self.conn()?;
        conn.execute(
            r#"
            INSERT INTO runs (
                id, target_path, status, created_at, updated_at, rules_json,
                test_file_mode, validation_json, protected_json, repository_id,
                repository_root_path, target_relative_path, model, rule_selection_plan_json
            ) VALUES (?1, ?2, 'queued', ?3, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                id,
                target_path,
                now,
                serde_json::to_string(&request.rules)?,
                serde_json::to_string(&request.test_file_mode.unwrap_or_default())?,
                serde_json::to_string(&request.validation_commands)?,
                serde_json::to_string(&request.protected_paths)?,
                request.repository_id,
                request.repository_root_path,
                request.target_relative_path,
                request.model,
                request
                    .rule_selection_plan
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
            ],
        )?;
        drop(conn);
        self.append_event(id, "Run queued")?;
        Ok(())
    }

    pub fn update_status(&self, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn()?.execute(
            "UPDATE runs SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, id],
        )?;
        Ok(())
    }

    pub fn set_validation_output(&self, id: &str, output: &str) -> Result<()> {
        self.conn()?.execute(
            "UPDATE runs SET validation_output = ?1, updated_at = ?2 WHERE id = ?3",
            params![output, Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn set_error(&self, id: &str, error: &str) -> Result<()> {
        self.conn()?.execute(
            "UPDATE runs SET error = ?1, updated_at = ?2 WHERE id = ?3",
            params![error, Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn set_run_metrics(&self, id: &str, metrics: &RunMetrics) -> Result<()> {
        self.conn()?.execute(
            "UPDATE runs SET metrics_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![serde_json::to_string(metrics)?, Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn metrics_for_run(&self, id: &str) -> Result<Option<RunMetrics>> {
        let metrics_json: Option<String> = self
            .conn()?
            .query_row(
                "SELECT metrics_json FROM runs WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        metrics_json
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }

    pub fn list_runs(&self, repository_id: Option<&str>) -> Result<Vec<RunRecord>> {
        let conn = self.conn()?;
        if let Some(repository_id) = repository_id {
            let mut stmt = conn.prepare(
                r#"
                SELECT id, target_path, status, created_at, updated_at, rules_json,
                       test_file_mode, validation_json, protected_json, validation_output, error,
                       repository_id, repository_root_path, target_relative_path, model,
                       rule_selection_plan_json
                FROM runs
                WHERE repository_id = ?1
                ORDER BY created_at DESC
                LIMIT 100
                "#,
            )?;
            let rows = stmt.query_map(params![repository_id], row_to_run)?;
            return rows
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into);
        }

        let mut stmt = conn.prepare(
            r#"
            SELECT id, target_path, status, created_at, updated_at, rules_json,
                   test_file_mode, validation_json, protected_json, validation_output, error,
                   repository_id, repository_root_path, target_relative_path, model,
                   rule_selection_plan_json
            FROM runs
            ORDER BY created_at DESC
            LIMIT 100
            "#,
        )?;
        let rows = stmt.query_map([], row_to_run)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_run(&self, id: &str) -> Result<Option<RunRecord>> {
        self.conn()?
            .query_row(
                r#"
                SELECT id, target_path, status, created_at, updated_at, rules_json,
                       test_file_mode, validation_json, protected_json, validation_output, error,
                       repository_id, repository_root_path, target_relative_path, model,
                       rule_selection_plan_json
                FROM runs
                WHERE id = ?1
                "#,
                params![id],
                row_to_run,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_patch(&self, patch: PatchRecord) -> Result<()> {
        self.conn()?.execute(
            r#"
            INSERT INTO patches (
                run_id, file_path, original_content, new_content, rule_id, summary, action
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                patch.run_id,
                patch.file_path,
                patch.original_content,
                patch.new_content,
                patch.rule_id,
                patch.summary,
                patch.action.as_str()
            ],
        )?;
        Ok(())
    }

    pub fn patches_for_run(&self, run_id: &str) -> Result<Vec<PatchRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT run_id, file_path, original_content, new_content, rule_id, summary, action
            FROM patches
            WHERE run_id = ?1
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![run_id], |row| {
            Ok(PatchRecord {
                run_id: row.get(0)?,
                file_path: row.get(1)?,
                original_content: row.get(2)?,
                new_content: row.get(3)?,
                rule_id: row.get(4)?,
                summary: row.get(5)?,
                action: PatchAction::from_str(row.get::<_, String>(6)?.as_str()),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn append_event(&self, run_id: &str, message: &str) -> Result<RunEvent> {
        let timestamp = Utc::now().to_rfc3339();
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO events (run_id, timestamp, message) VALUES (?1, ?2, ?3)",
            params![run_id, timestamp, message],
        )?;
        let id = conn.last_insert_rowid();
        Ok(RunEvent {
            id,
            run_id: run_id.to_string(),
            timestamp,
            message: message.to_string(),
        })
    }

    pub fn events_for_run(&self, run_id: &str) -> Result<Vec<RunEvent>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, run_id, timestamp, message FROM events WHERE run_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![run_id], |row| {
            Ok(RunEvent {
                id: row.get(0)?,
                run_id: row.get(1)?,
                timestamp: row.get(2)?,
                message: row.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("database lock poisoned"))
    }
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }

    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}

fn row_to_repository(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryRecord> {
    let root_path: String = row.get(2)?;
    let available = PathBuf::from(&root_path).is_dir();

    Ok(RepositoryRecord {
        id: row.get(0)?,
        label: row.get(1)?,
        root_path,
        available,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    let rules_json: String = row.get(5)?;
    let test_file_mode_json: String = row.get(6)?;
    let validation_json: String = row.get(7)?;
    let protected_json: String = row.get(8)?;
    let rule_selection_plan_json: Option<String> = row.get(15)?;

    Ok(RunRecord {
        id: row.get(0)?,
        target_path: row.get(1)?,
        status: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        rules: serde_json::from_str(&rules_json).unwrap_or_default(),
        rule_selection_plan: rule_selection_plan_json
            .and_then(|json| serde_json::from_str(&json).ok()),
        model: row.get(14)?,
        test_file_mode: serde_json::from_str::<String>(&test_file_mode_json)
            .unwrap_or(test_file_mode_json),
        validation_commands: serde_json::from_str(&validation_json).unwrap_or_default(),
        protected_paths: serde_json::from_str(&protected_json).unwrap_or_default(),
        validation_output: row.get(9)?,
        error: row.get(10)?,
        repository_id: row.get(11)?,
        repository_root_path: row.get(12)?,
        target_relative_path: row.get(13)?,
    })
}
