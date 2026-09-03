//! Command implementations. All output funnels through `render_*` helpers so
//! every command honors --json/--pretty/--full and prints truncation hints.

use std::io::Write;

use serde::Serialize;
use serde_json::json;

use crate::cli::{validate_session_id, Format, OutputOpts};
use crate::error::{exit, AxiError, AxiResult};
use crate::output::{fmt_ms, oneline, truncate};
use crate::proto::{AppServerClient, SessionInfo};
use crate::runtime::{HeadlessArgs, Runtime};
use crate::store;
use crate::window;

/// Print `msg` as a non-fatal diagnostic on stderr.
fn note(msg: &str) {
    let mut err = std::io::stderr();
    let _ = writeln!(err, "zcode-axi: {msg}");
}

fn truncate_hint(did_truncate: bool, opts: &OutputOpts, what: &str) {
    if did_truncate && opts.truncate {
        note(&format!(
            "output truncated ({what}); re-run with --full for untruncated output"
        ));
    }
}

// ---------------------------------------------------------------- status

#[derive(Serialize)]
pub struct StatusDoc {
    pub zcode_path: String,
    pub zcode_version: String,
    pub doctor_exit: i32,
    pub doctor: serde_json::Value,
    pub auth: String,
    pub auth_checked_by: String,
    pub campaign_window: window::WindowAdvice,
    pub campaign_window_disclaimer: String,
}

pub fn cmd_status(rt: &Runtime, opts: OutputOpts) -> AxiResult<()> {
    let version = rt.version()?;
    let doctor = rt.doctor()?;
    let (auth, auth_detail) = match rt.auth_probe() {
        Ok(true) => (
            "ok".to_string(),
            "headless round-trip succeeded".to_string(),
        ),
        Ok(false) => (
            "not-ok".to_string(),
            "headless round-trip failed".to_string(),
        ),
        Err(e) => ("not-ok".to_string(), e.to_string()),
    };
    let advice = window::advise_now();
    let doc = StatusDoc {
        zcode_path: rt
            .resolved_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| rt.program.clone()),
        zcode_version: version,
        doctor_exit: doctor.exit_code,
        doctor: serde_json::from_str(&doctor.raw).unwrap_or(json!({})),
        auth,
        auth_checked_by: format!("zcode doctor --json exit + trivial headless round-trip ({auth_detail}); credentials never read"),
        campaign_window: advice,
        campaign_window_disclaimer:
            "advisory only; zcode-axi does not know or claim actual quota state".to_string(),
    };

    match opts.format {
        Format::Json => {
            println!(
                "{}",
                serde_json::to_string(&doc).map_err(|e| AxiError::Runtime(e.to_string()))?
            );
        }
        Format::Pretty => {
            println!("zcode:        {}", doc.zcode_path);
            println!("version:      {}", doc.zcode_version);
            println!(
                "doctor:       exit={} cli.version={} cli.process={}",
                doc.doctor_exit, doctor.json.cli.version, doctor.json.cli.process_name
            );
            println!("auth:         {} ({auth_detail})", doc.auth);
            println!("free window:  {}", doc.campaign_window.window);
            println!("in window:    {}", doc.campaign_window.in_window);
            println!("  {}", doc.campaign_window.detail);
            println!("  [{}]", doc.campaign_window_disclaimer);
        }
        Format::Compact => {
            println!(
                "zcode={}\tversion={}\tdoctor_exit={}\tauth={}\tin_free_window={}",
                doc.zcode_path,
                doc.zcode_version,
                doc.doctor_exit,
                doc.auth,
                doc.campaign_window.in_window
            );
            println!("window_advisory\t{}", doc.campaign_window.detail);
        }
    }
    Ok(())
}

// ------------------------------------------------------------------- run

#[derive(Serialize)]
pub struct RunDoc {
    pub session_id: Option<String>,
    pub exit_code: Option<i32>,
    pub response: String,
    pub response_truncated: bool,
    pub usage: serde_json::Value,
    pub projection: serde_json::Value,
}

pub fn cmd_run(
    rt: &Runtime,
    opts: OutputOpts,
    cwd: &str,
    goal: &str,
    max_turns: Option<u32>,
) -> AxiResult<()> {
    cmd_run_inner(rt, opts, goal, Some(cwd), None, max_turns)
}

struct HeadlessDispatch {
    exit_code: Option<i32>,
    run: Option<crate::runtime::HeadlessRun>,
}

