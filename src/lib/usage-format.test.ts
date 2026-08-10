import { describe, expect, test } from "vitest";
import {
  barPercent,
  formatCost,
  formatDayFull,
  formatDayTick,
  formatTokens,
  parseDay,
  providerLabel,
  sourceLabel,
} from "@/lib/usage-format";

describe("parseDay", () => {
  test("reads a bucket date as local midnight, not UTC", () => {
    // `new Date("2026-07-24")` is UTC midnight — the previous day anywhere
    // west of London. Building from parts is what keeps the axis honest.
    const d = parseDay("2026-07-24")!;
    expect(d.getFullYear()).toBe(2026);
    expect(d.getMonth()).toBe(6);
    expect(d.getDate()).toBe(24);
    expect(d.getHours()).toBe(0);
  });

  test("rejects malformed and impossible dates instead of rolling them forward", () => {
    for (const bad of ["", "2026-7-24", "24-07-2026", "not-a-date", "2026-02-31", "2026-13-01"]) {
      expect(parseDay(bad), bad).toBeNull();
    }
  });
});

describe("day labels", () => {
  test("ticks are short and full labels name the weekday", () => {
    expect(formatDayTick("2026-07-24")).toBe("24 Jul");
    expect(formatDayTick("2026-01-05")).toBe("5 Jan");
    expect(formatDayFull("2026-07-24")).toBe("Fri 24 Jul 2026");
  });

  test("an unusable date never renders as 'NaN'", () => {
    expect(formatDayTick("garbage")).toBe("");
    expect(formatDayFull("garbage")).toBe("garbage");
  });
});

describe("formatTokens", () => {
  test("shows exact counts below a thousand", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(412)).toBe("412");
    expect(formatTokens(999)).toBe("999");
  });

  test("switches to compact units and drops a trailing .0", () => {
    expect(formatTokens(1_000)).toBe("1k");
    expect(formatTokens(1_240)).toBe("1.2k");
    expect(formatTokens(18_400)).toBe("18k");
    expect(formatTokens(999_400)).toBe("999k");
    expect(formatTokens(1_200_000)).toBe("1.2M");
    expect(formatTokens(24_000_000)).toBe("24M");
  });

  test("nonsense input renders as zero rather than NaN", () => {
    expect(formatTokens(-5)).toBe("0");
    expect(formatTokens(Number.NaN)).toBe("0");
    expect(formatTokens(Number.POSITIVE_INFINITY)).toBe("0");
  });
});

describe("formatCost", () => {
  test("keeps four decimals below a cent, so a single run isn't rounded to nothing", () => {
    // One prompt run against Sonnet: 412 in + 96 out.
    expect(formatCost(0.002676)).toBe("$0.0027");
    expect(formatCost(0.0001)).toBe("$0.0001");
  });

  test("uses two decimals and thousands separators once it's real money", () => {
    expect(formatCost(0.01)).toBe("$0.01");
    expect(formatCost(12.3456)).toBe("$12.35");
    expect(formatCost(1234.5)).toBe("$1,234.50");
  });

  test("zero and nonsense both read as $0.00", () => {
    expect(formatCost(0)).toBe("$0.00");
    expect(formatCost(-1)).toBe("$0.00");
    expect(formatCost(Number.NaN)).toBe("$0.00");
  });
});

describe("barPercent", () => {
  test("scales against the tallest bar", () => {
    expect(barPercent(10, 20)).toBe(50);
    expect(barPercent(20, 20)).toBe(100);
    expect(barPercent(0, 20)).toBe(0);
  });

  test("an all-zero range produces no bars instead of dividing by zero", () => {
    expect(barPercent(0, 0)).toBe(0);
    expect(barPercent(5, 0)).toBe(0);
  });

  test("never exceeds the track", () => {
    expect(barPercent(30, 20)).toBe(100);
  });
});

describe("labels", () => {
  test("every known source has a human name", () => {
    expect(sourceLabel("prompt")).toBe("Prompt shortcuts");
    expect(sourceLabel("assistant")).toBe("Quick-ask assistant");
    expect(sourceLabel("assistantSummary")).toBe("Quick Chat memory");
    expect(sourceLabel("reminderParse")).toBe("Reminder time parsing");
    expect(sourceLabel("reminderCreated")).toBe("Reminders created");
    expect(sourceLabel("reminderFired")).toBe("Reminders fired");
  });

  test("a source from a newer build falls back rather than leaking an enum name", () => {
    expect(sourceLabel("somethingNew")).toBe("Other");
    expect(sourceLabel("")).toBe("Other");
  });

  test("provider ids get their branded names, unknown ids pass through", () => {
    expect(providerLabel("openai_compatible")).toBe("OpenAI-compatible");
    expect(providerLabel("anthropic")).toBe("Anthropic");
    expect(providerLabel("my-gateway")).toBe("my-gateway");
  });
});
