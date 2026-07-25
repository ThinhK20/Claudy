//! Wake word: an opt-in always-on listener that opens the ask box when it
//! hears a spoken phrase ("hey claudy").
//!
//! Detection is deliberately cheap at rest. A continuous RMS check costs
//! essentially nothing while the room is quiet; whisper only runs on the last
//! couple of seconds once the VAD decides someone actually said something.
//!
//! Audio never leaves memory: the rolling window is a bounded `VecDeque` that
//! is dropped when the listener stops. Nothing is written to disk or sent
//! anywhere.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

use crate::stt::{SttEngine, WhisperEngine};
use crate::{assistant, audio, config, dictation, stt, tray};

/// How often the listener drains the capture buffer and re-runs the VAD.
const TICK_MS: u64 = 100;
/// Rolling window handed to whisper — long enough for "hey claudy" plus the
/// run-up, short enough that a transcription stays well inside a tick.
const WINDOW_SECS: f32 = 2.5;
const WINDOW_SAMPLES: usize = (WINDOW_SECS * audio::TARGET_SAMPLE_RATE as f32) as usize;
/// Silence this long after speech ends the utterance and triggers a check.
const SILENCE_MS: u64 = 350;
/// Someone talking continuously still gets checked this often, so a wake word
/// spoken mid-sentence isn't stuck waiting for a pause that never comes.
const MAX_SPEECH_MS: u64 = 2_500;
/// Ignore further matches this long after a trigger — one "hey claudy" must
/// not open the panel twice.
const COOLDOWN_MS: u64 = 2_000;
/// RMS above which a frame counts as speech rather than room noise.
const SPEECH_RMS: f32 = 0.02;
/// Below this there isn't enough audio to be worth transcribing (0.5 s).
const MIN_EVAL_SAMPLES: usize = 8_000;

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// How forgiving the phrase match is. Whisper mishears wake words constantly
/// ("hey cloudy", "hey claude"), so an exact match would almost never fire.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity {
    /// Exact words only. Fewest false triggers, most misses.
    Strict,
    #[default]
    Balanced,
    /// Widest tolerance. For noisy rooms or a strong accent.
    Lenient,
}

// Hand-written so an unknown string (or a non-string) falls back to the
// default instead of failing the WHOLE settings load — settings.json is
// hand-editable, and one typo here must not brick every other setting.
impl<'de> Deserialize<'de> for Sensitivity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d).unwrap_or_default().as_str() {
            "strict" => Self::Strict,
            "lenient" => Self::Lenient,
            _ => Self::Balanced,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct WakeWordSettings {
    /// Off by default: an always-on microphone is never something to opt a
    /// user into silently.
    pub enabled: bool,
    pub phrase: String,
    /// Whisper model id for detection ONLY — deliberately separate from
    /// `Settings::model` so a large dictation model is never run every
    /// couple of seconds.
    pub model: String,
    pub sensitivity: Sensitivity,
}

impl Default for WakeWordSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            phrase: "hey claudy".into(),
            model: "tiny".into(),
            sensitivity: Sensitivity::Balanced,
        }
    }
}

// ---------------------------------------------------------------------------
// Pure detection logic
// ---------------------------------------------------------------------------

/// Lowercase word tokens, punctuation dropped. Keeps alphanumerics of any
/// script so non-English phrases tokenize the same way.
pub fn normalize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Edit distance over chars (not bytes, so accented text behaves).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut row = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            row[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(row[j] + 1);
        }
        std::mem::swap(&mut prev, &mut row);
    }
    prev[b.len()]
}

/// Characters of slop allowed on a phrase word of `len` chars.
///
/// Proportional rather than a fixed budget: longer words earn more slack,
/// because a two-character slip in "claudy" is still obviously the wake word
/// while the same slip in "hi" is a different word entirely. Balanced allows
/// a third of the word to be wrong, which covers what whisper actually
/// returns ("cloudy", "claude", "claudie", "clody") without reaching
/// genuinely different words ("clarity" is three edits away).
pub fn tolerance_for(sensitivity: Sensitivity, len: usize) -> usize {
    match sensitivity {
        Sensitivity::Strict => 0,
        Sensitivity::Balanced => len / 3,
        Sensitivity::Lenient => len / 3 + 1,
    }
}

