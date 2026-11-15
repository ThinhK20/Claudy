import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import {
  getDictationPhase,
  onDictationState,
  type DictationPhase,
} from "@/lib/dictation-api";
import { onMicLevel } from "@/lib/stt-api";
import { LevelBars } from "@/components/transcription/level-bars";

export default function OverlayPage() {
  const [phase, setPhase] = useState<DictationPhase>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [level, setLevel] = useState(0);

  useEffect(() => {
    // Sync on mount: covers dev hot-reload while a dictation is in flight.
    getDictationPhase().then(setPhase).catch(() => {});
    const unState = onDictationState((s) => {
      setPhase(s.phase);
      setMessage(s.message);
    });
    const unLevel = onMicLevel(setLevel);
    return () => {
      unState.then((f) => f());
      unLevel.then((f) => f());
    };
  }, []);

  const errorText = message ?? "Something went wrong";

  // `dark` pins the theme tokens: the pill is always a dark surface, whatever
  // the app theme. The window is a fixed 300x70, so every line truncates
  // rather than wrapping or spilling out of it; `title` keeps the full text.
  return (
    <div className="dark flex h-screen items-center justify-center px-2">
      <div
        role="status"
        aria-live={phase === "error" ? "assertive" : "polite"}
        className="flex max-w-full min-w-0 items-center gap-2 rounded-full bg-black/80 px-4 py-2 text-sm text-white"
      >
        {phase === "recording" && (
          <>
            <span
              aria-hidden="true"
              className="bg-destructive h-2 w-2 shrink-0 rounded-full motion-safe:animate-pulse"
            />
            <LevelBars level={level} className="shrink-0" />
            <span className="shrink-0">Recording…</span>
            {/* Set when a tap latched the capture: the key is already back
                up, so only a second press ends it. */}
            {message && (
              <span className="min-w-0 truncate opacity-70" title={message}>
                {message}
              </span>
            )}
          </>
        )}
        {phase === "transcribing" && (
          <>
            <Loader2
              aria-hidden="true"
              className="h-4 w-4 shrink-0 animate-spin motion-reduce:animate-none"
            />
            <span className="truncate">Transcribing…</span>
          </>
        )}
        {phase === "error" && (
          <span className="text-destructive min-w-0 truncate" title={errorText}>
            {errorText}
          </span>
        )}
        {phase === "idle" && <span className="opacity-70">Ready</span>}
      </div>
    </div>
  );
}
