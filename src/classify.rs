//! Task-state classification from OCR text of the ZCode GUI window.
//!
//! Pure string logic — fully unit-testable without any capture. Marker
//! strings come from the SWARM brief plus observed ZCode GUI status-line
//! phrasing; matching is case-insensitive substring on whitespace-normalized
//! OCR text (OCR often splits/spaces words unpredictably, so markers are
//! kept short).

use serde::Serialize;

/// Observable task states of the ZCode GUI window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    AwaitingApproval,
    Done,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Running => "running",
            State::AwaitingApproval => "awaiting_approval",
            State::Done => "done",
        }
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// OCR markers for the in-app "Permission required" approval dialog. The
/// dialog headline is primary; the button labels only render inside it.
const APPROVAL_MARKERS: [&str; 4] = [
    "permission required",
    "awaiting approval",
    "always allow",
    "allow once",
];

/// Status-line markers shown while the agent is actively working.
const RUNNING_MARKERS: [&str; 2] = ["working for", "thinking"];

/// Completion markers.
const DONE_MARKERS: [&str; 1] = ["task completed"];

/// Collapse all whitespace runs to single spaces so line breaks inside a
/// marker phrase ("Permission\nrequired") still match.
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Classify OCR text into a task state. Returns None when no marker fires
/// (callers keep the previous state and may fall back to the task index).
///
/// Precedence: awaiting_approval beats running beats done — a permission
/// dialog can be open while the status line still reads "Working for Ns",
/// and the dialog is the actionable state.
pub fn classify(text: &str) -> Option<State> {
    let hay = normalize(text).to_lowercase();
    let has = |m: &str| hay.contains(m);
    if APPROVAL_MARKERS.iter().any(|m| has(m)) {
        return Some(State::AwaitingApproval);
    }
    if RUNNING_MARKERS.iter().any(|m| has(m)) {
        return Some(State::Running);
    }
    if DONE_MARKERS.iter().any(|m| has(m)) {
        return Some(State::Done);
    }
    None
}

/// Cross-check a `done` classification against the
/// task index: the most recent task having `task_status = "completed"`
/// supports Done. Any other status (or None) does not.
pub fn tasks_support_done(latest: Option<&crate::tasks::TaskRow>) -> bool {
    latest.is_some_and(|t| t.task_status == "completed")
}

/// Combine window OCR with the task index without allowing index-only proof.
pub fn classify_with_tasks(
    text: &str,
    latest: Option<&crate::tasks::TaskRow>,
) -> (Option<State>, Confidence) {
    let state = classify(text);
    let confidence = if state == Some(State::Done) && tasks_support_done(latest) {
        Confidence::OcrAndTasks
    } else {
        Confidence::Ocr
    };
    (state, confidence)
}

/// Which evidence produced a classification (reported in JSON events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// OCR marker matched.
    Ocr,
    /// OCR marker matched AND the task index agrees.
    OcrAndTasks,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::Ocr => "ocr",
            Confidence::OcrAndTasks => "ocr+tasks",
        }
    }
}
