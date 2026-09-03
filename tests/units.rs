//! Integration tests for zcode-axi. NEVER talks to the live zcode runtime:
//! protocol tests spawn the in-binary fake app-server over real stdio, and
//! runtime tests use throwaway shell-script fakes in a temp dir.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use serde_json::json;

// Reaching into the binary's modules via include of the crate is not
// possible for a bin target, so tests exercise the compiled binary's
// public behavior plus pure functions replicated through the lib-style
// modules below.
//
// To keep modules testable, the crate exposes them through a tiny lib
// target (src/lib.rs) while src/main.rs remains the thin binary entry.

use zcode_axi::cli::{validate_session_id, Format, OutputOpts};
use zcode_axi::commands::{render_run, RunDoc};
use zcode_axi::error::{classify_run_failure, exit, AxiError};
use zcode_axi::output::{fmt_ms, oneline, truncate};
use zcode_axi::proto::Request;
use zcode_axi::runtime::HeadlessRun;
use zcode_axi::window::{advise, LocalMoment};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_zcode-axi"))
}

// ------------------------------------------------------------- arg parsing

#[test]
fn session_id_validation() {
    assert!(validate_session_id("sess_21c33923-fd70-4ab2-bc51-f6f3c313bfdf").is_ok());
    assert!(validate_session_id("sess_abc123_XYZ-1").is_ok());
    assert!(validate_session_id("sess_").is_err()); // empty rest
    assert!(validate_session_id("wrong-prefix-123").is_err());
    assert!(validate_session_id("sess_bad;chars").is_err());
}

#[test]
fn output_opts_from_flags() {
    let defaults = OutputOpts::from_cli(false, false, false);
    assert_eq!(defaults.format, Format::Compact);
    assert!(defaults.truncate);

    let full = OutputOpts::from_cli(false, false, true);
    assert_eq!(full.format, Format::Compact);
    assert!(!full.truncate);

    let json = OutputOpts::from_cli(true, false, true);
    assert_eq!(json.format, Format::Json);

    let pretty = OutputOpts::from_cli(false, true, false);
    assert_eq!(pretty.format, Format::Pretty);
}

#[test]
fn json_and_pretty_conflict_is_rejected_by_clap() {
    let out = bin()
        .args(["--json", "--pretty", "sessions"])
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2)); // usage error
}

#[test]
fn missing_required_args_exit_2() {
    let out = bin().args(["run"]).output().expect("spawn");
    assert_eq!(out.status.code(), Some(2));
}

// ------------------------------------------------------------- exit codes

#[test]
fn error_to_exit_code_mapping() {
    assert_eq!(
        AxiError::Runtime("x".into()).exit_code(),
        exit::RUNTIME_ERROR
    );
    assert_eq!(
        AxiError::NotAuthenticated("x".into()).exit_code(),
        exit::NOT_AUTHENTICATED
    );
    assert_eq!(AxiError::Timeout("x".into()).exit_code(), exit::TIMEOUT);
    assert_eq!(
        AxiError::UnsupportedByRuntime("x".into()).exit_code(),
        exit::UNSUPPORTED_BY_RUNTIME
    );
    assert_eq!(AxiError::Store("x".into()).exit_code(), exit::RUNTIME_ERROR);
}

#[test]
fn run_failure_classification_auth_vs_runtime() {
    assert!(matches!(
        classify_run_failure("Error: please login to continue"),
        AxiError::NotAuthenticated(_)
    ));
    assert!(matches!(
        classify_run_failure("HTTP 401 unauthorized"),
        AxiError::NotAuthenticated(_)
    ));
    assert!(matches!(
        classify_run_failure("segmentation fault"),
        AxiError::Runtime(_)
    ));
}

// --------------------------------------------------------------- rendering

#[test]
fn truncation_and_hints() {
    let (s, cut) = truncate("short", 10);
    assert_eq!(s, "short");
    assert!(!cut);

    let long = "x".repeat(100);
    let (s, cut) = truncate(&long, 10);
    assert_eq!(s.chars().count(), 11); // 10 chars + ellipsis
    assert!(cut);

    // unicode safety: cut at char boundaries, not bytes
    let emoji = "🎉".repeat(50);
    let (s, _) = truncate(&emoji, 3);
    assert_eq!(s.chars().count(), 4);
}

#[test]
fn oneline_flattens_newlines() {
    assert_eq!(oneline("a\nb\r\nc"), "a⏎b⏎c");
}

