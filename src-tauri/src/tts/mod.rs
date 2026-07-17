use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use serde::Serialize;
use tauri::AppHandle;

use crate::config;
use crate::models;

pub mod kokoro;
pub mod playback;

pub use kokoro::KokoroEngine;
pub use playback::Playback;

/// Synthesized audio: mono f32 samples at `sample_rate`.
#[derive(Clone)]
pub struct TtsAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// A downloadable Kokoro asset. `sha1` is optional — the release assets aren't
/// published with pinned hashes, so integrity relies on the model loader
/// rejecting a corrupt file (which degrades to text, never a crash).
pub struct TtsAssetSpec {
    pub id: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub sha1: Option<&'static str>,
    pub label: &'static str,
    pub size: &'static str,
}

/// Kokoro v1.0 int8 assets (thewh1teagle/kokoro-onnx `model-files-v1.0`).
pub const KOKORO_ASSETS: &[TtsAssetSpec] = &[
    TtsAssetSpec {
        id: "kokoro-model",
        filename: "kokoro-v1.0.int8.onnx",
        url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.int8.onnx",
        sha1: None,
        label: "Kokoro model (int8)",
        size: "88 MB",
    },
    TtsAssetSpec {
        id: "kokoro-voices",
        filename: "voices-v1.0.bin",
        url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin",
        sha1: None,
        label: "Kokoro voices",
        size: "27 MB",
    },
];

/// Curated English voice ids exposed in Settings. Each maps to a Kokoro
/// `Voice` variant in [`kokoro::voice_from_id`]; the frontend voice list must
/// stay in sync. Consumed by tests that guard the id → variant mapping.
#[allow(dead_code)]
pub const VOICE_IDS: &[&str] = &[
    "af_heart",
    "af_bella",
    "af_nicole",
    "af_sarah",
    "am_adam",
    "am_michael",
    "am_puck",
    "bf_emma",
    "bf_isabella",
    "bm_george",
    "bm_lewis",
];

/// Start speaking after roughly the first sentence: chunk on sentence
/// boundaries, capping each chunk near this many characters.
pub const MAX_CHUNK_CHARS: usize = 300;

pub fn asset_get(id: &str) -> Option<&'static TtsAssetSpec> {
    KOKORO_ASSETS.iter().find(|a| a.id == id)
}

/// Absolute path where an asset lives (alongside the Whisper models).
pub fn asset_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let asset = asset_get(id).ok_or_else(|| format!("Unknown TTS asset '{id}'"))?;
    let settings = config::load(app)?;
    Ok(models::resolve_dir(&settings.models_dir_override).join(asset.filename))
}

/// (model_path, voices_path). Both may or may not exist on disk yet.
pub fn model_and_voices_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    Ok((asset_path(app, "kokoro-model")?, asset_path(app, "kokoro-voices")?))
}

pub fn assets_downloaded(app: &AppHandle) -> bool {
    match model_and_voices_paths(app) {
        Ok((m, v)) => m.is_file() && v.is_file(),
        Err(_) => false,
    }
}

/// Shared TTS runtime state: the lazily-loaded engine, the playback thread
/// handle, and the last synthesized audio (for replay without re-synth).
#[derive(Default)]
pub struct TtsState {
    pub engine: tokio::sync::Mutex<Option<Arc<KokoroEngine>>>,
    pub playback: Playback,
    pub last_audio: Mutex<Option<TtsAudio>>,
}

impl TtsState {
    /// Load the engine once and cache it; subsequent calls return the cached
    /// handle. Errors if the assets aren't present or the model fails to load.
    pub async fn get_or_load_engine(&self, app: &AppHandle) -> Result<Arc<KokoroEngine>, String> {
        let mut guard = self.engine.lock().await;
        if let Some(engine) = guard.as_ref() {
            return Ok(engine.clone());
        }
        let (model, voices) = model_and_voices_paths(app)?;
        if !model.is_file() || !voices.is_file() {
            return Err("Voice model not downloaded — see Settings".into());
        }
        let engine = Arc::new(KokoroEngine::load(&model, &voices).await?);
        *guard = Some(engine.clone());
        Ok(engine)
    }
}

