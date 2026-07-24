/**
 * Turning a browser keydown into a Tauri accelerator string.
 *
 * Every name produced here has to survive BOTH ends of the pipeline:
 *   1. `KeyboardEvent.code`, which is what Chromium/WebView2 gives us, and
 *   2. `global-hotkey`'s `parse_key`, which must not only ACCEPT the name but
 *      also map it to a Windows virtual-key code.
 *
 * (2) is the sharp edge: `Fn` and `NumpadEqual` parse happily and then either
 * fail at `RegisterHotKey` time or bind the wrong key, so neither may appear in
 * the sets below. This is also why `Fn` can never be a modifier — Windows has
 * no Fn modifier bit and no VK for it, and on nearly all keyboards Fn is
 * resolved in firmware and never reaches the OS at all. What reaches us is
 * whatever key the Fn LAYER emits, which is what these sets are here to accept.
 */

/** Keys that never terminate a capture on their own. */
const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta"]);

/**
 * Bindable with NO modifier: nothing types these by accident, and they are
 * exactly what a programmable keyboard's Fn layer tends to emit.
 */
const STANDALONE_KEYS = new Set([
  "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
  "AudioVolumeUp", "AudioVolumeDown", "AudioVolumeMute",
  "MediaPlayPause", "MediaStop", "MediaTrackNext", "MediaTrackPrevious",
]);

/**
 * Bindable only WITH a modifier — bare, they would be stolen from every other
 * app on the machine. Letters, digits and F1–F12 are matched by pattern below.
 */
const MODIFIED_KEYS = new Set([
  "Space", "Enter", "Tab",
  "Insert", "Home", "End", "PageUp", "PageDown",
  "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
  "PrintScreen", "ScrollLock", "Pause", "CapsLock", "NumLock",
  "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4",
  "Numpad5", "Numpad6", "Numpad7", "Numpad8", "Numpad9",
  "NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide",
  "NumpadDecimal", "NumpadEnter",
]);

/** Codes a webview reports when it received a key it cannot name. */
const INVISIBLE_CODES = new Set(["", "Unidentified"]);

/**
 * The accelerator token for a physical key, or null when we cannot bind it.
 * Letters and digits keep their short display form ("A", not "KeyA") — the
 * Rust parser accepts both and existing stored settings use the short one.
 */
function mainKey(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  if (STANDALONE_KEYS.has(code) || MODIFIED_KEYS.has(code)) return code;
  return null;
}

export interface KeyCapture {
  /** Accelerator to store, or null when this press cannot be bound. */
  accelerator: string | null;
  /**
   * Why `accelerator` is null, for display. Empty string means "ignore this
   * press silently" — a modifier on its own, while the user is still reaching
   * for the main key.
   */
  message: string;
}

/** The fields we need off a keydown; a bare object keeps this testable. */
export interface KeyLike {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/**
 * Map a keydown to a Tauri accelerator, or explain why it cannot be one.
 * Silence is the enemy here: a rejected key that says nothing is what makes an
 * Fn-layer press look like a dead recorder.
 */
export function captureShortcut(e: KeyLike): KeyCapture {
  if (MODIFIERS.has(e.key)) return { accelerator: null, message: "" };

  if (INVISIBLE_CODES.has(e.code)) {
    return {
      accelerator: null,
      message:
        "Your keyboard didn't send a key Windows can see. Fn is resolved inside " +
        "the keyboard's firmware and never reaches Windows — remap that Fn layer " +
        "key to F13–F24 in your keyboard's configurator, then record it here.",
    };
  }

  const main = mainKey(e.code);
  if (!main) {
    return {
      accelerator: null,
      message: `Windows sees that key as "${e.code}", which can't be used as a global shortcut.`,
    };
  }

  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");

  if (mods.length === 0 && !STANDALONE_KEYS.has(main)) {
    return {
      accelerator: null,
      message: `"${main}" needs Ctrl, Alt, Shift or Win held with it.`,
    };
  }

  return { accelerator: [...mods, main].join("+"), message: "" };
}
