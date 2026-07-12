use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::{self, Settings};
use crate::{audio, models};

/// Minimum audio length worth transcribing (0.5 s at 16 kHz).
const MIN_SAMPLES: usize = 8_000;

pub trait SttEngine: Send {
    fn transcribe(&mut self, samples_16k_mono: &[f32], language: &str) -> Result<String, String>;
}

pub struct WhisperEngine {
    ctx: WhisperContext,
}

/// Keep-warm cache: the loaded engine plus the model path it was loaded from.
#[derive(Default)]
pub struct SttState(pub Mutex<Option<(PathBuf, WhisperEngine)>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    pub text: String,
    pub duration_ms: u64,
}

pub fn normalize_language(language: &str) -> &str {
    let trimmed = language.trim();
    if trimmed.is_empty() { "auto" } else { trimmed }
}

pub fn resolve_model_path(settings: &Settings) -> Result<PathBuf, String> {
    if settings.model.is_empty() {
        return Err("No model selected. Choose a model on the Transcription page.".into());
    }
    let path = models::resolve_dir(&settings.models_dir_override)
        .join(models::model_filename(&settings.model));
    if !path.is_file() {
        return Err(format!(
            "Model '{}' is not downloaded. Download it on the Transcription page.",
            settings.model
        ));
    }
    Ok(path)
}

impl WhisperEngine {
    pub fn load(model_path: &Path) -> Result<Self, String> {
        let path = model_path
            .to_str()
            .ok_or("Model path is not valid UTF-8")?;
        let ctx = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .map_err(|e| format!("Could not load model: {e}"))?;
        Ok(Self { ctx })
    }
}

impl SttEngine for WhisperEngine {
    fn transcribe(&mut self, samples_16k_mono: &[f32], language: &str) -> Result<String, String> {
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Transcription setup failed: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(normalize_language(language)));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state
            .full(params, samples_16k_mono)
            .map_err(|e| format!("Transcription failed: {e}"))?;

        let mut text = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(segment) = state.get_segment(i) {
                let piece = segment
                    .to_str_lossy()
                    .map_err(|e| format!("Could not read transcription text: {e}"))?;
                text.push_str(&piece);
            }
        }
        Ok(text.trim().to_string())
    }
}

/// Transcribes already-captured 16 kHz mono samples using the settings-selected
/// model and language. Runs whisper on a blocking thread. Honors keep_model_warm.
pub async fn transcribe_samples(
    app: &AppHandle,
    samples: Vec<f32>,
) -> Result<TranscriptionResult, String> {
    if samples.len() < MIN_SAMPLES {
        return Err("Recording was too short to transcribe".into());
    }
    let settings = config::load(app)?;
    let model_path = resolve_model_path(&settings)?;
    let app = app.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<SttState>();
        let mut cache = state.0.lock().map_err(|_| "stt state poisoned")?;

        // Reuse the warm engine only if it was loaded from the same model file.
        let mut engine = match cache.take() {
            Some((path, engine)) if path == model_path => engine,
            _ => WhisperEngine::load(&model_path)?,
        };

        let started = std::time::Instant::now();
        let text = engine.transcribe(&samples, &settings.language)?;
        let duration_ms = started.elapsed().as_millis() as u64;

        if settings.keep_model_warm {
            *cache = Some((model_path, engine));
        } // else: engine drops here, freeing model memory

        Ok(TranscriptionResult { text, duration_ms })
    })
    .await
    .map_err(|e| format!("Transcription task failed: {e}"))?
}

#[tauri::command]
pub async fn stop_capture_and_transcribe(app: AppHandle) -> Result<TranscriptionResult, String> {
    let samples = audio::stop(&app.state::<audio::AudioState>())?;
    transcribe_samples(&app, samples).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_language_defaults_empty_to_auto() {
        assert_eq!(normalize_language(""), "auto");
        assert_eq!(normalize_language("  "), "auto");
        assert_eq!(normalize_language("vi"), "vi");
    }

    #[test]
    fn resolve_model_path_requires_a_selected_model() {
        let settings = Settings::default(); // model is ""
        let err = resolve_model_path(&settings).unwrap_err();
        assert!(err.contains("No model selected"));
    }

    #[test]
    fn resolve_model_path_requires_the_file_to_exist() {
        let mut settings = Settings::default();
        settings.model = "tiny".into();
        settings.models_dir_override = std::env::temp_dir()
            .join("claudy-empty-models")
            .to_string_lossy()
            .into_owned();
        let err = resolve_model_path(&settings).unwrap_err();
        assert!(err.contains("not downloaded"));
    }

    #[test]
    fn whisper_engine_load_fails_cleanly_for_missing_file() {
        let err = WhisperEngine::load(Path::new("Z:\\missing\\ggml-none.bin"))
            .err()
            .expect("loading a missing model file must fail");
        assert!(err.contains("Could not load model"));
    }
}
