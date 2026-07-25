/**
 * Whisper emits bracketed placeholders ("[BLANK_AUDIO]", "(wind blowing)")
 * when it hears no speech. Sending one of those as a question would be worse
 * than doing nothing.
 *
 * Mirrors `is_effectively_empty` in `src-tauri/src/dictation.rs`; keep the two
 * in step.
 */
export function isEffectivelyEmpty(text: string): boolean {
  const t = text.trim();
  return (
    t.length === 0 ||
    (t.startsWith("[") && t.endsWith("]")) ||
    (t.startsWith("(") && t.endsWith(")"))
  );
}
