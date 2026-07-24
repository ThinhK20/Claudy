import { describe, expect, it } from "vitest";
import { captureShortcut, type KeyLike } from "@/lib/accelerator";

/** A keydown with no modifiers held; override what the case cares about. */
const press = (over: Partial<KeyLike>): KeyLike => ({
  key: "",
  code: "",
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...over,
});

describe("captureShortcut", () => {
  it("keeps the short display form for letters", () => {
    const got = captureShortcut(press({ key: "D", code: "KeyD", ctrlKey: true, shiftKey: true }));
    expect(got.accelerator).toBe("Ctrl+Shift+D");
  });

  it("keeps the short display form for digits", () => {
    const got = captureShortcut(press({ key: "1", code: "Digit1", altKey: true }));
    expect(got.accelerator).toBe("Alt+1");
  });

  it("orders modifiers Ctrl, Alt, Shift, Super", () => {
    const got = captureShortcut(
      press({ key: "G", code: "KeyG", ctrlKey: true, altKey: true, shiftKey: true, metaKey: true }),
    );
    expect(got.accelerator).toBe("Ctrl+Alt+Shift+Super+G");
  });

  it("accepts F13 with no modifier at all", () => {
    // The whole point of the change: an Fn layer emits this on its own.
    const got = captureShortcut(press({ key: "F13", code: "F13" }));
    expect(got.accelerator).toBe("F13");
  });

  it("accepts a bare media key", () => {
    const got = captureShortcut(press({ key: "MediaPlayPause", code: "MediaPlayPause" }));
    expect(got.accelerator).toBe("MediaPlayPause");
  });

  it("accepts a named key when a modifier is held", () => {
    const got = captureShortcut(press({ key: "Home", code: "Home", ctrlKey: true }));
    expect(got.accelerator).toBe("Ctrl+Home");
  });

  it("rejects a bare letter and names the missing modifier", () => {
    const got = captureShortcut(press({ key: "a", code: "KeyA" }));
    expect(got.accelerator).toBeNull();
    expect(got.message).toContain("Ctrl, Alt, Shift or Win");
  });

  it("still requires a modifier for F1-F12", () => {
    const got = captureShortcut(press({ key: "F5", code: "F5" }));
    expect(got.accelerator).toBeNull();
    expect(got.message).toContain("F5");
  });

  it("ignores a modifier pressed on its own, silently", () => {
    const got = captureShortcut(press({ key: "Control", code: "ControlLeft", ctrlKey: true }));
    expect(got.accelerator).toBeNull();
    expect(got.message).toBe("");
  });

  it("names the unusable key when Windows can see it but cannot bind it", () => {
    // Punctuation is deliberately out: `code` is physical, VK_OEM_* is layout-dependent.
    const got = captureShortcut(press({ key: ";", code: "Semicolon", ctrlKey: true }));
    expect(got.accelerator).toBeNull();
    expect(got.message).toContain("Semicolon");
  });

  it("explains the firmware-only Fn case when no key reaches the webview", () => {
    const got = captureShortcut(press({ key: "Unidentified", code: "Unidentified" }));
    expect(got.accelerator).toBeNull();
    expect(got.message).toContain("firmware");
  });

  it("treats an empty code as the firmware-only case too", () => {
    const got = captureShortcut(press({ key: "Unidentified", code: "" }));
    expect(got.accelerator).toBeNull();
    expect(got.message).toContain("firmware");
  });

  it("does not bind NumpadEqual, which global-hotkey maps to the wrong VK", () => {
    const got = captureShortcut(press({ key: "=", code: "NumpadEqual", ctrlKey: true }));
    expect(got.accelerator).toBeNull();
  });
});
