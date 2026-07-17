# AI Voice Assistant with Local Kokoro TTS

## Context

Claudy currently has two flows: offline dictation (whisper.cpp) and AI prompt shortcuts (selection → AI → clipboard). This feature adds a third: a **quick-ask assistant** — global shortcut → compact chat input at the cursor → AI answer (provider-native web search when available) → floating response panel + spoken audio via **local Kokoro TTS** (82M int8 ONNX, ~88 MB model + ~27 MB voices).

User decisions locked in:
- **TTS**: local Kokoro via ONNX Runtime, behind a `TtsEngine` trait (open-source, offline, cross-platform — no Windows Speech dependency). Crate: **`kokoro-tts` 0.3.3** (`mzdk100/kokoro`, Apache-2.0, `use-cmudict` feature = pure-Rust G2P, **no system espeak-ng needed**; pins `ort 2.0.0-rc.12` which auto-downloads ONNX Runtime binaries — no new cmake requirement).
- **Web search**: provider-native — Anthropic `web_search` server tool + Gemini Google Search grounding; unavailable (silent no-op) for ollama/openai_compatible.
- Non-streaming v1, each question independent, implement directly on `main`.

Everything else reuses existing architecture: shortcut lifecycle (`shortcuts.rs`), cursor/monitor math (`overlay.rs`), AI provider layer (`ai/*`), keyring secrets, store-backed settings (`config.rs` + `settings-store.ts`), model download infra (`download.rs`), window-label routing (`App.tsx`), one-module-per-domain IPC wrappers (`src/lib/*.ts`).

## Architecture at a glance

- **One new static window** `assistant` (transparent, alwaysOnTop, skipTaskbar, undecorated, hidden at start) that morphs input (~420×150) → loading/panel (~460×380) via `set_size`. Unlike `overlay` it stays **focusable**; `show` calls `set_focus()`.
- New Rust modules: `assistant.rs` (flow/state/window/commands), `tts/mod.rs` (trait + asset catalog + chunking), `tts/kokoro.rs` (engine), `tts/playback.rs` (rodio).
- IPC: commands `ask_assistant, close_assistant, assistant_new_question, get_assistant_state, stop_assistant_speech, replay_assistant_speech, download_tts_model, tts_model_status, delete_tts_model`; event `assistant-state { phase: idle|input|loading|answering|speaking|error, question?, answer?, message?, ttsError? }`; reuse `model-download-progress`.

## Phase 1 — Window, shortcut, ask → text answer

1. **`src-tauri/src/config.rs`**: add nested `AssistantSettings` (serde `default` + `camelCase`, like `AiSettings`): `shortcut` ("Ctrl+Shift+Space"), `tts_voice` ("af_heart"), `speech_speed` (1.0), `volume` (1.0), `auto_speak` (true), `auto_web_search` (false), `panel_close_secs` (15, 0=never), `keep_open_while_speaking` (true). In `update_settings`, mirror the dictation-shortcut re-registration branch for `assistant.shortcut` (restructure so both shortcuts changing in one call works: collect re-registrations → save once → `sync_prompts`).
2. **`src-tauri/src/shortcuts.rs`**: `register_assistant(app, old, new)` modeled on `register_dictation`, firing `assistant::toggle` on `Pressed`. Include in `register_all` (suspend/resume then work for free). Change `desired_prompt_bindings` to take labeled reserved pairs `[("dictation", accel), ("assistant", accel)]`; `check_shortcut` gains optional `for_assistant` param; update existing tests.
3. **`src-tauri/src/assistant.rs`** (new):
   - `AssistantState { phase: Mutex<Phase>, generation: AtomicU64, last_answer: Mutex<Option<String>> }` managed in `lib.rs`. Generation bump on ask/close drops stale results.
   - Pure helper `anchor_at_cursor(cursor, work_pos, work_size, window, offset) -> (i32, i32)` — cursor-anchored, clamped to work area, flips above/left of cursor near bottom/right edges (sibling of `overlay::bottom_center`, unit-tested).
   - `toggle` (hidden→`show_input`, visible→`close`); `show_input` uses the `overlay::show` monitor chain (`cursor_position` → `monitor_from_point` → `primary_monitor`), logical→physical via `scale_factor()`, then `set_size` + `set_position` + `show` + `set_focus`, publish `input`.
   - `ask(app, question)`: publish `loading`, resize to panel size, `async_runtime::spawn` → `ai::complete_with_options` → publish `answering { answer }` (generation-checked) or `error { message }`. New question while loading just bumps generation.
   - `close`: bump generation, stop playback (Phase 3), hide, publish `idle`. `publish` mirrors `dictation::publish`.
