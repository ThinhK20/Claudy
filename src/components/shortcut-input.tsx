import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  checkShortcut,
  resumeGlobalShortcuts,
  suspendGlobalShortcuts,
} from "@/lib/shortcuts-api";
import { cn } from "@/lib/utils";

/** Keys that never terminate a capture on their own. */
const MODIFIERS = new Set(["Control", "Shift", "Alt", "Meta"]);

/**
 * Map a keydown to a Tauri accelerator string, or null when the event is
 * not capturable. Supported main keys: letters, digits, F1–F24, Space —
 * always with at least one modifier (global shortcuts need one).
 */
export function acceleratorFromEvent(e: {
  key: string;
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}): string | null {
  if (MODIFIERS.has(e.key)) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (mods.length === 0) return null;
  let main: string | null = null;
  if (/^Key[A-Z]$/.test(e.code)) main = e.code.slice(3);
  else if (/^Digit[0-9]$/.test(e.code)) main = e.code.slice(5);
  else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.key)) main = e.key;
  else if (e.code === "Space") main = "Space";
  if (!main) return null;
  return [...mods, main].join("+");
}

interface ShortcutInputProps {
  value: string;
  onChange: (accel: string) => void;
  /** Id of the prompt being edited, so it doesn't conflict with itself. */
  excludePromptId?: string;
  /** True when editing the dictation combo itself. */
  forDictation?: boolean;
  /** false = an empty binding is not allowed (dictation must stay bound). */
  allowClear?: boolean;
}

export function ShortcutInput({
  value,
  onChange,
  excludePromptId,
  forDictation = false,
  allowClear = true,
}: ShortcutInputProps) {
  const [capturing, setCapturing] = useState(false);
  const [warning, setWarning] = useState("");
  const capturingRef = useRef(false);

  // Registered combos are consumed by the OS (RegisterHotKey) and never
  // reach the webview, so capture releases ALL global shortcuts first.
  // Suspend is best-effort: if it fails, capture still works for combos
  // the OS doesn't consume.
  const startCapture = () => {
    if (capturingRef.current) return;
    capturingRef.current = true;
    setCapturing(true);
    void suspendGlobalShortcuts().catch(() => {});
  };

  // Single-shot exit funnel for every path (accepted combo, Escape, clear,
  // blur, unmount). Resume must COMPLETE before the caller's onChange runs:
  // update_settings re-registers the dictation combo and errors if the old
  // one isn't currently registered.
  const endCapture = async () => {
    if (!capturingRef.current) return;
    capturingRef.current = false;
    setCapturing(false);
    try {
      await resumeGlobalShortcuts();
    } catch {
      // Healed at next app start by shortcuts::init.
    }
  };

  // Unmount while capturing (e.g. editor dialog closed): restore shortcuts.
  useEffect(() => {
    return () => {
      void endCapture();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Live conflict warning for the current value (also validates the
  // pre-existing value when an editor opens).
  useEffect(() => {
    let stale = false;
    if (!value) {
      setWarning("");
      return;
    }
    checkShortcut(value, { excludePromptId, forDictation })
      .then((check) => {
        if (!stale) setWarning(check.ok ? "" : check.message);
      })
      .catch((e: unknown) => {
        if (!stale) setWarning(String(e));
      });
    return () => {
      stale = true;
    };
  }, [value, excludePromptId, forDictation]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLButtonElement>) => {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      void endCapture();
      return;
    }
    if (e.key === "Backspace" || e.key === "Delete") {
      if (allowClear) void endCapture().then(() => onChange(""));
      return;
    }
    const accel = acceleratorFromEvent(e);
    if (accel) void endCapture().then(() => onChange(accel));
  };

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-2">
        <Button
          type="button"
          variant="outline"
          onClick={startCapture}
          onKeyDown={onKeyDown}
          onBlur={() => void endCapture()}
          className={cn(
            "w-56 justify-start font-mono",
            capturing && "ring-ring ring-2",
            !value && !capturing && "text-muted-foreground",
          )}
        >
          {capturing ? "Press a key combination…" : value || "Not set"}
        </Button>
        {allowClear && value && !capturing && (
          <Button type="button" variant="ghost" size="sm" onClick={() => onChange("")}>
            Clear
          </Button>
        )}
      </div>
      <p className="text-muted-foreground text-xs">
        {capturing
          ? `Esc cancels${allowClear ? ", Backspace clears" : ""}`
          : "Click, then press a combination like Ctrl+Shift+G"}
      </p>
      {warning && <p className="text-destructive text-sm">{warning}</p>}
    </div>
  );
}