/// Does `transcript` contain `phrase`, allowing for whisper's mishearings?
/// Slides a phrase-length window over the transcript's words so the phrase
/// still matches when it is surrounded by other speech.
pub fn matches_phrase(transcript: &str, phrase: &str, sensitivity: Sensitivity) -> bool {
    let phrase_words = normalize(phrase);
    if phrase_words.is_empty() {
        return false;
    }
    let words = normalize(transcript);
    if words.len() < phrase_words.len() {
        return false;
    }
    (0..=words.len() - phrase_words.len()).any(|start| {
        phrase_words
            .iter()
            .zip(&words[start..])
            .all(|(p, w)| levenshtein(p, w) <= tolerance_for(sensitivity, p.chars().count()))
    })
}

/// Advice for a phrase the user typed, or `None` when it looks usable.
pub fn phrase_warning(phrase: &str) -> Option<String> {
    let words = normalize(phrase);
    if words.is_empty() {
        return Some("Enter a wake phrase.".into());
    }
    if words.len() < 2 || words.iter().map(|w| w.chars().count()).sum::<usize>() < 6 {
        return Some(
            "Short phrases trigger by accident — use at least two words, like \"hey claudy\"."
                .into(),
        );
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VadPhase {
    #[default]
    Silent,
    Speaking,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VadAction {
    /// Keep buffering.
    Wait,
    /// Run whisper over the current window.
    Evaluate,
}

/// Pure VAD transition. `silent_ms` is time since the last loud frame,
/// `speaking_ms` how long the current utterance has run.
///
/// The caller MUST restart its `speaking_ms` clock whenever this returns
/// `Evaluate`; otherwise continuous speech would re-evaluate every tick.
pub fn on_frame(
    phase: VadPhase,
    is_speech: bool,
    silent_ms: u64,
    speaking_ms: u64,
) -> (VadPhase, VadAction) {
    match phase {
        VadPhase::Silent if is_speech => (VadPhase::Speaking, VadAction::Wait),
        VadPhase::Silent => (VadPhase::Silent, VadAction::Wait),
        VadPhase::Speaking if !is_speech && silent_ms >= SILENCE_MS => {
            (VadPhase::Silent, VadAction::Evaluate)
        }
        VadPhase::Speaking if speaking_ms >= MAX_SPEECH_MS => {
            (VadPhase::Speaking, VadAction::Evaluate)
        }
        VadPhase::Speaking => (VadPhase::Speaking, VadAction::Wait),
    }
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Append to the rolling window, keeping only the newest `cap` samples.
pub fn push_window(window: &mut VecDeque<f32>, samples: &[f32], cap: usize) {
    window.extend(samples.iter().copied());
    while window.len() > cap {
        window.pop_front();
    }
}

// ---------------------------------------------------------------------------
// Listener
// ---------------------------------------------------------------------------

struct Listener {
    stop_tx: mpsc::Sender<()>,
    join: JoinHandle<()>,
}

/// Everything the listener thread needs, snapshotted at start so it never
/// touches the settings store while running.
struct ListenConfig {
    device: String,
    phrase: String,
    sensitivity: Sensitivity,
    model_path: PathBuf,
    language: String,
    notifications_enabled: bool,
}

#[derive(Default)]
pub struct WakeState {
    listener: Mutex<Option<Listener>>,
    /// Serializes reconcile so two rapid state changes can't both act on a
    /// stale view of whether a listener is running.
    op: tokio::sync::Mutex<()>,
}

impl WakeState {
    pub fn is_listening(&self) -> bool {
        self.listener.lock().map(|l| l.is_some()).unwrap_or(false)
    }
}

#[derive(Clone, Serialize)]
struct WakeEvent {
    phase: &'static str,
    message: Option<String>,
}

fn emit(app: &AppHandle, phase: &'static str, message: Option<String>) {
    let _ = app.emit("wake-state", WakeEvent { phase, message });
}

fn publish(app: &AppHandle, phase: &'static str, message: Option<String>) {
    emit(app, phase, message);
    tray::refresh(app);
}

/// Should a listener be running right now? Everything that wants the
/// microphone — dictation, the ask box, the mic test — wins over the wake
/// word, which also keeps Claudy from hearing its own spoken answers.
fn should_listen(app: &AppHandle) -> bool {
    let Ok(settings) = config::load(app) else {
        return false;
    };
    if !settings.assistant.wake_word.enabled {
        return false;
    }
    let dictation_idle = app
        .state::<dictation::DictationState>()
        .phase
        .lock()
        .map(|p| *p == dictation::Phase::Idle)
        .unwrap_or(false);
    let assistant_idle = app
        .state::<assistant::AssistantState>()
        .phase
        .lock()
        .map(|p| *p == assistant::Phase::Idle)
        .unwrap_or(false);
    let mic_free = app
        .state::<audio::AudioState>()
        .0
        .lock()
        .map(|slot| slot.is_none())
        .unwrap_or(false);
    dictation_idle && assistant_idle && mic_free
}

/// Start or stop the listener to match the app's current state. Idempotent,
/// cheap to over-call, and safe from any thread — the work happens on a
/// spawned task so a caller on the main event loop is never blocked.
pub fn reconcile(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<WakeState>();
        let _op = state.op.lock().await;
        // Re-read the desired state INSIDE the lock: if two reconciles queue
        // up, whichever runs last settles on the truth.
        let want = should_listen(&app);
        if want == state.is_listening() {
            return;
        }
        if want {
            start(&app);
        } else {
            stop(&app);
        }
    });
}

/// Rebuild the listener against freshly saved settings. `reconcile` alone
/// can't do this: it only compares running-vs-wanted, and a phrase, model, or
/// microphone change leaves both sides "on" while the running thread still
/// holds the OLD snapshot.
pub fn restart(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<WakeState>();
        let _op = state.op.lock().await;
        if state.is_listening() {
            stop(&app);
        }
        if should_listen(&app) {
            start(&app);
        }
    });
}

