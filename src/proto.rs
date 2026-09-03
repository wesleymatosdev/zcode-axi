//! ZCode Protocol client: newline-delimited JSON frames over the
//! `zcode app-server` child's stdio. NOT JSON-RPC 2.0 — the runtime rejects
//! the `jsonrpc` envelope key (see docs/protocol.md §1.1).

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AxiError, AxiResult};

/// Time budget for one request/response round-trip with the app-server.
pub const FRAME_TIMEOUT: Duration = Duration::from_secs(15);

/// A client request frame: `{"id":N,"method":...,"params":{...}}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, method: &str, params: Value) -> Self {
        Self {
            id,
            method: method.to_string(),
            params,
        }
    }

    /// Encode as the exact single-line frame the runtime accepts.
    pub fn encode(&self) -> String {
        let mut s = serde_json::to_string(self).unwrap_or_default();
        s.push('\n');
        s
    }
}

/// The error object of an error response frame.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProtocolError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// A response frame: `{"id":N,"result":...}` or `{"id":N,"error":{...}}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Response {
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

/// A session row as returned by `session/list`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionInfo {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub status: String,
    pub title: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    #[serde(rename = "sessionKind")]
    pub session_kind: String,
    #[serde(default)]
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(default)]
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    #[serde(default)]
    pub workspace: Value,
}

impl SessionInfo {
    pub fn workspace_path(&self) -> &str {
        self.workspace
            .get("workspacePath")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
}

enum Frame {
    Line(String),
    Eof,
}

/// Live client over a spawned `zcode app-server` child.
pub struct AppServerClient {
    _child: ChildGuard,
    stdin: Option<ChildStdin>,
    frames: mpsc::Receiver<Frame>,
    next_id: u64,
}

/// Kills the child on drop so a hung server can never outlive the client.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl AppServerClient {
    /// Spawn `program app-server`. `program` is injectable so tests can run
    /// a fake server binary.
    pub fn spawn(program: &str) -> AxiResult<Self> {
        let mut child = Command::new(program)
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AxiError::Runtime(format!("failed to spawn {program} app-server: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AxiError::Runtime("app-server stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AxiError::Runtime("app-server stdout unavailable".into()))?;

        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("axi-appserver-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            let _ = tx.send(Frame::Eof);
                            break;
                        }
                        Ok(_) => {
                            if tx.send(Frame::Line(line)).is_err() {
                                break;
                            }
                        }
                        Err(_) => {
                            let _ = tx.send(Frame::Eof);
                            break;
                        }
                    }
                }
            })
            .map_err(|e| AxiError::Runtime(format!("reader thread spawn failed: {e}")))?;

        Ok(Self {
            _child: ChildGuard(child),
            stdin: Some(stdin),
            frames: rx,
            next_id: 1,
        })
    }

    /// Send one request and wait (up to [`FRAME_TIMEOUT`]) for its response.
    /// Server frames with a different id are skipped.
    pub fn call(&mut self, method: &str, params: Value) -> AxiResult<Value> {
        let req = Request::new(self.next_id, method, params);
        self.next_id += 1;
        if let Some(stdin) = self.stdin.as_mut() {
            stdin
                .write_all(req.encode().as_bytes())
                .and_then(|_| stdin.flush())
                .map_err(|e| AxiError::Runtime(format!("app-server write failed: {e}")))?;
        }

        let deadline = Instant::now() + FRAME_TIMEOUT;
        loop {
            let frame = self
                .frames
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| {
                    AxiError::Timeout("app-server did not respond within frame timeout".into())
                })?;
            let line = match frame {
                Frame::Line(l) => l,
                Frame::Eof => {
                    return Err(AxiError::Runtime(
                        "app-server closed stdout before responding".into(),
                    ))
                }
            };
            let resp: Response = serde_json::from_str(line.trim())
                .map_err(|e| AxiError::Runtime(format!("unparseable app-server frame: {e}")))?;
            if resp.id != json!(req.id) {
                continue;
            }
            if let Some(err) = resp.error {
                return Err(protocol_error(method, err));
            }
            return Ok(resp.result.unwrap_or(Value::Null));
        }
    }

    /// `session/list` → all sessions.
    pub fn session_list(&mut self) -> AxiResult<Vec<SessionInfo>> {
        let result = self.call("session/list", json!({}))?;
        let sessions = result
            .get("sessions")
            .cloned()
            .unwrap_or_else(|| Value::Array(vec![]));
        serde_json::from_value(sessions)
            .map_err(|e| AxiError::Runtime(format!("session/list payload mismatch: {e}")))
    }

    /// `session/stop`. Maps the runtime's "Session is not active" error
    /// (-32004) to [`AxiError::UnsupportedByRuntime`] → exit code 5.
    pub fn session_stop(&mut self, session_id: &str) -> AxiResult<()> {
        self.call("session/stop", json!({ "sessionId": session_id }))
            .map(|_| ())
            .map_err(|e| match e {
                AxiError::Runtime(msg) if msg.contains("-32004") => {
                    AxiError::UnsupportedByRuntime(format!(
                        "session {session_id} is not active in the app-server; \
                         only active sessions can be stopped"
                    ))
                }
                other => other,
            })
    }
}

/// Render a protocol error response into an AxiError, annotating -32004 so
/// callers can special-case the not-active condition.
fn protocol_error(method: &str, err: ProtocolError) -> AxiError {
    AxiError::Runtime(format!(
        "app-server error {code} for {method}: {msg}",
        code = err.code,
        msg = err.message
    ))
}
