import { describe, expect, it } from "vitest";
import {
  isSpeechLevel,
  MAX_MS,
  nextPhase,
  NO_SPEECH_MS,
  SILENCE_MS,
  SPEECH_LEVEL,
  type ListenPhase,
} from "@/lib/silence-detector";

describe("isSpeechLevel", () => {
  it("treats the threshold itself as speech", () => {
    expect(isSpeechLevel(SPEECH_LEVEL)).toBe(true);
  });

  it("treats room noise and silence as not speech", () => {
    expect(isSpeechLevel(0)).toBe(false);
    expect(isSpeechLevel(SPEECH_LEVEL - 0.001)).toBe(false);
  });
});

describe("nextPhase", () => {
  it("starts speaking on the first loud sample", () => {
    expect(nextPhase("waiting", true, 0, 0)).toBe("speaking");
  });

  it("never ends the recording before any speech is heard", () => {
    // The moment right after the mic opens is always silent — ending there
    // would send an empty recording.
    expect(nextPhase("waiting", false, 10_000, 0)).toBe("waiting");
    expect(nextPhase("waiting", false, 10_000, NO_SPEECH_MS - 1)).toBe("waiting");
  });

  it("gives up when the wake word is followed by nothing", () => {
    expect(nextPhase("waiting", false, NO_SPEECH_MS, NO_SPEECH_MS)).toBe("gave-up");
  });

  it("ends the question after a long enough pause", () => {
    expect(nextPhase("speaking", false, SILENCE_MS, 3_000)).toBe("done");
  });

  it("survives a breath taken mid-sentence", () => {
    // The single most important behavior to get right: a short pause must not
    // cut the speaker off.
    expect(nextPhase("speaking", false, SILENCE_MS - 1, 3_000)).toBe("speaking");
  });

  it("keeps listening while speech continues", () => {
    expect(nextPhase("speaking", true, 0, 3_000)).toBe("speaking");
  });

  it("caps a very long question rather than losing it", () => {
    expect(nextPhase("speaking", true, 0, MAX_MS)).toBe("done");
  });

  it("holds terminal phases whatever it is fed", () => {
    for (const phase of ["done", "gave-up"] as ListenPhase[]) {
      expect(nextPhase(phase, true, 0, 0)).toBe(phase);
      expect(nextPhase(phase, false, 99_999, 99_999)).toBe(phase);
    }
  });
});
