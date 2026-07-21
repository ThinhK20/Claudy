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

`DirectML.dll` is a **load-time import** of `claudy.exe` (provided by ONNX
Runtime), so it must ship next to the exe or the installed app will not launch.
It is declared in `src-tauri/tauri.bundle.windows.conf.json`, which is applied
via `--config` only for Windows *installer* builds — never for `tauri dev`, so
the dev loop is unaffected.

To build a self-contained installer locally:

1. `cargo build --release --manifest-path src-tauri/Cargo.toml` once — this makes
   ONNX Runtime download `DirectML.dll` into `src-tauri/target/release/`.
2. Copy it into place: `src-tauri/target/release/DirectML.dll` →
   `src-tauri/DirectML.dll`.
3. `npm run tauri:build:win` (a plain `npm run tauri build` skips the DLL).

CI performs this staging automatically (see below).

## Continuous integration and releases

Two GitHub Actions workflows live in `.github/workflows/`:

- **`ci.yml`** runs on every push/PR to `main`: a frontend type-check + build
  (`tsc && vite build`) and `cargo clippy`/`cargo test` on Windows and Linux.
- **`release.yml`** runs when a `v*` tag is pushed (or via manual dispatch). It
  builds installers for Windows (NSIS `.exe`), macOS (`.dmg`, Apple Silicon and
  Intel), and Linux (`.deb`, `.AppImage`), then opens a **draft** GitHub Release
  with the artifacts attached for review before publishing.

The runners install the full build toolchain (Rust, Node, CMake, LLVM/libclang,
platform libs) so **end users never need any of it** — ONNX Runtime and
whisper.cpp are statically linked into the executable, and the Windows job stages
`DirectML.dll` into the installer.

### Cutting a release

1. Bump `version` in both `package.json` and `src-tauri/tauri.conf.json`.
2. Commit, then tag: `git tag v0.1.1 && git push origin v0.1.1`.
3. Wait for `release.yml` to finish, review the draft release, and publish it.

macOS and Linux artifacts are unsigned and built from untested code paths — treat
them as best-effort. `fail-fast: false` means a failure on one platform still
produces artifacts for the others.

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
