mod ai;
mod assistant;
mod audio;
mod config;
mod dictation;
mod download;
mod inject;
mod models;
mod notify;
mod overlay;
mod prompt_flow;
mod prompts;
mod secrets;
mod selection;
mod shortcuts;
mod stt;
mod tray;
mod tts;

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
        .manage(assistant::AssistantState::default())
        .manage(tts::TtsState::default())
        .manage(prompt_flow::PromptFlowState::default())
        .manage(shortcuts::PromptShortcuts::default())
        .invoke_handler(tauri::generate_handler![
            config::get_settings,
            config::update_settings,
            secrets::set_api_key,
            secrets::has_api_key,
            secrets::delete_api_key,
            ai::test_provider,
            ai::active_provider_supports_images,
            prompts::list_prompts,
            prompts::save_prompt,
            prompts::delete_prompt,
            prompts::export_prompts,
            prompts::import_prompts,
            prompt_flow::run_prompt,
            shortcuts::check_shortcut,
            shortcuts::suspend_global_shortcuts,
            shortcuts::resume_global_shortcuts,
            models::list_models,
            models::delete_model,
            models::get_models_dir,
            download::download_model,
            download::download_tts_model,
            download::cancel_model_download,
            tts::tts_model_status,
            tts::delete_tts_model,
            audio::list_audio_devices,
            audio::start_capture,
            audio::stop_capture,
            stt::stop_capture_and_transcribe,
            inject::paste_text,
            dictation::toggle_dictation,
            dictation::get_dictation_state,
            assistant::ask_assistant,
            assistant::close_assistant,
            assistant::resize_assistant_input,
            assistant::set_assistant_dialog_open,
            assistant::assistant_new_question,
            assistant::stop_assistant_speech,
            assistant::replay_assistant_speech,
            assistant::get_assistant_state
        ])
        .setup(|app| {
            tray::create(app.handle())?;
            overlay::init(app.handle())?;
            shortcuts::init(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            use tauri::Manager;
            match window.label() {
                "main" => {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let _ = window.hide();
                        api.prevent_close();
                    }
                }
                "assistant" => {
                    // Blur-dismiss, but only while still typing — losing focus
                    // after an answer is shown must not close the panel.
                    if let tauri::WindowEvent::Focused(false) = event {
                        use std::sync::atomic::Ordering;
                        let app = window.app_handle();
                        let state = app.state::<assistant::AssistantState>();
                        let phase = *state.phase.lock().unwrap();
                        // Don't dismiss while a file-picker dialog is open — it
                        // steals focus but the user is still attaching an image.
                        let picking = state.dialog_open.load(Ordering::SeqCst);
                        if phase == assistant::Phase::Input && !picking {
                            assistant::close(app);
                        }
                    }
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
