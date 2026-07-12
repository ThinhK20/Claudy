# Claudy Phase 2: Audio + STT — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Offline speech-to-text working end-to-end inside the app: enumerate microphones, capture 16 kHz mono audio with a live level meter, download Whisper models into the project `models/` directory, and transcribe a test recording from the Transcription page.

**Architecture:** Three new Rust modules — `models` (catalog + download manager with progress events, resume, SHA-1 verification), `audio` (cpal capture on a dedicated thread because cpal `Stream` is `!Send`, plus pure-function downmix/resample), `stt` (`SttEngine` trait + `WhisperEngine` with configurable keep-warm caching). The React Transcription page drives everything via commands and events; no OS-level shortcuts yet (Phase 3).

**Tech Stack:** whisper-rs 0.16 (whisper.cpp bindings — needs CMake + LLVM/libclang to build), cpal 0.17 (0.18.1 mixes windows-core 0.61/0.62 and fails to compile on Windows), reqwest 0.12 (streaming), tokio, sha1 + hex, Tauri events, shadcn/ui (card, progress, badge).

**Spec:** `docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md`

**Roadmap context:** Plan 2 of 6. Phase 1 (scaffold/shell) is merged on `main`. Later: 3 Dictation E2E, 4 AI providers, 5 Management UI, 6 Polish.

## Global Constraints

