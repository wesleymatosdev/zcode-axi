//! Error type and process exit-code mapping (see docs/EXIT-CODES.md).

use std::fmt;

/// Process exit codes. Stable contract for machine callers (Hermes).
/// Usage errors (exit 2) are produced by the argument parser itself.
pub mod exit {
    pub const OK: u8 = 0;
    pub const RUNTIME_ERROR: u8 = 1;
    pub const NOT_AUTHENTICATED: u8 = 3;
    pub const TIMEOUT: u8 = 4;
    pub const UNSUPPORTED_BY_RUNTIME: u8 = 5;
}

#[derive(Debug)]
pub enum AxiError {
    /// The zcode runtime failed, is missing, or returned an error.
    Runtime(String),
    /// The runtime is reachable but the caller is not authenticated.
    NotAuthenticated(String),
    /// An operation exceeded its time budget.
    Timeout(String),
    /// The runtime cannot perform this operation (explicit, never faked).
    UnsupportedByRuntime(String),
    /// Persisted session storage is unusable or missing the requested row.
    Store(String),
}

impl fmt::Display for AxiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AxiError::Runtime(m) => write!(f, "runtime error: {m}"),
            AxiError::NotAuthenticated(m) => write!(f, "not authenticated: {m}"),
            AxiError::Timeout(m) => write!(f, "timeout: {m}"),
            AxiError::UnsupportedByRuntime(m) => {
                write!(f, "unsupported by runtime: {m}")
            }
            AxiError::Store(m) => write!(f, "session store error: {m}"),
        }
    }
}

impl std::error::Error for AxiError {}

impl AxiError {
    pub fn exit_code(&self) -> u8 {
        match self {
            AxiError::Runtime(_) => exit::RUNTIME_ERROR,
            AxiError::NotAuthenticated(_) => exit::NOT_AUTHENTICATED,
            AxiError::Timeout(_) => exit::TIMEOUT,
            AxiError::UnsupportedByRuntime(_) => exit::UNSUPPORTED_BY_RUNTIME,
            AxiError::Store(_) => exit::RUNTIME_ERROR,
        }
    }
}

pub type AxiResult<T> = Result<T, AxiError>;

/// Classify a failed headless run by its stderr text: auth-looking failures
/// map to NOT_AUTHENTICATED, everything else to Runtime.
pub fn classify_run_failure(stderr: &str) -> AxiError {
    let lower = stderr.to_ascii_lowercase();
    let auth_markers = [
        "login",
        "log in",
        "sign in",
        "auth",
        "401",
        "credential",
        "oauth",
        "api key",
    ];
    if auth_markers.iter().any(|m| lower.contains(m)) {
        return AxiError::NotAuthenticated(stderr_excerpt(stderr));
    }
    AxiError::Runtime(stderr_excerpt(stderr))
}

/// Collapse a runtime stderr blob to a single-line excerpt for messages.
pub fn stderr_excerpt(stderr: &str) -> String {
    let first = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("unknown runtime failure");
    let mut s: String = first.chars().take(300).collect();
    if first.chars().count() > 300 {
        s.push('…');
    }
    s
}
