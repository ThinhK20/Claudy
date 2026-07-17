import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type AssistantPhase =
  | "idle"
  | "input"
  | "loading"
  | "answering"
  | "speaking"
  | "error";

export interface AssistantState {
  phase: AssistantPhase;
  question: string | null;
  answer: string | null;
  message: string | null;
  ttsError: string | null;
}

/** Submit a question; the answer arrives via the `assistant-state` event. */
export const askAssistant = (question: string): Promise<void> =>
  invoke("ask_assistant", { question });

export const closeAssistant = (): Promise<void> => invoke("close_assistant");

/** Reset the panel to the input phase for a follow-up question. */
export const assistantNewQuestion = (): Promise<void> =>
  invoke("assistant_new_question");

export const stopAssistantSpeech = (): Promise<void> =>
  invoke("stop_assistant_speech");

export const replayAssistantSpeech = (): Promise<void> =>
  invoke("replay_assistant_speech");

// --- Voice model (Kokoro TTS) assets ---

export interface TtsAssetInfo {
  id: string;
  label: string;
  size: string;
  downloaded: boolean;
}

export const ttsModelStatus = (): Promise<TtsAssetInfo[]> => invoke("tts_model_status");

export const downloadTtsModel = (id: string): Promise<void> =>
  invoke("download_tts_model", { id });

export const deleteTtsModel = (id: string): Promise<void> =>
  invoke("delete_tts_model", { id });

/** Stored phase (event-only phases like "error" are never returned here). */
export const getAssistantPhase = (): Promise<AssistantPhase> =>
  invoke("get_assistant_state");

export const onAssistantState = (
  cb: (state: AssistantState) => void,
): Promise<UnlistenFn> =>
  listen<AssistantState>("assistant-state", (event) => cb(event.payload));
