/** Shown when a stored `dueAt` can't be parsed (hand-edited reminders.json). */
export const UNKNOWN_DUE_LABEL = "Unknown time";

// Fixed English names rather than toLocaleDateString: the output must match
// the Rust-side `format_due_label` used in notifications, and locale-dependent
// formatting would make both the UI and its tests drift by machine.
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = [
  "Jan", "Feb", "Mar", "Apr", "May", "Jun",
  "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/** RFC3339 -> Date, or null when the value is unusable. */
export function parseDue(dueAt: string): Date | null {
  const d = new Date(dueAt);
  return Number.isNaN(d.getTime()) ? null : d;
}

const startOfDay = (d: Date): number =>
  new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

/** Whole calendar days from `now` to `due`. Rounded, so a DST shift inside the
 *  span can't turn "tomorrow" into 0.96 days. */
function dayOffset(due: Date, now: Date): number {
  return Math.round((startOfDay(due) - startOfDay(now)) / 86_400_000);
}

const clock = (d: Date): string =>
  `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;

/**
 * "Today 16:32" / "Tomorrow 09:00" / "Yesterday 11:30" / "Wed 29 Jul 14:30".
 * Mirrors the Rust `format_due_label` so a toast and its row agree.
 */
export function formatDue(dueAt: string, now: Date): string {
  const due = parseDue(dueAt);
  if (!due) return UNKNOWN_DUE_LABEL;
  const time = clock(due);
  switch (dayOffset(due, now)) {
    case 0:
      return `Today ${time}`;
    case 1:
      return `Tomorrow ${time}`;
    case -1:
      return `Yesterday ${time}`;
    default:
      return `${WEEKDAYS[due.getDay()]} ${due.getDate()} ${MONTHS[due.getMonth()]} ${time}`;
  }
}

/** Past due. An unparseable `dueAt` counts as overdue so it surfaces at the
 *  top of the list instead of hiding — the same call the Rust scheduler makes. */
export function isOverdue(dueAt: string, now: Date): boolean {
  const due = parseDue(dueAt);
  return due === null || due.getTime() <= now.getTime();
}

/** Sort comparator: soonest first, unparseable entries first so they're seen. */
export function byDueAt(a: { dueAt: string }, b: { dueAt: string }): number {
  const at = parseDue(a.dueAt)?.getTime() ?? -Infinity;
  const bt = parseDue(b.dueAt)?.getTime() ?? -Infinity;
  return at - bt;
}

const pad = (n: number): string => String(n).padStart(2, "0");

/**
 * Value for an `<input type="datetime-local">`, which wants local wall-clock
 * "YYYY-MM-DDTHH:mm" with no zone. Empty string when unparseable.
 */
export function toDateTimeLocal(dueAt: string): string {
  const d = parseDue(dueAt);
  if (!d) return "";
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}`
  );
}

/** Inverse of `toDateTimeLocal`: local wall-clock -> RFC3339 with this
 *  machine's offset, which is the shape the Rust side stores. */
export function fromDateTimeLocal(value: string): string {
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return "";
  const offsetMins = -d.getTimezoneOffset();
  const sign = offsetMins >= 0 ? "+" : "-";
  const abs = Math.abs(offsetMins);
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}:00` +
    `${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`
  );
}
