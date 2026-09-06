//! Generate an OCR/classification evidence fixture: renders a synthetic
//! ZCode-like frame ("Working for 12s" status line + a "Permission required"
//! dialog) with a built-in 5x7 bitmap font, runs it through the REAL ocrs
//! engine and classifier, and writes PNG + OCR text + classification to
//! evidence/.
//!
//! Run: cargo run --release --example ocr-fixture [outdir]

use zcode_axi::classify::{classify, State};
use zcode_axi::framediff::write_png;
use zcode_axi::ocr::Ocr;

/// 5x7 uppercase font, one u8 per row, bit 4 = leftmost pixel.
fn glyph(c: u8) -> Option<[u8; 7]> {
    let g: &[u8; 7] = match c {
        b'1' => &[
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'2' => &[
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        b'A' => &[
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'C' => &[
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        b'D' => &[
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        b'E' => &[
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        b'F' => &[
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'I' => &[
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'K' => &[
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        b'L' => &[
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        b'M' => &[
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        b'N' => &[
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        b'O' => &[
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'P' => &[
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'R' => &[
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        b'S' => &[
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'T' => &[
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'U' => &[
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'W' => &[
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        b'Y' => &[
            0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100,
        ],
        b' ' => &[0; 7],
        _ => return None,
    };
    Some(*g)
}

/// Draw one text line into the RGBA buffer at (x, y), scaled by `s`.
fn draw_line(rgba: &mut [u8], w: usize, text: &str, x: usize, y: usize, s: usize) {
    let (fg, bg) = ([25u8, 25, 30], [245u8, 245, 247]);
    let mut cx = x;
    for ch in text.bytes() {
        if let Some(rows) = glyph(ch.to_ascii_uppercase()) {
            for (ry, &row) in rows.iter().enumerate() {
                for rx in 0..5 {
                    let on = row & (1 << (4 - rx)) != 0;
                    let color = if on { fg } else { bg };
                    for yy in 0..s {
                        for xx in 0..s {
                            let px = (y + ry * s + yy) * w + cx + rx * s + xx;
                            let i = px * 4;
                            rgba[i] = color[0];
                            rgba[i + 1] = color[1];
                            rgba[i + 2] = color[2];
                            rgba[i + 3] = 255;
                        }
                    }
                }
            }
        }
        cx += 6 * s; // 5 columns + 1 spacing
    }
}

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "evidence".to_string());
    std::fs::create_dir_all(&out_dir).expect("create evidence dir");

    let (w, h, s) = (760usize, 200usize, 5usize);
    let mut frame = vec![245u8; w * h * 4];

    // status line, like the ZCode GUI footer while an agent runs
    draw_line(&mut frame, w, "WORKING FOR 12S", 30, 20, s);
    // permission dialog headline + buttons (the actionable state)
    draw_line(&mut frame, w, "PERMISSION REQUIRED", 30, 80, s);
    draw_line(&mut frame, w, "ALLOW ONCE   ALLOW ALWAYS", 30, 130, s);

    let png_path = std::path::Path::new(&out_dir).join("fixture-permission-frame.png");
    write_png(&png_path, &frame, w as u32, h as u32).expect("write png");

    // run the REAL production OCR path over the rendered frame
    let ocr = Ocr::load().expect("load ocrs models");
    let text = ocr.text(&frame, w as u32, h as u32).expect("ocr text");
    std::fs::write(
        std::path::Path::new(&out_dir).join("fixture-permission-frame.ocr.txt"),
        &text,
    )
    .expect("write ocr text");

    let state = classify(&text);
    println!("ocr text: {text:?}");
    println!("classified: {:?}", state.map(|s| s.as_str()));
    assert_eq!(
        state,
        Some(State::AwaitingApproval),
        "fixture frame must classify as awaiting_approval"
    );
    println!("fixture OK: {png_path:?}");
}