/// Shared headless dispatch for `run` and `resume`: capability-probe
/// --max-turns, run to completion, and fail with the mapped error on
/// non-zero exits.
fn dispatch_headless(
    rt: &Runtime,
    goal: &str,
    cwd: &str,
    resume: Option<&str>,
    max_turns: Option<u32>,
) -> AxiResult<HeadlessDispatch> {
    let forward = max_turns.map(|_| rt.supports_max_turns());
    if max_turns.is_some() && forward == Some(false) {
        note(&format!(
            "runtime {} does not accept --max-turns (help/parsing drift in 0.16.5); running without it",
            rt.version().unwrap_or_default()
        ));
    }
    let outcome = rt.headless_run(&HeadlessArgs {
        prompt: goal.to_string(),
        cwd: Some(cwd.to_string()),
        resume: resume.map(str::to_string),
        max_turns,
        max_turns_forward: forward.unwrap_or(false),
    });
    if outcome.exit_code != Some(0) {
        return Err(outcome.failure());
    }
    if outcome.run.is_none() {
        return Err(AxiError::Runtime(
            "headless run exited 0 but produced no JSON contract on stdout".into(),
        ));
    }
    Ok(HeadlessDispatch {
        exit_code: outcome.exit_code,
        run: outcome.run,
    })
}

// --------------------------------------------------------------- resume

pub fn cmd_resume(
    rt: &Runtime,
    opts: OutputOpts,
    id: &str,
    goal: &str,
    max_turns: Option<u32>,
) -> AxiResult<()> {
    if let Err(msg) = validate_session_id(id) {
        return Err(AxiError::Runtime(format!("usage: {msg}")));
    }
    // The session store is the source of truth for "exists".
    if let Some(path) = store::default_db_path() {
        if let Ok(conn) = store::open_read_only(&path) {
            if store::session_by_id(&conn, id)?.is_none() {
                return Err(AxiError::Runtime(format!(
                    "session {id} not found in persisted store"
                )));
            }
        }
    }
    cmd_run_inner(rt, opts, goal, None, Some(id), max_turns)
}

/// `run`/`resume` share rendering; resume passes its own cwd handling.
fn cmd_run_inner(
    rt: &Runtime,
    opts: OutputOpts,
    goal: &str,
    cwd: Option<&str>,
    resume: Option<&str>,
    max_turns: Option<u32>,
) -> AxiResult<()> {
    let workdir = cwd
        .map(str::to_string)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| ".".to_string());
    let outcome = dispatch_headless(rt, goal, &workdir, resume, max_turns)?;
    let run = outcome.run;
    let (resp, resp_trunc) = match &run {
        Some(r) if opts.truncate => truncate(&r.response, 2000),
        Some(r) => (r.response.clone(), false),
        None => (String::new(), false),
    };
    let doc = RunDoc {
        session_id: run.as_ref().map(|r| r.session_id.clone()),
        exit_code: outcome.exit_code,
        response: resp,
        response_truncated: resp_trunc,
        usage: run.as_ref().map(|r| r.usage.clone()).unwrap_or(json!(null)),
        projection: run
            .as_ref()
            .map(|r| r.projection.clone())
            .unwrap_or(json!(null)),
    };
    print!("{}", render_run(&doc, opts.format));
    if resp_trunc {
        truncate_hint(true, &opts, "response");
    }
    Ok(())
}

/// Pure renderer for a completed headless run (kept separate for tests).
pub fn render_run(doc: &RunDoc, format: Format) -> String {
    let mut out = String::new();
    match format {
        Format::Json => {
            out.push_str(&serde_json::to_string(doc).unwrap_or_default());
            out.push('\n');
        }
        Format::Pretty => {
            use std::fmt::Write as _;
            let _ = writeln!(
                out,
                "session_id: {}",
                doc.session_id.as_deref().unwrap_or("-")
            );
            let _ = writeln!(
                out,
                "exit_code:  {}",
                doc.exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "-".into())
            );
            let _ = writeln!(out, "response:");
            for line in doc.response.lines() {
                let _ = writeln!(out, "  {line}");
            }
        }
        Format::Compact => {
            use std::fmt::Write as _;
            let _ = writeln!(
                out,
                "session_id={}\texit_code={}",
                doc.session_id.as_deref().unwrap_or("-"),
                doc.exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "-".into())
            );
            let _ = writeln!(out, "response={}", oneline(&doc.response));
        }
    }
    out
}

// -------------------------------------------------------------- sessions

const DEFAULT_LIST_LIMIT: usize = 20;
const TITLE_WIDTH: usize = 60;

#[derive(Serialize)]
struct SessionsDoc {
    count: usize,
    total_available: usize,
    source: &'static str,
    sessions: Vec<serde_json::Value>,
}