#[test]
fn fmt_ms_renders_utc_civil() {
    // 2026-09-03T00:00:00Z == 1788393600
    assert_eq!(fmt_ms(1_788_393_600_000), "2026-09-03 00:00:00Z");
}

#[test]
fn run_doc_renders_json_compact_pretty() {
    let doc = RunDoc {
        session_id: Some("sess_test".into()),
        exit_code: Some(0),
        response: "OK".into(),
        response_truncated: false,
        usage: json!({"totalTokens": 1}),
        projection: json!({"status": "idle"}),
    };
    let j = render_run(&doc, Format::Json);
    let parsed: serde_json::Value = serde_json::from_str(j.trim()).unwrap();
    assert_eq!(parsed["session_id"], "sess_test");
    assert_eq!(parsed["response"], "OK");

    let c = render_run(&doc, Format::Compact);
    assert!(c.contains("session_id=sess_test"));
    assert!(c.contains("exit_code=0"));
    assert!(c.contains("response=OK"));

    let p = render_run(&doc, Format::Pretty);
    assert!(p.contains("session_id: sess_test"));
    assert!(p.contains("  OK"));

    // multiline responses stay one line in compact mode
    let multi = RunDoc {
        response: "line1\nline2".into(),
        ..doc
    };
    let c2 = render_run(&multi, Format::Compact);
    assert!(c2.contains("response=line1⏎line2"));
    assert!(!c2.contains("response=line1\nline2\nexit"));
}

// ------------------------------------------------------------- protocol

#[test]
fn request_frame_encoding_matches_runtime_contract() {
    let req = Request::new(7, "session/list", json!({}));
    assert_eq!(
        req.encode(),
        "{\"id\":7,\"method\":\"session/list\",\"params\":{}}\n"
    );
    // The runtime REJECTS a "jsonrpc" key; the encoder must never emit one.
    assert!(!req.encode().contains("jsonrpc"));

    let stop = Request::new(8, "session/stop", json!({"sessionId": "sess_x"}));
    assert_eq!(
        stop.encode(),
        "{\"id\":8,\"method\":\"session/stop\",\"params\":{\"sessionId\":\"sess_x\"}}\n"
    );
}

/// THE fake-stdio-server test: drives the real AppServerClient against the
/// in-binary canned server (`zcode-axi __fake-app-server`) over actual OS
/// pipes. Never touches the live app-server.
#[test]
fn client_round_trip_against_fake_stdio_server() {
    use zcode_axi::proto::AppServerClient;
    let exe = env!("CARGO_BIN_EXE_zcode-axi");
    let mut client = AppServerClient::spawn(exe).expect("spawn fake app-server");

    // session/list
    let sessions = client.session_list().expect("session/list");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "sess_fake-0001");
    assert_eq!(sessions[0].status, "idle");
    assert_eq!(sessions[0].workspace_path(), "/tmp/fake");

    // session/stop success path
    client.session_stop("sess_fake-0001").expect("session/stop");

    // unknown method surfaces as a Runtime error mentioning the code
    let err = client.call("bogus/method", json!({})).unwrap_err();
    assert!(err.to_string().contains("-32601"), "got: {err}");
}

#[test]
fn client_spawn_failure_is_runtime_error() {
    use zcode_axi::proto::AppServerClient;
    let err = match AppServerClient::spawn("/nonexistent/zcode-axi-fake") {
        Err(e) => e,
        Ok(_) => panic!("spawn of nonexistent program must fail"),
    };
    assert!(matches!(err, AxiError::Runtime(_)));
}

// --------------------------------------------------------------- runtime

/// The verbatim headless JSON contract captured from zcode 0.16.5
/// (docs/protocol.md §3.2) must parse.
#[test]
fn headless_contract_parses() {
    let raw = r#"{
      "sessionId": "sess_69b50c9d-6684-41c6-85a0-7baad0e2421f",
      "traceId": "e8ccf63b-c15b-4ca4-8a7b-a129a6b5ca05",
      "turnId": "turn_7c94b56e-3d1a-47eb-a898-6679efafbf10",
      "response": "OK",
      "usage": {"source": "provider", "modelRequestCount": 1, "inputTokens": 12308,
                "outputTokens": 14, "totalTokens": 12322, "cacheReadTokens": 9728,
                "cacheWriteTokens": 0, "reasoningTokens": 0, "webFetchRequests": 0,
                "webSearchRequests": 0},
      "eventCount": 23,
      "projection": {"status": "idle", "turnCount": 1, "totalTokenCount": 12322,
                     "contextUsed": 12322, "contextWindow": 200000}
    }"#;
    let run: HeadlessRun = serde_json::from_str(raw).expect("contract parse");
    assert_eq!(run.session_id, "sess_69b50c9d-6684-41c6-85a0-7baad0e2421f");
    assert_eq!(run.response, "OK");
    assert_eq!(run.projection["status"], "idle");

    // Tolerant to missing optional fields (only sessionId required).
    let minimal: HeadlessRun = serde_json::from_str(r#"{"sessionId":"sess_m"}"#).unwrap();
    assert_eq!(minimal.session_id, "sess_m");
    assert_eq!(minimal.response, "");
}

