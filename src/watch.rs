//! The `zcode-axi watch` loop: capture the ZCode window (xcap), diff frames
//! via `framediff`, OCR changed frames (`ocr`), classify (`classify`),
//! cross-check the task index (`tasks`), and emit one JSON line per event to
//! stdout. Telegram alerts fire only on state transitions and only through
//! the per-state rate limiter (`notify`).

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::json;
use xcap::Window;

use crate::classify::{classify, tasks_support_done, Confidence, State};
use crate::error::{AxiError, AxiResult};
use crate::framediff;
use crate::notify::{self, RateDecision, RateLimiter};
use crate::ocr::Ocr;
use crate::output::oneline;
use crate::tasks;

/// Uniform frames tolerated mid-run before declaring permission revoked.
const UNIFORM_STREAK_LIMIT: u32 = 3;
/// Consecutive OCR failures tolerated before giving up.
const OCR_ERROR_LIMIT: u32 = 5;
/// Minimum spacing between task-index cross-check subprocesses.
const TASKS_REFRESH: Duration = Duration::from_secs(10);
/// Heartbeat cadence for liveness while idle.
const HEARTBEAT: Duration = Duration::from_secs(10);
/// Cap on evidence files written per run (`--dump-frames`).
const MAX_DUMPS: u32 = 10;
/// Frames wider than this are downscaled before OCR (speed).
const OCR_TARGET_WIDTH: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum NotifyMode {
    None,
    Telegram,
}

impl NotifyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            NotifyMode::None => "none",
            NotifyMode::Telegram => "telegram",
        }
    }
}

/// unix ms since epoch (clock source for events and rate limiting).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One JSON-line event. Field order is fixed by the struct — deterministic
/// output for machine consumers.
#[derive(Serialize)]
pub struct WatchEvent {
    pub ts: String,
    pub ts_ms: u64,
    pub event: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_blocks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iterations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub states_seen: Option<Vec<String>>,
}

impl WatchEvent {
    fn print(&self) {
        println!("{}", serde_json::to_string(self).unwrap_or_default());
    }
}

fn ev(event: &'static str) -> WatchEvent {
    let ms = now_ms();
    WatchEvent {
        ts: crate::output::fmt_ms(ms as i64),
        ts_ms: ms,
        event,
        from: None,
        state: None,
        confidence: None,
        task_id: None,
        task_title: None,
        frame_hash: None,
        diff_blocks: None,
        notified: None,
        notify_detail: None,
        reason: None,
        iterations: None,
        states_seen: None,
    }
}

/// Find the ZCode window by case-insensitive substring over title or app
/// name. Errors list candidate windows so misconfiguration is debuggable.
///
/// macOS Screen Recording denial hides OTHER applications' windows from
/// enumeration entirely (system surfaces like "Window Server — Menubar" stay
/// visible). When nothing matches and no regular application window is
/// enumerable, that denial is the far more likely cause than "the app has no
/// window" — report it as SCREEN_PERMISSION_DENIED (exit 6) with guidance,
/// once, and never retry.
pub fn find_window(substr: &str) -> AxiResult<Window> {
    let needle = substr.to_lowercase();
    let windows =
        Window::all().map_err(|e| AxiError::Runtime(format!("cannot enumerate windows: {e}")))?;
    let mut candidates: Vec<String> = Vec::new();
    for w in &windows {
        let title = w.title().unwrap_or_default();
        let app = w.app_name().unwrap_or_default();
        candidates.push(format!("{app} — {title}"));
        if title.to_lowercase().contains(&needle) || app.to_lowercase().contains(&needle) {
            return Ok(w.clone());
        }
    }
    let only_system_surfaces = windows.iter().all(|w| {
        w.app_name()
            .map(|a| a == "Window Server" || a == "Dock")
            .unwrap_or(true)
    });
    if only_system_surfaces {
        return Err(permission_error(&format!(
            "no window matches --window-substr {substr:?} and only system \
             surfaces are enumerable ({}) — macOS hides other apps' windows \
             when Screen Recording is not granted",
            candidates.join(" | ")
        )));
    }
    candidates.truncate(15);
    let mut listing = candidates.join(" | ");
    if windows.len() > 15 {
        listing.push_str(&format!(" | (+{} more)", windows.len() - 15));
    }
    Err(AxiError::Runtime(format!(
        "no window matches --window-substr {substr:?}; windows: {listing}"
    )))
}

fn permission_error(detail: &str) -> AxiError {
    AxiError::ScreenPermissionDenied(format!(
        "{detail}; grant it in System Settings > Privacy & Security > Screen \
Recording for the app running zcode-axi, then re-run. zcode-axi will not retry."
    ))
}

/// Snapshot of the task index, refreshed at most every TASKS_REFRESH.
/// Cross-checks are best-effort: index errors never kill the loop.
struct TasksCache {
    db: Option<std::path::PathBuf>,
    last_refresh: Option<Instant>,
    latest: Option<tasks::TaskRow>,
}

