//! PoC: selected ZCode window capture → OCR → marker search.
//!
//! Tests the Wesley-directed hypothesis: instead of enumerating windows with
//! xcap and matching "ZCode" (which can silently match a menubar-sized
//! surface and never read the real GUI). This diagnostic uses the production
//! guarded window selector and searches only that window's OCR text.
//!
//! Run from the bundle host (has Screen Recording):
//!   ~/Applications/ZCodeWatcher.app/Contents/MacOS/zcode-axi --example ...
//! or: cargo run --release --example screenpipe-poc

use std::time::Instant;
use zcode_axi::error::AxiError;

fn main() -> Result<(), AxiError> {
    // When launched via LaunchServices there is no stdout — tee everything
    // to a file so the PoC result is inspectable.
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/screenpipe-poc-output.txt".to_string());
    let file = std::fs::File::create(&out_path)
        .map_err(|e| AxiError::Runtime(format!("cannot create {out_path}: {e}")))?;
    let writer = std::io::BufWriter::new(file);
    let mut out = TeeWriter {
        file: writer,
        buf: Vec::new(),
    };
    let code = real_main(&mut out);
    let _ = std::io::Write::flush(&mut out);
    if let Err(e) = code {
        let _ = std::io::Write::write_all(&mut out, format!("\nFATAL: {e}\n").as_bytes());
        let _ = std::io::Write::flush(&mut out);
    }
    Ok(())
}

/// Dumb tee: writes to the file only (stdout is useless under LaunchServices).
struct TeeWriter {
    file: std::io::BufWriter<std::fs::File>,
    buf: Vec<u8>,
}

impl std::io::Write for TeeWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        self.file.write(data)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn real_main<W: std::io::Write>(out: &mut W) -> Result<(), AxiError> {
    let zcode_running = std::process::Command::new("pgrep")
        .args(["-x", "ZCode"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let _ = writeln!(out, "ZCode GUI process running: {zcode_running}");

    let start = Instant::now();
    let window = zcode_axi::watch::find_window("ZCode")?;
    let img = window
        .capture_image()
        .map_err(|e| AxiError::Runtime(format!("capture: {e}")))?;
    let (w, h) = (img.width(), img.height());
    let _ = writeln!(out, "captured window {}x{} in {:?}", w, h, start.elapsed());

    if zcode_axi::framediff::is_uniform(
        &zcode_axi::framediff::signature(img.as_raw(), w as usize, h as usize)
            .map_err(|e| AxiError::Runtime(format!("frame signature failed: {e}")))?,
    ) {
        return Err(AxiError::Runtime(
            "solid-black frame — Screen Recording denied for this host process; \
             run through ~/Applications/ZCodeWatcher.app via launch-zcode-watcher.sh"
                .into(),
        ));
    }

    let t = Instant::now();
    let ocr = zcode_axi::ocr::Ocr::load()?;
    let _ = writeln!(out, "ocr engine loaded in {:?}", t.elapsed());
    let t = Instant::now();
    let text = ocr.text(img.as_raw(), w, h)?;
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
