//! Frame comparison for the watch loop: downsampled grayscale signatures,
//! block-diff change detection (robust against cursor blink), uniform-frame
//! detection (macOS Screen Recording denial yields solid-black captures),
//! FNV-1a hashing, and PNG evidence dumps.

/// Signature grid dimensions. 64×36 ≈ one block per ~2.8% of a 16:9 window;
/// a blinking cursor changes a handful of blocks, a dialog changes many.
pub const SIG_W: usize = 64;
pub const SIG_H: usize = 36;

/// Per-block grayscale delta that counts as "this block changed".
pub const BLOCK_DELTA: u8 = 25;

/// Fraction of blocks that must differ for a frame to count as changed.
pub const CHANGE_FRACTION: f32 = 0.02;

/// A downsampled grayscale signature of one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameSig {
    pub blocks: Vec<u8>,
}

/// Result of comparing two signatures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeInfo {
    pub changed: bool,
    pub diff_blocks: usize,
    pub total_blocks: usize,
}

impl ChangeInfo {
    pub fn fraction(&self) -> f32 {
        if self.total_blocks == 0 {
            return 0.0;
        }
        self.diff_blocks as f32 / self.total_blocks as f32
    }
}

/// Compute the grayscale block-average signature of an RGBA frame.
/// Block grid is SIG_W×SIG_H stretched over the whole frame.
pub fn signature(rgba: &[u8], width: usize, height: usize) -> FrameSig {
    let mut blocks = vec![0u8; SIG_W * SIG_H];
    assert_eq!(rgba.len(), width * height * 4, "frame must be RGBA8");
    for by in 0..SIG_H {
        let y0 = by * height / SIG_H;
        let y1 = ((by + 1) * height / SIG_H).max(y0 + 1);
        for bx in 0..SIG_W {
            let x0 = bx * width / SIG_W;
            let x1 = ((bx + 1) * width / SIG_W).max(x0 + 1);
            let mut sum: u64 = 0;
            let mut n: u64 = 0;
            for y in y0..y1.min(height) {
                let row = y * width;
                for x in x0..x1.min(width) {
                    let i = (row + x) * 4;
                    // Rec. 601 luma — cheap and adequate for differencing.
                    sum += u64::from(rgba[i]) * 299
                        + u64::from(rgba[i + 1]) * 587
                        + u64::from(rgba[i + 2]) * 114;
                    n += 1;
                }
            }
            blocks[by * SIG_W + bx] = if n == 0 {
                0
            } else {
                (sum / (n * 1000)).min(255) as u8
            };
        }
    }
    FrameSig { blocks }
}

/// True when every block has the same value — on macOS this is the shape of
/// a capture taken without Screen Recording permission (solid black frame),
/// and a real GUI window (title bar, borders, text) is never uniform.
pub fn is_uniform(sig: &FrameSig) -> bool {
    sig.blocks.iter().all(|&b| b == sig.blocks[0])
}

/// Compare two signatures with block counting and a fractional threshold.
pub fn diff(prev: &FrameSig, cur: &FrameSig) -> ChangeInfo {
    let total = cur.blocks.len().min(prev.blocks.len());
    let mut changed_blocks = 0;
    for i in 0..total {
        let a = prev.blocks[i] as i16;
        let b = cur.blocks[i] as i16;
        if (a - b).abs() > BLOCK_DELTA as i16 {
            changed_blocks += 1;
        }
    }
    let needed = ((total as f32) * CHANGE_FRACTION).ceil() as usize;
    ChangeInfo {
        changed: changed_blocks >= needed.max(1),
        diff_blocks: changed_blocks,
        total_blocks: total,
    }
}

/// FNV-1a 64-bit over the signature bytes — reported in JSON events so
/// operators can see when frames actually changed.
pub fn hash(sig: &FrameSig) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in &sig.blocks {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Nearest-neighbor RGBA downscale so `width > target` frames shrink before
/// OCR (detection-model runtime scales with pixels). Returns the input
/// untouched when already small enough.
pub fn downscale_rgba(
    rgba: &[u8],
    width: usize,
    height: usize,
    target_w: usize,
) -> (Vec<u8>, usize, usize) {
    if width <= target_w || width == 0 || height == 0 {
        return (rgba.to_vec(), width, height);
    }
    let step = width as f64 / target_w as f64;
    let new_w = target_w;
    let new_h = ((height as f64) / step).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(new_w * new_h * 4);
    for y in 0..new_h {
        let sy = ((y as f64 + 0.5) * step - 0.5)
            .round()
            .clamp(0.0, (height - 1) as f64) as usize;
        let row = sy * width;
        for x in 0..new_w {
            let sx = ((x as f64 + 0.5) * step - 0.5)
                .round()
                .clamp(0.0, (width - 1) as f64) as usize;
            let i = (row + sx) * 4;
            out.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    (out, new_w, new_h)
}

/// Encode an RGBA8 buffer as a PNG file (evidence dumps).
pub fn write_png(
    path: &std::path::Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc
        .write_header()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}
