use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// App-wide notification choke point. `enabled` is the caller's already-
/// loaded `settings.notifications_enabled` — pass `true` for failures that
/// happen BEFORE settings could be read.
pub fn send(app: &AppHandle, enabled: bool, body: &str) {
    send_with_title(app, enabled, "Claudy", body);
}

/// Same choke point with a caller-chosen title, so a reminder toast is
/// distinguishable from a prompt/dictation one at a glance.
pub fn send_with_title(app: &AppHandle, enabled: bool, title: &str, body: &str) {
    if !enabled {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}
