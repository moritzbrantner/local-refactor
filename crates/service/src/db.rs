use crate::RunCreateRequest;
use anyhow::Result;
use chrono::Utc;
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
pub struct RunRecord {
    pub id: String,
    pub target_path: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub rules: Vec<String>,
    pub test_file_mode: String,
    pub validation_commands: Vec<String>,
    pub protected_paths: Vec<String>,
    pub validation_output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PatchRecord {
    pub run_id: String,
    pub file_path: String,
    pub original_content: String,
    pub new_content: String,
    pub rule_id: String,
    pub summary: String,
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

            CREATE TABLE IF NOT EXISTS patches (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                original_content TEXT NOT NULL,
                new_content TEXT NOT NULL,
                rule_id TEXT NOT NULL,
                summary TEXT NOT NULL,
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
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_run(&self, id: &str, request: &RunCreateRequest) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn()?;
        conn.execute(
            r#"
            INSERT INTO runs (
                id, target_path, status, created_at, updated_at, rules_json,
                test_file_mode, validation_json, protected_json
            ) VALUES (?1, ?2, 'queued', ?3, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                id,
                request.target_path,
                now,
                serde_json::to_string(&request.rules)?,
                serde_json::to_string(&request.test_file_mode.unwrap_or_default())?,
                serde_json::to_string(&request.validation_commands)?,
                serde_json::to_string(&request.protected_paths)?,
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

    pub fn list_runs(&self) -> Result<Vec<RunRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, target_path, status, created_at, updated_at, rules_json,
                   test_file_mode, validation_json, protected_json, validation_output, error
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
                       test_file_mode, validation_json, protected_json, validation_output, error
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
                run_id, file_path, original_content, new_content, rule_id, summary
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                patch.run_id,
                patch.file_path,
                patch.original_content,
                patch.new_content,
                patch.rule_id,
                patch.summary
            ],
        )?;
        Ok(())
    }

    pub fn patches_for_run(&self, run_id: &str) -> Result<Vec<PatchRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT run_id, file_path, original_content, new_content, rule_id, summary
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

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    let rules_json: String = row.get(5)?;
    let test_file_mode_json: String = row.get(6)?;
    let validation_json: String = row.get(7)?;
    let protected_json: String = row.get(8)?;

    Ok(RunRecord {
        id: row.get(0)?,
        target_path: row.get(1)?,
        status: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        rules: serde_json::from_str(&rules_json).unwrap_or_default(),
        test_file_mode: serde_json::from_str::<String>(&test_file_mode_json)
            .unwrap_or(test_file_mode_json),
        validation_commands: serde_json::from_str(&validation_json).unwrap_or_default(),
        protected_paths: serde_json::from_str(&protected_json).unwrap_or_default(),
        validation_output: row.get(9)?,
        error: row.get(10)?,
    })
}
