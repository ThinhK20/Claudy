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
    tracked_download(&app, &id, models::model_url(&id), final_path, Some(spec.sha1)).await
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
    tracked_download(&app, &id, asset.url.to_string(), final_path, asset.sha1).await
}

/// Register `id` in the cancellation map, run the download, then clean up and
/// emit a terminal error/cancelled event on failure. Shared by every
/// downloadable asset so cancellation and progress reporting stay uniform.
async fn tracked_download(
    app: &AppHandle,
    id: &str,
    url: String,
    final_path: std::path::PathBuf,
    expected_sha1: Option<&str>,
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

    let result = download_file(app, id, &url, &final_path, expected_sha1, cancel).await;

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

/// Generic resumable download to `final_path` with progress events and an
/// optional SHA-1 integrity check. Keeps a `<name>.part` file so a cancelled or
/// interrupted transfer can resume via an HTTP Range request.
async fn download_file(
    app: &AppHandle,
    id: &str,
    url: &str,
    final_path: &Path,
    expected_sha1: Option<&str>,
    cancel: Arc<AtomicBool>,
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

    let client = reqwest::Client::new();
    let mut req = client.get(url);
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

    if let Some(expected) = expected_sha1 {
        emit_progress(app, progress(id, downloaded, total, "verifying", None));
        let expected = expected.to_string();
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
    }

    tokio::fs::rename(&part_path, final_path)
        .await
        .map_err(|e| format!("Could not finalize download: {e}"))?;
    emit_progress(app, progress(id, downloaded, total, "done", None));
    Ok(())
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
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sha1_of_missing_file_is_error() {
        assert!(sha1_hex_of_file(Path::new("Z:\\definitely\\missing.bin")).is_err());
    }
}
