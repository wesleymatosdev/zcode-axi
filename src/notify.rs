//! Transition notifications: hermes `send telegram` dispatch plus the
//! per-state rate limiter (max 1 alert per state per 5 minutes). All logic
//! is time-injected (unix ms) so tests are deterministic without sleeping.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::classify::State;

/// Minimum spacing between alerts for the SAME state.
pub const RATE_LIMIT: Duration = Duration::from_secs(5 * 60);

/// Hermes CLI used for telegram pushes (path from the SWARM brief).
pub fn hermes_cmd() -> (PathBuf, PathBuf) {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/Shared"));
    let python = home
        .join(".hermes")
        .join("hermes-agent")
        .join("venv")
        .join("bin")
        .join("python");
    let hermes = home.join(".hermes").join("hermes-agent").join("hermes");
    (python, hermes)
}

/// One-line alert body: task title + transition.
pub fn message(task_title: &str, from: Option<State>, to: State) -> String {
    let arrow = from
        .map(|f| f.as_str().to_string())
        .unwrap_or_else(|| "start".into());
    format!("zcode-axi: {task_title}: {arrow} -> {}", to.as_str())
}

/// Outcome of a rate-limit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateDecision {
    /// Alert may be sent; the state's clock was consumed.
    Allowed,
    /// Too soon since the last alert for this state.
    Suppressed { since_ms: u64 },
}

/// Max one alert per state per rate-limit window. Times are unix ms.
#[derive(Debug, Default)]
pub struct RateLimiter {
    window_ms: u64,
    last_sent: HashMap<State, u64>,
}

impl RateLimiter {
    pub fn new(window: Duration) -> Self {
        Self {
            window_ms: window.as_millis() as u64,
            last_sent: HashMap::new(),
        }
    }

    /// Check-and-consume: returns Allowed only if this state has not been
    /// alerted within the window.
    pub fn check(&mut self, state: State, now_ms: u64) -> RateDecision {
        match self.last_sent.get(&state) {
            Some(&last) if now_ms.saturating_sub(last) < self.window_ms => {
                RateDecision::Suppressed {
                    since_ms: now_ms.saturating_sub(last),
                }
            }
            _ => {
                self.last_sent.insert(state, now_ms);
                RateDecision::Allowed
            }
        }
    }
}

/// Result of a telegram dispatch attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyOutcome {
    pub attempted: bool,
    pub ok: bool,
    pub detail: String,
}

impl NotifyOutcome {
    fn skipped(detail: &str) -> Self {
        Self {
            attempted: false,
            ok: false,
            detail: detail.to_string(),
        }
    }
}

/// Send one telegram alert via hermes. Direct argv — no shell, message text
/// can never be interpreted as arguments.
pub fn send_telegram(msg: &str) -> NotifyOutcome {
    let (python, hermes) = hermes_cmd();
    if !python.exists() || !hermes.exists() {
        return NotifyOutcome::skipped(&format!(
            "hermes not found at {} / {}",
            python.display(),
            hermes.display()
        ));
    }
    match Command::new(&python)
        .arg(&hermes)
        .arg("send")
        .arg("telegram")
        .arg(msg)
        .output()
    {
        Ok(out) if out.status.success() => NotifyOutcome {
            attempted: true,
            ok: true,
            detail: String::new(),
        },
        Ok(out) => NotifyOutcome {
            attempted: true,
            ok: false,
            detail: format!(
                "exit {}: {}",
                out.status.code().unwrap_or(-1),
                crate::output::oneline(String::from_utf8_lossy(&out.stderr).trim())
            ),
        },
        Err(e) => NotifyOutcome {
            attempted: true,
            ok: false,
            detail: format!("spawn failed: {e}"),
        },
    }
}
