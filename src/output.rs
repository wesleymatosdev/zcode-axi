//! Output helpers: truncation with explicit hints (never silently cut).

/// Truncate `s` to `max_chars` Unicode scalar values, appending an ellipsis.
/// Returns the (possibly truncated) string and whether truncation happened.
pub fn truncate(s: &str, max_chars: usize) -> (String, bool) {
    if s.chars().count() <= max_chars {
        return (s.to_string(), false);
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    (out, true)
}

/// Flatten any string to a single line (newlines → ⏎) so compact one-line
/// records stay one line.
pub fn oneline(s: &str) -> String {
    s.replace("\r\n", "⏎").replace(['\n', '\r'], "⏎")
}

/// Format a unix millisecond timestamp as local-ish UTC string (stable,
/// timezone-free rendering for machine logs; not a wall-clock claim).
pub fn fmt_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Civil conversion reused from window module for a readable UTC date.
    let (y, m, d) = crate::window::civil_from_days_pub(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}