/// Split text into speakable chunks on sentence boundaries, each capped near
/// [`MAX_CHUNK_CHARS`] so speech can begin after the first sentence.
pub fn chunk_text(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for sentence in split_sentences(text) {
        if current.is_empty() {
            current = sentence;
        } else if current.chars().count() + 1 + sentence.chars().count() <= MAX_CHUNK_CHARS {
            current.push(' ');
            current.push_str(&sentence);
        } else {
            chunks.push(std::mem::take(&mut current));
            current = sentence;
        }
        // A single over-long sentence stands alone rather than growing forever.
        if current.chars().count() >= MAX_CHUNK_CHARS {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
        .into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

/// Split into sentences, keeping terminators. A boundary is a `.`/`!`/`?`/
/// newline followed by whitespace or end-of-text.
fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut sentences = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        current.push(c);
        let is_terminator = matches!(c, '.' | '!' | '?' | '\n');
        let next_breaks = chars.get(i + 1).map(|n| n.is_whitespace()).unwrap_or(true);
        if is_terminator && next_breaks {
            let s = current.trim().to_string();
            if !s.is_empty() {
                sentences.push(s);
            }
            current.clear();
        }
    }
    let tail = current.trim().to_string();
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
}

/// Strip Markdown so the TTS reads plain prose: fenced code blocks become a
/// short spoken note, and headings/emphasis/list markers/links are flattened.
pub fn speakable_text(markdown: &str) -> String {
    let mut pieces: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            if !in_fence {
                pieces.push("code omitted.".to_string());
            }
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let cleaned = strip_inline(line);
        if !cleaned.trim().is_empty() {
            pieces.push(cleaned);
        }
    }
    // Collapse whitespace; sentence boundaries survive via terminators.
    pieces.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Flatten inline markdown on one line: links to their text, and emphasis /
/// heading / list / quote markers removed.
fn strip_inline(line: &str) -> String {
    let delinked = strip_links(line);
    let mut trimmed = delinked.trim_start();
    // Leading block markers: heading #, blockquote >, list bullets.
    trimmed = trimmed.trim_start_matches(['#', '>', ' ']);
    if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
        trimmed = rest;
    }
    // Emphasis / code markers. `_` is kept to avoid splitting snake_case words.
    trimmed
        .chars()
        .filter(|c| !matches!(c, '*' | '`' | '~' | '#'))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Replace `[text](url)` and `![alt](url)` with their text/alt.
fn strip_links(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        // Drop an image's leading '!' when a link follows.
        if chars[i] == '!' && chars.get(i + 1) == Some(&'[') {
            i += 1;
            continue;
        }
        if chars[i] == '[' {
            if let Some(close) = find_from(&chars, i + 1, ']') {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(paren) = find_from(&chars, close + 2, ')') {
                        out.extend(&chars[i + 1..close]);
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn find_from(chars: &[char], start: usize, target: char) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i] == target)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsAssetInfo {
    pub id: String,
    pub label: String,
    pub size: String,
    pub downloaded: bool,
}

/// Status of each TTS asset for the Settings download UI.
#[tauri::command]
pub fn tts_model_status(app: AppHandle) -> Result<Vec<TtsAssetInfo>, String> {
    KOKORO_ASSETS
        .iter()
        .map(|a| {
            Ok(TtsAssetInfo {
                id: a.id.into(),
                label: a.label.into(),
                size: a.size.into(),
                downloaded: asset_path(&app, a.id)?.is_file(),
            })
        })
        .collect()
}

/// Delete a downloaded TTS asset and drop the cached engine so a re-download is
/// picked up cleanly.
#[tauri::command]
pub async fn delete_tts_model(app: AppHandle, id: String) -> Result<(), String> {
    use tauri::Manager;
    let path = asset_path(&app, &id)?;
    if !path.is_file() {
        return Err(format!("'{id}' is not downloaded"));
    }
    std::fs::remove_file(&path).map_err(|e| format!("Could not delete asset: {e}"))?;
    // Evict the cached engine (it may hold the deleted file open / stale).
    *app.state::<TtsState>().engine.lock().await = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_a_single_chunk() {
        let chunks = chunk_text("Hello there. How are you?");
        assert_eq!(chunks, vec!["Hello there. How are you?"]);
    }

    #[test]
    fn empty_or_blank_text_yields_no_chunks() {
        assert!(chunk_text("").is_empty());
        assert!(chunk_text("   \n  ").is_empty());
    }

    #[test]
    fn long_text_splits_and_first_chunk_ends_on_a_sentence_boundary() {
        let sentence = "This is a fairly long sentence that carries some weight. ";
        let text = sentence.repeat(12); // well over MAX_CHUNK_CHARS
        let chunks = chunk_text(&text);
        assert!(chunks.len() > 1, "expected multiple chunks");
        for c in &chunks {
            // No chunk wildly exceeds the cap (single sentences are < cap here).
            assert!(c.chars().count() <= MAX_CHUNK_CHARS + sentence.len());
        }
        assert!(chunks[0].trim_end().ends_with('.'), "first chunk: {}", chunks[0]);
    }

    #[test]
    fn speakable_text_strips_markdown() {
        assert_eq!(speakable_text("# Heading"), "Heading");
        assert_eq!(speakable_text("This is **bold** and *italic*."), "This is bold and italic.");
        assert_eq!(speakable_text("See [the docs](https://example.com) now."), "See the docs now.");
        assert_eq!(speakable_text("- one\n- two"), "one two");
    }

    #[test]
    fn speakable_text_replaces_code_fences() {
        let md = "Before.\n```rust\nfn main() {}\n```\nAfter.";
        let out = speakable_text(md);
        assert!(out.contains("code omitted"), "got: {out}");
        assert!(out.contains("Before.") && out.contains("After."), "got: {out}");
        assert!(!out.contains("fn main"), "code body leaked: {out}");
    }

    #[test]
    fn asset_catalog_is_well_formed() {
        assert_eq!(KOKORO_ASSETS.len(), 2);
        assert!(asset_get("kokoro-model").is_some());
        assert!(asset_get("kokoro-voices").is_some());
        assert!(asset_get("nope").is_none());
        for a in KOKORO_ASSETS {
            assert!(a.url.starts_with("https://"), "{} url must be https", a.id);
            assert!(!a.filename.is_empty());
        }
    }
}
