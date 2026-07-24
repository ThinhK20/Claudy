import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { captureShortcut } from "@/lib/accelerator";
import {
  checkShortcut,
  resumeGlobalShortcuts,
  suspendGlobalShortcuts,
} from "@/lib/shortcuts-api";
import { cn } from "@/lib/utils";

interface ShortcutInputProps {
  value: string;
  onChange: (accel: string) => void;
  /** Id of the prompt being edited, so it doesn't conflict with itself. */
  excludePromptId?: string;
  /** True when editing the dictation combo itself. */
  forDictation?: boolean;
  /** True when editing the assistant combo itself. */
  forAssistant?: boolean;
  /** false = an empty binding is not allowed (dictation must stay bound). */
  allowClear?: boolean;
}

export function ShortcutInput({
  value,
  onChange,
  excludePromptId,
  forDictation = false,
  forAssistant = false,
  allowClear = true,
}: ShortcutInputProps) {
  const [capturing, setCapturing] = useState(false);
  const [warning, setWarning] = useState("");
  // Why the last press couldn't be bound. Without this a rejected key is
  // indistinguishable from a dead recorder — which is exactly how an Fn-layer
  // key that Windows can't see used to look.
  const [rejected, setRejected] = useState("");
  const capturingRef = useRef(false);

  // Registered combos are consumed by the OS (RegisterHotKey) and never
  // reach the webview, so capture releases ALL global shortcuts first.
  // Suspend is best-effort: if it fails, capture still works for combos
  // the OS doesn't consume.
  const startCapture = () => {
    if (capturingRef.current) return;
    capturingRef.current = true;
    setRejected("");
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
    setRejected("");
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
    checkShortcut(value, { excludePromptId, forDictation, forAssistant })
      .then((check) => {
        if (!stale) setWarning(check.ok ? "" : check.message);
      })
      .catch((e: unknown) => {
        if (!stale) setWarning(String(e));
      });
    return () => {
      stale = true;
    };
  }, [value, excludePromptId, forDictation, forAssistant]);

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
    const { accelerator, message } = captureShortcut(e);
    if (accelerator) {
      setRejected("");
      void endCapture().then(() => onChange(accelerator));
      return;
    }
    // Capture stays open so the user can try another key. An empty message
    // means a modifier on its own — say nothing, they're still reaching.
    setRejected(message);
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
          : "Click, then press a combination like Ctrl+Shift+G. A bare F13–F24 or media key works too — but it's taken from every app system-wide."}
      </p>
      {rejected && <p className="text-destructive text-sm">{rejected}</p>}
      {!rejected && warning && <p className="text-destructive text-sm">{warning}</p>}
    </div>
  );
}
