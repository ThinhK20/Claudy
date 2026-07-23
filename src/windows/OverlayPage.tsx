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

  return (
    <div className="flex h-screen items-center justify-center">
      <div className="flex items-center gap-2 rounded-full bg-black/80 px-4 py-2 text-sm text-white">
        {phase === "recording" && (
          <>
            <span className="h-2 w-2 animate-pulse rounded-full bg-red-500" />
            <LevelBars level={level} />
            <span>Recording…</span>
            {/* Set when a tap latched the capture: the key is already back
                up, so only a second press ends it. */}
            {message && <span className="opacity-60">{message}</span>}
          </>
        )}
        {phase === "transcribing" && (
          <>
            <Loader2 className="h-4 w-4 animate-spin" />
            <span>Transcribing…</span>
          </>
        )}
        {phase === "error" && (
          <span className="text-red-400">{message ?? "Something went wrong"}</span>
        )}
        {phase === "idle" && <span className="opacity-60">Ready</span>}
      </div>
    </div>
  );
}