- Whisper models live ONLY in the project-scope `models/` directory (gitignored) — never AppData or any user-profile location. Default dir: `<project root>/models` in dev, `<exe dir>/models` in release. `settings.modelsDirOverride` may override.
- Settings JSON keys camelCase; Rust struct fields snake_case with `#[serde(rename_all = "camelCase")]`. Event payloads follow the same rule.
- No telemetry. Only network traffic allowed in Phase 2: explicit model downloads from `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/`.
- Audio is processed in-memory only; never written to disk.
- No silent failures: every command returns `Result<_, String>` with a user-readable message; the UI surfaces every error.
- Windows 11 is the verification platform. Cargo binaries live at `$env:USERPROFILE\.cargo\bin` (not on Git Bash PATH — use PowerShell or prefix the path).
- Rust: TDD for all pure logic (catalog, path resolution, DSP, hashing, settings→model-path resolution). Hardware/network paths get manual verification steps. Frontend is manually verified this phase (component-test infra arrives with Phase 5's CRUD UIs).
- Existing interfaces from Phase 1 you may rely on: `config::load(&AppHandle) -> Result<Settings, String>`, `Settings { model, language, mic_device, keep_model_warm, models_dir_override, .. }` (`src-tauri/src/config.rs`), `useSettings` Zustand store with `settings`, `load()`, `update(patch)` (`src/lib/settings-store.ts`).

---

### Task 1: Toolchain prerequisites + Phase 2 dependencies compile

whisper-rs builds whisper.cpp from source: it requires **CMake** (build) and **LLVM/libclang** (bindgen). Neither is installed on this machine yet.

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `docs/BUILDING.md`

**Interfaces:**
- Produces: `cargo build` succeeds with all Phase 2 crates. Later tasks assume `whisper_rs`, `cpal`, `reqwest`, `tokio`, `futures_util`, `sha1`, `hex` are available.

- [ ] **Step 1: Install CMake and LLVM (PowerShell)**

```powershell
winget install --id Kitware.CMake --silent --accept-package-agreements --accept-source-agreements
winget install --id LLVM.LLVM --silent --accept-package-agreements --accept-source-agreements
```

- [ ] **Step 2: Set LIBCLANG_PATH persistently and verify tools**

```powershell
[Environment]::SetEnvironmentVariable("LIBCLANG_PATH", "C:\Program Files\LLVM\bin", "User")
# current session (installer PATH changes need a fresh shell; use full paths now):
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
$env:Path = "C:\Program Files\CMake\bin;C:\Program Files\LLVM\bin;$env:USERPROFILE\.cargo\bin;$env:Path"
cmake --version
clang --version
```

Expected: CMake ≥ 3.28 and clang ≥ 17 print versions. If winget placed CMake elsewhere, locate with `Get-ChildItem "C:\Program Files" -Filter cmake.exe -Recurse -Depth 3` and adjust.

- [ ] **Step 3: Add Phase 2 dependencies**

In `src-tauri/Cargo.toml` `[dependencies]`, add:

```toml
whisper-rs = "0.16"
# 0.18.1 mixes windows-core 0.61/0.62 and fails to compile on Windows
cpal = "0.17"
reqwest = { version = "0.12", features = ["stream"] }
tokio = { version = "1", features = ["fs", "io-util"] }
futures-util = "0.3"
sha1 = "0.11"
hex = "0.4"
```

- [ ] **Step 4: Verify it compiles**

```powershell
cd src-tauri; cargo build
```

Expected: success (first build is slow — whisper.cpp compiles from source, several minutes). Common failures: `libclang not found` → LIBCLANG_PATH wrong; `cmake not found` → PATH missing CMake; MSVC errors → VS Build Tools VC workload (already installed per Phase 1).

- [ ] **Step 5: Document the build prerequisites**

Create `docs/BUILDING.md`:

```markdown
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
```

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock docs/BUILDING.md
git commit -m "chore: add Phase 2 STT/audio dependencies and build docs"
```

---

### Task 2: Model catalog + models-dir resolution + list/delete commands (TDD)

**Files:**
- Create: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `config::load`, `Settings.models_dir_override` (Phase 1).
- Produces (used by Tasks 3, 5, 6):
  - `models::CATALOG: &[ModelSpec]` — `ModelSpec { id, label, disk_size, sha1: &'static str }`
  - `models::catalog_get(id: &str) -> Option<&'static ModelSpec>`
  - `models::model_filename(id: &str) -> String` → `"ggml-{id}.bin"`
  - `models::model_url(id: &str) -> String`
  - `models::resolve_dir(override_path: &str) -> PathBuf`
  - Commands: `list_models() -> Vec<ModelInfo>` (`ModelInfo { id, label, disk_size, downloaded }` camelCase), `delete_model(id)`, `get_models_dir() -> String`

- [ ] **Step 1: Write the tests + module skeleton**

Create `src-tauri/src/models.rs`:

```rust
use serde::Serialize;
use std::path::PathBuf;
use tauri::AppHandle;

use crate::config;

pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub disk_size: &'static str,
    pub sha1: &'static str,
}

/// SHA-1 hashes from https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec { id: "tiny",           label: "Tiny (multilingual)",  disk_size: "75 MiB",  sha1: "bd577a113a864445d4c299885e0cb97d4ba92b5f" },
    ModelSpec { id: "tiny.en",        label: "Tiny (English)",       disk_size: "75 MiB",  sha1: "c78c86eb1a8faa21b369bcd33207cc90d64ae9df" },
    ModelSpec { id: "base",           label: "Base (multilingual)",  disk_size: "142 MiB", sha1: "465707469ff3a37a2b9b8d8f89f2f99de7299dac" },
    ModelSpec { id: "base.en",        label: "Base (English)",       disk_size: "142 MiB", sha1: "137c40403d78fd54d454da0f9bd998f78703390c" },
    ModelSpec { id: "small",          label: "Small (multilingual)", disk_size: "466 MiB", sha1: "55356645c2b361a969dfd0ef2c5a50d530afd8d5" },
    ModelSpec { id: "small.en",       label: "Small (English)",      disk_size: "466 MiB", sha1: "db8a495a91d927739e50b3fc1cc4c6b8f6c2d022" },
    ModelSpec { id: "medium",         label: "Medium (multilingual)",disk_size: "1.5 GiB", sha1: "fd9727b6e1217c2f614f9b698455c4ffd82463b4" },
    ModelSpec { id: "large-v3-turbo", label: "Large v3 Turbo",       disk_size: "1.5 GiB", sha1: "4af2b29d7ec73d781377bfd1758ca957a807e941" },
];

pub fn catalog_get(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|m| m.id == id)
}

pub fn model_filename(id: &str) -> String {
    format!("ggml-{id}.bin")
}

pub fn model_url(id: &str) -> String {
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{id}.bin")
}

/// Project-scope models dir. Override wins; otherwise `<project root>/models`
/// in dev builds and `<exe dir>/models` in release. Never a user-profile path.
pub fn resolve_dir(override_path: &str) -> PathBuf {
    if !override_path.is_empty() {
        return PathBuf::from(override_path);
    }
    default_dir()
}

#[cfg(debug_assertions)]
fn default_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = <project root>/src-tauri at compile time
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent")
        .join("models")
}

#[cfg(not(debug_assertions))]
fn default_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("models")))
        .unwrap_or_else(|| PathBuf::from("models"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_ids_are_unique_and_hashes_are_sha1_hex() {
        let mut seen = HashSet::new();
        for m in CATALOG {
            assert!(seen.insert(m.id), "duplicate id {}", m.id);
            assert_eq!(m.sha1.len(), 40, "{} sha1 length", m.id);
            assert!(m.sha1.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn filename_and_url_are_derived_from_id() {
        assert_eq!(model_filename("base.en"), "ggml-base.en.bin");
        assert_eq!(
            model_url("tiny"),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
        );
    }

    #[test]
    fn resolve_dir_prefers_override() {
        assert_eq!(resolve_dir("D:\\custom\\models"), PathBuf::from("D:\\custom\\models"));
    }

    #[test]
    fn default_dir_is_project_scope_not_user_profile() {
        let dir = resolve_dir("");
        assert!(dir.ends_with("models"));
        let s = dir.to_string_lossy().to_lowercase();
        assert!(!s.contains("appdata"), "models dir must never be under AppData: {s}");
    }
}
```

Add `mod models;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests to verify they pass**

```powershell
cd src-tauri; cargo test models::
```

Expected: 4 tests PASS (if an assertion fails, fix the code — not the test).

- [ ] **Step 3: Add the commands**

Append to `src-tauri/src/models.rs`:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub disk_size: String,
    pub downloaded: bool,
}

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let settings = config::load(app)?;
    Ok(resolve_dir(&settings.models_dir_override))
}

#[tauri::command]
pub fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    let dir = models_dir(&app)?;
    Ok(CATALOG
        .iter()
        .map(|m| ModelInfo {
            id: m.id.into(),
            label: m.label.into(),
            disk_size: m.disk_size.into(),
            downloaded: dir.join(model_filename(m.id)).is_file(),
        })
        .collect())
}

#[tauri::command]
pub fn delete_model(app: AppHandle, id: String) -> Result<(), String> {
    catalog_get(&id).ok_or_else(|| format!("Unknown model '{id}'"))?;
    let path = models_dir(&app)?.join(model_filename(&id));
    if !path.is_file() {
        return Err(format!("Model '{id}' is not downloaded"));
    }
    std::fs::remove_file(&path).map_err(|e| format!("Could not delete model: {e}"))
}

#[tauri::command]
pub fn get_models_dir(app: AppHandle) -> Result<String, String> {
    Ok(models_dir(&app)?.to_string_lossy().into_owned())
}
```

Register in `src-tauri/src/lib.rs`'s `invoke_handler`:

```rust
        .invoke_handler(tauri::generate_handler![
            config::get_settings,
            config::update_settings,
            models::list_models,
            models::delete_model,
            models::get_models_dir
        ])
```

- [ ] **Step 4: Verify compile + tests, smoke-test via devtools**

```powershell
cd src-tauri; cargo test
```

Expected: all tests pass. Then `npm run tauri dev`, open devtools in the main window:

```js
await window.__TAURI__.core.invoke("list_models");    // 8 entries, all downloaded: false
await window.__TAURI__.core.invoke("get_models_dir"); // ends with \models under the project root
```

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/models.rs src-tauri/src/lib.rs
git commit -m "feat: add whisper model catalog with project-scope models dir"
```

---

### Task 3: Model download manager — progress, resume, checksum, cancel

**Files:**
- Create: `src-tauri/src/download.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `models::{catalog_get, model_filename, model_url, resolve_dir}`, `config::load` (Task 2 / Phase 1).
- Produces (used by Task 6):
  - Commands: `download_model(id: String)` (async, resolves when finished/failed/cancelled), `cancel_model_download(id: String)`
  - Managed state: `download::Downloads` (registered in `lib.rs` via `.manage(...)`)
  - Event `"model-download-progress"` with payload `DownloadProgress { id, downloaded, total, status, message }` (camelCase); `status ∈ "downloading" | "verifying" | "done" | "error" | "cancelled"`
  - `download::sha1_hex_of_file(path: &Path) -> Result<String, String>`

- [ ] **Step 1: Write the failing test for the hash helper**

Create `src-tauri/src/download.rs`:

```rust
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

use crate::{config, models};

/// Per-model cancellation flags for in-flight downloads.
#[derive(Default)]
pub struct Downloads(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: String,
    pub downloaded: u64,
    pub total: u64,
    pub status: String, // "downloading" | "verifying" | "done" | "error" | "cancelled"
    pub message: Option<String>,
}

pub fn sha1_hex_of_file(path: &Path) -> Result<String, String> {
    use sha1::{Digest, Sha1};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_of_known_content_matches_reference_vector() {
        let dir = std::env::temp_dir();
        let path = dir.join("claudy-sha1-test.bin");
        std::fs::write(&path, b"abc").unwrap();
        // SHA-1("abc") reference vector
        assert_eq!(
            sha1_hex_of_file(&path).unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d1a3"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sha1_of_missing_file_is_error() {
        assert!(sha1_hex_of_file(Path::new("Z:\\definitely\\missing.bin")).is_err());
    }
}
```

Add `mod download;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests**

```powershell
cd src-tauri; cargo test download::
```

Expected: 2 tests PASS.

- [ ] **Step 3: Implement the download flow + commands**

Append to `src-tauri/src/download.rs`:

```rust
fn emit_progress(app: &AppHandle, p: DownloadProgress) {
    let _ = app.emit("model-download-progress", p);
}

fn progress(id: &str, downloaded: u64, total: u64, status: &str, message: Option<String>) -> DownloadProgress {
    DownloadProgress { id: id.into(), downloaded, total, status: status.into(), message }
}

#[tauri::command]
pub async fn download_model(app: AppHandle, id: String) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<Downloads>();
        let mut map = state.0.lock().map_err(|_| "downloads state poisoned")?;
        if map.contains_key(&id) {
            return Err(format!("Model '{id}' is already downloading"));
        }
        map.insert(id.clone(), cancel.clone());
    }

    let result = run_download(&app, &id, cancel).await;

    let state = app.state::<Downloads>();
    if let Ok(mut map) = state.0.lock() {
        map.remove(&id);
    }

    if let Err(e) = &result {
        let status = if e == "cancelled" { "cancelled" } else { "error" };
        emit_progress(&app, progress(&id, 0, 0, status, Some(e.clone())));
    }
    result
}

