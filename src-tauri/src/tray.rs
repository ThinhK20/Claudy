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

/// Filled circle on transparency, generated at runtime (no asset file).
fn dot_icon_rgba(size: u32, color: [u8; 3]) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = (size / 2) as i32;
    let r = (size / 2 - 2) as i32;
    for y in 0..size as i32 {
        for x in 0..size as i32 {
            let (dx, dy) = (x - c, y - c);
            if dx * dx + dy * dy <= r * r {
                let i = ((y as u32 * size + x as u32) * 4) as usize;
                rgba[i] = color[0];
                rgba[i + 1] = color[1];
                rgba[i + 2] = color[2];
                rgba[i + 3] = 0xFF;
            }
        }
    }
    rgba
}

pub fn recording_icon_rgba(size: u32) -> Vec<u8> {
    dot_icon_rgba(size, [0xE7, 0x00, 0x2E]) // red
}

/// Blue, so "armed and waiting for a wake word" reads differently at a glance
/// from "your microphone is being recorded right now".
pub fn listening_icon_rgba(size: u32) -> Vec<u8> {
    dot_icon_rgba(size, [0x2E, 0x7D, 0xE7]) // blue
}

/// What the tray is currently telling the user. Ordered by urgency —
/// `Recording` outranks `Listening`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Idle,
    Listening,
    Recording,
}

/// Pure: the status implied by the two things that can own the microphone.
pub fn status_of(recording: bool, listening: bool) -> Status {
    if recording {
        Status::Recording
    } else if listening {
        Status::Listening
    } else {
        Status::Idle
    }
}

/// Re-derive the tray icon from live state. Deliberately reads that state
/// itself rather than taking a flag: dictation and the wake listener both
/// change it, and a caller-supplied bool would let one of them clear the
/// other's icon depending on which published last.
pub fn refresh(app: &AppHandle) {
    let recording = app
        .state::<crate::dictation::DictationState>()
        .phase
        .lock()
        .map(|p| *p == crate::dictation::Phase::Recording)
        .unwrap_or(false);
    let listening = app.state::<crate::wake::WakeState>().is_listening();
    let status = status_of(recording, listening);
    // The wake listener publishes from a worker thread; tray mutation belongs
    // on the thread that owns the icon.
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || set_status(&app, status));
}

fn set_status(app: &AppHandle, status: Status) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    let (rgba, tooltip) = match status {
        Status::Recording => (Some(recording_icon_rgba(TRAY_ICON_SIZE)), "Claudy — recording"),
        Status::Listening => (
            Some(listening_icon_rgba(TRAY_ICON_SIZE)),
            "Claudy — listening for your wake word",
        ),
        Status::Idle => (None, "Claudy"),
    };
    match rgba {
        Some(buf) => {
            let icon = tauri::image::Image::new_owned(buf, TRAY_ICON_SIZE, TRAY_ICON_SIZE);
            let _ = tray.set_icon(Some(icon));
        }
        None => {
            let _ = tray.set_icon(app.default_window_icon().cloned());
        }
    }
    let _ = tray.set_tooltip(Some(tooltip));
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

    #[test]
    fn listening_icon_is_a_different_colour_from_recording() {
        // The two states must be distinguishable in the tray at a glance.
        let listening = listening_icon_rgba(SIZE);
        let recording = recording_icon_rgba(SIZE);
        assert_ne!(
            pixel(&listening, SIZE / 2, SIZE / 2),
            pixel(&recording, SIZE / 2, SIZE / 2)
        );
        assert_eq!(pixel(&listening, SIZE / 2, SIZE / 2)[3], 0xFF, "opaque");
    }

    #[test]
    fn recording_outranks_listening() {
        // Starting dictation stops the wake listener, but the two states
        // change independently — recording must win in the overlap.
        assert_eq!(status_of(true, true), Status::Recording);
        assert_eq!(status_of(true, false), Status::Recording);
        assert_eq!(status_of(false, true), Status::Listening);
        assert_eq!(status_of(false, false), Status::Idle);
    }
}
