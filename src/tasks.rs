//! Read-only access to the zcode GUI task index
//! (`~/.zcode/v2/tasks-index.sqlite`). The database is ONLY ever touched
//! through a `sqlite3 -readonly` subprocess — zcode-axi never links sqlite
//! for this store and never opens it for writing (the bundled rusqlite dep
//! is used exclusively for the separate session store in `store.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{AxiError, AxiResult};

/// Default task-index location for the zcode GUI.
pub fn default_tasks_db_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".zcode")
            .join("v2")
            .join("tasks-index.sqlite"),
    )
}

/// sqlite3 binary to invoke (env override mirrors --zcode-bin conventions).
pub fn sqlite3_bin() -> String {
    std::env::var("ZCODE_AXI_SQLITE3_BIN").unwrap_or_else(|_| "sqlite3".to_string())
}

/// One row of the `tasks` table (subset used by zcode-axi).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskRow {
    pub task_id: String,
    #[serde(default)]
    pub task_status: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub updated_at: i64,
}

/// Query non-deleted tasks, most recently updated first, bounded to `limit`.
pub fn latest_tasks(db: &Path, limit: usize) -> AxiResult<Vec<TaskRow>> {
    // `limit` is an integer rendered by this code, never caller text, so the
    // interpolation below cannot change the statement shape.
    let sql = format!(
        "SELECT task_id, task_status, title, updated_at FROM tasks \
         WHERE deleted = 0 ORDER BY updated_at DESC LIMIT {limit}"
    );
    query(db, &sql)
}

/// Run `sqlite3 -readonly -json <db> <sql>` and parse the JSON rows.
/// Public so tests can exercise arbitrary fixture statements.
pub fn query(db: &Path, sql: &str) -> AxiResult<Vec<TaskRow>> {
    if !db.exists() {
        return Err(AxiError::Runtime(format!(
            "zcode task index not found at {}",
            db.display()
        )));
    }
    let out = Command::new(sqlite3_bin())
        .arg("-readonly")
        .arg("-json")
        .arg(db)
        .arg(sql)
        .output()
        .map_err(|e| AxiError::Runtime(format!("cannot spawn {}: {e}", sqlite3_bin())))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(AxiError::Runtime(format!(
            "sqlite3 -readonly failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            crate::output::oneline(err.trim())
        )));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_rows(&stdout)
}

/// Parse `sqlite3 -json` output (an array, or empty output for zero rows).
pub fn parse_rows(stdout: &str) -> AxiResult<Vec<TaskRow>> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(trimmed)
        .map_err(|e| AxiError::Runtime(format!("cannot parse sqlite3 -json output: {e}")))
}

/// The single most recently updated task, if the index has any.
pub fn most_recent_task(db: &Path) -> AxiResult<Option<TaskRow>> {
    Ok(latest_tasks(db, 1)?.into_iter().next())
}
