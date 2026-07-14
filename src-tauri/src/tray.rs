use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Open Claudy").build(app)?;
    let toggle = MenuItemBuilder::with_id("toggle_dictation", "Toggle Dictation").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&open, &toggle])
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("bundled icon").clone())
        .tooltip("Claudy")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "toggle_dictation" => crate::dictation::toggle(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

const TRAY_ICON_SIZE: u32 = 32;

/// Filled red circle on transparency, generated at runtime (no asset file).
pub fn recording_icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = (size / 2) as i32;
    let r = (size / 2 - 2) as i32;
    for y in 0..size as i32 {
        for x in 0..size as i32 {
            let (dx, dy) = (x - c, y - c);
            if dx * dx + dy * dy <= r * r {
                let i = ((y as u32 * size + x as u32) * 4) as usize;
                rgba[i] = 0xE7; // red
                rgba[i + 1] = 0x00;
                rgba[i + 2] = 0x2E;
                rgba[i + 3] = 0xFF;
            }
        }
    }
    rgba
}

/// State-reflecting tray icon: red dot + tooltip while recording, app icon
/// otherwise. Called only from dictation::publish.
pub fn set_recording(app: &AppHandle, recording: bool) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    if recording {
        let icon = tauri::image::Image::new_owned(
            recording_icon_rgba(TRAY_ICON_SIZE),
            TRAY_ICON_SIZE,
            TRAY_ICON_SIZE,
        );
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_tooltip(Some("Claudy — recording"));
    } else {
        let _ = tray.set_icon(app.default_window_icon().cloned());
        let _ = tray.set_tooltip(Some("Claudy"));
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 32;

    fn pixel(rgba: &[u8], x: u32, y: u32) -> &[u8] {
        let i = ((y * SIZE + x) * 4) as usize;
        &rgba[i..i + 4]
    }

    #[test]
    fn recording_icon_buffer_has_rgba_length() {
        let rgba = recording_icon_rgba(SIZE);
        assert_eq!(rgba.len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn recording_icon_center_is_opaque_red() {
        let rgba = recording_icon_rgba(SIZE);
        let p = pixel(&rgba, SIZE / 2, SIZE / 2);
        assert!(p[0] > 0xC0, "red channel, got {p:?}");
        assert_eq!(p[3], 0xFF, "opaque alpha");
    }

    #[test]
    fn recording_icon_corners_are_transparent() {
        let rgba = recording_icon_rgba(SIZE);
        assert_eq!(pixel(&rgba, 0, 0)[3], 0);
        assert_eq!(pixel(&rgba, SIZE - 1, SIZE - 1)[3], 0);
    }
}
