# Claudy

Claudy is a privacy-first desktop assistant that lives in your system tray and adds three system-wide superpowers to any application via global keyboard shortcuts:

- **Offline voice dictation** — local speech-to-text (whisper.cpp) that types straight into whatever app has focus. Audio never leaves your machine.
- **AI prompt shortcuts** — user-defined prompts that transform the text you have selected in any app, powered by the AI provider of your choice. The result lands on your clipboard (auto-paste is opt-in); your original selection is never overwritten.
- **Quick-ask voice assistant** — a global shortcut opens a compact ask box at your cursor; the answer appears in a floating panel and can be read aloud by a local neural voice (Kokoro TTS). Supported providers can search the web to answer.

Built with Tauri 2 and React. Windows-first — macOS and Linux code paths exist but are untested.

## Features

### Dictation
- Global dictation toggle (default `Ctrl+Shift+D`) with a floating recording-pill overlay
- Fully offline transcription via whisper.cpp — audio is processed in-memory, never written to disk or sent anywhere
- In-app Whisper model manager: download models from Hugging Face with progress and checksum verification, or delete them
- Microphone selection and a built-in test recorder

### AI prompt shortcuts
- Create prompts with their own global shortcuts and templates using `{{selected_text}}`, `{{clipboard}}`, `{{date}}`, and `{{time}}`
- Works on selected text in any application: shortcut → capture selection → run prompt → result to clipboard → notification
- Prompt manager with search, enable/disable, run, delete, and JSON import/export
- Multiple AI providers behind one interface: **Anthropic**, **OpenAI-compatible** (OpenAI, Groq, etc.), **Google Gemini**, and **Ollama** (local)
- Per-provider configuration with connection testing; API keys are stored in the OS credential store (Windows Credential Manager / Keychain / Secret Service), never in config files

### Quick-ask voice assistant
- Global shortcut (default `Ctrl+Shift+Space`) opens a focused ask box anchored at your cursor
- Answers appear in a floating panel: copy, ask a follow-up, replay, or let it auto-close
- Optional spoken answers via **local Kokoro TTS** (int8 ONNX, ~115 MB) — fully offline, with selectable voice, speed, and volume; download the voice model on demand from Settings
- Provider-native web search when enabled: Anthropic `web_search` and Gemini Google Search grounding (silently skipped by providers without it)
- TTS is best-effort: if the voice model is missing or synthesis fails, the written answer still shows with an inline note

### App
- Runs from the system tray; closing the window hides it instead of quitting
- Settings for shortcuts (with conflict detection and a shortcut recorder), theme, autostart, and auto-paste
- Every prompt-flow exit path produces a notification — no silent failures

## Getting started

### Prerequisites (Windows)

- Node.js ≥ 20
- Rust (MSVC toolchain) ≥ 1.77
- Visual Studio Build Tools 2022 with the Desktop C++ workload
- CMake and LLVM (required by whisper-rs) — set `LIBCLANG_PATH=C:\Program Files\LLVM\bin`

See [docs/BUILDING.md](docs/BUILDING.md) for full setup details, macOS/Linux prerequisites, and platform limitations.

### Development

```sh
npm install
npm run tauri dev
```

### Release build

```sh
npm run tauri build
```

Produces a per-user NSIS installer (no admin prompt) at `src-tauri/target/release/bundle/nsis/Claudy_<version>_x64-setup.exe`. The installer is unsigned, so Windows SmartScreen will warn on first run.

After installing, download a Whisper model from the Transcription page before using dictation. Models are stored locally in the project `models/` directory.

## Tech stack

- **Frontend:** React 19, TypeScript, Vite 7, Tailwind CSS v4, shadcn/ui + Radix UI, Zustand
- **Shell:** Tauri 2 (global-shortcut, clipboard-manager, notification, autostart, store, single-instance plugins)
- **Rust core:** whisper-rs (STT), kokoro-tts + ort/ONNX Runtime (local TTS), cpal (audio capture) + rodio (playback), enigo (input simulation), keyring (secrets), reqwest + tokio (AI providers)

## Project layout

```
src/                React UI (pages, components, Tauri API bridges in src/lib)
src-tauri/src/      Rust core: audio, stt, models, shortcuts, selection,
                    inject, prompts, prompt_flow, tray, overlay, assistant,
                    ai/ providers, tts/ (kokoro engine + playback)
docs/BUILDING.md    Platform build instructions
docs/superpowers/   Design spec and phase-by-phase implementation plans
models/             Locally downloaded Whisper models (gitignored)
```

## Privacy

- Zero telemetry
- Dictation audio is processed in-memory and never leaves your machine
- API keys live only in the OS credential store
- Selected text is sent only to the AI provider you configured, only when you trigger a prompt

## Documentation

- [Building & platform notes](docs/BUILDING.md)
- [Design spec](docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md)
- Implementation plans: `docs/superpowers/plans/`
