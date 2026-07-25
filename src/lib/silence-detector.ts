/**
 * Decides when a spoken question has ended, from the live microphone level.
 *
 * Deliberately mirrors `wake::on_frame` in `src-tauri/src/wake.rs` — same
 * shape, same terminal-action style — so the two voice-activity detectors in
 * this app read as one idea rather than two inventions.
 */

/** Level at or above which a sample counts as speech. The `mic-level` event
 *  carries the same RMS scale as the wake listener, so this matches
 *  `SPEECH_RMS` in `wake.rs`. */
export const SPEECH_LEVEL = 0.02;

/** A pause this long ends the question. Long enough to survive the breath
 *  people take mid-sentence, short enough to still feel responsive. */
export const SILENCE_MS = 1200;

/** Said the wake word, then never actually asked anything. */
export const NO_SPEECH_MS = 7000;

/** Hard cap, so a very long question is still sent rather than lost. */
export const MAX_MS = 45000;

/** How often the caller should advance the detector. */
export const TICK_MS = 100;

export type ListenPhase = "waiting" | "speaking" | "done" | "gave-up";

export const isSpeechLevel = (level: number): boolean => level >= SPEECH_LEVEL;

/**
 * Pure: advance the detector by one sample. `done` and `gave-up` are terminal
 * — the caller acts on them and stops ticking.
 *
 * @param silentMs  time since the last sample at or above `SPEECH_LEVEL`
 * @param elapsedMs time since recording started
 */
export function nextPhase(
  phase: ListenPhase,
  isSpeech: boolean,
  silentMs: number,
  elapsedMs: number,
): ListenPhase {
  switch (phase) {
    case "waiting":
      // Never stops from here: the moment just after the microphone opens is
      // always silent, and ending there would send an empty recording.
      if (isSpeech) return "speaking";
      return elapsedMs >= NO_SPEECH_MS ? "gave-up" : "waiting";
    case "speaking":
      if (elapsedMs >= MAX_MS) return "done";
      return !isSpeech && silentMs >= SILENCE_MS ? "done" : "speaking";
    default:
      return phase; // terminal
  }
}