#[tauri::command]
pub fn cancel_model_download(app: AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<Downloads>();
    let map = state.0.lock().map_err(|_| "downloads state poisoned")?;
    match map.get(&id) {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            Ok(())
        }
        None => Err(format!("Model '{id}' is not downloading")),
    }
}

async fn run_download(app: &AppHandle, id: &str, cancel: Arc<AtomicBool>) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let spec = models::catalog_get(id).ok_or_else(|| format!("Unknown model '{id}'"))?;
    let settings = config::load(app)?;
    let dir = models::resolve_dir(&settings.models_dir_override);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Could not create models dir: {e}"))?;

    let final_path = dir.join(models::model_filename(id));
    let part_path = dir.join(format!("{}.part", models::model_filename(id)));
    if final_path.is_file() {
        return Err(format!("Model '{id}' is already downloaded"));
    }

    // Resume from a previous partial download if present.
    let mut offset = match tokio::fs::metadata(&part_path).await {
        Ok(m) => m.len(),
        Err(_) => 0,
    };

    let client = reqwest::Client::new();
    let mut req = client.get(models::model_url(id));
    if offset > 0 {
        req = req.header("Range", format!("bytes={offset}-"));
    }
    let resp = req.send().await.map_err(|e| format!("Download failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("Download failed: HTTP {status}"));
    }

    // 206 = server honored the Range; anything else means restart from zero.
    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resuming {
        offset = 0;
    }
    let total = offset + resp.content_length().unwrap_or(0);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(resuming)
        .write(true)
        .truncate(!resuming)
        .open(&part_path)
        .await
        .map_err(|e| format!("Could not open {}: {e}", part_path.display()))?;

    let mut downloaded = offset;
    let mut last_emit = std::time::Instant::now();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            // keep the .part file so a later attempt resumes
            return Err("cancelled".into());
        }
        let chunk = chunk.map_err(|e| format!("Download interrupted: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write failed: {e}"))?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed().as_millis() >= 250 {
            emit_progress(app, progress(id, downloaded, total, "downloading", None));
            last_emit = std::time::Instant::now();
        }
    }
    file.flush().await.map_err(|e| format!("Write failed: {e}"))?;
    drop(file);

    emit_progress(app, progress(id, downloaded, total, "verifying", None));
    let expected = spec.sha1;
    let verify_path = part_path.clone();
    let actual = tauri::async_runtime::spawn_blocking(move || sha1_hex_of_file(&verify_path))
        .await
        .map_err(|e| format!("Verification task failed: {e}"))??;
    if actual != expected {
        tokio::fs::remove_file(&part_path).await.ok(); // corrupt — don't resume from it
        return Err(format!(
            "Checksum mismatch for '{id}' (expected {expected}, got {actual}). The download was discarded; please retry."
        ));
    }

    tokio::fs::rename(&part_path, &final_path)
        .await
        .map_err(|e| format!("Could not finalize model file: {e}"))?;
    emit_progress(app, progress(id, downloaded, total, "done", None));
    Ok(())
}
```

In `src-tauri/src/lib.rs`, manage the state (before `.invoke_handler`) and register the commands:

```rust
        .manage(download::Downloads::default())
