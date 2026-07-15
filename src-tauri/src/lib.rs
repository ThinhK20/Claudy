mod ai;
mod audio;
mod config;
mod dictation;
mod download;
mod inject;
mod models;
mod overlay;
mod secrets;
mod shortcuts;
mod stt;
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .manage(download::Downloads::default())
        .manage(audio::AudioState::default())
        .manage(stt::SttState::default())
        .manage(dictation::DictationState::default())
        .invoke_handler(tauri::generate_handler![
            config::get_settings,
            config::update_settings,
            secrets::set_api_key,
            secrets::has_api_key,
            secrets::delete_api_key,
            models::list_models,
            models::delete_model,
            models::get_models_dir,
            download::download_model,
            download::cancel_model_download,
            audio::list_audio_devices,
            audio::start_capture,
            audio::stop_capture,
            stt::stop_capture_and_transcribe,
            inject::paste_text,
            dictation::toggle_dictation,
            dictation::get_dictation_state
        ])
        .setup(|app| {
            tray::create(app.handle())?;
            overlay::init(app.handle())?;
            shortcuts::init(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
