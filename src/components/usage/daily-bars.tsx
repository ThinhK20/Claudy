import { useState } from "react";
import type { DayBucket } from "@/lib/usage-api";
import {
  barPercent,
  formatCost,
  formatDayFull,
  formatDayTick,
  formatTokens,
} from "@/lib/usage-format";

/** Roughly how many x-axis ticks fit before they collide. */
const MAX_TICKS = 8;

/**
 * Runs per day. One series, so no legend — the heading names it.
 *
 * Bars use `--primary` rather than `--chart-1`: the chart ramp in index.css is
 * a monochrome placeholder with the *same* value in light and dark
 * (`oklch(0.87 0 0)`), which is nearly invisible on the white card. `--primary`
 * is chosen per mode (0.205 on light, 0.922 on dark), so contrast holds both
 * ways.
 */
export function DailyBars({ days }: { days: DayBucket[] }) {
  const [hovered, setHovered] = useState<number | null>(null);

  if (days.length === 0) {
    return (
      <p className="text-muted-foreground py-8 text-center text-sm">
        Nothing recorded yet.
      </p>
    );
  }

  const max = Math.max(...days.map((d) => d.runs));
  // Label the ends plus a few in between; every date collides past ~8.
  const tickEvery = Math.max(1, Math.ceil(days.length / MAX_TICKS));
  const shown = hovered === null ? null : days[hovered];

  return (
    <div className="flex flex-col gap-2">
      {/* A readout rather than a floating tooltip: one element, no layering,
          and it stays put while you sweep across the chart. */}
      <div className="flex min-h-5 flex-wrap items-baseline justify-between gap-x-4 text-sm">
        <span className="font-medium">
          {shown
            ? formatDayFull(shown.date)
            : `Peak ${max} ${max === 1 ? "run" : "runs"} in a day`}
        </span>
        <span className="text-muted-foreground tabular-nums">
          {shown
            ? `${shown.runs} ${shown.runs === 1 ? "run" : "runs"} · ${formatTokens(
                shown.inputTokens + shown.outputTokens,
              )} tokens · ${formatCost(shown.costUsd)}`
            : "Hover a day for detail"}
        </span>
      </div>

      <div
        role="img"
        aria-label={`Runs per day, ${formatDayFull(days[0].date)} to ${formatDayFull(
          days[days.length - 1].date,
        )}. Peak ${max} runs.`}
        className="border-border flex h-40 items-end gap-[2px] border-b"
        onMouseLeave={() => setHovered(null)}
      >
        {days.map((day, i) => (
          <button
            key={day.date}
            type="button"
            // Full-height hit target: the bar itself is unclickably short on a
            // quiet day, and a zero-run day has no bar at all.
            className="group flex h-full flex-1 cursor-default items-end"
            onMouseEnter={() => setHovered(i)}
            onFocus={() => setHovered(i)}
            onBlur={() => setHovered(null)}
            title={`${formatDayFull(day.date)}: ${day.runs} ${day.runs === 1 ? "run" : "runs"}`}
          >
            <span
              aria-hidden
              className={
                "bg-primary w-full rounded-t-[4px] transition-opacity " +
                (hovered !== null && hovered !== i ? "opacity-40" : "opacity-100")
              }
              // A day with runs always shows something; 0 stays empty so the
              // eye can tell "quiet" from "nothing".
              style={{
                height: day.runs > 0 ? `max(3px, ${barPercent(day.runs, max)}%)` : "0px",
              }}
            />
          </button>
        ))}
      </div>

      <div className="text-muted-foreground flex gap-[2px] text-xs">
        {days.map((day, i) => (
          <span key={day.date} className="flex-1 truncate text-center">
            {i % tickEvery === 0 || i === days.length - 1 ? formatDayTick(day.date) : ""}
          </span>
        ))}
      </div>
    </div>
  );
}
