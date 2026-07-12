import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { useSettings } from "@/lib/settings-store";
import { startCapture, stopCaptureAndTranscribe } from "@/lib/stt-api";

type RecorderState = "idle" | "recording" | "transcribing";

export function TestRecorder() {
  const [state, setState] = useState<RecorderState>("idle");
  const [result, setResult] = useState<string | null>(null);
  const [durationMs, setDurationMs] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const settings = useSettings((s) => s.settings);

  const hasModel = Boolean(settings?.model);

  const toggle = async () => {
    setError(null);
    try {
      if (state === "idle") {
        setResult(null);
        setDurationMs(null);
        await startCapture(settings?.micDevice ?? "");
        setState("recording");
      } else if (state === "recording") {
        setState("transcribing");
        const r = await stopCaptureAndTranscribe();
        setResult(r.text);
        setDurationMs(r.durationMs);
        setState("idle");
      }
    } catch (e: unknown) {
      setError(String(e));
      setState("idle");
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Test transcription</CardTitle>
        <CardDescription>
          Record a short clip and transcribe it with the active model
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {!hasModel && (
          <p className="text-muted-foreground text-sm">
            Download and select a model above to enable the test.
          </p>
        )}
        <div>
          <Button onClick={toggle} disabled={!hasModel || state === "transcribing"}>
            {state === "idle" && "Start recording"}
            {state === "recording" && "Stop & transcribe"}
            {state === "transcribing" && "Transcribing…"}
          </Button>
        </div>
        {error && <p className="text-destructive text-sm">{error}</p>}
        {result !== null && (
          <div className="rounded-md border p-3">
            <p className="text-sm whitespace-pre-wrap">{result || "(no speech detected)"}</p>
            {durationMs !== null && (
              <p className="text-muted-foreground mt-2 text-xs">Transcribed in {durationMs} ms</p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
