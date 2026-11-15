use serde::Serialize;
use std::path::PathBuf;
use tauri::AppHandle;

use crate::config;

pub struct ModelSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub disk_size: &'static str,
    pub sha256: &'static str,
}

/// SHA-256 hashes are the Git LFS object ids Hugging Face publishes for each
/// file (https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main).
pub const CATALOG: &[ModelSpec] = &[
    ModelSpec { id: "tiny",           label: "Tiny (multilingual)",  disk_size: "75 MiB",  sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21" },
    ModelSpec { id: "tiny.en",        label: "Tiny (English)",       disk_size: "75 MiB",  sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f" },
    ModelSpec { id: "base",           label: "Base (multilingual)",  disk_size: "142 MiB", sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe" },
    ModelSpec { id: "base.en",        label: "Base (English)",       disk_size: "142 MiB", sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002" },
    ModelSpec { id: "small",          label: "Small (multilingual)", disk_size: "466 MiB", sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b" },
    ModelSpec { id: "small.en",       label: "Small (English)",      disk_size: "466 MiB", sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d" },
    ModelSpec { id: "medium",         label: "Medium (multilingual)",disk_size: "1.5 GiB", sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208" },
    ModelSpec { id: "large-v3-turbo", label: "Large v3 Turbo",       disk_size: "1.5 GiB", sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69" },
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_ids_are_unique_and_hashes_are_sha256_hex() {
        let mut seen = HashSet::new();
        for m in CATALOG {
            assert!(seen.insert(m.id), "duplicate id {}", m.id);
            assert_eq!(m.sha256.len(), 64, "{} sha256 length", m.id);
            assert!(m.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
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
