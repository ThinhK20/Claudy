# Claudy — Cross-Platform AI Desktop Assistant: Design Spec

**Date:** 2026-07-12
**Status:** Approved by user (pending spec review)

## Overview

Claudy is a lightweight, privacy-first desktop productivity assistant built with Tauri 2.x. It provides two core capabilities available system-wide via global keyboard shortcuts:

1. **Global dictation** — offline speech-to-text that types into any focused application.
2. **AI prompt shortcuts** — user-defined prompts that run against selected text anywhere on the system, with results delivered to the clipboard.

Primary target: Windows 11 (development and verification). Code remains cross-platform (macOS/Linux compatibility passes deferred to the final phase).

## Decisions Locked

| Decision | Choice |
|---|---|
| STT engine | whisper.cpp via `whisper-rs`, behind an `SttEngine` trait |
| Platform priority | Windows first; macOS/Linux later compatibility pass |
| MVP scope | Both core features (dictation + AI shortcuts), built in phases |
| Frontend | React + TypeScript + Vite + Tailwind + shadcn/ui |
| Architecture | Rust-core monolith; webview UI is purely presentational |
| Model storage | **Project-scope `models/` directory only** — never AppData or other user-profile locations. Gitignored. Path configurable in settings but defaults to `<project root>/models`. Rationale: avoid hidden storage burden on users. |
| Text injection | Clipboard-paste strategy (save clipboard → set text → simulate Ctrl+V → restore), not per-key typing |
| API key storage | OS credential store only (`keyring` crate — Windows Credential Manager), never in JSON |

## Architecture

Single Tauri 2.x process. All system-level functionality lives in Rust and works with every window closed (tray-first app). The frontend renders three surfaces: main window, recording overlay, tray menu.

### Tech stack

- **Shell:** Tauri 2.x + plugins: `global-shortcut`, `clipboard-manager`, `notification`, `autostart`, `store`, `single-instance`
- **Rust:** `cpal` (audio), `whisper-rs` (STT), `enigo` (input simulation), `keyring` (secrets), `reqwest` + `tokio` (AI HTTP), `serde`
- **Frontend:** React, TypeScript, Vite, Tailwind, shadcn/ui, Zustand

### Rust modules

| Module | Responsibility |
|---|---|
| `audio` | Mic enumeration, 16kHz mono PCM capture, level metering, device-error handling |
| `stt` | `SttEngine` trait + `WhisperEngine`; language selection; model load/unload (keep-warm configurable) |
| `models` | Whisper model download manager (Hugging Face, progress events, checksum, resume) into project `models/` dir |
| `shortcuts` | Accelerator → action registry (dictation toggle, prompt IDs); conflict detection; live re-registration |
| `selection` | Read selected text: save clipboard → simulate Ctrl+C → read → restore clipboard |
| `inject` | Insert text via clipboard-paste strategy |
| `prompts` | Prompt CRUD + JSON persistence + template rendering: `{{selected_text}}`, `{{clipboard}}`, `{{date}}`, `{{time}}` |
| `ai` | `AiProvider` trait; impls: `openai_compatible` (OpenAI/LM Studio/Azure/local servers), `ollama`, `anthropic`, `gemini`. New provider = one new file |
| `config` | Typed settings over store plugin; secrets delegated to keyring |
| `notify` | Desktop notifications respecting user preferences |
| `overlay` | Recording pill window lifecycle (show/hide/position) |

### Core flows

**Dictation:** global shortcut → overlay appears, recording starts → shortcut again → overlay shows "transcribing" → whisper on worker thread → text injected into still-focused app → overlay hides.

**Prompt shortcut:** shortcut → read selection → if empty: notification, abort → render template → call provider → result to clipboard → success notification. Original text never overwritten; auto-paste is an opt-in setting (default off).

### UI surfaces (shadcn/ui)

- **Main window** — sidebar nav: Prompts (search, enable toggle, CRUD, duplicate, import/export JSON), Transcription (model picker + downloads, language, mic picker with live level meter), Providers (per-provider config, API keys, connection test), Settings (theme, autostart, notifications, clipboard/auto-paste, shortcut editor with conflict warnings). Close = hide to tray.
- **Overlay** — small transparent always-on-top pill, skip-taskbar; mic icon + live level while recording, spinner while transcribing, error state on failure.
- **Tray** — state-reflecting icon (idle/recording); menu: open settings, toggle dictation, quit.

## Storage & Privacy

- `settings.json` + `prompts.json` via store plugin (small, human-readable, import/export-friendly).
- Whisper models: project `models/` directory only (see Decisions).
- API keys: OS credential store only.
- Audio processed in-memory, never written to disk (except opt-in debug setting).
- Zero telemetry. No network traffic except user-configured AI providers and explicit model downloads.

## Error Handling

- Mic absent/busy → overlay error state + notification.
- Model not downloaded → notification deep-linking to model download page.
- Provider failure → notification with reason (timeout, auth, rate-limit).
- Shortcut registration conflict → surfaced in shortcut editor UI.
- No silent failures: every user-triggered action ends in visible success or visible error.

## Testing

- Rust unit tests: template engine, config round-trip, provider request construction (mock HTTP server), shortcut registry logic.
- Frontend: component tests for prompt manager CRUD and settings forms.
- Manual E2E checklist per phase for OS-integration behavior (injection, shortcuts, overlay) that can't be automated cheaply on Windows.

## Build Phases

1. **Scaffold** — Tauri 2 + React + shadcn, tray, single-instance, window shell, config service.
2. **Audio + STT** — capture, whisper-rs, model download manager, Transcription settings page.
3. **Dictation E2E** — global shortcut, overlay window, paste injection. *First success criterion met.*
4. **AI layer** — provider trait + 4 providers, prompt engine, selection reading, notifications. *Prompt shortcuts work.*
5. **Management UI** — prompt manager, shortcut manager, provider settings, import/export.
6. **Polish** — theme, autostart, NSIS packaging, macOS/Linux compatibility passes.

## Known Risks

- `whisper-rs` requires CMake + LLVM on Windows — documented setup step in phase 2.
- Keystroke/paste injection may trip antivirus heuristics — paste strategy minimizes surface; code signing later.
- Elevated (admin) target apps ignore injection from non-elevated processes — documented limitation.
- Wayland restricts global shortcuts/injection — isolated behind `shortcuts`/`inject` module boundaries for the later Linux pass.

## Future Enhancements (architecture must not block)

OCR, screen capture, translation shortcuts, AI chat window, clipboard history, prompt marketplace/chaining, voice commands, real-time transcription (via `SttEngine` trait), text expansion, workflow automation, plugin system.
