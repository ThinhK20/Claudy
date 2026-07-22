import { describe, expect, it } from "vitest";
import { insertAtCursor } from "@/lib/insert-at-cursor";

describe("insertAtCursor", () => {
  it("inserts into an empty box", () => {
    expect(insertAtCursor("", "hello", 0, 0)).toEqual({ text: "hello", caret: 5 });
  });

  it("appends after existing text with a separating space", () => {
    expect(insertAtCursor("hi", "there", 2, 2)).toEqual({ text: "hi there", caret: 8 });
  });

  it("does not double up a space when one already precedes the caret", () => {
    expect(insertAtCursor("hi ", "there", 3, 3)).toEqual({ text: "hi there", caret: 8 });
  });

  it("inserts mid-string, spacing on both sides as needed", () => {
    expect(insertAtCursor("ab cd", "X", 3, 3)).toEqual({ text: "ab X cd", caret: 4 });
  });

  it("replaces the selected range", () => {
    expect(insertAtCursor("foo bar", "baz", 4, 7)).toEqual({ text: "foo baz", caret: 7 });
  });

  it("trims surrounding whitespace on the inserted text", () => {
    expect(insertAtCursor("hi", "  there  ", 2, 2)).toEqual({ text: "hi there", caret: 8 });
  });

  it("leaves the value untouched for whitespace-only input", () => {
    expect(insertAtCursor("hi", "   ", 2, 2)).toEqual({ text: "hi", caret: 2 });
  });

  it("clamps out-of-range selection offsets", () => {
    expect(insertAtCursor("hi", "yo", 99, 99)).toEqual({ text: "hi yo", caret: 5 });
  });
});
