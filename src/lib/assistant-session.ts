/**
 * Copy and layout rules for the quick-chat follow-up indicator.
 *
 * Pure so it is testable at all: the ask box itself is a `.tsx` component and
 * the vitest config only collects `.test.ts` files.
 */

/**
 * The subtle "you are still in the same conversation" line above the textarea,
 * or `null` when there is nothing to say.
 *
 * Remind Mode returns `null` on purpose: it already owns that row with its own
 * label, and a reminder is not part of the conversation anyway.
 */
export function followUpHint(turnCount: number, remindMode: boolean): string | null {
  if (remindMode || turnCount < 1) return null;
  const plural = turnCount === 1 ? "message" : "messages";
  return `Following up · ${turnCount} earlier ${plural}`;
}

/**
 * Whether the ask box reserves its label row for the follow-up chip. Mirrors
 * `input_size` in `src-tauri/src/assistant/mod.rs`, which sizes the window for
 * exactly this row — the two must agree or the box clips.
 */
export function showsFollowUpRow(turnCount: number, remindMode: boolean): boolean {
  return followUpHint(turnCount, remindMode) !== null;
}
