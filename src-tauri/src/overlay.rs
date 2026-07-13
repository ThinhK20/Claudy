use tauri::{AppHandle, Manager, PhysicalPosition};

pub const OVERLAY_LABEL: &str = "overlay";
const BOTTOM_MARGIN_PX: u32 = 96;

/// Top-left position that horizontally centers `window` in the monitor work
/// area and rests it `margin` px above the bottom edge. Physical pixels.
pub fn bottom_center(
    work_pos: (i32, i32),
    work_size: (u32, u32),
    window: (u32, u32),
    margin: u32,
) -> (i32, i32) {
    let x = work_pos.0 + (work_size.0 as i32 - window.0 as i32) / 2;
    let y = work_pos.1 + work_size.1 as i32 - window.1 as i32 - margin as i32;
    (x, y)
}

/// `focus: false` in tauri.conf.json only affects creation; non-focusable
/// (WS_EX_NOACTIVATE on Windows) guarantees `show()` never steals focus
/// from the app the user is dictating into.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or("overlay window missing")?;
    w.set_focusable(false).map_err(|e| e.to_string())
}

/// Positions the pill at the bottom-center of the monitor the cursor is on
/// (dictation targets the app under the user's attention), then shows it.
/// Never calls set_focus.
pub fn show(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or("overlay window missing")?;
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or("no monitor found")?;
    let area = monitor.work_area();
    let size = w.outer_size().map_err(|e| e.to_string())?;
    let (x, y) = bottom_center(
        (area.position.x, area.position.y),
        (area.size.width, area.size.height),
        (size.width, size.height),
        BOTTOM_MARGIN_PX,
    );
    w.set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    w.show().map_err(|e| e.to_string())
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or("overlay window missing")?;
    w.hide().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_horizontally_and_sits_above_bottom_edge() {
        // 1920x1040 work area at origin, 300x70 window, 96px margin
        let (x, y) = bottom_center((0, 0), (1920, 1040), (300, 70), 96);
        assert_eq!(x, (1920 - 300) / 2);
        assert_eq!(y, 1040 - 70 - 96);
    }

    #[test]
    fn handles_secondary_monitor_with_negative_origin() {
        // Monitor to the left of primary: origin (-2560, 120)
        let (x, y) = bottom_center((-2560, 120), (2560, 1400), (300, 70), 96);
        assert_eq!(x, -2560 + (2560 - 300) / 2);
        assert_eq!(y, 120 + 1400 - 70 - 96);
    }

    #[test]
    fn window_wider_than_work_area_does_not_panic() {
        // Degenerate case must not panic; x may be negative relative to origin.
        let (x, _) = bottom_center((0, 0), (200, 400), (300, 70), 96);
        assert_eq!(x, (200 - 300) / 2);
    }
}