pub fn cmd_sessions(rt: &Runtime, opts: OutputOpts) -> AxiResult<()> {
    // Primary source: live app-server. Fallback: persisted store on disk.
    let live: AxiResult<Vec<SessionInfo>> =
        AppServerClient::spawn(&rt.program).and_then(|mut c| c.session_list());
    match live {
        Ok(mut sessions) => {
            sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            let total = sessions.len();
            let limit = if opts.truncate {
                DEFAULT_LIST_LIMIT
            } else {
                total
            };
            let shown: Vec<&SessionInfo> = sessions.iter().take(limit).collect();
            render_live_sessions(&shown, &opts);
            if shown.len() < total {
                note(&format!(
                    "{} of {} sessions shown; re-run with --full for all",
                    shown.len(),
                    total
                ));
            }
            Ok(())
        }
        Err(e) => {
            note(&format!(
                "app-server session/list failed ({e}); falling back to persisted store"
            ));
            let path = store::default_db_path().ok_or_else(|| {
                AxiError::Store("cannot locate ~/.zcode/cli/db/db.sqlite (HOME unset)".into())
            })?;
            let conn = store::open_read_only(&path)?;
            let sessions = store::all_sessions(&conn)?;
            let total = sessions.len();
            let limit = if opts.truncate {
                DEFAULT_LIST_LIMIT
            } else {
                total
            };
            render_stored_sessions(&sessions[..limit.min(total)], total, &opts);
            Ok(())
        }
    }
}

fn render_live_sessions(sessions: &[&SessionInfo], opts: &OutputOpts) {
    let mut any_cut = false;
    match opts.format {
        Format::Json => {
            let arr: Vec<serde_json::Value> = sessions
                .iter()
                .map(|s| serde_json::to_value(s).unwrap_or(json!({})))
                .collect();
            let doc = SessionsDoc {
                count: arr.len(),
                total_available: arr.len(),
                source: "app-server session/list",
                sessions: arr,
            };
            println!("{}", serde_json::to_string(&doc).unwrap_or_default());
        }
        Format::Pretty => {
            for s in sessions {
                let (title, cut) = truncate(&oneline(&s.title), TITLE_WIDTH);
                any_cut |= cut;
                println!(
                    "{:<40} {:<8} {:<20} {:<30} {}",
                    s.session_id,
                    s.status,
                    fmt_ms(s.updated_at),
                    s.workspace_path(),
                    title
                );
            }
        }
        Format::Compact => {
            for s in sessions {
                let (title, cut) = truncate(&oneline(&s.title), TITLE_WIDTH);
                any_cut |= cut;
                println!(
                    "{}\t{}\t{}\t{}",
                    s.session_id,
                    s.status,
                    fmt_ms(s.updated_at),
                    title
                );
            }
        }
    }
    truncate_hint(any_cut, opts, "titles");
}

fn render_stored_sessions(shown: &[store::StoredSession], total: usize, opts: &OutputOpts) {
    match opts.format {
        Format::Json => {
            let arr: Vec<serde_json::Value> = shown
                .iter()
                .map(|s| serde_json::to_value(s).unwrap_or(json!({})))
                .collect();
            let doc = SessionsDoc {
                count: arr.len(),
                total_available: total,
                source: "persisted store (~/.zcode/cli/db/db.sqlite)",
                sessions: arr,
            };
            println!("{}", serde_json::to_string(&doc).unwrap_or_default());
        }
        Format::Pretty | Format::Compact => {
            for s in shown {
                let (title, _) = truncate(&oneline(&s.title), TITLE_WIDTH);
                println!("{}\t{}\t{}", s.id, fmt_ms(s.time_updated), title);
            }
        }
    }
    if shown.len() < total {
        note(&format!(
            "{} of {} sessions shown; re-run with --full for all",
            shown.len(),
            total
        ));
    }
}

// --------------------------------------------------------------- inspect

const INSPECT_MESSAGES: usize = 10;
const INSPECT_TEXT_WIDTH: usize = 400;

#[derive(Serialize)]
struct InspectDoc {
    session: store::StoredSession,
    messages: Vec<serde_json::Value>,
    messages_total: usize,
    messages_shown: usize,
    live_status: Option<String>,
}

