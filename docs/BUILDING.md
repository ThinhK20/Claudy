# Building Claudy

## Prerequisites (Windows)

- Node.js ≥ 20, Rust (MSVC toolchain) ≥ 1.77
- Visual Studio Build Tools 2022 with the "Desktop development with C++" workload
- CMake (`winget install Kitware.CMake`) — required by whisper-rs
- LLVM (`winget install LLVM.LLVM`) — required by whisper-rs (bindgen);
  set `LIBCLANG_PATH=C:\Program Files\LLVM\bin`

## Run

    npm install
    npm run tauri dev

## Whisper models

Models are stored in the project-scope `models/` directory only (gitignored).
Download them from the app's Transcription page — never commit them.