4. **`src-tauri/src/ai/mod.rs`**: `RequestOptions { web_search: bool }`; defaulted trait methods `supports_web_search() -> false` and `build_request_with(...) -> build_request(...)`; `complete_with_options(app, prompt, opts)` (same body as `complete_with` but using `build_request_with`). `complete`/`complete_with` delegate — zero behavior change for `prompt_flow`.
5. **Config files**: add `assistant` window to `tauri.conf.json` `app.windows` (420×150, transparent, alwaysOnTop, skipTaskbar, decorations:false, resizable:false, shadow:false, visible:false, focus:false); add `"assistant"` to `src-tauri/capabilities/default.json` windows.
6. **Frontend**:
   - `src/App.tsx`: branch `label === "assistant"` → new `src/windows/AssistantPage.tsx`.
   - `src/lib/assistant-api.ts`: typed invoke/listen wrappers (pattern: `dictation-api.ts`).
   - `AssistantPage.tsx`: paints its own opaque rounded background (html/body/#root are transparent). Input phase: auto-focused textarea (refocus on `input` event + rAF retry — WebView2 quirk), Enter submits, Shift+Enter newline, Escape → `closeAssistant`. Loading: question echo + spinner. Answering: scrollable `whitespace-pre-wrap` answer + actions (Copy via clipboard plugin — permission already granted; Ask another → `assistant_new_question`; Close). **Auto-close timer lives here**: starts on `answering`, paused on hover/keydown/scroll and while speaking (when `keepOpenWhileSpeaking`), 0 disables, fires `closeAssistant`.
   - `src/lib/settings-store.ts`: mirror `AssistantSettings`.
   - `src/pages/SettingsPage.tsx`: new "Assistant" Card — `ShortcutInput` (reused; thread a `forAssistant` prop through `shortcut-input.tsx`/`shortcuts-api.ts`), auto-web-search toggle, auto-close duration Select.

## Phase 2 — Provider-native web search

1. **`ai/anthropic.rs`**: `supports_web_search() -> true`; `build_request_with` adds `"tools": [{"type": "web_search_20250305", "name": "web_search", "max_uses": 3}]` (verify current tool version string against Anthropic docs at implementation). **Fix `parse_response` unconditionally** to concatenate all `type=="text"` content blocks (web-search responses interleave `server_tool_use`/`web_search_tool_result`/`text`); citations ignored in v1.
2. **`ai/gemini.rs`**: `supports_web_search() -> true`; adds `"tools": [{"google_search": {}}]`; `parse_response` concatenates all `candidates[0].content.parts[*].text`.
3. **`assistant.rs`**: `web_search = settings.assistant.auto_web_search && provider.supports_web_search()`.

## Phase 3 — Local Kokoro TTS

1. **`Cargo.toml`**: `kokoro-tts = { version = "0.3.3", features = ["use-cmudict"] }` (pin exact), `rodio` (default-features off, `playback`). **Compile-check rodio's transitive cpal early** — repo pins `cpal 0.17` because 0.18.1 mixes windows-core versions; two cpal majors can coexist, but if it breaks, designated fallback: hand-rolled playback on existing cpal 0.17 (`tts/playback.rs`: output stream over `Arc<Mutex<VecDeque<f32>>>` + AtomicBool stop + atomic volume, ~100 lines).
2. **`tts/mod.rs`**: `TtsEngine { id, synth(text, voice, speed) -> TtsAudio { samples: Vec<f32>, sample_rate } }` trait (blocking; run in `spawn_blocking`); `TtsState { engine: lazy Option<Arc<dyn TtsEngine>>, playback, last_audio }`; pure helpers `chunk_text` (sentence-boundary ≤ ~300 chars so speech starts after the first sentence) and `speakable_text` (strip markdown: code fences → "code omitted", headings/emphasis/links → plain); asset catalog `KOKORO_ASSETS` (int8 onnx + voices-v1.0.bin from the `mzdk100/kokoro` V1.0 GitHub release — pin exact URLs + sha1 during implementation); commands `tts_model_status`, `download_tts_model`, `delete_tts_model`. Assets live in `models::resolve_dir` alongside Whisper models.
3. **`tts/kokoro.rs`**: `KokoroEngine::load(model, voices)` (cached in `TtsState`); pure `voice_from_id("af_heart", speed) -> Voice::AfHeart(speed)` mapping (curated English voices, helpful unknown-id error). API verified: `KokoroTts::new(model, voices).await` → `tts.synth(text, voice).await -> (Vec<f32>, Duration)` at 24 kHz mono.
4. **`download.rs` refactor**: extract generic `download_file(app, id, url, dest, sha1: Option<&str>, cancel)` from `run_download` (keeps `.part` resume, Range, progress events, sha verify); whisper path becomes a thin wrapper; TTS downloads go through the same `Downloads` cancellation map (ids `kokoro-model`/`kokoro-voices`).
5. **`assistant.rs` speak flow**: after `answering`, if `auto_speak` && assets downloaded: load-or-get engine, `spawn_blocking` chunk-by-chunk synth → append to playback (publish `speaking` on first chunk, back to `answering` on drain/stop; generation check between chunks aborts stale speech; accumulate `last_audio` for replay). TTS failure is **non-fatal**: answer stays, `ttsError` inline note (also used for "Voice model not downloaded — see Settings"). `stop_assistant_speech`, `replay_assistant_speech` (replays `last_audio`, no re-synth); `close` stops playback.
6. **Frontend**: assistant-api additions; AssistantPage Stop/Replay buttons + `ttsError` note; SettingsPage Assistant card: voice Select (af_heart, af_bella, af_nicole, af_sarah, am_adam, am_michael, am_puck, bf_emma, bf_isabella, bm_george, bm_lewis), speed Select (0.75–2.0), volume slider (add `src/components/ui/slider.tsx` from installed radix-ui — no new dep), auto-speak + keep-open toggles, "Download voice model (~115 MB)" button with progress/cancel/delete (mirrors `model-manager.tsx`).

## Phase 4 — Polish & hardening

- Blur-dismiss: `on_window_event` in `lib.rs` for label `assistant` — `Focused(false)` closes only while phase == Input.
- `assistant_new_question`: resize back to input size, refocus, publish `input`.
- Error phase renders Retry (re-asks stored question). No OS notifications for assistant errors (panel is the surface).
- **Installer check**: verify `onnxruntime.dll` handling (ort dynamic vs static link); if dynamic, add to `tauri.conf.json > bundle > resources`. Build NSIS installer, install clean, run TTS.
- `cargo clippy`, full `cargo test`, `npm run build`, README touch-up.

## Tests (Rust-only, colocated `#[cfg(test)]`, per repo convention)

- `assistant::anchor_at_cursor`: default below-right placement, right/bottom edge flip, negative-origin monitor, oversized window no-panic.
- `config`: assistant defaults, camelCase round-trip, missing block → defaults.
- `shortcuts`: labeled reserved-pair conflicts (assistant + dictation accel both block prompts).
- `ai`: anthropic/gemini web-search body on/off, multi-block/multi-part parsing, default-impl passthrough for ollama/openai_compatible, one httpmock round-trip.
- `tts`: `chunk_text`, `speakable_text`, `voice_from_id`, asset catalog sanity; download refactor keeps existing sha1 tests green.

## Verification (end-to-end, per phase)

`cd src-tauri && cargo test` + `npm run build` after each phase; `cargo tauri dev`:
1. **P1**: shortcut opens popup at cursor (both monitors, screen edges), textarea focused immediately, Enter → answer panel, Escape closes and focus returns to previous app, shortcut editing in Settings live-re-registers, ShortcutInput capture suspend/resume restores assistant combo.
2. **P2**: "what happened in the news today" with auto-web-search on Anthropic and Gemini; ollama unchanged.
3. **P3**: download voice model from Settings (progress/cancel/resume), speech starts ~1–2 s after answer, stop/replay/voice/speed/volume effective, new question interrupts old speech, missing-model and synth-failure degrade to text.
4. **P4**: NSIS installer on clean profile, TTS works.

## Risks

1. **WebView2 focus** on freshly shown transparent window → frontend rAF refocus retry; if flaky, `set_focus` retry loop in Rust.
2. **Focus return on close** — usually automatic on Windows when a topmost window hides; if not, capture foreground HWND before show, restore on close (`#[cfg(windows)]`).
3. **rodio/cpal windows-core mixing** — early compile check; cpal-0.17 fallback designed.
4. **kokoro-tts G2P quality** (cmudict: numbers/acronyms weaker than espeak) — acceptable v1; `TtsEngine` is the swap point. Single-maintainer crate → pin version.
5. **ort is 2.0.0-rc** (pinned transitively) + DLL bundling → explicit Phase 4 verification.
6. Anthropic web-search tool version string / Gemini grounding config may have newer revisions — verify docs during Phase 2 (single-constant changes).

## Implementation status (2026-07-18)

All four phases implemented on `main`. `cargo test` (111 pass), `cargo clippy`, `cargo build`, and `npm run build` are green.

Resolved during implementation:
- **kokoro-tts API**: `KokoroTts::new(model, voices).await`, `synth(text, voice).await -> (Vec<f32>, Duration)` at 24 kHz; voice variants carry `f32` speed (Nicole's variant is `Voice::AfNicole`, not `AfNicola`).
- **Dependency risk gate passed**: `kokoro-tts` + `ort 2.0.0-rc.12` + `rodio 0.22` compile against the pinned `cpal 0.17.3` (single version, no windows-core mixing) — the cpal-0.17 fallback was not needed.
- **rodio 0.22 playback**: `DeviceSinkBuilder::open_default_sink()` → `Player::connect_new(sink.mixer())` → `SamplesBuffer::new(ChannelCount, SampleRate, Vec<f32>)`. cpal streams are `!Send` on Windows, so `tts/playback.rs` owns a dedicated thread controlled by a command channel + atomics (mirrors `audio.rs`).
- **Assets**: `thewh1teagle/kokoro-onnx` `model-files-v1.0` (`kokoro-v1.0.int8.onnx` ~88 MB, `voices-v1.0.bin` ~27 MB); URLs HEAD-verified. `sha1` is `None` (release assets ship no pinned hashes) — a corrupt file is rejected by the model loader and degrades to text.
- **ort links ONNX Runtime statically** — no `onnxruntime.dll` is produced, so no `tauri.conf.json > bundle > resources` change is required.

Requires manual verification (interactive / hardware-bound, not runnable headless here):
- P1 cursor anchoring across real monitors and screen edges; WebView2 focus on first show; focus return on close.
- P3 end-to-end: download the voice model from Settings, confirm speech starts ~1–2 s after the answer, and that stop/replay/voice/speed/volume all take effect.
- P4 NSIS installer on a clean profile, then run TTS (confirm no missing native DLL at runtime).
