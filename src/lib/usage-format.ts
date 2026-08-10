import type { UsageSource } from "@/lib/usage-api";

// Fixed English month names rather than toLocaleDateString, for the same
// reason as reminder-format.ts: locale-dependent output would make the UI and
// its tests drift by machine.
const MONTHS = [
  "Jan", "Feb", "Mar", "Apr", "May", "Jun",
  "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/**
 * "YYYY-MM-DD" (a `DayBucket.date`) -> local-midnight Date, or null.
 * Built from parts rather than `new Date(s)`: a bare date string parses as UTC
 * midnight, which is the previous day anywhere west of London.
 */
export function parseDay(date: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) return null;
  const [y, m, d] = date.split("-").map(Number);
  const parsed = new Date(y, m - 1, d);
  // Rejects 2026-02-31 and friends, which Date would roll forward.
  return parsed.getMonth() === m - 1 && parsed.getDate() === d ? parsed : null;
}

/** Chart axis tick: "24 Jul". Empty string for an unusable date. */
export function formatDayTick(date: string): string {
  const d = parseDay(date);
  return d ? `${d.getDate()} ${MONTHS[d.getMonth()]}` : "";
}

/** Tooltip heading: "Fri 24 Jul 2026". Falls back to the raw value. */
export function formatDayFull(date: string): string {
  const d = parseDay(date);
  if (!d) return date;
  return `${WEEKDAYS[d.getDay()]} ${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()}`;
}

/** Drop a trailing ".0" so 1.0k reads as 1k. */
const trim = (n: number): string => String(Number(n.toFixed(1)));

/**
 * Compact token counts: 412, 1.2k, 18k, 1.2M.
 * One decimal only below 10 of a unit — past that the extra digit is noise.
 */
export function formatTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0";
  if (n < 1_000) return String(Math.round(n));
  if (n < 10_000) return `${trim(n / 1_000)}k`;
  if (n < 1_000_000) return `${Math.round(n / 1_000)}k`;
  if (n < 10_000_000) return `${trim(n / 1_000_000)}M`;
  return `${Math.round(n / 1_000_000)}M`;
}

/**
 * A cost figure. Sub-cent amounts keep four decimals rather than collapsing to
 * "<$0.01": a single prompt run genuinely costs fractions of a cent, and
 * rounding that away would make the whole table read as zero.
 */
export function formatCost(usd: number): string {
  if (!Number.isFinite(usd) || usd <= 0) return "$0.00";
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  return `$${usd.toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}`;
}

/** Bar height as a percentage of the tallest bar. 0 when there is no data. */
export function barPercent(value: number, max: number): number {
  if (!Number.isFinite(value) || !Number.isFinite(max) || max <= 0 || value <= 0) {
    return 0;
  }
  return Math.min(100, (value / max) * 100);
}

const SOURCE_LABELS: Record<UsageSource, string> = {
  prompt: "Prompt shortcuts",
  assistant: "Quick-ask assistant",
  assistantSummary: "Quick Chat memory",
  reminderParse: "Reminder time parsing",
  reminderCreated: "Reminders created",
  reminderFired: "Reminders fired",
  other: "Other",
};

/** Human name for a usage source. Unknown values — a log written by a newer
 *  build — fall back to "Other" rather than rendering a raw enum name. */
export function sourceLabel(source: string): string {
  return SOURCE_LABELS[source as UsageSource] ?? SOURCE_LABELS.other;
}

const PROVIDER_LABELS: Record<string, string> = {
  anthropic: "Anthropic",
  openai_compatible: "OpenAI-compatible",
  gemini: "Google Gemini",
  ollama: "Ollama",
};

/** Human name for a provider id; an unknown id renders as-is. */
export function providerLabel(id: string): string {
  return PROVIDER_LABELS[id] ?? id;
}
