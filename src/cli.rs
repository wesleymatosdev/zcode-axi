//! Command-line surface (clap derive).

use clap::{Parser, Subcommand};

/// Machine-friendly control plane around the official zcode runtime.
///
/// Never prompts, never opens the TUI, never touches login state.
#[derive(Debug, Parser)]
#[command(name = "zcode-axi", version, disable_help_subcommand = true)]
pub struct Cli {
    /// Output as a single JSON document (machine consumers).
    #[arg(long, global = true, conflicts_with = "pretty")]
    pub json: bool,

    /// Output as aligned human-readable text.
    #[arg(long, global = true, conflicts_with = "json")]
    pub pretty: bool,

    /// Disable truncation of titles/messages/fields.
    #[arg(long, global = true)]
    pub full: bool,

    /// zcode binary to invoke (default: resolve `zcode` from PATH).
    #[arg(long, global = true, env = "ZCODE_AXI_ZCODE_BIN")]
    pub zcode_bin: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Runtime discovery: version, auth inference, campaign-window advisory.
    Status,

    /// Dispatch a headless run and print its session id and exit code.
    Run {
        /// Working directory for the run (passed as --cwd).
        #[arg(long)]
        cwd: String,

        /// Prompt text for the headless run.
        #[arg(long)]
        goal: String,

        /// Max model turns. Forwarded to the runtime only if it supports the
        /// flag; otherwise a warning is printed and the run proceeds without.
        #[arg(long)]
        max_turns: Option<u32>,
    },

    /// List sessions known to the runtime (live via app-server).
    Sessions,

    /// Show one persisted session: metadata + recent messages (read-only).
    Inspect { id: String },

    /// Wait until a session reports status "idle" (via app-server).
    Wait {
        id: String,

        /// Give up after S seconds (default 300).
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },

    /// Continue a persisted session with a new headless prompt.
    Resume {
        /// Session id (sess_...).
        id: String,

        /// Prompt text for the resumed headless run.
        #[arg(long)]
        goal: String,

        /// Max model turns (same forwarding rule as `run`).
        #[arg(long)]
        max_turns: Option<u32>,
    },

    /// Stop an active session via the app-server.
    Cancel { id: String },

    /// Watch the ZCode GUI window and report task-state transitions
    /// (running / awaiting_approval / done) as JSON lines on stdout.
    Watch {
        /// Case-insensitive substring matched against window title/app name.
        #[arg(long, default_value = "ZCode")]
        window_substr: String,

        /// Poll interval in milliseconds.
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,

        /// Push a telegram alert (via hermes) on state transitions.
        #[arg(long, default_value = "none")]
        notify: crate::watch::NotifyMode,

        /// Stop after S seconds (0 = run until interrupted).
        #[arg(long, default_value_t = 0)]
        duration_secs: u64,

        /// Save PNG + OCR text of changed frames to DIR (evidence; max 10/run).
        #[arg(long)]
        dump_frames: Option<std::path::PathBuf>,
    },

    /// List tasks from the zcode GUI task index (read-only sqlite3 -readonly).
    Tasks {
        /// Maximum rows to show (default 20).
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    /// NOT A USER COMMAND: canned app-server used by unit tests. Named
    /// `app-server` so the real client's fixed argv (`<exe> app-server`)
    /// reaches it.
    #[command(name = "app-server", hide = true)]
    FakeAppServer,
}

/// Options that select an output format. Derived from global flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Compact, one-line-per-item (default).
    #[default]
    Compact,
    /// Aligned human table (--pretty).
    Pretty,
    /// Single JSON document (--json).
    Json,
}

/// Per-invocation output options derived from global flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutputOpts {
    pub format: Format,
    /// Whether long fields may be truncated. `--full` disables truncation.
    pub truncate: bool,
}

impl OutputOpts {
    pub fn from_cli(json: bool, pretty: bool, full: bool) -> Self {
        let format = if json {
            Format::Json
        } else if pretty {
            Format::Pretty
        } else {
            Format::Compact
        };
        Self {
            format,
            truncate: !full,
        }
    }
}

/// Validates a zcode session id (sess_...) for commands that resume/wait.
pub fn validate_session_id(id: &str) -> Result<(), String> {
    let rest = id
        .strip_prefix("sess_")
        .ok_or_else(|| format!("session id must start with 'sess_', got {id:?}"))?;
    let valid = !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!("session id has unexpected characters: {id:?}"))
    }
}
