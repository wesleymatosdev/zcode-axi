//! zcode runtime access: discovery, version, doctor, capability probing,
//! and headless runs (`zcode --json -p ...`). The OFFICIAL installed binary
//! only — never wrapped, never modified, login state never touched.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::error::{classify_run_failure, AxiError, AxiResult};

/// Resolved zcode runtime handle.
#[derive(Debug, Clone)]
pub struct Runtime {
    /// Program exactly as it will be executed (path or PATH-resolved name).
    pub program: String,
    /// Absolute path if resolved from PATH, else None.
    pub resolved_path: Option<PathBuf>,
}

impl Runtime {
    /// Resolve the zcode binary. Precedence: explicit override (CLI flag or
    /// `ZCODE_AXI_ZCODE_BIN`), then `zcode` on PATH.
    pub fn discover(override_bin: Option<&str>) -> AxiResult<Self> {
        let program = override_bin
            .map(str::to_string)
            .unwrap_or_else(|| "zcode".to_string());
        let resolved_path = if program.contains('/') {
            let p = PathBuf::from(&program);
            if p.is_file() {
                Some(p)
            } else {
                return Err(AxiError::Runtime(format!(
                    "zcode binary not found at {program}"
                )));
            }
        } else {
            which(&program)?
        };
        Ok(Self {
            program,
            resolved_path,
        })
    }

    /// `zcode --version` → trimmed version string.
    pub fn version(&self) -> AxiResult<String> {
        let out = Command::new(&self.program)
            .arg("--version")
            .stderr(Stdio::null())
            .output()
            .map_err(|e| AxiError::Runtime(format!("failed to run zcode --version: {e}")))?;
        if !out.status.success() {
            return Err(AxiError::Runtime("zcode --version exited non-zero".into()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// `zcode doctor --json` → parsed fields + raw JSON. Read-only; never
    /// reads credentials.
    pub fn doctor(&self) -> AxiResult<DoctorReport> {
        let out = Command::new(&self.program)
            .args(["doctor", "--json"])
            .stderr(Stdio::null())
            .output()
            .map_err(|e| AxiError::Runtime(format!("failed to run zcode doctor: {e}")))?;
        if !out.status.success() {
            return Err(AxiError::Runtime(format!(
                "zcode doctor --json exited with {}",
                out.status.code().unwrap_or(-1)
            )));
        }
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let parsed: DoctorJson = serde_json::from_str(&raw)
            .map_err(|e| AxiError::Runtime(format!("zcode doctor output not JSON: {e}")))?;
        Ok(DoctorReport {
            exit_code: out.status.code().unwrap_or(0),
            json: parsed,
            raw,
        })
    }

    /// Probe whether the runtime parser accepts `--max-turns` WITHOUT a
    /// model call: an unknown option aborts argument parsing before
    /// `--version` short-circuits (verified against 0.16.5, where --help
    /// lists the flag but the parser rejects it).
    pub fn supports_max_turns(&self) -> bool {
        Command::new(&self.program)
            .args(["--max-turns", "1", "--version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Dispatch one headless run and block until it finishes. Returns the
    /// parsed JSON contract (docs/protocol.md §3.2), the child's exit code,
    /// and raw stderr for failure classification.
    pub fn headless_run(&self, args: &HeadlessArgs) -> HeadlessOutcome {
        let mut cmd = Command::new(&self.program);
        cmd.arg("--json");
        if let Some(cwd) = &args.cwd {
            cmd.args(["--cwd", cwd]);
        }
        if let Some(resume) = &args.resume {
            cmd.args(["--resume", resume]);
        }
        if args.max_turns_forward {
            if let Some(n) = args.max_turns {
                cmd.arg("--max-turns").arg(n.to_string());
            }
        }
        cmd.arg("-p").arg(&args.prompt);
        let out = match cmd.stderr(Stdio::piped()).output() {
            Ok(o) => o,
            Err(e) => {
                return HeadlessOutcome {
                    exit_code: None,
                    run: None,
                    stderr: format!("failed to spawn zcode: {e}"),
                }
            }
        };
        let exit_code = out.status.code();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let run = if stdout.is_empty() {
            None
        } else {
            serde_json::from_str::<HeadlessRun>(&stdout).ok()
        };
        HeadlessOutcome {
            exit_code,
            run,
            stderr,
        }
    }

    /// Trivial auth inference round-trip: a one-line headless prompt.
    /// Ok(true) = authenticated, Ok(false) = runtime answered but the
    /// exchange failed in a non-auth way, Err = could not run at all.
    /// Credentials are never read.
    pub fn auth_probe(&self) -> Result<bool, AxiError> {
        let outcome = self.headless_run(&HeadlessArgs {
            cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
            resume: None,
            max_turns: None,
            max_turns_forward: false,
            prompt: "Reply with exactly: OK".to_string(),
        });
        match outcome.exit_code {
            Some(0) => Ok(outcome
                .run
                .as_ref()
                .map(|r| !r.response.trim().is_empty())
                .unwrap_or(false)),
            Some(_) => Ok(false),
            None => Err(AxiError::Runtime(format!(
                "auth probe could not run: {}",
                outcome.stderr
            ))),
        }
    }
}

/// Arguments for one headless run.
#[derive(Debug, Clone)]
pub struct HeadlessArgs {
    pub prompt: String,
    pub cwd: Option<String>,
    pub resume: Option<String>,
    pub max_turns: Option<u32>,
    /// Whether `--max-turns` may be forwarded (capability probe passed).
    pub max_turns_forward: bool,
}

/// Everything a headless invocation produced.
#[derive(Debug)]
pub struct HeadlessOutcome {
    pub exit_code: Option<i32>,
    pub run: Option<HeadlessRun>,
    pub stderr: String,
}

impl HeadlessOutcome {
    /// Map a failed run to the right AxiError (auth vs generic runtime).
    pub fn failure(&self) -> AxiError {
        classify_run_failure(&self.stderr)
    }
}

/// Headless JSON output contract (see docs/protocol.md §3.2).
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct HeadlessRun {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(default)]
    pub response: String,
    #[serde(default)]
    #[serde(rename = "traceId")]
    pub trace_id: String,
    #[serde(default)]
    pub usage: serde_json::Value,
    #[serde(default)]
    pub projection: serde_json::Value,
}

#[derive(Debug)]
pub struct DoctorReport {
    pub exit_code: i32,
    pub json: DoctorJson,
    pub raw: String,
}

/// Subset of `zcode doctor --json` that zcode-axi reports.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct DoctorJson {
    pub cli: DoctorCli,
    #[serde(default)]
    pub runtime: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct DoctorCli {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    #[serde(rename = "processName")]
    pub process_name: String,
}

/// Minimal PATH lookup for an executable bit set and regular file.
pub fn which(program: &str) -> AxiResult<Option<PathBuf>> {
    let path = std::env::var_os("PATH")
        .ok_or_else(|| AxiError::Runtime("PATH is not set; pass --zcode-bin explicitly".into()))?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if is_executable_file(&candidate) {
            return Ok(Some(candidate));
        }
    }
    Err(AxiError::Runtime(format!(
        "runtime not found: `{program}` is not on PATH; install zcode or pass --zcode-bin"
    )))
}

fn is_executable_file(p: &PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}
