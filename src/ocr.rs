//! OCR over captured frames using the pure-Rust `ocrs` engine (RTen
//! runtime, no system dependencies, no network). Models are the upstream
//! text-detection/text-recognition `.rten` files; see `models_dir()` for
//! resolution order.

use std::path::PathBuf;

use ocrs::{ImageSource, OcrEngine, OcrEngineParams};

use crate::error::{AxiError, AxiResult};

/// Directory holding `text-detection.rten` + `text-recognition.rten`.
/// Resolution: `$ZCODE_AXI_OCR_MODELS_DIR`, then `$HOME/.cache/zcode-axi/ocrs`.
pub fn models_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ZCODE_AXI_OCR_MODELS_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".cache")
            .join("zcode-axi")
            .join("ocrs"),
    )
}

pub const MODEL_DOWNLOAD_HINT: &str = "download them with: \
curl -fsSL -o ~/.cache/zcode-axi/ocrs/text-detection.rten \
https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten && \
curl -fsSL -o ~/.cache/zcode-axi/ocrs/text-recognition.rten \
https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

/// A configured OCR engine. Constructed once per watch run.
pub struct Ocr {
    engine: OcrEngine,
}

impl Ocr {
    /// Load models and build the engine.
    pub fn load() -> AxiResult<Self> {
        let dir = models_dir().ok_or_else(|| {
            AxiError::Runtime(
                "cannot resolve OCR models dir (HOME unset and ZCODE_AXI_OCR_MODELS_DIR not set)"
                    .into(),
            )
        })?;
        let detection_path = dir.join("text-detection.rten");
        let recognition_path = dir.join("text-recognition.rten");
        if !detection_path.exists() || !recognition_path.exists() {
            return Err(AxiError::Runtime(format!(
                "ocrs models missing under {}; {MODEL_DOWNLOAD_HINT}",
                dir.display()
            )));
        }
        let detection_model = rten::Model::load_file(&detection_path).map_err(|e| {
            AxiError::Runtime(format!("cannot load {}: {e}", detection_path.display()))
        })?;
        let recognition_model = rten::Model::load_file(&recognition_path).map_err(|e| {
            AxiError::Runtime(format!("cannot load {}: {e}", recognition_path.display()))
        })?;
        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .map_err(|e| AxiError::Runtime(format!("cannot init ocrs engine: {e}")))?;
        Ok(Self { engine })
    }

    /// Recognize all text in an RGBA8 frame. `get_text` preserves reading
    /// order, which is all the classifier needs.
    pub fn text(&self, rgba: &[u8], width: u32, height: u32) -> AxiResult<String> {
        let src = ImageSource::from_bytes(rgba, (width, height))
            .map_err(|e| AxiError::Runtime(format!("ocr input rejected: {e}")))?;
        let input = self
            .engine
            .prepare_input(src)
            .map_err(|e| AxiError::Runtime(format!("ocr preprocess failed: {e}")))?;
        self.engine
            .get_text(&input)
            .map_err(|e| AxiError::Runtime(format!("ocr failed: {e}")))
    }
}