```

```rust
        .invoke_handler(tauri::generate_handler![
            config::get_settings,
            config::update_settings,
            models::list_models,
            models::delete_model,
            models::get_models_dir,
            download::download_model,
            download::cancel_model_download
        ])
```

- [ ] **Step 4: Verify compile + tests, then a real small download via devtools**

```powershell
cd src-tauri; cargo test
```

Expected: all pass. Then `npm run tauri dev`; in main-window devtools:

```js
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
await listen("model-download-progress", (e) => console.log(e.payload));
await invoke("download_model", { id: "tiny" }); // ~75 MB
await invoke("list_models");                     // tiny → downloaded: true
```

Expected: progress payloads stream in, then `verifying`, then `done`; `models/ggml-tiny.bin` exists at the project root; nothing written outside the project. Also verify cancel/resume: start `download_model({ id: "base" })`, call `cancel_model_download({ id: "base" })` mid-flight → `cancelled` event and `models/ggml-base.bin.part` remains; re-run `download_model` → completes (resumed) with a valid checksum. Delete `ggml-base.bin` afterwards via `invoke("delete_model", { id: "base" })` to keep disk usage low. Keep `ggml-tiny.bin` — Tasks 5–6 need it.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/download.rs src-tauri/src/lib.rs
git commit -m "feat: add model download manager with progress, resume and checksum"
```

---

### Task 4: Audio capture — device enumeration, capture thread, level events, resampling (TDD)

cpal's `Stream` is `!Send`, so the stream must be created, owned, and dropped on one dedicated thread. Commands talk to that thread through channels.

**Files:**
- Create: `src-tauri/src/audio.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces (used by Task 5 and Task 6):
  - `audio::AudioState` managed state; `audio::start(app: AppHandle, device_name: String) -> Result<CaptureHandle, String>`, `audio::stop(state: &AudioState) -> Result<Vec<f32>, String>` — returns **16 kHz mono f32** samples
  - Commands: `list_audio_devices() -> Vec<String>`, `start_capture(device: String)` (`""` = system default), `stop_capture()` (discards samples — used for mic test)
  - Event `"mic-level"` payload `{ level: f32 }` (RMS 0..~1) every 50 ms while capturing
  - Pure fns: `downmix_to_mono(&[f32], channels: u16) -> Vec<f32>`, `resample_linear(&[f32], from_rate: u32, to_rate: u32) -> Vec<f32>`

- [ ] **Step 1: Write the DSP tests + pure functions**

Create `src-tauri/src/audio.rs`:

```rust
use serde::Serialize;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const TARGET_SAMPLE_RATE: u32 = 16_000;
const LEVEL_EMIT_INTERVAL_MS: u64 = 50;
const SETUP_TIMEOUT_SECS: u64 = 5;

pub fn downmix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    if channels == 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