impl TasksCache {
    fn new() -> Self {
        Self {
            db: tasks::default_tasks_db_path(),
            last_refresh: None,
            latest: None,
        }
    }

    fn latest(&mut self) -> Option<&tasks::TaskRow> {
        let fresh = self
            .last_refresh
            .is_some_and(|t| t.elapsed() < TASKS_REFRESH);
        if !fresh {
            if let Some(db) = self.db.clone() {
                if let Ok(rows) = tasks::latest_tasks(&db, 1) {
                    self.latest = rows.into_iter().next();
                }
            }
            self.last_refresh = Some(Instant::now());
        }
        self.latest.as_ref()
    }
}

/// Mutable tracking state for one watch run.
struct WatchCtx {
    prev_state: Option<State>,
    states_seen: Vec<String>,
    limiter: RateLimiter,
    tasks: TasksCache,
    notify_mode: NotifyMode,
}

/// Options for the watch run (mirrors the CLI flags).
pub struct WatchOpts {
    pub window_substr: String,
    pub interval: Duration,
    pub notify: NotifyMode,
    pub duration: Option<Duration>,
    pub dump_dir: Option<std::path::PathBuf>,
}

pub fn cmd_watch(opts: WatchOpts) -> AxiResult<()> {
    let window = find_window(&opts.window_substr)?;
    let ocr = Ocr::load()?;

    // Permission gate: the FIRST capture must be non-uniform. A solid-black
    // frame here means Screen Recording was denied — fail once, never retry.
    let first = window
        .capture_image()
        .map_err(|e| permission_error(&format!("capture failed: {e}")))?;
    let (w, h) = first.dimensions();
    let first_sig = framediff::signature(first.as_raw(), w as usize, h as usize);
    if framediff::is_uniform(&first_sig) {
        return Err(permission_error("capture is a solid-black frame"));
    }

    WatchEvent {
        reason: Some(format!(
            "window={}x{} interval_ms={} notify={}",
            w,
            h,
            opts.interval.as_millis(),
            opts.notify.as_str()
        )),
        ..ev("watch_start")
    }
    .print();

    let mut ctx = WatchCtx {
        prev_state: None,
        states_seen: Vec::new(),
        limiter: RateLimiter::new(notify::RATE_LIMIT),
        tasks: TasksCache::new(),
        notify_mode: opts.notify,
    };
    let mut prev_sig = first_sig;
    let mut uniform_streak: u32 = 0;
    let mut ocr_errors: u32 = 0;
    let mut dumps: u32 = 0;
    let mut iterations: u64 = 0;
    let started = Instant::now();
    let mut last_heartbeat = Instant::now();

    loop {
        if let Some(d) = opts.duration {
            if started.elapsed() >= d {
                break;
            }
        }
        let iter_start = Instant::now();

        let frame = window
            .capture_image()
            .map_err(|e| permission_error(&format!("capture failed: {e}")))?;
        let (fw, fh) = frame.dimensions();
        let rgba = frame.as_raw().clone();
        let sig = framediff::signature(&rgba, fw as usize, fh as usize);

        if framediff::is_uniform(&sig) {
            uniform_streak += 1;
            if uniform_streak >= UNIFORM_STREAK_LIMIT {
                return Err(permission_error(&format!(
                    "{UNIFORM_STREAK_LIMIT} consecutive solid-black frames"
                )));
            }
        } else {
            uniform_streak = 0;
        }

        let change = framediff::diff(&prev_sig, &sig);
        if iterations == 0 || change.changed {
            prev_sig = sig.clone();
            // OCR the (downscaled) changed frame.
            let (buf, ow, oh) =
                framediff::downscale_rgba(&rgba, fw as usize, fh as usize, OCR_TARGET_WIDTH);
            match ocr.text(&buf, ow as u32, oh as u32) {
                Ok(text) => {
                    ocr_errors = 0;
                    if let Some(dir) = &opts.dump_dir {
                        if dumps < MAX_DUMPS {
                            let ts = now_ms();
                            let _ = framediff::write_png(
                                &dir.join(format!("frame-{ts}.png")),
                                &rgba,
                                fw,
                                fh,
                            );
                            let _ = std::fs::write(dir.join(format!("frame-{ts}.ocr.txt")), &text);
                            dumps += 1;
                        }
                    }
                    handle_text(&text, &sig, &change, &mut ctx);
                }
                Err(e) => {
                    ocr_errors += 1;
                    eprintln!("zcode-axi: ocr error ({ocr_errors}/{OCR_ERROR_LIMIT}): {e}");
                    if ocr_errors >= OCR_ERROR_LIMIT {
                        return Err(AxiError::Runtime(format!(
                            "ocr failed {OCR_ERROR_LIMIT} times in a row; aborting watch"
                        )));
                    }
                }
            }
        }

        if last_heartbeat.elapsed() >= HEARTBEAT {
            WatchEvent {
                state: Some(
                    ctx.prev_state
                        .map(|s| s.as_str().to_string())
                        .unwrap_or_else(|| "unknown".into()),
                ),
                iterations: Some(iterations),
                ..ev("heartbeat")
            }
            .print();
            last_heartbeat = Instant::now();
        }

        iterations += 1;
        if let Some(d) = opts.duration {
            if started.elapsed() >= d {
                break;
            }
        }
        std::thread::sleep(opts.interval.saturating_sub(iter_start.elapsed()));
    }

    WatchEvent {
        reason: Some("duration".to_string()),
        iterations: Some(iterations),
        states_seen: Some(ctx.states_seen.clone()),
        ..ev("watch_end")
    }
    .print();
    Ok(())
}

