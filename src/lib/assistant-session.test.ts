import { describe, expect, test } from "vitest";

import { followUpHint, showsFollowUpRow } from "@/lib/assistant-session";

describe("followUpHint", () => {
  test("says nothing on the first question of a session", () => {
    expect(followUpHint(0, false)).toBeNull();
  });

  test("counts one earlier message in the singular", () => {
    expect(followUpHint(1, false)).toBe("Following up · 1 earlier message");
  });

  test("counts several earlier messages in the plural", () => {
    expect(followUpHint(3, false)).toBe("Following up · 3 earlier messages");
  });

  test("stays quiet in Remind Mode, which owns that row itself", () => {
    expect(followUpHint(3, true)).toBeNull();
  });
});

describe("showsFollowUpRow", () => {
  test("mirrors the window sizing truth table in the Rust input_size", () => {
    // (turnCount, remindMode) -> does the box need the extra label row?
    expect(showsFollowUpRow(0, false)).toBe(false);
    expect(showsFollowUpRow(1, false)).toBe(true);
    expect(showsFollowUpRow(0, true)).toBe(false);
    // Remind Mode already added the row, so the chip must not add a second.
    expect(showsFollowUpRow(4, true)).toBe(false);
  });
});
