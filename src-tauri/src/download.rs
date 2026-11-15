use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

use crate::{config, models, tts};

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

pub fn sha256_hex_of_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
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

fn emit_progress(app: &AppHandle, p: DownloadProgress) {
    let _ = app.emit("model-download-progress", p);
}

fn progress(id: &str, downloaded: u64, total: u64, status: &str, message: Option<String>) -> DownloadProgress {
    DownloadProgress { id: id.into(), downloaded, total, status: status.into(), message }
}

#[tauri::command]
pub async fn download_model(app: AppHandle, id: String) -> Result<(), String> {
    let spec = models::catalog_get(&id).ok_or_else(|| format!("Unknown model '{id}'"))?;
    let dir = models::resolve_dir(&config::load(&app)?.models_dir_override);
    let final_path = dir.join(models::model_filename(&id));
    if final_path.is_file() {
        return Err(format!("Model '{id}' is already downloaded"));
    }
    tracked_download(&app, &id, models::model_url(&id), final_path, spec.sha256).await
}

/// Download a Kokoro TTS asset (`kokoro-model` / `kokoro-voices`) through the
/// same cancellation map and progress events as Whisper models.
#[tauri::command]
pub async fn download_tts_model(app: AppHandle, id: String) -> Result<(), String> {
    let asset = tts::asset_get(&id).ok_or_else(|| format!("Unknown TTS asset '{id}'"))?;
    let dir = models::resolve_dir(&config::load(&app)?.models_dir_override);
    let final_path = dir.join(asset.filename);
    if final_path.is_file() {
        return Err(format!("'{id}' is already downloaded"));
    }
    tracked_download(&app, &id, asset.url.to_string(), final_path, asset.sha256).await
}

/// Register `id` in the cancellation map, run the download, then clean up and
/// emit a terminal error/cancelled event on failure. Shared by every
/// downloadable asset so cancellation and progress reporting stay uniform.
async fn tracked_download(
    app: &AppHandle,
    id: &str,
    url: String,
    final_path: std::path::PathBuf,
    expected_sha256: &str,
) -> Result<(), String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let downloads = app.state::<Downloads>();
        let mut map = downloads.0.lock().map_err(|_| "downloads state poisoned")?;
        if map.contains_key(id) {
            return Err(format!("'{id}' is already downloading"));
        }
        map.insert(id.to_string(), cancel.clone());
    }

    let emit = |p: DownloadProgress| emit_progress(app, p);
    let result = download_file(&emit, id, &url, &final_path, expected_sha256, &cancel).await;

    if let Ok(mut map) = app.state::<Downloads>().0.lock() {
        map.remove(id);
    }
    if let Err(e) = &result {
        let status = if e == "cancelled" { "cancelled" } else { "error" };
        emit_progress(app, progress(id, 0, 0, status, Some(e.clone())));
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

/// Give up on a server we cannot reach at all after this long.
const CONNECT_TIMEOUT_SECS: u64 = 15;
/// Give up when an open connection delivers nothing for this long.
const READ_TIMEOUT_SECS: u64 = 30;
/// How often a waiting download re-checks its cancel flag.
const CANCEL_POLL_MS: u64 = 100;

/// Run `fut`, but return `Err("cancelled")` as soon as `cancel` is set — even
/// while `fut` is waiting on a server that has gone quiet.
async fn or_cancel<T>(
    cancel: &AtomicBool,
    fut: impl std::future::Future<Output = T>,
) -> Result<T, String> {
    use futures_util::future::{select, Either};
    let stop = async {
        while !cancel.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(CANCEL_POLL_MS)).await;
        }
    };
    match select(std::pin::pin!(fut), std::pin::pin!(stop)).await {
        Either::Left((value, _)) => Ok(value),
        Either::Right(_) => Err("cancelled".into()),
    }
}