pub fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((samples.len() as f64) / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = samples[idx];
            let b = samples[(idx + 1).min(samples.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_stereo_frames() {
        let stereo = [1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        assert_eq!(downmix_to_mono(&stereo, 2), vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn downmix_mono_is_identity() {
        let mono = [0.1, 0.2, 0.3];
        assert_eq!(downmix_to_mono(&mono, 1), mono.to_vec());
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let s = [0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&s, 16_000, 16_000), s.to_vec());
    }

    #[test]
    fn resample_48k_to_16k_yields_one_third_length() {
        let s: Vec<f32> = (0..48_000).map(|i| (i % 100) as f32 / 100.0).collect();
        let out = resample_linear(&s, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
    }

    #[test]
    fn resample_preserves_constant_signal() {
        let s = vec![0.5f32; 4410];
        let out = resample_linear(&s, 44_100, 16_000);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-6));
        assert_eq!(out.len(), 1600);
    }
}
```

Add `mod audio;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run tests**

```powershell
cd src-tauri; cargo test audio::
```

Expected: 5 tests PASS.

- [ ] **Step 3: Implement the capture thread + commands**

Append to `src-tauri/src/audio.rs`:

```rust
pub struct CaptureHandle {
    stop_tx: mpsc::Sender<()>,
    join: JoinHandle<Result<Vec<f32>, String>>,
}

#[derive(Default)]
pub struct AudioState(pub Mutex<Option<CaptureHandle>>);

#[derive(Clone, Serialize)]
struct MicLevel {
    level: f32,
}

struct StreamParts {
    stream: cpal::Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    level: Arc<Mutex<f32>>,
    channels: u16,
    sample_rate: u32,
}

fn build_stream(device_name: &str) -> Result<StreamParts, String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let device = if device_name.is_empty() {
        host.default_input_device()
            .ok_or("No microphone found. Connect a microphone and try again.")?
    } else {
        host.input_devices()
            .map_err(|e| format!("Could not list microphones: {e}"))?
            .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
            .ok_or_else(|| format!("Microphone '{device_name}' not found"))?
    };

    let supported = device
        .default_input_config()
        .map_err(|e| format!("Microphone is unavailable or busy: {e}"))?;
    let channels = supported.channels();
    let sample_rate = supported.sample_rate().0;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.config();

    let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let level = Arc::new(Mutex::new(0f32));

    let err_cb = |e: cpal::StreamError| eprintln!("audio stream error: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            let buf = buffer.clone();
            let lvl = level.clone();
            device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _| ingest(data, &buf, &lvl),
                    err_cb,
                    None,
                )
                .map_err(|e| format!("Could not open microphone: {e}"))?
        }
        cpal::SampleFormat::I16 => {
            let buf = buffer.clone();
            let lvl = level.clone();
            device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let floats: Vec<f32> =
                            data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                        ingest(&floats, &buf, &lvl);
                    },
                    err_cb,
                    None,
                )
                .map_err(|e| format!("Could not open microphone: {e}"))?
        }
        other => return Err(format!("Unsupported microphone sample format: {other:?}")),
    };

    Ok(StreamParts { stream, buffer, level, channels, sample_rate })
}

fn ingest(data: &[f32], buffer: &Arc<Mutex<Vec<f32>>>, level: &Arc<Mutex<f32>>) {
    if let Ok(mut buf) = buffer.lock() {
        buf.extend_from_slice(data);
    }
    if !data.is_empty() {
        let rms = (data.iter().map(|s| s * s).sum::<f32>() / data.len() as f32).sqrt();
        if let Ok(mut l) = level.lock() {
            *l = rms;
        }
    }
}

/// Spawns the capture thread. Returns once the stream is confirmed running.
pub fn start(app: AppHandle, device_name: String) -> Result<CaptureHandle, String> {
    use cpal::traits::StreamTrait;

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    let join = std::thread::spawn(move || -> Result<Vec<f32>, String> {
        let parts = match build_stream(&device_name) {
            Ok(p) => p,
            Err(e) => {
                let _ = ready_tx.send(Err(e.clone()));
                return Err(e);
            }
        };
        if let Err(e) = parts.stream.play() {
            let e = format!("Could not start microphone: {e}");
            let _ = ready_tx.send(Err(e.clone()));
            return Err(e);
        }
        let _ = ready_tx.send(Ok(()));

        loop {
            match stop_rx.recv_timeout(Duration::from_millis(LEVEL_EMIT_INTERVAL_MS)) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    let level = parts.level.lock().map(|l| *l).unwrap_or(0.0);
                    let _ = app.emit("mic-level", MicLevel { level });
                }
            }
        }

        drop(parts.stream); // stops the callback before we read the buffer
        let raw = parts
            .buffer
            .lock()
            .map(|b| b.clone())
            .map_err(|_| "audio buffer poisoned".to_string())?;
        let mono = downmix_to_mono(&raw, parts.channels);
        Ok(resample_linear(&mono, parts.sample_rate, TARGET_SAMPLE_RATE))
    });

    match ready_rx.recv_timeout(Duration::from_secs(SETUP_TIMEOUT_SECS)) {
        Ok(Ok(())) => Ok(CaptureHandle { stop_tx, join }),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => Err("Microphone setup timed out".into()),
    }
}

/// Stops the capture thread and returns 16 kHz mono samples.
pub fn stop(state: &AudioState) -> Result<Vec<f32>, String> {
    let handle = state
        .0
        .lock()
        .map_err(|_| "audio state poisoned")?
        .take()
        .ok_or("Not recording")?;
    let _ = handle.stop_tx.send(());
    handle
        .join
        .join()
        .map_err(|_| "Capture thread panicked".to_string())?
}

#[tauri::command]
pub fn list_audio_devices() -> Result<Vec<String>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| format!("Could not list microphones: {e}"))?;
    Ok(devices.filter_map(|d| d.name().ok()).collect())
}

#[tauri::command]
pub fn start_capture(
    app: AppHandle,
    state: tauri::State<AudioState>,
    device: String,
) -> Result<(), String> {
    let mut slot = state.0.lock().map_err(|_| "audio state poisoned")?;
    if slot.is_some() {
        return Err("Already recording".into());
    }
    *slot = Some(start(app, device)?);
    Ok(())
}

#[tauri::command]
pub fn stop_capture(state: tauri::State<AudioState>) -> Result<(), String> {
    stop(&state).map(|_| ()) // mic test: samples are discarded, never stored
}
```

In `src-tauri/src/lib.rs`: add `.manage(audio::AudioState::default())` next to the existing `.manage(...)`, and extend `invoke_handler` with `audio::list_audio_devices, audio::start_capture, audio::stop_capture`.

- [ ] **Step 4: Verify compile + tests + live mic via devtools**

```powershell
cd src-tauri; cargo test
```

Expected: all pass. Then `npm run tauri dev`; in devtools:

```js
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
await invoke("list_audio_devices");   // your mic(s) listed
const un = await listen("mic-level", (e) => console.log(e.payload.level));
await invoke("start_capture", { device: "" });
// speak — levels rise above the silent baseline
await invoke("stop_capture");
```

Also verify the error paths: `start_capture` twice → `"Already recording"`; `start_capture` with `{ device: "Nonexistent Mic" }` → not-found error.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/audio.rs src-tauri/src/lib.rs
git commit -m "feat: add cpal audio capture with level metering and 16kHz resampling"
```

---

### Task 5: STT — `SttEngine` trait, `WhisperEngine`, keep-warm, transcribe command (TDD)

**Files:**
- Create: `src-tauri/src/stt.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `audio::{AudioState, stop}` (Task 4), `models::{resolve_dir, model_filename}` (Task 2), `config::{load, Settings}` (Phase 1).
- Produces (used by Task 6 and by Phase 3's dictation flow):
  - `stt::SttEngine` trait: `fn transcribe(&mut self, samples_16k_mono: &[f32], language: &str) -> Result<String, String>`
  - `stt::WhisperEngine::load(model_path: &Path) -> Result<WhisperEngine, String>`
  - `stt::SttState` managed state (keep-warm engine cache)
  - `stt::resolve_model_path(settings: &Settings) -> Result<PathBuf, String>`
  - `stt::transcribe_samples(app: &AppHandle, samples: Vec<f32>) -> Result<TranscriptionResult, String>` (async; Phase 3 calls this after its own capture stop)
  - Command: `stop_capture_and_transcribe() -> TranscriptionResult { text, duration_ms }` (camelCase over IPC)

- [ ] **Step 1: Write the tests + trait + pure logic**

Create `src-tauri/src/stt.rs`:

```rust
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
        let err = WhisperEngine::load(Path::new("Z:\\missing\\ggml-none.bin")).unwrap_err();
        assert!(err.contains("Could not load model"));
    }
}
```

- [ ] **Step 2: Implement engine + command**

Append to `src-tauri/src/stt.rs`:

```rust
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
```

Add `mod stt;` to `src-tauri/src/lib.rs`, add `.manage(stt::SttState::default())`, and add `stt::stop_capture_and_transcribe` to `invoke_handler`.

- [ ] **Step 3: Run tests**

```powershell
cd src-tauri; cargo test
```

Expected: all tests pass, including the 4 new `stt::` tests (`whisper_engine_load_fails_cleanly_for_missing_file` exercises real whisper-rs error handling without needing a model).

- [ ] **Step 4: Verify real transcription via devtools**

`npm run tauri dev`; in devtools (requires `ggml-tiny.bin` from Task 3's verification):

```js
const { invoke } = window.__TAURI__.core;
const s = await invoke("get_settings");
await invoke("update_settings", { settings: { ...s, model: "tiny", language: "en" } });
await invoke("start_capture", { device: "" });
// speak a sentence in English, then:
await invoke("stop_capture_and_transcribe");
```

Expected: `{ text: "<roughly what you said>", durationMs: <number> }`. Run it twice — the second run should be noticeably faster (keep-warm; `keepModelWarm` defaults to `true`). Also verify the guard: set `model: ""` and confirm the command returns the "No model selected" error.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/stt.rs src-tauri/src/lib.rs
git commit -m "feat: add SttEngine trait with whisper-rs engine and keep-warm cache"
```

---

### Task 6: Transcription page — model manager, mic settings, test recorder

**Files:**
- Create: `src/lib/stt-api.ts`, `src/components/transcription/model-manager.tsx`, `src/components/transcription/mic-settings.tsx`, `src/components/transcription/test-recorder.tsx`
- Modify: `src/pages/TranscriptionPage.tsx` (replace placeholder)

**Interfaces:**
- Consumes: every command/event from Tasks 2–5; `useSettings` (Phase 1); shadcn `button`, `select`, `switch`, `label` (Phase 1) plus `card`, `progress`, `badge` (added here).
- Produces: fully functional Transcription page. `src/lib/stt-api.ts` is the typed IPC surface later phases reuse.

- [ ] **Step 1: Add the missing shadcn components**

```powershell
npx shadcn@latest add card progress badge
```

- [ ] **Step 2: Typed IPC wrappers**

Create `src/lib/stt-api.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ModelInfo {
  id: string;
  label: string;
  diskSize: string;
  downloaded: boolean;
}

export type DownloadStatus = "downloading" | "verifying" | "done" | "error" | "cancelled";

export interface DownloadProgress {
  id: string;
  downloaded: number;
  total: number;
  status: DownloadStatus;
  message: string | null;
}

export interface TranscriptionResult {
  text: string;
  durationMs: number;
}

export const listModels = (): Promise<ModelInfo[]> => invoke("list_models");
export const downloadModel = (id: string): Promise<void> => invoke("download_model", { id });
export const cancelModelDownload = (id: string): Promise<void> =>
  invoke("cancel_model_download", { id });
export const deleteModel = (id: string): Promise<void> => invoke("delete_model", { id });
export const getModelsDir = (): Promise<string> => invoke("get_models_dir");
export const listAudioDevices = (): Promise<string[]> => invoke("list_audio_devices");
export const startCapture = (device: string): Promise<void> => invoke("start_capture", { device });
export const stopCapture = (): Promise<void> => invoke("stop_capture");
export const stopCaptureAndTranscribe = (): Promise<TranscriptionResult> =>
  invoke("stop_capture_and_transcribe");

export const onDownloadProgress = (
  cb: (progress: DownloadProgress) => void,
): Promise<UnlistenFn> =>
  listen<DownloadProgress>("model-download-progress", (event) => cb(event.payload));

export const onMicLevel = (cb: (level: number) => void): Promise<UnlistenFn> =>
  listen<{ level: number }>("mic-level", (event) => cb(event.payload.level));
```

- [ ] **Step 3: Model manager component**

Create `src/components/transcription/model-manager.tsx`:

```tsx
import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { useSettings } from "@/lib/settings-store";
import {
  cancelModelDownload,
  deleteModel,
  downloadModel,
  getModelsDir,
  listModels,
  onDownloadProgress,
  type DownloadProgress,
  type ModelInfo,
} from "@/lib/stt-api";

export function ModelManager() {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [modelsDir, setModelsDir] = useState("");
  const [progress, setProgress] = useState<Record<string, DownloadProgress | undefined>>({});
  const [error, setError] = useState<string | null>(null);
  const activeModel = useSettings((s) => s.settings?.model ?? "");
  const updateSettings = useSettings((s) => s.update);

  const refresh = useCallback(async () => {
    try {
      setModels(await listModels());
      setModelsDir(await getModelsDir());
    } catch (e: unknown) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    let unlisten: (() => void) | undefined;
    onDownloadProgress((p) => {
      setProgress((prev) => ({ ...prev, [p.id]: p }));
      if (p.status === "done") refresh();
      if (p.status === "error") setError(p.message ?? "Download failed");
    }).then((un) => {
      unlisten = un;
    });
    return () => unlisten?.();
  }, [refresh]);

  const handleDownload = async (id: string) => {
    setError(null);
    try {
      await downloadModel(id);
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  const handleDelete = async (id: string) => {
    setError(null);
    try {
      if (activeModel === id) await updateSettings({ model: "" });
      await deleteModel(id);
      setProgress((prev) => ({ ...prev, [id]: undefined }));
      await refresh();
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Models</CardTitle>
        <CardDescription>
          Whisper models are stored in <code className="text-xs">{modelsDir}</code>
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {error && <p className="text-destructive text-sm">{error}</p>}
        {models.map((m) => {
          const p = progress[m.id];
          const isDownloading = p?.status === "downloading" || p?.status === "verifying";
          return (
            <div key={m.id} className="flex items-center gap-3 rounded-md border p-3">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-medium">{m.label}</span>
                  <span className="text-muted-foreground text-xs">{m.diskSize}</span>
                  {activeModel === m.id && <Badge>Active</Badge>}
                </div>
                {isDownloading && p && (
                  <div className="mt-2 flex items-center gap-2">
                    <Progress
                      value={p.total > 0 ? (p.downloaded / p.total) * 100 : 0}
                      className="h-2"
                    />
                    <span className="text-muted-foreground w-20 shrink-0 text-xs">
                      {p.status === "verifying"
                        ? "Verifying…"
                        : `${Math.round((p.downloaded / Math.max(p.total, 1)) * 100)}%`}
                    </span>
                  </div>
                )}
              </div>
              {isDownloading ? (
                <Button variant="outline" size="sm" onClick={() => cancelModelDownload(m.id)}>
                  Cancel
                </Button>
              ) : m.downloaded ? (
                <div className="flex gap-2">
                  {activeModel !== m.id && (
                    <Button size="sm" onClick={() => updateSettings({ model: m.id })}>
                      Use
                    </Button>
                  )}
                  <Button variant="outline" size="sm" onClick={() => handleDelete(m.id)}>
                    Delete
                  </Button>
                </div>
              ) : (
                <Button size="sm" onClick={() => handleDownload(m.id)}>
                  Download
                </Button>
              )}
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 4: Mic settings component (device picker + live level meter + language + keep-warm)**

Create `src/components/transcription/mic-settings.tsx`. Note: Radix `SelectItem` forbids empty-string values, so the system-default mic uses the `__default__` sentinel mapped to `""` in settings.

```tsx
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { useSettings } from "@/lib/settings-store";
import { listAudioDevices, onMicLevel, startCapture, stopCapture } from "@/lib/stt-api";

const DEFAULT_DEVICE = "__default__";

const LANGUAGES = [
  { code: "auto", label: "Auto-detect" },
  { code: "en", label: "English" },
  { code: "vi", label: "Vietnamese" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
  { code: "zh", label: "Chinese" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "es", label: "Spanish" },
  { code: "pt", label: "Portuguese" },
];

export function MicSettings() {
  const [devices, setDevices] = useState<string[]>([]);
  const [isTesting, setIsTesting] = useState(false);
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const isTestingRef = useRef(false);
  const settings = useSettings((s) => s.settings);
  const updateSettings = useSettings((s) => s.update);

  useEffect(() => {
    listAudioDevices()
      .then(setDevices)
      .catch((e: unknown) => setError(String(e)));
    return () => {
      unlistenRef.current?.();
      if (isTestingRef.current) stopCapture().catch(() => {});
    };
  }, []);

  if (!settings) return null;

  const toggleTest = async () => {
    setError(null);
    try {
      if (isTesting) {
        await stopCapture();
        unlistenRef.current?.();
        unlistenRef.current = null;
        setIsTesting(false);
        isTestingRef.current = false;
        setLevel(0);
      } else {
        unlistenRef.current = await onMicLevel(setLevel);
        await startCapture(settings.micDevice);
        setIsTesting(true);
        isTestingRef.current = true;
      }
    } catch (e: unknown) {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setIsTesting(false);
      isTestingRef.current = false;
      setError(String(e));
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Microphone & Language</CardTitle>
        <CardDescription>Input device and transcription language</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {error && <p className="text-destructive text-sm">{error}</p>}
        <div className="flex items-center gap-3">
          <Label className="w-28 shrink-0">Microphone</Label>
          <Select
            value={settings.micDevice || DEFAULT_DEVICE}
            onValueChange={(v) =>
              updateSettings({ micDevice: v === DEFAULT_DEVICE ? "" : v })
            }
          >
            <SelectTrigger className="flex-1">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT_DEVICE}>System default</SelectItem>
              {devices.map((d) => (
                <SelectItem key={d} value={d}>
                  {d}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button variant="outline" size="sm" onClick={toggleTest}>
            {isTesting ? "Stop test" : "Test mic"}
          </Button>
        </div>
        {isTesting && (
          <div className="bg-muted h-2 w-full overflow-hidden rounded-full">
            <div
              className="h-full bg-green-500 transition-[width] duration-75"
              style={{ width: `${Math.min(level * 400, 100)}%` }}
            />
          </div>
        )}
        <div className="flex items-center gap-3">
          <Label className="w-28 shrink-0">Language</Label>
          <Select
            value={settings.language}
            onValueChange={(v) => updateSettings({ language: v })}
          >
            <SelectTrigger className="flex-1">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LANGUAGES.map((l) => (
                <SelectItem key={l.code} value={l.code}>
                  {l.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-3">
          <Label className="w-28 shrink-0" htmlFor="keep-warm">
            Keep model warm
          </Label>
          <Switch
            id="keep-warm"
            checked={settings.keepModelWarm}
            onCheckedChange={(v) => updateSettings({ keepModelWarm: v })}
          />
          <span className="text-muted-foreground text-xs">
            Faster repeat transcriptions; uses more memory
          </span>
        </div>
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 5: Test recorder component**

Create `src/components/transcription/test-recorder.tsx`:

```tsx
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { useSettings } from "@/lib/settings-store";
import { startCapture, stopCaptureAndTranscribe } from "@/lib/stt-api";

type RecorderState = "idle" | "recording" | "transcribing";

export function TestRecorder() {
  const [state, setState] = useState<RecorderState>("idle");
  const [result, setResult] = useState<string | null>(null);
  const [durationMs, setDurationMs] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const settings = useSettings((s) => s.settings);

  const hasModel = Boolean(settings?.model);

  const toggle = async () => {
    setError(null);
    try {
      if (state === "idle") {
        setResult(null);
        setDurationMs(null);
        await startCapture(settings?.micDevice ?? "");
        setState("recording");
      } else if (state === "recording") {
        setState("transcribing");
        const r = await stopCaptureAndTranscribe();
        setResult(r.text);
        setDurationMs(r.durationMs);
        setState("idle");
      }
    } catch (e: unknown) {
      setError(String(e));
      setState("idle");
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Test transcription</CardTitle>
        <CardDescription>
          Record a short clip and transcribe it with the active model
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {!hasModel && (
          <p className="text-muted-foreground text-sm">
            Download and select a model above to enable the test.
          </p>
        )}
        <div>
          <Button onClick={toggle} disabled={!hasModel || state === "transcribing"}>
            {state === "idle" && "Start recording"}
            {state === "recording" && "Stop & transcribe"}
            {state === "transcribing" && "Transcribing…"}
          </Button>
        </div>
        {error && <p className="text-destructive text-sm">{error}</p>}
        {result !== null && (
          <div className="rounded-md border p-3">
            <p className="text-sm whitespace-pre-wrap">{result || "(no speech detected)"}</p>
            {durationMs !== null && (
              <p className="text-muted-foreground mt-2 text-xs">Transcribed in {durationMs} ms</p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 6: Assemble the page**

Replace `src/pages/TranscriptionPage.tsx`:

```tsx
import { MicSettings } from "@/components/transcription/mic-settings";
import { ModelManager } from "@/components/transcription/model-manager";
import { TestRecorder } from "@/components/transcription/test-recorder";

export default function TranscriptionPage() {
  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Transcription</h1>
        <p className="text-muted-foreground mt-1">
          Whisper models, microphone and language settings.
        </p>
      </div>
      <ModelManager />
      <MicSettings />
      <TestRecorder />
    </div>
  );
}
```

- [ ] **Step 7: Typecheck + full manual verification**

```powershell
npx tsc --noEmit
npm run tauri dev
```

Walk the Phase 2 checklist below in the running app.

- [ ] **Step 8: Commit**

```powershell
git add src/lib/stt-api.ts src/components/transcription src/pages/TranscriptionPage.tsx src/components/ui package.json package-lock.json
git commit -m "feat: add Transcription page with model manager, mic settings and test recorder"
```

---

## Verification (end of Phase 2)

Manual E2E checklist (Windows 11, `npm run tauri dev`):

1. `cd src-tauri; cargo test` — all Rust tests green (models, download, audio DSP, stt).
2. `npx tsc --noEmit` — no type errors.
3. Transcription page lists 8 models with sizes; models dir path shown ends with `\models` under the project root — **not** AppData.
4. Download "Tiny (multilingual)": progress bar advances, "Verifying…", then Downloaded state; `models/ggml-tiny.bin` exists in the project; click Use → Active badge.
5. Cancel mid-download (use "Base"): progress stops, `.part` file remains; re-download resumes and completes with a valid checksum; Delete removes it and clears Active if it was active.
6. Mic picker lists real devices; Test mic → meter moves when speaking, near-zero when silent; Stop test works.
7. Test transcription: record a sentence → text appears and roughly matches; second run is faster (keep-warm on); toggle Keep model warm off → still works.
8. Language: set Vietnamese, speak Vietnamese with a multilingual model → reasonable output; set English → English output.
9. Error paths are all visible in the UI (no silent failures): no model selected, recording too short (~instant stop), starting capture twice, unknown device.
10. Privacy: no audio files written anywhere; only network traffic is the explicit Hugging Face model download.
