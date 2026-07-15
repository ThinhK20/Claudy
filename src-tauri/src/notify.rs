use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// App-wide notification choke point. `enabled` is the caller's already-
/// loaded `settings.notifications_enabled` — pass `true` for failures that
/// happen BEFORE settings could be read.
pub fn send(app: &AppHandle, enabled: bool, body: &str) {
    if !enabled {
        return;
    }
    let _ = app
        .notification()
        .builder()
        .title("Claudy")
        .body(body)
        .show();
}