pub fn cmd_inspect(rt: &Runtime, opts: OutputOpts, id: &str) -> AxiResult<()> {
    if let Err(msg) = validate_session_id(id) {
        return Err(AxiError::Runtime(format!("usage: {msg}")));
    }
    let path = store::default_db_path().ok_or_else(|| {
        AxiError::Store("cannot locate ~/.zcode/cli/db/db.sqlite (HOME unset)".into())
    })?;
    let conn = store::open_read_only(&path)?;
    let session = store::session_by_id(&conn, id)?
        .ok_or_else(|| AxiError::Store(format!("session {id} not found in persisted store")))?;

    let total = store::messages_for(&conn, id, None)?.len();
    let limit = if opts.truncate {
        Some(INSPECT_MESSAGES)
    } else {
        None
    };
    let msgs = store::messages_for(&conn, id, limit)?;
    let shown = msgs.len();

    // Best-effort live status from the app-server; silence failures.
    let live_status = AppServerClient::spawn(&rt.program)
        .and_then(|mut c| c.session_list())
        .ok()
        .and_then(|list| {
            list.into_iter()
                .find(|s| s.session_id == id)
                .map(|s| s.status)
        });

    let mut any_cut = false;
    let msg_values: Vec<serde_json::Value> = msgs
        .iter()
        .map(|m| {
            let (text, cut) = if opts.truncate {
                truncate(&oneline(&m.text), INSPECT_TEXT_WIDTH)
            } else {
                (oneline(&m.text), false)
            };
            any_cut |= cut;
            json!({
                "role": m.role,
                "time": fmt_ms(m.time_created),
                "text": text,
                "text_truncated": cut,
            })
        })
        .collect();

    match opts.format {
        Format::Json => {
            let doc = InspectDoc {
                session,
                messages: msg_values,
                messages_total: total,
                messages_shown: shown,
                live_status,
            };
            println!(
                "{}",
                serde_json::to_string(&doc).map_err(|e| AxiError::Runtime(e.to_string()))?
            );
        }
        Format::Pretty | Format::Compact => {
            println!("id:            {}", session.id);
            println!("title:         {}", session.title);
            println!("directory:     {}", session.directory);
            println!("task_type:     {}", session.task_type);
            println!("created:       {}", fmt_ms(session.time_created));
            println!("updated:       {}", fmt_ms(session.time_updated));
            println!(
                "live_status:   {}",
                live_status.as_deref().unwrap_or("(unavailable)")
            );
            println!(
                "messages:      {shown} shown of {total}{}",
                if shown < total {
                    " (use --full for all)"
                } else {
                    ""
                }
            );
            for m in &msg_values {
                println!(
                    "  [{}] {}",
                    m.get("role").and_then(|v| v.as_str()).unwrap_or("?"),
                    m.get("text").and_then(|v| v.as_str()).unwrap_or("")
                );
            }
        }
    }
    truncate_hint(any_cut, &opts, "message text");
    Ok(())
}

// ------------------------------------------------------------------ wait

pub fn cmd_wait(rt: &Runtime, id: &str, timeout_secs: u64) -> AxiResult<()> {
    if let Err(msg) = validate_session_id(id) {
        return Err(AxiError::Runtime(format!("usage: {msg}")));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut seen = false;
    loop {
        let mut client = AppServerClient::spawn(&rt.program)?;
        let status = client
            .session_list()?
            .into_iter()
            .find(|s| s.session_id == id)
            .map(|s| s.status);
        match status {
            Some(st) => {
                seen = true;
                if st == "idle" {
                    println!("session {id} status=idle");
                    return Ok(());
                }
                note(&format!("session {id} status={st}; continuing to wait"));
            }
            None if seen => {
                return Err(AxiError::Runtime(format!(
                    "session {id} disappeared from session/list while waiting"
                )));
            }
            None => {
                note(&format!(
                    "session {id} not present in session/list yet; polling"
                ));
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(AxiError::Timeout(format!(
                "session {id} did not reach status=idle within {timeout_secs}s"
            )));
        }
        let sleep = std::time::Duration::from_secs(1).min(deadline - std::time::Instant::now());
        std::thread::sleep(sleep);
    }
}

// --------------------------------------------------------------- cancel

pub fn cmd_cancel(rt: &Runtime, id: &str) -> AxiResult<()> {
    if let Err(msg) = validate_session_id(id) {
        return Err(AxiError::Runtime(format!("usage: {msg}")));
    }
    let mut client = AppServerClient::spawn(&rt.program)?;
    client.session_stop(id)?;
    println!("stopped {id}");
    Ok(())
}

// --------------------------------------------------- fake app-server (tests)

/// Canned in-process app-server used ONLY by unit tests: reads one frame,
/// answers `session/list`/`session/stop` with fixture data.
pub fn fake_app_server_main() -> i32 {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let reply = match method {
            "session/list" => serde_json::json!({
                "id": id,
                "result": { "sessions": [{
                    "sessionId": "sess_fake-0001",
                    "status": "idle",
                    "title": "fake session",
                    "mode": "build",
                    "sessionKind": "interactive",
                    "createdAt": 1788470000000i64,
                    "updatedAt": 1788470600000i64,
                    "workspace": {"workspacePath": "/tmp/fake", "workspaceKey": "/tmp/fake"}
                }]}
            }),
            "session/stop" => serde_json::json!({
                "id": id,
                "result": { "stopped": true }
            }),
            _ => serde_json::json!({
                "id": id,
                "error": { "code": -32601, "message": format!("Method not found: {method}") }
            }),
        };
        let _ = writeln!(stdout, "{reply}");
        let _ = stdout.flush();
    }
    exit::OK as i32
}