/// Generic resumable download to `final_path` with progress events and a
/// mandatory SHA-256 integrity check — nothing is ever moved into place
/// unverified. Keeps a `<name>.part` file so a cancelled or interrupted
/// transfer can resume via an HTTP Range request.
///
/// Takes a progress sink instead of an `AppHandle` so the whole transfer is
/// testable against a mock server without a Tauri runtime.
async fn download_file(
    emit: &impl Fn(DownloadProgress),
    id: &str,
    url: &str,
    final_path: &Path,
    expected_sha256: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let dir = final_path
        .parent()
        .ok_or("Invalid destination path (no parent directory)")?;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("Could not create download dir: {e}"))?;

    let file_name = final_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid destination filename")?;
    let part_path = dir.join(format!("{file_name}.part"));

    // Resume from a previous partial download if present.
    let mut offset = match tokio::fs::metadata(&part_path).await {
        Ok(m) => m.len(),
        Err(_) => 0,
    };

    // No overall timeout — a multi-gigabyte model legitimately takes a long
    // time — but a server that stops answering must not hang the download.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .read_timeout(std::time::Duration::from_secs(READ_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut req = client.get(url);
    if offset > 0 {
        req = req.header("Range", format!("bytes={offset}-"));
    }
    let resp = or_cancel(cancel, req.send())
        .await?
        .map_err(|e| format!("Download failed: {e}"))?;
    let status = resp.status();

    // 416 on a resume means the server has nothing past `offset`: the .part is
    // already the whole file (a finished transfer whose rename never happened).
    // Skip straight to verification, which deletes it if it is anything else —
    // so a bogus .part costs one retry instead of failing forever.
    let already_complete = offset > 0 && status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE;
    if !status.is_success() && !already_complete {
        return Err(format!("Download failed: HTTP {status}"));
    }

    let mut downloaded = offset;
    let mut total = offset;
    if !already_complete {
        // 206 = server honored the Range; anything else means restart from zero.
        let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;
        if !resuming {
            offset = 0;
        }
        downloaded = offset;
        total = offset + resp.content_length().unwrap_or(0);

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(resuming)
            .write(true)
            .truncate(!resuming)
            .open(&part_path)
            .await
            .map_err(|e| format!("Could not open {}: {e}", part_path.display()))?;

        let mut last_emit = std::time::Instant::now();
        let mut stream = resp.bytes_stream();
        // Raced against the cancel flag rather than checked per chunk: a
        // stalled stream produces no chunk to check it on. On cancel the
        // .part file is kept so a later attempt resumes.
        while let Some(chunk) = or_cancel(cancel, stream.next()).await? {
            let chunk = chunk.map_err(|e| format!("Download interrupted: {e}"))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Write failed: {e}"))?;
            downloaded += chunk.len() as u64;
            if last_emit.elapsed().as_millis() >= 250 {
                emit(progress(id, downloaded, total, "downloading", None));
                last_emit = std::time::Instant::now();
            }
        }
        file.flush().await.map_err(|e| format!("Write failed: {e}"))?;
    }

    emit(progress(id, downloaded, total, "verifying", None));
    let verify_path = part_path.clone();
    let actual = tokio::task::spawn_blocking(move || sha256_hex_of_file(&verify_path))
        .await
        .map_err(|e| format!("Verification task failed: {e}"))??;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        tokio::fs::remove_file(&part_path).await.ok(); // corrupt — don't resume from it
        return Err(format!(
            "Checksum mismatch for '{id}' (expected {expected_sha256}, got {actual}). The download was discarded; please retry."
        ));
    }

    tokio::fs::rename(&part_path, final_path)
        .await
        .map_err(|e| format!("Could not finalize download: {e}"))?;
    emit(progress(id, downloaded, total, "done", None));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256("abc") reference vector (FIPS 180-2).
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// A fresh directory per test, so parallel tests never share a `.part`.
    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("claudy-dl-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sha256_of_known_content_matches_reference_vector() {
        let dir = scratch_dir();
        let path = dir.join("abc.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(sha256_hex_of_file(&path).unwrap(), ABC_SHA256);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sha256_of_missing_file_is_error() {
        assert!(sha256_hex_of_file(Path::new("Z:\\definitely\\missing.bin")).is_err());
    }

    #[tokio::test]
    async fn a_verified_download_lands_at_the_final_path() {
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/m.bin");
            then.status(200).body("abc");
        });
        let dir = scratch_dir();
        let dest = dir.join("m.bin");
        let cancel = AtomicBool::new(false);

        download_file(&|_| {}, "m", &server.url("/m.bin"), &dest, ABC_SHA256, &cancel)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
        assert!(!dir.join("m.bin.part").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_checksum_mismatch_discards_the_download_instead_of_installing_it() {
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/m.bin");
            then.status(200).body("tampered");
        });
        let dir = scratch_dir();
        let dest = dir.join("m.bin");
        let cancel = AtomicBool::new(false);

        let err = download_file(&|_| {}, "m", &server.url("/m.bin"), &dest, ABC_SHA256, &cancel)
            .await
            .unwrap_err();

        assert!(err.contains("Checksum mismatch"), "got: {err}");
        assert!(!dest.exists(), "an unverified file must never be installed");
        assert!(!dir.join("m.bin.part").exists(), "a corrupt part must not be resumed from");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_complete_part_file_is_verified_and_installed_when_the_server_says_416() {
        // The transfer finished earlier but the final rename didn't happen. A
        // Range request starting at the file's own length is unsatisfiable, and
        // that used to fail every retry until the .part was deleted by hand.
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/m.bin").header("range", "bytes=3-");
            then.status(416);
        });
        let dir = scratch_dir();
        let dest = dir.join("m.bin");
        std::fs::write(dir.join("m.bin.part"), b"abc").unwrap();
        let cancel = AtomicBool::new(false);

        download_file(&|_| {}, "m", &server.url("/m.bin"), &dest, ABC_SHA256, &cancel)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_stalled_download_can_still_be_cancelled() {
        // The server accepts the request and then says nothing. Cancel used to
        // be checked only when a chunk arrived, so this hung until one did.
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/m.bin");
            then.status(200).delay(std::time::Duration::from_secs(8)).body("abc");
        });
        let dir = scratch_dir();
        let dest = dir.join("m.bin");
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });

        let started = std::time::Instant::now();
        let err = download_file(&|_| {}, "m", &server.url("/m.bin"), &dest, ABC_SHA256, &cancel)
            .await
            .unwrap_err();

        assert_eq!(err, "cancelled");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(4),
            "cancel must not wait for the server: took {:?}",
            started.elapsed()
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
