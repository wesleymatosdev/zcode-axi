//! One-shot window capture → OCR → marker search diagnostic.
//!
//! Hypothesis under test (Wesley, 2026-09-05): window-enumeration matching
//! ("ZCode" substring over xcap windows) can silently bind to a bogus surface
//! (the 21:39 run matched a 2560x30 menubar-sized window) and never read the
//! real GUI. This diagnostic now uses the same guarded main-window selection
//! as the monitor and never captures the desktop.
//!
//! Writes a human-readable report to `out`; prints nothing to stdout (the
//! bundle launches via LaunchServices have no stdout).

use std::io::Write as _;
use std::time::Instant;

use crate::error::AxiError;

pub fn cmd_poc(out_path: &str) -> Result<(), AxiError> {
    let mut out = std::fs::File::create(out_path)
        .map_err(|e| AxiError::Runtime(format!("cannot create {out_path}: {e}")))?;

    // Release builds lose panic messages under LaunchServices (no stderr);
    // catch and record any panic in the report instead of dying silently.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/screenpipe-poc-panic.log")
        {
            let _ = writeln!(f, "PANIC: {info}");
        }
        prev_hook(info);
    }));

    let zcode_running = std::process::Command::new("pgrep")
        .args(["-x", "ZCode"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let _ = writeln!(out, "ZCode GUI process running: {zcode_running}");

    let start = Instant::now();
    // Ask for Screen Recording BEFORE enumeration: an enumeration-only
    // process never triggers the consent dialog and would exit 6 forever.
    crate::watch::ensure_capture_permission()?;
    let window = crate::watch::find_window("ZCode")?;
    let _ = writeln!(out, "capturing selected ZCode window...");
    let _ = out.flush();
    let img = window
        .capture_image()
        .map_err(|e| AxiError::Runtime(format!("capture: {e}")))?;
    let _ = out.flush();
    let (w, h) = (img.width(), img.height());
    let _ = writeln!(out, "captured window {}x{} in {:?}", w, h, start.elapsed());

    if crate::framediff::is_uniform(
        &crate::framediff::signature(img.as_raw(), w as usize, h as usize)
            .map_err(|e| AxiError::Runtime(format!("frame signature failed: {e}")))?,
    ) {
        return Err(AxiError::Runtime(
            "solid-black frame — Screen Recording denied for this host process".into(),
        ));
    }

    let t = Instant::now();
    // Downscale before OCR to reduce processing time and memory use while
    // keeping text legible.
    let scale = 1280f32 / w as f32;
    let (dw, dh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let small = image::imageops::resize(&img, dw, dh, image::imageops::FilterType::Lanczos3);
    let _ = writeln!(out, "downscaled {}x{} -> {}x{} for OCR", w, h, dw, dh);
    let _ = out.flush();
    let _ = writeln!(out, "loading ocr models...");
    let _ = out.flush();
    let ocr = crate::ocr::Ocr::load()?;
    let _ = writeln!(out, "ocr engine loaded in {:?}", t.elapsed());
    let _ = out.flush();
    let _ = writeln!(out, "running ocr on {}x{}...", dw, dh);
    let _ = out.flush();
    let t = Instant::now();
    let text = ocr.text(small.as_raw(), dw, dh)?;
    let _ = out.flush();
    let _ = writeln!(out, "ocr pass in {:?} — {} chars", t.elapsed(), text.len());
    let _ = writeln!(out, "--- RAW OCR TEXT (first 1500 chars) ---");
    let preview: String = text.chars().take(1500).collect();
    let _ = writeln!(out, "{preview}");
    let _ = writeln!(out, "--- END RAW ---");

    let lower = text.to_lowercase();
    let markers: [(&str, Vec<&str>); 4] = [
        ("working", vec!["working", "working for"]),
        (
            "awaiting_approval",
            vec![
                "allow once",
                "allow only this time",
                "allow always",
                "permission",
            ],
        ),
        ("done", vec!["worked for", "completed", "done"]),
        ("idle", vec!["type your task", "ask zcode", "new task"]),
    ];
    let mut hits: Vec<String> = Vec::new();
    for (state, pats) in &markers {
        for p in pats {
            if lower.contains(p) {
                hits.push(format!("{state} (matched '{p}')"));
            }
        }
    }
    let _ = writeln!(out, "--- VERDICT ---");
    if hits.is_empty() {
        let _ = writeln!(
            out,
            "NO markers matched in window OCR. OCR text is real (non-trivial) but no \
             task-state marker visible — check whether the ZCode window is on screen and \
             what its status line reads."
        );
    } else {
        for h in &hits {
            let _ = writeln!(out, "HIT: {h}");
        }
    }
    Ok(())
}