/// Fake zcode shell script driving Runtime behavior end-to-end without the
/// real runtime: capability probe, headless success, auth failure.
fn write_fake_zcode(dir: &std::path::Path, body: &str) -> PathBuf {
    let p = dir.join("fake-zcode");
    let mut f = std::fs::File::create(&p).unwrap();
    writeln!(f, "#!/bin/sh\n{body}").unwrap();
    drop(f);
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    p
}

fn tempdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zcode-axi-test-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn runtime_headless_success_and_max_turns_probe() {
    use zcode_axi::runtime::{HeadlessArgs, Runtime};
    let dir = tempdir("headless-ok");
    let contract = json!({
        "sessionId": "sess_fake_headless",
        "response": "OK",
        "projection": {"status": "idle"}
    })
    .to_string();
    let script = format!(
        r#"
case "$1" in
  --version) echo 9.9.9-fake; exit 0;;
esac
if [ "$1" = "--max-turns" ]; then
  echo "Unknown option '--max-turns'" >&2; exit 1
fi
echo '{contract}'
exit 0
"#
    );
    let bin = write_fake_zcode(&dir, &script);
    let rt = Runtime::discover(Some(bin.to_str().unwrap())).unwrap();

    assert_eq!(rt.version().unwrap(), "9.9.9-fake");
    assert!(!rt.supports_max_turns(), "fake rejects --max-turns");

    let outcome = rt.headless_run(&HeadlessArgs {
        prompt: "Reply with exactly: OK".into(),
        cwd: Some("/tmp".into()),
        resume: None,
        max_turns: None,
        max_turns_forward: false,
    });
    assert_eq!(outcome.exit_code, Some(0));
    let run = outcome.run.expect("contract");
    assert_eq!(run.session_id, "sess_fake_headless");
    assert_eq!(run.response, "OK");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn runtime_auth_failure_maps_to_not_authenticated() {
    use zcode_axi::runtime::Runtime;
    let dir = tempdir("auth-fail");
    let script = r#"
if [ "$1" = "--max-turns" ]; then exit 1; fi
echo "Error: please login first (zcode login)" >&2
exit 1
"#;
    let bin = write_fake_zcode(&dir, script);
    let rt = Runtime::discover(Some(bin.to_str().unwrap())).unwrap();
    let outcome = rt.headless_run(&zcode_axi::runtime::HeadlessArgs {
        prompt: "x".into(),
        cwd: None,
        resume: None,
        max_turns: None,
        max_turns_forward: false,
    });
    assert_ne!(outcome.exit_code, Some(0));
    assert!(matches!(outcome.failure(), AxiError::NotAuthenticated(_)));

    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------------------------ store

fn fixture_store(path: &std::path::Path) {
    use rusqlite::Connection;
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE session (
          id text primary key, project_id text not null, workspace_id text,
          parent_id text, slug text not null, directory text not null,
          path text, title text not null, version text not null, share_url text,
          summary_additions integer, summary_deletions integer,
          summary_files integer, summary_diffs text, revert text,
          permission text, time_created integer not null,
          time_updated integer not null, time_compacting integer,
          time_archived integer, task_type text not null default 'interactive'
        );
        CREATE TABLE message (
          id text primary key, session_id text not null references session(id),
          time_created integer not null, time_updated integer not null,
          data text not null, sequence integer
        );
        CREATE TABLE part (
          id text primary key, message_id text not null references message(id),
          session_id text not null, time_created integer not null,
          time_updated integer not null, data text not null, sequence integer
        );
        INSERT INTO session (id, project_id, slug, directory, title, version,
                             time_created, time_updated, task_type)
        VALUES ('sess_store1', 'p1', 's', '/tmp/w', 'stored one', '0.16.5',
                1788441600000, 1788441700000, 'interactive');
        INSERT INTO session (id, project_id, slug, directory, title, version,
                             time_created, time_updated, task_type)
        VALUES ('sess_store2', 'p1', 's', '/tmp/w', 'stored two', '0.16.5',
                1788441500000, 1788441800000, 'interactive');
        INSERT INTO message (id, session_id, time_created, time_updated, data)
        VALUES ('m1', 'sess_store1', 1788441600001, 1788441600001,
                '{"role":"user"}');
        INSERT INTO message (id, session_id, time_created, time_updated, data)
        VALUES ('m2', 'sess_store1', 1788441600002, 1788441600002,
                '{"role":"assistant"}');
        INSERT INTO message (id, session_id, time_created, time_updated, data)
        VALUES ('m3', 'sess_store1', 1788441600003, 1788441600003,
                '{"role":"assistant"}');
        INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
        VALUES ('p1', 'm1', 'sess_store1', 1, 1, '{"type":"text","text":"do the thing"}');
        INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
        VALUES ('p2', 'm2', 'sess_store1', 2, 2, '{"type":"step-start"}');
        INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
        VALUES ('p3', 'm2', 'sess_store1', 3, 3, '{"type":"text","text":"all done"}');
        INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
        VALUES ('p4', 'm3', 'sess_store1', 4, 4, '{"type":"text","text":"final answer"}');
        "#,
    )
    .unwrap();
}