fn start(app: &AppHandle) {
    let settings = match config::load(app) {
        Ok(s) => s,
        Err(e) => {
            publish(app, "error", Some(format!("Could not load settings: {e}")));
            return;
        }
    };
    let wake = &settings.assistant.wake_word;
    let model_path = match stt::model_path_for(&settings.models_dir_override, &wake.model) {
        Ok(p) => p,
        Err(e) => {
            // Same treatment as dictation's missing-model preflight: say what's
            // wrong and put the user on the page that fixes it.
            publish(app, "error", Some(e.clone()));
            crate::notify::send(app, true, &format!("Wake word off — {e}"));
            tray::show_main(app);
            let _ = app.emit_to("main", "navigate", "transcription");
            return;
        }
    };

    let cfg = ListenConfig {
        device: settings.mic_device.clone(),
        phrase: wake.phrase.clone(),
        sensitivity: wake.sensitivity,
        model_path,
        language: stt::normalize_language(&settings.language).to_string(),
        notifications_enabled: settings.notifications_enabled,
    };

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread_app = app.clone();
    let join = std::thread::spawn(move || listen_loop(thread_app, cfg, stop_rx));
    *app.state::<WakeState>().listener.lock().unwrap() = Some(Listener { stop_tx, join });
    publish(app, "listening", None);
}

fn stop(app: &AppHandle) {
    let listener = app.state::<WakeState>().listener.lock().unwrap().take();
    if let Some(l) = listener {
        let _ = l.stop_tx.send(());
        let _ = l.join.join();
    }
    publish(app, "off", None);
}

