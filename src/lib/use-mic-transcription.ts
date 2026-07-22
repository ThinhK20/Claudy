import { useCallback, useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  onMicLevel,
  startCapture,
  stopCapture,
  stopCaptureAndTranscribe,
} from "@/lib/stt-api";

export type MicState = "idle" | "recording" | "transcribing";

interface UseMicTranscriptionOptions {
  /// Microphone device name (`""` = system default), from settings.
  device: string;
  /// Called with the transcribed text once recording stops and whisper returns.
  onText: (text: string) => void;
}

export interface UseMicTranscription {
  state: MicState;
  /// Live mic level (0..1) while recording, for a level meter.
  level: number;
  error: string | null;
  /// Start recording when idle, or stop-and-transcribe when recording.
  toggle: () => void;
  /// Discard any in-flight recording and reset — safe to call anytime.
  cancel: () => void;
}

/// Bridges the native capture → whisper pipeline (`stt-api`) into a component.
/// Only one recording can run at a time process-wide (single audio slot in
/// Rust), so this keeps a `recordingRef` and always releases the slot + the
/// `mic-level` listener on cancel/unmount to avoid stranding the microphone.
export function useMicTranscription({
  device,
  onText,
}: UseMicTranscriptionOptions): UseMicTranscription {
  const [state, setState] = useState<MicState>("idle");
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const unlistenRef = useRef<UnlistenFn | null>(null);
  const recordingRef = useRef(false);
  // Keep the latest onText without resubscribing the whole toggle callback.
  const onTextRef = useRef(onText);
  onTextRef.current = onText;

  const dropLevelListener = useCallback(() => {
    unlistenRef.current?.();
    unlistenRef.current = null;
    setLevel(0);
  }, []);

  const cancel = useCallback(() => {
    if (recordingRef.current) {
      recordingRef.current = false;
      void stopCapture().catch(() => {});
    }
    dropLevelListener();
    setState("idle");
    setError(null);
  }, [dropLevelListener]);

  const toggle = useCallback(() => {
    void (async () => {
      setError(null);
      try {
        if (!recordingRef.current) {
          await startCapture(device);
          recordingRef.current = true;
          setState("recording");
          unlistenRef.current = await onMicLevel(setLevel);
        } else {
          recordingRef.current = false;
          dropLevelListener();
          setState("transcribing");
          const r = await stopCaptureAndTranscribe();
          onTextRef.current(r.text);
          setState("idle");
        }
      } catch (e: unknown) {
        recordingRef.current = false;
        dropLevelListener();
        setError(String(e));
        setState("idle");
      }
    })();
  }, [device, dropLevelListener]);

  // Release the audio slot and listener if the component unmounts mid-recording.
  useEffect(() => {
    return () => {
      if (recordingRef.current) {
        recordingRef.current = false;
        void stopCapture().catch(() => {});
      }
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, []);

  return { state, level, error, toggle, cancel };
}
