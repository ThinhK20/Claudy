import { describe, expect, test } from "vitest";
import {
  UNKNOWN_DUE_LABEL,
  byDueAt,
  formatDayLabel,
  formatDue,
  fromDateTimeLocal,
  isOverdue,
  joinDateTimeLocal,
  parseDue,
  splitDateTimeLocal,
  toDateTimeLocal,
} from "@/lib/reminder-format";

// Fri 24 Jul 2026, 14:30 local — the same reference point the Rust parser
// tests use. Built from local parts so the suite is timezone-independent.
const NOW = new Date(2026, 6, 24, 14, 30, 0);

/** An RFC3339 `dueAt` at a local wall-clock time, as the backend would store it. */
function due(
  year: number,
  month: number,
  day: number,
  hour: number,
  minute = 0,
): string {
  return fromDateTimeLocal(new Date(year, month - 1, day, hour, minute).toString());
}

describe("formatDue", () => {
  test("names today, tomorrow and yesterday instead of printing a date", () => {
    expect(formatDue(due(2026, 7, 24, 16, 32), NOW)).toBe("Today 16:32");
    expect(formatDue(due(2026, 7, 25, 9), NOW)).toBe("Tomorrow 09:00");
    expect(formatDue(due(2026, 7, 23, 11, 30), NOW)).toBe("Yesterday 11:30");
  });

  test("falls back to a weekday and date further out", () => {
    expect(formatDue(due(2026, 7, 29, 14, 30), NOW)).toBe("Wed 29 Jul 14:30");
    expect(formatDue(due(2026, 8, 12, 9), NOW)).toBe("Wed 12 Aug 09:00");
  });

  test("pads single-digit hours and minutes", () => {
    expect(formatDue(due(2026, 7, 24, 9, 5), NOW)).toBe("Today 09:05");
  });

  test("says so rather than rendering Invalid Date", () => {
    expect(formatDue("not-a-timestamp", NOW)).toBe(UNKNOWN_DUE_LABEL);
    expect(formatDue("", NOW)).toBe(UNKNOWN_DUE_LABEL);
  });
});

describe("isOverdue", () => {
  test("is true only once the due time has arrived", () => {
    expect(isOverdue(due(2026, 7, 24, 14, 29), NOW)).toBe(true);
    expect(isOverdue(due(2026, 7, 24, 14, 30), NOW)).toBe(true);
    expect(isOverdue(due(2026, 7, 24, 14, 31), NOW)).toBe(false);
  });

  test("treats an unreadable due time as overdue so it surfaces", () => {
    expect(isOverdue("not-a-timestamp", NOW)).toBe(true);
  });
});

describe("byDueAt", () => {
  test("orders soonest first and floats unreadable entries to the top", () => {
    const list = [
      { dueAt: due(2026, 7, 26, 9) },
      { dueAt: "broken" },
      { dueAt: due(2026, 7, 25, 9) },
    ];
    const sorted = [...list].sort(byDueAt);
    expect(sorted[0].dueAt).toBe("broken");
    expect(sorted[1].dueAt).toBe(due(2026, 7, 25, 9));
    expect(sorted[2].dueAt).toBe(due(2026, 7, 26, 9));
  });
});

describe("datetime-local round trip", () => {
  test("renders the local wall clock the input element expects", () => {
    expect(toDateTimeLocal(due(2026, 7, 24, 16, 32))).toBe("2026-07-24T16:32");
    expect(toDateTimeLocal(due(2026, 12, 5, 9, 5))).toBe("2026-12-05T09:05");
  });

  test("survives a round trip back to RFC3339", () => {
    const original = due(2026, 7, 24, 16, 32);
    const restored = fromDateTimeLocal(toDateTimeLocal(original));
    expect(parseDue(restored)?.getTime()).toBe(parseDue(original)?.getTime());
  });

  test("returns empty strings for unusable input instead of throwing", () => {
    expect(toDateTimeLocal("nope")).toBe("");
    expect(fromDateTimeLocal("nope")).toBe("");
  });
});

describe("splitDateTimeLocal", () => {
  test("gives back the day at local midnight, not UTC midnight", () => {
    const { date, time } = splitDateTimeLocal("2026-07-24T16:32");
    // Would be the 23rd anywhere west of London if this parsed as UTC.
    expect(date?.getDate()).toBe(24);
    expect(date?.getMonth()).toBe(6);
    expect(date?.getFullYear()).toBe(2026);
    expect(date?.getHours()).toBe(0);
    expect(time).toBe("16:32");
  });

  test("drops seconds a time field may append", () => {
    expect(splitDateTimeLocal("2026-07-24T16:32:09").time).toBe("16:32");
  });

  test("reports each unusable half rather than throwing", () => {
    expect(splitDateTimeLocal("")).toEqual({ date: null, time: "" });
    expect(splitDateTimeLocal("nope").date).toBeNull();
    expect(splitDateTimeLocal("2026-13-01T09:00").date).toBeNull();
    expect(splitDateTimeLocal("2026-07-24").time).toBe("");
    expect(splitDateTimeLocal("2026-07-24T1").time).toBe("");
  });
});

describe("joinDateTimeLocal", () => {
  test("pads the date back into the shape toDateTimeLocal produces", () => {
    expect(joinDateTimeLocal(new Date(2026, 11, 5), "09:05")).toBe("2026-12-05T09:05");
  });

  test("survives a round trip through the split", () => {
    const original = "2026-07-24T16:32";
    const { date, time } = splitDateTimeLocal(original);
    expect(joinDateTimeLocal(date!, time)).toBe(original);
  });

  test("falls back to midnight for a clock still being typed", () => {
    expect(joinDateTimeLocal(new Date(2026, 6, 24), "")).toBe("2026-07-24T00:00");
    expect(joinDateTimeLocal(new Date(2026, 6, 24), "9:")).toBe("2026-07-24T00:00");
  });
});

describe("formatDayLabel", () => {
  test("names the weekday and spells the month, with no clock", () => {
    expect(formatDayLabel(new Date(2026, 6, 25))).toBe("Sat 25 Jul 2026");
    expect(formatDayLabel(new Date(2026, 11, 5, 23, 59))).toBe("Sat 5 Dec 2026");
  });
});
