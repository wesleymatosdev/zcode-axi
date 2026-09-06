//! GUI dispatch lane: a file-based handoff queue between zcode-axi and the
//! coordinator's GUI driver. A Rust CLI binary cannot drive the ZCode GUI,
//! so `run --gui` ENQUEUES a dispatch request here instead of spawning the
//! headless runner; the coordinator drains the queue and creates the GUI
//! task itself. Nothing under ~/.zcode is touched (the GUI's task index is
//! read-only to us by standing rule) — the queue lives inside this repo.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AxiError, AxiResult};
use crate::window::civil_from_days_pub;

/// Overrides the queue directory (tests, non-repo installs). Unset: the
/// compile-time repo root's `gui-queue/` directory.
pub const QUEUE_DIR_ENV: &str = "ZCODE_AXI_GUI_QUEUE_DIR";

/// One GUI dispatch request — the exact on-disk JSON contract. Field order
/// is the machine contract (serde emits declaration order).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: String,
    /// Unix epoch milliseconds.
    pub created_at: i64,
    pub brief_path: String,
    pub cwd: String,
    pub mode: String,
    /// Telegram target for completion alerts (e.g. `telegram:W`), if any.
    pub notify: Option<String>,
    pub status: String,
    pub attempts: u32,
}

/// Resolve the queue directory (env override, else repo-root/gui-queue).
pub fn queue_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(QUEUE_DIR_ENV) {
        return PathBuf::from(dir);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("gui-queue")
}

/// Unix epoch milliseconds (0 if the clock is before the epoch).
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// UTC wall clock as `YYYYMMDDTHHMMSS` — sortable, filesystem-safe.
fn utc_stamp_now() -> String {
    let secs = now_ms().div_euclid(1000);
    let (y, m, d) = civil_from_days_pub(secs.div_euclid(86_400));
    let tod = secs.rem_euclid(86_400);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Filesystem-safe slug from a brief path: lowercase file stem with
/// non-alphanumeric runs collapsed to single `-` (leading/trailing dropped).
/// Empty result falls back to `brief`.
pub fn slugify(brief_path: &str) -> String {
    let stem = Path::new(brief_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = String::new();
    let mut last_dash = false;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "brief".into()
    } else {
        out
    }
}

/// `base`, or `base-2`, `base-3`, ... — first stem the `exists` probe
/// accepts. Pure so slug-collision handling is unit-testable.
pub fn unique_stem(base: String, exists: &dyn Fn(&str) -> bool) -> String {
    if !exists(&base) {
        return base;
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if !exists(&candidate) {
            return candidate;
        }
    }
    unreachable!("collision loop always returns")
}

/// Write one dispatch request to the queue. Returns the entry and its file.
pub fn enqueue(
    brief_path: &str,
    cwd: &str,
    mode: &str,
    notify: Option<String>,
) -> AxiResult<(QueueEntry, PathBuf)> {
    let dir = queue_dir();
    std::fs::create_dir_all(&dir).map_err(|e| {
        AxiError::Runtime(format!("cannot create queue dir {}: {e}", dir.display()))
    })?;
    let id = unique_stem(
        format!("{}-{}", utc_stamp_now(), slugify(brief_path)),
        &|stem| dir.join(format!("{stem}.json")).exists(),
    );
    let entry = QueueEntry {
        id: id.clone(),
        created_at: now_ms(),
        brief_path: brief_path.to_string(),
        cwd: cwd.to_string(),
        mode: mode.to_string(),
        notify,
        status: "queued".to_string(),
        attempts: 0,
    };
    let path = dir.join(format!("{id}.json"));
    write_atomic(
        &path,
        &serde_json::to_string_pretty(&entry)
            .map_err(|e| AxiError::Runtime(format!("queue entry does not serialize: {e}")))?,
    )?;
    Ok((entry, path))
}

/// Every `*.json` queue file, newest (lexicographically greatest name)
/// first. Filenames are UTC-timestamp-prefixed, so name order is age order.
fn json_files(dir: &Path) -> AxiResult<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| AxiError::Runtime(format!("cannot read queue dir {}: {e}", dir.display())))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    names.sort();
    names.reverse();
    Ok(names)
}

fn read_entry(path: &Path) -> Option<QueueEntry> {
    let body = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&body).ok()
}

fn note(msg: &str) {
    let mut err = std::io::stderr();
    let _ = writeln!(err, "zcode-axi: {msg}");
}

/// All queue entries, newest first, plus the count of unparseable files
/// (reported on stderr, never fatal — a stray file must not kill `list`).
pub fn list() -> AxiResult<(Vec<QueueEntry>, usize)> {
    let mut entries = Vec::new();
    let mut skipped = 0usize;
    for path in json_files(&queue_dir())? {
        match read_entry(&path) {
            Some(entry) => entries.push(entry),
            None => {
                skipped += 1;
                note(&format!(
                    "skipping unparseable queue file {}",
                    path.display()
                ));
            }
        }
    }
    Ok((entries, skipped))
}

/// Flip one entry's status to `claimed` (rewrite in place). Idempotent for
/// already-claimed entries; unknown ids fail with the documented exit-1
/// runtime error so callers never dispatch against a phantom id.
pub fn claim(id: &str) -> AxiResult<(QueueEntry, PathBuf)> {
    let dir = queue_dir();
    for path in json_files(&dir)? {
        if let Some(mut entry) = read_entry(&path) {
            if entry.id == id {
                entry.status = "claimed".to_string();
                write_atomic(
                    &path,
                    &serde_json::to_string_pretty(&entry).map_err(|e| {
                        AxiError::Runtime(format!("queue entry does not serialize: {e}"))
                    })?,
                )?;
                return Ok((entry, path));
            }
        }
    }
    Err(AxiError::Runtime(format!(
        "gui-queue entry {id} not found in {}",
        dir.display()
    )))
}

/// Write via temp file + rename so machine readers never see partial JSON.
fn write_atomic(path: &Path, body: &str) -> AxiResult<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.flush()
    };
    write().map_err(|e| {
        AxiError::Runtime(format!("cannot write queue file {}: {e}", tmp.display()))
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        AxiError::Runtime(format!(
            "cannot finalize queue file {}: {e}",
            path.display()
        ))
    })?;
    Ok(())
}
