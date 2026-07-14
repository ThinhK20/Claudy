import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DictationPhase = "idle" | "recording" | "transcribing" | "error";

export interface DictationState {
  phase: DictationPhase;
  message: string | null;
}

export const toggleDictation = (): Promise<void> => invoke("toggle_dictation");

/** Stored phase (never "error" — errors are transient event-only states). */
export const getDictationPhase = (): Promise<Exclude<DictationPhase, "error">> =>
  invoke("get_dictation_state");

export const onDictationState = (
  cb: (state: DictationState) => void,
): Promise<UnlistenFn> =>
  listen<DictationState>("dictation-state", (event) => cb(event.payload));

export const onNavigate = (cb: (page: string) => void): Promise<UnlistenFn> =>
  listen<string>("navigate", (event) => cb(event.payload));
