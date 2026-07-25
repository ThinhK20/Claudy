import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type WakePhase = "off" | "listening" | "triggered" | "error";

export interface WakeState {
  phase: WakePhase;
  message: string | null;
}

/** Is the wake-word listener holding the microphone right now? */
export const wakeWordListening = (): Promise<boolean> => invoke("wake_word_listening");

/** Advice for a phrase being typed; `""` when it looks usable. */
export const checkWakePhrase = (phrase: string): Promise<string> =>
  invoke("check_wake_phrase", { phrase });

export const onWakeState = (cb: (state: WakeState) => void): Promise<UnlistenFn> =>
  listen<WakeState>("wake-state", (event) => cb(event.payload));