/// Owns the capture stream and the rolling window for as long as it runs.
/// Both are dropped on the way out, so stopping the listener releases the
/// microphone and discards every buffered sample.
fn listen_loop(app: AppHandle, cfg: ListenConfig, stop_rx: mpsc::Receiver<()>) {
    use cpal::traits::StreamTrait;

    let fail = |message: String| {
        emit(&app, "error", Some(message.clone()));
        crate::notify::send(
            &app,
            cfg.notifications_enabled,
            &format!("Wake word off — {message}"),
        );
    };

    let parts = match audio::build_stream(&cfg.device) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    if let Err(e) = parts.stream.play() {
        return fail(format!("Could not start microphone: {e}"));
    }
    let mut engine = match WhisperEngine::load(&cfg.model_path) {
        Ok(e) => e,
        Err(e) => return fail(e),
    };

    let mut window: VecDeque<f32> = VecDeque::with_capacity(WINDOW_SAMPLES);
    let mut phase = VadPhase::Silent;
    let start_time = Instant::now();
    let mut last_loud = start_time;
    let mut speech_started = start_time;
    let mut last_trigger: Option<Instant> = None;

    loop {
        match stop_rx.recv_timeout(Duration::from_millis(TICK_MS)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        let raw = match parts.buffer.lock() {
            Ok(mut b) => std::mem::take(&mut *b),
            Err(_) => break, // poisoned capture buffer: nothing useful left to do
        };
        if raw.is_empty() {
            continue;
        }
        let mono = audio::downmix_to_mono(&raw, parts.channels);
        let samples = audio::resample_linear(&mono, parts.sample_rate, audio::TARGET_SAMPLE_RATE);
        push_window(&mut window, &samples, WINDOW_SAMPLES);

        let now = Instant::now();
        let is_speech = rms(&samples) >= SPEECH_RMS;
        if is_speech {
            last_loud = now;
        }
        let was_silent = phase == VadPhase::Silent;
        let (next, action) = on_frame(
            phase,
            is_speech,
            (now - last_loud).as_millis() as u64,
            (now - speech_started).as_millis() as u64,
        );
        if was_silent && next == VadPhase::Speaking {
            speech_started = now;
        }
        phase = next;
        if action == VadAction::Wait {
            continue;
        }
        // Re-arm the utterance clock, as `on_frame` requires.
        speech_started = now;

        if window.len() < MIN_EVAL_SAMPLES {
            window.clear();
            continue;
        }
        let heard: Vec<f32> = window.iter().copied().collect();
        window.clear();
        match engine.transcribe(&heard, &cfg.language) {
            // One bad window is not worth stopping over — keep listening.
            Err(e) => eprintln!("wake word transcription failed: {e}"),
            Ok(text) => {
                let past_cooldown = last_trigger
                    .map(|t| (now - t).as_millis() as u64 >= COOLDOWN_MS)
                    .unwrap_or(true);
                if past_cooldown && matches_phrase(&text, &cfg.phrase, cfg.sensitivity) {
                    last_trigger = Some(now);
                    trigger(&app);
                }
            }
        }
    }
}

/// Open the ask box already recording. Window work is dispatched to the main
/// thread — we're deep inside a plain worker thread here.
fn trigger(app: &AppHandle) {
    emit(app, "triggered", None);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(e) = assistant::show_input_voice(&handle) {
            eprintln!("wake word could not open the ask box: {e}");
        }
    });
}

/// Startup hook: brings the listener up if it is enabled in settings.
pub fn init(app: &AppHandle) {
    reconcile(app);
}

#[tauri::command]
pub fn wake_word_listening(state: tauri::State<'_, WakeState>) -> Result<bool, String> {
    Ok(state.is_listening())
}

