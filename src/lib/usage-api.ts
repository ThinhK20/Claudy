import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** What produced a run. `other` is the sink for a value this build doesn't
 *  know — a hand-edited usage.json or a log written by a newer version. */
export type UsageSource =
  | "prompt"
  | "assistant"
  | "reminderParse"
  | "reminderCreated"
  | "reminderFired"
  | "other";

export type UsageRange = "7d" | "30d" | "90d" | "all";

export const RANGE_OPTIONS: { value: UsageRange; label: string }[] = [
  { value: "7d", label: "Last 7 days" },
  { value: "30d", label: "Last 30 days" },
  { value: "90d", label: "Last 90 days" },
  { value: "all", label: "All time" },
];

/** One column of the chart. Days with no activity are present with zeroes, so
 *  the axis is evenly spaced and has no gaps. */
export interface DayBucket {
  /** Local calendar date, "YYYY-MM-DD". */
  date: string;
  runs: number;
  inputTokens: number;
  outputTokens: number;
  costUsd: number;
}

export interface PromptStat {
  promptId: string;
  /** Snapshot from the most recent run — a deleted prompt still reads sensibly. */
  name: string;
  runs: number;
  failures: number;
  inputTokens: number;
  outputTokens: number;
  costUsd: number;
  /** RFC3339 with offset. */
  lastUsedAt: string;
}

export interface SourceStat {
  source: UsageSource;
  runs: number;
  inputTokens: number;
  outputTokens: number;
  costUsd: number;
}

export interface ModelStat {
  provider: string;
  model: string;
  runs: number;
  inputTokens: number;
  outputTokens: number;
  costUsd: number;
  /** False = tokens counted, but no price is known for this model. */
  priced: boolean;
}

export interface UsageSummary {
  range: UsageRange;
  totalRuns: number;
  failureCount: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  estimatedCostUsd: number;
  /** At least one run used an unpriced model — the total is a floor, not an
   *  estimate. Always surface this rather than presenting the number as whole. */
  costIsPartial: boolean;
  /** When the bundled price table was last checked, e.g. "2026-07". */
  pricesAsOf: string;
  daily: DayBucket[];
  topPrompts: PromptStat[];
  bySource: SourceStat[];
  byModel: ModelStat[];
}

/** A user price override, stored in settings under `usage.modelPrices`. */
export interface ModelPrice {
  model: string;
  inputPerMtok: number;
  outputPerMtok: number;
}

/** A row of the pricing editor: the effective price and where it came from. */
export interface ModelPriceRow {
  model: string;
  /** "" for a bundled-table entry that has never run here. */
  provider: string;
  inputPerMtok: number;
  outputPerMtok: number;
  overridden: boolean;
  priced: boolean;
  /** Appears in the usage log — the rows worth editing first. */
  inUse: boolean;
}

/** Everything the Usage page renders, aggregated in Rust. */
export const usageSummary = (range: UsageRange): Promise<UsageSummary> =>
  invoke("usage_summary", { range });

/** Wipes the log. No undo — keep it behind a confirmation. */
export const clearUsage = (): Promise<void> => invoke("clear_usage");

/** Models seen in the log, then overrides, then the bundled table. */
export const listModelPrices = (): Promise<ModelPriceRow[]> =>
  invoke("list_model_prices");

/** Fired by the backend after every write, so an open page refreshes without
 *  polling. */
export const onUsageChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("usage-changed", () => cb());
