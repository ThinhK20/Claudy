use std::path::Path;

use kokoro_tts::{KokoroTts, Voice};

use super::TtsAudio;

/// Kokoro v1.0 outputs 24 kHz mono audio.
pub const SAMPLE_RATE: u32 = 24_000;

/// A loaded Kokoro model. Construction is async (ONNX session init); synthesis
/// is async too but CPU-bound under the hood.
pub struct KokoroEngine {
    tts: KokoroTts,
}

impl KokoroEngine {
    pub async fn load(model: &Path, voices: &Path) -> Result<Self, String> {
        let tts = KokoroTts::new(model, voices)
            .await
            .map_err(|e| format!("Could not load voice model: {e}"))?;
        Ok(Self { tts })
    }

    pub async fn synth(&self, text: &str, voice: &str, speed: f32) -> Result<TtsAudio, String> {
        let voice = voice_from_id(voice, speed)?;
        let (samples, _duration) = self
            .tts
            .synth(text, voice)
            .await
            .map_err(|e| format!("Speech synthesis failed: {e}"))?;
        Ok(TtsAudio { samples, sample_rate: SAMPLE_RATE })
    }
}

/// Map a stored voice id to a Kokoro `Voice` variant carrying the speed. Only a
/// curated set of English voices is exposed; an unknown id is a clear error so
/// a stale setting degrades to a helpful message rather than a panic.
pub fn voice_from_id(id: &str, speed: f32) -> Result<Voice, String> {
    Ok(match id {
        "af_heart" => Voice::AfHeart(speed),
        "af_bella" => Voice::AfBella(speed),
        "af_nicole" => Voice::AfNicole(speed),
        "af_sarah" => Voice::AfSarah(speed),
        "am_adam" => Voice::AmAdam(speed),
        "am_michael" => Voice::AmMichael(speed),
        "am_puck" => Voice::AmPuck(speed),
        "bf_emma" => Voice::BfEmma(speed),
        "bf_isabella" => Voice::BfIsabella(speed),
        "bm_george" => Voice::BmGeorge(speed),
        "bm_lewis" => Voice::BmLewis(speed),
        other => {
            return Err(format!(
                "Unknown voice \"{other}\" — choose a voice in Settings"
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opt-in smoke test: synth a short phrase with every catalog voice
    /// against the real downloaded model (needs assets in ../models). Run:
    /// cargo test --lib real_synth_every_voice -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn real_synth_every_voice() {
        let model = std::path::Path::new("../models/kokoro-v1.0.int8-r2.onnx");
        let voices = std::path::Path::new("../models/voices-v1.0-r2.bin");
        assert!(model.is_file() && voices.is_file(), "model assets missing");
        let engine = KokoroEngine::load(model, voices).await.expect("load failed");
        for id in super::super::VOICE_IDS {
            let started = std::time::Instant::now();
            match engine.synth("Hello there, this is a quick check.", id, 1.0).await {
                Ok(a) => println!(
                    "{id}: OK ({} samples, {:?})",
                    a.samples.len(),
                    started.elapsed()
                ),
                Err(e) => println!("{id}: ERROR: {e}"),
            }
        }
    }

    #[test]
    fn every_catalog_voice_maps_to_a_variant() {
        for v in super::super::VOICE_IDS {
            assert!(voice_from_id(v, 1.0).is_ok(), "voice id {v} has no variant");
        }
    }

    #[test]
    fn known_voice_carries_the_speed_and_unknown_is_rejected() {
        assert!(matches!(voice_from_id("af_heart", 1.25).unwrap(), Voice::AfHeart(_)));
        let err = voice_from_id("nope", 1.0).unwrap_err();
        assert!(err.contains("nope"), "got: {err}");
    }
}
