//! Read-only access to zcode's persisted session store
//! (`~/.zcode/cli/db/db.sqlite`). Never writes, never creates.

use std::path::PathBuf;

use rusqlite::Connection;
use serde::Serialize;

use crate::error::{AxiError, AxiResult};

/// Default store location for the official runtime.
pub fn default_db_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".zcode")
            .join("cli")
            .join("db")
            .join("db.sqlite"),
    )
}

/// Open the persisted store strictly read-only (URI `mode=ro`).
pub fn open_read_only(path: &std::path::Path) -> AxiResult<Connection> {
    if !path.exists() {
        return Err(AxiError::Runtime(format!(
            "zcode session store not found at {}",
            path.display()
        )));
    }
    let uri = format!("file:{}?mode=ro", path.display());
    Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| AxiError::Store(format!("cannot open store read-only: {e}")))
}

/// Session metadata row (subset of the `session` table).
#[derive(Debug, Clone, Serialize)]
pub struct StoredSession {
    pub id: String,
    pub directory: String,
    pub title: String,
    pub version: String,
    pub time_created: i64,
    pub time_updated: i64,
    #[serde(rename = "taskType", default)]
    pub task_type: String,
}

/// One message with its concatenated text parts.
#[derive(Debug, Clone, Serialize)]
pub struct StoredMessage {
    pub role: String,
    pub time_created: i64,
    pub text: String,
}

pub fn session_by_id(conn: &Connection, id: &str) -> AxiResult<Option<StoredSession>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, directory, title, version, time_created, time_updated, task_type \
             FROM session WHERE id = ?1",
        )
        .map_err(|e| AxiError::Store(format!("session query prepare failed: {e}")))?;
    let mut rows = stmt
        .query([id])
        .map_err(|e| AxiError::Store(format!("session query failed: {e}")))?;
    if let Some(row) = rows.next().map_err(|e| AxiError::Store(e.to_string()))? {
        Ok(Some(StoredSession {
            id: row.get(0).map_err(|e| AxiError::Store(e.to_string()))?,
            directory: row.get(1).map_err(|e| AxiError::Store(e.to_string()))?,
            title: row.get(2).map_err(|e| AxiError::Store(e.to_string()))?,
            version: row.get(3).map_err(|e| AxiError::Store(e.to_string()))?,
            time_created: row.get(4).map_err(|e| AxiError::Store(e.to_string()))?,
            time_updated: row.get(5).map_err(|e| AxiError::Store(e.to_string()))?,
            task_type: row
                .get::<_, Option<String>>(6)
                .map_err(|e| AxiError::Store(e.to_string()))?
                .unwrap_or_default(),
        }))
    } else {
        Ok(None)
    }
}

/// All sessions ordered most-recently-updated first.
pub fn all_sessions(conn: &Connection) -> AxiResult<Vec<StoredSession>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, directory, title, version, time_created, time_updated, task_type \
             FROM session ORDER BY time_updated DESC",
        )
        .map_err(|e| AxiError::Store(format!("session query prepare failed: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(StoredSession {
                id: row.get(0)?,
                directory: row.get(1)?,
                title: row.get(2)?,
                version: row.get(3)?,
                time_created: row.get(4)?,
                time_updated: row.get(5)?,
                task_type: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        })
        .map_err(|e| AxiError::Store(format!("session query failed: {e}")))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| AxiError::Store(e.to_string()))?);
    }
    Ok(out)
}

/// Messages (role + concatenated `text` parts) for one session, oldest first.
/// When `limit` is set, the NEWEST `limit` messages are kept, in order.
pub fn messages_for(
    conn: &Connection,
    id: &str,
    limit: Option<usize>,
) -> AxiResult<Vec<StoredMessage>> {
    let mut stmt = conn
        .prepare(
            "SELECT m.time_created, COALESCE(m.data, '{}'), \
                    (SELECT group_concat(p.data, char(10)) FROM part p \
                      WHERE p.message_id = m.id AND p.data LIKE '%\"type\":\"text\"%') \
             FROM message m WHERE m.session_id = ?1 ORDER BY m.time_created, m.id",
        )
        .map_err(|e| AxiError::Store(format!("message query prepare failed: {e}")))?;

    let rows = stmt
        .query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| AxiError::Store(format!("message query failed: {e}")))?;

    let mut all: Vec<(i64, String, Option<String>)> = Vec::new();
    for r in rows {
        all.push(r.map_err(|e| AxiError::Store(e.to_string()))?);
    }
    let start = limit.map(|l| all.len().saturating_sub(l)).unwrap_or(0);

    let mut out = Vec::new();
    for (time, data, parts) in &all[start..] {
        let role = serde_json::from_str::<serde_json::Value>(data)
            .ok()
            .and_then(|v| {
                v.get("role")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "unknown".to_string());
        let mut text = String::new();
        if let Some(parts_json) = parts {
            for line in parts_json.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    if v.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(t);
                        }
                    }
                }
            }
        }
        out.push(StoredMessage {
            role,
            time_created: *time,
            text,
        });
    }
    Ok(out)
}
