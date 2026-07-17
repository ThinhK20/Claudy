# Building Claudy

Windows 11 is the development and verification target. The macOS and Linux
sections below are **code-level only and untested** — no hardware was
available for verification.

## Prerequisites — Windows (verified)

- Node.js ≥ 20, Rust (MSVC toolchain) ≥ 1.77
- Visual Studio Build Tools 2022 with the "Desktop development with C++" workload
- CMake (`winget install Kitware.CMake`) — required by whisper-rs
- LLVM (`winget install LLVM.LLVM`) — required by whisper-rs (bindgen);
  set `LIBCLANG_PATH=C:\Program Files\LLVM\bin`

## Prerequisites — macOS (untested)

- Node.js ≥ 20, Rust ≥ 1.77
- Xcode Command Line Tools (`xcode-select --install`)
- CMake (`brew install cmake`) — required by whisper-rs

## Prerequisites — Linux (untested)

- Node.js ≥ 20, Rust ≥ 1.77
- Tauri system libraries: `webkit2gtk-4.1`, `librsvg2`, and
  `libayatana-appindicator3` (tray support)
- `libxdo` — required by enigo (input simulation)
- `libasound2-dev` (ALSA) — required by cpal (audio capture)
- CMake + clang — required by whisper-rs
- A Secret Service provider (GNOME Keyring or KWallet) — required by the
  keyring crate for API key storage

## Run (development)

    npm install
    npm run tauri dev

## Release build (Windows / NSIS)

    npm run tauri build

The installer lands at
`src-tauri/target/release/bundle/nsis/Claudy_<version>_x64-setup.exe`.
It installs per-user (no admin prompt) and creates Start Menu and Desktop
shortcuts.

The binary is unsigned, so Windows SmartScreen shows an "unrecognized app"
warning on first run — click "More info" → "Run anyway". Code signing is a
future enhancement.

## Platform limitations

**macOS (untested):**
- Input simulation (dictation paste, selection probe) requires the
  Accessibility permission: System Settings → Privacy & Security →
  Accessibility.
- Synthetic copy/paste chords use Cmd (handled automatically at compile
  time), but the **default dictation shortcut is `Ctrl+Shift+D` on every
  platform** — macOS users should rebind it in Settings.

**Linux (untested):**
- Global shortcuts and input injection require an X11 session; on Wayland
  they are restricted or unavailable.
- The tray icon requires an appindicator implementation
  (`libayatana-appindicator3`).

## Whisper models

Models are stored in the project-scope `models/` directory only (gitignored).
Download them from the app's Transcription page — never commit them.