/// Classify OCR text, update tracked state, and emit a transition event when
/// the state changed. Alerting decisions live here too: transitions only,
/// gated by the per-state rate limiter.
fn handle_text(
    text: &str,
    sig: &framediff::FrameSig,
    change: &framediff::ChangeInfo,
    ctx: &mut WatchCtx,
) {
    let mut confidence = Confidence::Ocr;
    let mut state = classify(text);

    // Cross-check with the task index: confirm OCR `done`, or settle an
    // unknown when the most recent task completed (transcript-settled).
    match state {
        Some(State::Done) => {
            if tasks_support_done(ctx.tasks.latest()) {
                confidence = Confidence::OcrAndTasks;
            }
        }
        None => {
            if tasks_support_done(ctx.tasks.latest()) {
                state = Some(State::Done);
                confidence = Confidence::TasksOnly;
            }
        }
        _ => {}
    }

    let Some(new_state) = state else {
        return; // unknown text: keep previous state, no event
    };
    if ctx.prev_state == Some(new_state) {
        return; // not a transition
    }

    let from = ctx.prev_state;
    ctx.prev_state = Some(new_state);
    if !ctx.states_seen.contains(&new_state.as_str().to_string()) {
        ctx.states_seen.push(new_state.as_str().to_string());
    }

    let decision = ctx.limiter.check(new_state, now_ms());
    let (notified, notify_detail) = match ctx.notify_mode {
        NotifyMode::None => (None, None),
        NotifyMode::Telegram => {
            let title = ctx
                .tasks
                .latest()
                .map(|t| t.title.clone())
                .unwrap_or_else(|| "zcode task".into());
            match decision {
                RateDecision::Allowed => {
                    let out = notify::send_telegram(&notify::message(&title, from, new_state));
                    (
                        Some(out.ok),
                        Some(if out.detail.is_empty() {
                            "ok".to_string()
                        } else {
                            out.detail
                        }),
                    )
                }
                RateDecision::Suppressed { since_ms } => (
                    Some(false),
                    Some(format!(
                        "rate-limited: {since_ms}ms since last {new_state} alert"
                    )),
                ),
            }
        }
    };

    let task = ctx.tasks.latest();
    WatchEvent {
        from: from.map(|f| f.as_str().to_string()),
        state: Some(new_state.as_str().to_string()),
        confidence: Some(confidence.as_str()),
        task_id: task.map(|t| t.task_id.clone()),
        task_title: task.map(|t| crate::output::truncate(&oneline(&t.title), 80).0),
        frame_hash: Some(format!("{:016x}", framediff::hash(sig))),
        diff_blocks: Some(change.diff_blocks),
        notified,
        notify_detail,
        ..ev("state")
    }
    .print();
}

/// `zcode-axi tasks`: render the task index read via `sqlite3 -readonly`.
pub fn cmd_tasks(opts: crate::cli::OutputOpts, limit: usize) -> AxiResult<()> {
    use crate::cli::Format;
    let db = tasks::default_tasks_db_path().ok_or_else(|| {
        AxiError::Runtime("cannot locate ~/.zcode/v2/tasks-index.sqlite (HOME unset)".into())
    })?;
    let rows = tasks::latest_tasks(&db, limit)?;
    match opts.format {
        Format::Json => {
            let doc = json!({
                "count": rows.len(),
                "source": "sqlite3 -readonly ~/.zcode/v2/tasks-index.sqlite",
                "tasks": rows,
            });
            println!("{doc}");
        }
        Format::Pretty => {
            println!("{:<42} {:<10} {:<24} title", "task_id", "status", "updated");
            for t in &rows {
                let (title, _) = crate::output::truncate(&oneline(&t.title), 60);
                println!(
                    "{:<42} {:<10} {:<24} {}",
                    t.task_id,
                    t.task_status,
                    crate::output::fmt_ms(t.updated_at),
                    title
                );
            }
        }
        Format::Compact => {
            for t in &rows {
                let (title, _) = crate::output::truncate(&oneline(&t.title), 60);
                println!(
                    "{}\t{}\t{}\t{}",
                    t.task_id,
                    t.task_status,
                    crate::output::fmt_ms(t.updated_at),
                    title
                );
            }
        }
    }
    Ok(())
}
