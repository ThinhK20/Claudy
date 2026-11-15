import { describe, expect, test } from "vitest";
import {
  isSetupComplete,
  isSetupItemDone,
  parseStoredIds,
  SETUP_ITEM_IDS,
  type SetupFacts,
} from "@/lib/setup-checklist";

const NOTHING: SetupFacts = { hasSpeechModel: false, hasProviderKey: false };
const ALL_AUTO: SetupFacts = { hasSpeechModel: true, hasProviderKey: true };

describe("isSetupItemDone", () => {
  test("ticks the speech model item from settings, not from a manual mark", () => {
    expect(isSetupItemDone("speech-model", NOTHING, new Set())).toBe(false);
    expect(isSetupItemDone("speech-model", ALL_AUTO, new Set())).toBe(true);
    // A stale manual mark cannot claim a model that isn't there.
    expect(isSetupItemDone("speech-model", NOTHING, new Set(["speech-model"]))).toBe(false);
  });

  test("ticks the provider item from the stored key", () => {
    expect(isSetupItemDone("provider", NOTHING, new Set())).toBe(false);
    expect(isSetupItemDone("provider", ALL_AUTO, new Set())).toBe(true);
  });

  test("treats an unknown key state as not done", () => {
    expect(
      isSetupItemDone("provider", { hasSpeechModel: true, hasProviderKey: null }, new Set()),
    ).toBe(false);
  });

  test("ticks items the frontend cannot observe only when marked by hand", () => {
    expect(isSetupItemDone("mic-test", ALL_AUTO, new Set())).toBe(false);
    expect(isSetupItemDone("mic-test", NOTHING, new Set(["mic-test"]))).toBe(true);
    expect(isSetupItemDone("shortcut", NOTHING, new Set(["shortcut"]))).toBe(true);
  });
});

describe("isSetupComplete", () => {
  test("needs every item", () => {
    expect(isSetupComplete(ALL_AUTO, new Set(["mic-test"]))).toBe(false);
    expect(isSetupComplete(ALL_AUTO, new Set(["mic-test", "shortcut"]))).toBe(true);
    expect(isSetupComplete(NOTHING, new Set(SETUP_ITEM_IDS))).toBe(false);
  });
});

describe("parseStoredIds", () => {
  test("reads a stored list and drops anything that isn't a known id", () => {
    expect(parseStoredIds('["mic-test","bogus",3]')).toEqual(["mic-test"]);
  });

  test("returns an empty list for missing or corrupt storage", () => {
    expect(parseStoredIds(null)).toEqual([]);
    expect(parseStoredIds("not json")).toEqual([]);
    expect(parseStoredIds('{"a":1}')).toEqual([]);
  });
});
