import { describe, expect, test } from "vitest";
import { errorMessage } from "@/lib/error-message";

describe("errorMessage", () => {
  test("passes a Tauri command's string rejection through unchanged", () => {
    expect(errorMessage("Model not found")).toBe("Model not found");
  });

  test("uses an Error's message without the 'Error:' prefix", () => {
    expect(errorMessage(new Error("Network down"))).toBe("Network down");
  });

  test("reads a message field from a plain object", () => {
    expect(errorMessage({ message: "Bad key" })).toBe("Bad key");
  });

  test("falls back to a generic sentence for empty or unknown values", () => {
    expect(errorMessage("")).toBe("Something went wrong.");
    expect(errorMessage("   ")).toBe("Something went wrong.");
    expect(errorMessage(null)).toBe("Something went wrong.");
    expect(errorMessage(undefined)).toBe("Something went wrong.");
    expect(errorMessage(new Error(""))).toBe("Something went wrong.");
  });

  test("stringifies other values rather than dropping them", () => {
    expect(errorMessage(42)).toBe("42");
  });
});