/// Live advice for the phrase editor: `""` when the phrase looks usable.
#[tauri::command]
pub fn check_wake_phrase(phrase: String) -> Result<String, String> {
    Ok(phrase_warning(&phrase).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRICT: Sensitivity = Sensitivity::Strict;
    const BALANCED: Sensitivity = Sensitivity::Balanced;
    const LENIENT: Sensitivity = Sensitivity::Lenient;

    #[test]
    fn normalize_lowercases_and_drops_punctuation() {
        assert_eq!(normalize("Hey, Claudy!"), vec!["hey", "claudy"]);
        assert_eq!(normalize("  "), Vec::<String>::new());
    }

    #[test]
    fn normalize_keeps_non_ascii_letters() {
        assert_eq!(normalize("Xin chào!"), vec!["xin", "chào"]);
    }

    #[test]
    fn levenshtein_counts_character_edits() {
        assert_eq!(levenshtein("claudy", "claudy"), 0);
        assert_eq!(levenshtein("claudy", "cloudy"), 1);
        assert_eq!(levenshtein("claudy", "claude"), 1);
        assert_eq!(levenshtein("", "hey"), 3);
    }

    #[test]
    fn exact_phrase_matches_at_every_sensitivity() {
        for s in [STRICT, BALANCED, LENIENT] {
            assert!(matches_phrase("hey claudy", "hey claudy", s), "{s:?}");
        }
    }

    #[test]
    fn punctuation_and_case_never_block_a_match() {
        assert!(matches_phrase("Hey, Claudy!", "hey claudy", BALANCED));
    }

    #[test]
    fn common_whisper_mishearings_still_match() {
        // The whole reason the match is fuzzy — these are what whisper
        // actually returns for "hey claudy".
        for heard in ["hey cloudy", "hey claude", "hey claudie", "hey clody"] {
            assert!(
                matches_phrase(heard, "hey claudy", BALANCED),
                "should have matched: {heard}"
            );
        }
    }

    #[test]
    fn phrase_matches_when_surrounded_by_other_speech() {
        assert!(matches_phrase(
            "so anyway hey claudy what is the weather",
            "hey claudy",
            BALANCED
        ));
    }

    #[test]
    fn unrelated_speech_does_not_match() {
        for heard in ["hey there", "hey everyone", "the cloud is down"] {
            assert!(
                !matches_phrase(heard, "hey claudy", BALANCED),
                "should NOT have matched: {heard}"
            );
        }
    }

    #[test]
    fn transcript_shorter_than_the_phrase_never_matches() {
        assert!(!matches_phrase("hey", "hey claudy", LENIENT));
        assert!(!matches_phrase("", "hey claudy", LENIENT));
    }

    #[test]
    fn an_empty_phrase_never_matches_anything() {
        // Guards the always-on path: a blank phrase must not fire on silence.
        assert!(!matches_phrase("hey claudy", "", LENIENT));
        assert!(!matches_phrase("anything at all", "  !!  ", LENIENT));
    }

    #[test]
    fn strict_rejects_the_mishearings_balanced_accepts() {
        assert!(!matches_phrase("hey cloudy", "hey claudy", STRICT));
        assert!(matches_phrase("hey cloudy", "hey claudy", BALANCED));
    }

    #[test]
    fn lenient_accepts_a_wider_slip_than_balanced() {
        // "clarity" is three edits from "claudy" — a genuinely different word
        // that only the widest setting should let through.
        assert!(!matches_phrase("hey clarity", "hey claudy", BALANCED));
        assert!(matches_phrase("hey clarity", "hey claudy", LENIENT));
    }

    #[test]
    fn tolerance_grows_with_word_length_and_sensitivity() {
        assert_eq!(tolerance_for(STRICT, 10), 0, "strict never allows a slip");
        assert_eq!(tolerance_for(BALANCED, 2), 0, "short words must be exact");
        assert_eq!(tolerance_for(BALANCED, 3), 1);
        assert_eq!(tolerance_for(BALANCED, 6), 2);
        assert_eq!(tolerance_for(BALANCED, 10), 3);
        assert_eq!(tolerance_for(LENIENT, 3), 2);
        assert_eq!(tolerance_for(LENIENT, 10), 4);
    }

    #[test]
    fn phrase_warning_flags_phrases_that_would_misfire() {
        assert!(phrase_warning("hey claudy").is_none());
        assert!(phrase_warning("ok computer").is_none());
        assert!(phrase_warning("").is_some(), "empty");
        assert!(phrase_warning("   ").is_some(), "whitespace only");
        assert!(phrase_warning("claudy").is_some(), "single word");
        assert!(phrase_warning("a b").is_some(), "too few characters");
    }

    #[test]
    fn silence_stays_silent_and_never_evaluates() {
        assert_eq!(
            on_frame(VadPhase::Silent, false, 10_000, 0),
            (VadPhase::Silent, VadAction::Wait)
        );
    }

    #[test]
    fn speech_starts_an_utterance_without_evaluating_yet() {
        assert_eq!(
            on_frame(VadPhase::Silent, true, 0, 0),
            (VadPhase::Speaking, VadAction::Wait)
        );
    }

    #[test]
    fn a_pause_after_speech_ends_the_utterance_and_evaluates() {
        assert_eq!(
            on_frame(VadPhase::Speaking, false, SILENCE_MS, 900),
            (VadPhase::Silent, VadAction::Evaluate)
        );
    }

    #[test]
    fn a_brief_gap_mid_sentence_does_not_end_the_utterance() {
        assert_eq!(
            on_frame(VadPhase::Speaking, false, SILENCE_MS - 1, 900),
            (VadPhase::Speaking, VadAction::Wait)
        );
    }

    #[test]
    fn continuous_speech_is_evaluated_without_waiting_for_a_pause() {
        // "hey claudy what's the weather" said in one breath must still fire.
        assert_eq!(
            on_frame(VadPhase::Speaking, true, 0, MAX_SPEECH_MS),
            (VadPhase::Speaking, VadAction::Evaluate)
        );
    }

    #[test]
    fn rms_is_zero_for_silence_and_positive_for_signal() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 100]), 0.0);
        assert!(rms(&[0.5, -0.5, 0.5, -0.5]) > SPEECH_RMS);
    }

    #[test]
    fn the_window_keeps_only_the_newest_samples() {
        let mut w = VecDeque::new();
        push_window(&mut w, &[1.0, 2.0, 3.0], 4);
        push_window(&mut w, &[4.0, 5.0], 4);
        assert_eq!(w.iter().copied().collect::<Vec<f32>>(), vec![2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn the_window_never_exceeds_its_cap_even_on_a_huge_push() {
        // The bound is what keeps an always-on listener from growing forever.
        let mut w = VecDeque::new();
        push_window(&mut w, &vec![0.1; WINDOW_SAMPLES * 3], WINDOW_SAMPLES);
        assert_eq!(w.len(), WINDOW_SAMPLES);
    }

    #[test]
    fn wake_word_is_off_by_default() {
        let d = WakeWordSettings::default();
        assert!(!d.enabled, "an always-on microphone must be opt-in");
        assert_eq!(d.phrase, "hey claudy");
        assert_eq!(d.model, "tiny", "detection must not default to a large model");
        assert_eq!(d.sensitivity, Sensitivity::Balanced);
    }

    #[test]
    fn sensitivity_serializes_lowercase_for_the_frontend() {
        assert_eq!(serde_json::to_string(&Sensitivity::Lenient).unwrap(), "\"lenient\"");
    }

    #[test]
    fn settings_round_trip_camel_case_and_fill_missing_with_defaults() {
        let json = serde_json::json!({ "enabled": true, "sensitivity": "strict" });
        let s: WakeWordSettings = serde_json::from_value(json).unwrap();
        assert!(s.enabled);
        assert_eq!(s.sensitivity, Sensitivity::Strict);
        assert_eq!(s.phrase, "hey claudy"); // missing -> default
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["sensitivity"], "strict");
    }

    #[test]
    fn an_unusable_sensitivity_does_not_fail_the_whole_load() {
        // A hand-edit typo must cost you the sensitivity, not the phrase too.
        for bad in [
            serde_json::json!("Strict"), // right word, wrong case
            serde_json::json!("nonsense"),
            serde_json::json!(7),
            serde_json::json!(null),
        ] {
            let json = serde_json::json!({ "phrase": "ok computer", "sensitivity": bad });
            let s: WakeWordSettings = serde_json::from_value(json)
                .unwrap_or_else(|e| panic!("sensitivity {bad} broke the load: {e}"));
            assert_eq!(s.sensitivity, Sensitivity::Balanced, "for {bad}");
            assert_eq!(s.phrase, "ok computer", "the rest must survive");
        }
    }
}
