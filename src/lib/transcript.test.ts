import { describe, expect, it } from "vitest";
import { isEffectivelyEmpty } from "@/lib/transcript";

describe("isEffectivelyEmpty", () => {
  it("treats blank text as empty", () => {
    expect(isEffectivelyEmpty("")).toBe(true);
    expect(isEffectivelyEmpty("   \n")).toBe(true);
  });

  it("treats whisper's non-speech placeholders as empty", () => {
    // Auto-sending one of these as a question is the failure this prevents.
    expect(isEffectivelyEmpty("[BLANK_AUDIO]")).toBe(true);
    expect(isEffectivelyEmpty(" (wind blowing) ")).toBe(true);
  });

  it("keeps real speech", () => {
    expect(isEffectivelyEmpty("What is the weather?")).toBe(false);
    expect(isEffectivelyEmpty("[unclear] but real speech")).toBe(false);
  });
});