#[test]
fn store_reads_sessions_and_messages_read_only() {
    use zcode_axi::store;
    let dir = tempdir("store");
    let db = dir.join("fixture.sqlite");
    fixture_store(&db);

    let conn = store::open_read_only(&db).expect("open ro");

    let s1 = store::session_by_id(&conn, "sess_store1")
        .unwrap()
        .expect("found");
    assert_eq!(s1.title, "stored one");
    assert_eq!(s1.directory, "/tmp/w");
    assert!(store::session_by_id(&conn, "sess_missing")
        .unwrap()
        .is_none());

    // ordering: most recently updated first
    let all = store::all_sessions(&conn).unwrap();
    assert_eq!(all[0].id, "sess_store2");

    // messages: roles + text parts, step-start parts excluded
    let msgs = store::messages_for(&conn, "sess_store1", None).unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].text, "do the thing");
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].text, "all done"); // step-start skipped

    // limit keeps the NEWEST messages, in chronological order
    let limited = store::messages_for(&conn, "sess_store1", Some(2)).unwrap();
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].role, "assistant");
    assert_eq!(limited[1].text, "final answer");

    // read-only proof: writes through this handle must fail
    let write_result = conn.execute("UPDATE session SET title='x'", []);
    assert!(write_result.is_err());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn store_missing_file_is_runtime_error() {
    use zcode_axi::store;
    let err = store::open_read_only(&PathBuf::from("/nonexistent/db.sqlite")).unwrap_err();
    assert!(matches!(err, AxiError::Runtime(_)));
}

// ----------------------------------------------------------------- window

/// UTC timestamps with known São Paulo local times (UTC-3).
#[test]
fn window_advice_positions() {
    // Sep 3 16:00 UTC = 13:00 local on day 1 → inside.
    let in_window = advise(1_788_451_200);
    assert!(in_window.in_window);
    assert_eq!(in_window.next_window_unix, None);

    // Sep 3 10:00 UTC = 07:00 local → before today's window.
    let before = advise(1_788_429_600);
    assert!(!before.in_window);
    // next open = Sep 3 12:00 local = 15:00 UTC
    assert_eq!(before.next_window_unix, Some(1_788_447_600));

    // Sep 4 01:30 UTC = Sep 3 22:30 local → after today's window.
    let after = advise(1_788_485_400);
    assert!(!after.in_window);
    // next open = Sep 4 12:00 local = 15:00 UTC that day
    assert_eq!(after.next_window_unix, Some(1_788_534_000));

    // Aug 30 12:00 UTC → campaign not started; opens Sep 3 15:00 UTC.
    let early = advise(1_788_091_200);
    assert!(!early.in_window);
    assert_eq!(early.next_window_unix, Some(1_788_447_600));

    // Oct 1 12:00 UTC → campaign over.
    let late = advise(1_790_856_000);
    assert!(!late.in_window);
    assert_eq!(late.next_window_unix, None);
}

#[test]
fn local_moment_offset_is_minus_three_hours() {
    // Sep 3 00:30 UTC → local Sep 2 21:30
    let m = LocalMoment::from_unix(1_788_395_400);
    assert_eq!((m.date.year, m.date.month, m.date.day), (2026, 9, 2));
    assert_eq!((m.hour, m.minute), (21, 30));
    // round-trip
    assert_eq!(m.to_unix(), 1_788_395_400);
}
