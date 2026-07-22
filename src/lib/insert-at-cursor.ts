export interface InsertResult {
  /// The new textarea value with the text inserted.
  text: string;
  /// Where the caret should sit afterwards — right after the inserted text.
  caret: number;
}

const clamp = (n: number, min: number, max: number): number =>
  Math.min(Math.max(n, min), max);

/// Insert `insert` into `value`, replacing the [selStart, selEnd) selection.
/// Surrounding whitespace on `insert` is trimmed and a single separating space
/// is added when it would otherwise butt up against existing non-whitespace, so
/// dropping transcribed speech into the middle of a prompt reads naturally.
export function insertAtCursor(
  value: string,
  insert: string,
  selStart: number,
  selEnd: number,
): InsertResult {
  const piece = insert.trim();
  const start = clamp(selStart, 0, value.length);
  const end = clamp(selEnd, start, value.length);

  if (piece === "") {
    // Nothing to add — leave the value untouched, caret at the selection end.
    return { text: value, caret: end };
  }

  const before = value.slice(0, start);
  const after = value.slice(end);

  const lead = before.length > 0 && !/\s$/.test(before) ? " " : "";
  const trail = after.length > 0 && !/^\s/.test(after) ? " " : "";

  const text = `${before}${lead}${piece}${trail}${after}`;
  const caret = before.length + lead.length + piece.length;
  return { text, caret };
}
