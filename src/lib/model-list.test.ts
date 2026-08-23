import { describe, expect, test } from "vitest";
import { formatModelList, parseModelList } from "@/lib/model-list";

describe("parseModelList", () => {
  test("returns an empty list for an empty field", () => {
    // Empty is meaningful: it means "use the built-in chain", not "a model
    // with no name".
    expect(parseModelList("")).toEqual([]);
    expect(parseModelList("   ")).toEqual([]);
  });

  test("trims each entry and drops the blanks a stray comma leaves behind", () => {
    expect(parseModelList(" a , , b ")).toEqual(["a", "b"]);
    expect(parseModelList("gemini-2.5-flash,")).toEqual(["gemini-2.5-flash"]);
  });

  test("keeps the order the user typed, since it is the try order", () => {
    expect(parseModelList("gemini-2.5-flash, gemini-2.0-flash")).toEqual([
      "gemini-2.5-flash",
      "gemini-2.0-flash",
    ]);
  });
});

describe("formatModelList", () => {
  test("round-trips through parse without changing the list", () => {
    const models = ["claude-sonnet-5", "claude-haiku-4-5-20251001"];
    expect(parseModelList(formatModelList(models))).toEqual(models);
  });

  test("shows an unset list as an empty field", () => {
    expect(formatModelList([])).toBe("");
  });
});
