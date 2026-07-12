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
