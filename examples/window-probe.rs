//! Debug probe: enumerate all xcap windows (title + app name), then capture
//! the first enumerated window and report whether the frame is uniform
//! (solid black = Screen Recording permission denied on macOS).
//!
//! Run: cargo run --example window-probe [-- substr]

use xcap::Window;

fn main() {
    let substr = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let windows = Window::all().expect("enumerate windows");
    println!("{} window(s) visible:", windows.len());
    for w in &windows {
        println!(
            "  app={:?} title={:?} {}x{}",
            w.app_name().unwrap_or_default(),
            w.title().unwrap_or_default(),
            w.width().unwrap_or_default(),
            w.height().unwrap_or_default()
        );
    }
    let target = windows.iter().find(|w| {
        w.title()
            .unwrap_or_default()
            .to_lowercase()
            .contains(&substr)
            || w.app_name()
                .unwrap_or_default()
                .to_lowercase()
                .contains(&substr)
    });
    let Some(w) = target else {
        println!("no window matches {substr:?}");
        return;
    };
    match w.capture_image() {
        Ok(img) => {
            let bytes = img.as_raw();
            let uniform = bytes.iter().all(|&b| b == bytes[0]);
            println!(
                "captured {}x{}, uniform={uniform} (first pixel {})",
                img.width(),
                img.height(),
                bytes[0]
            );
        }
        Err(e) => println!("capture failed: {e}"),
    }
}
