import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { DailyBars } from "@/components/usage/daily-bars";
import { ModelPrices } from "@/components/usage/model-prices";
import { useSettings } from "@/lib/settings-store";
import {
  onUsageChanged,
  RANGE_OPTIONS,
  usageSummary,
  type UsageRange,
  type UsageSummary,
} from "@/lib/usage-api";
import { formatCost, formatTokens, providerLabel, sourceLabel } from "@/lib/usage-format";

export default function UsagePage() {
  const recording = useSettings((s) => s.settings?.usage.enabled ?? true);
  const [range, setRange] = useState<UsageRange>("7d");
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  const [showPricing, setShowPricing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setSummary(await usageSummary(range));
      setError(null);
    } catch (e: unknown) {
      setError(String(e));
    }
  }, [range]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // Recording happens in the background; refresh without polling.
  useEffect(() => {
    const unlisten = onUsageChanged(() => void reload());
    return () => {
      unlisten.then((f) => f());
    };
  }, [reload]);

  return (
    <div className="flex flex-col gap-6 p-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold">Usage</h1>
          <p className="text-muted-foreground mt-1">
            What you've run and what it cost. Kept on this computer only.
          </p>
        </div>
        <Select value={range} onValueChange={(v) => setRange(v as UsageRange)}>
          <SelectTrigger className="w-44">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {RANGE_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {error && <p className="text-destructive text-sm">{error}</p>}

      {!recording && (
        <Card>
          <CardContent className="text-muted-foreground text-sm">
            Recording is off, so nothing new is being added. Turn it back on under
            Settings → Usage stats.
          </CardContent>
        </Card>
      )}

      {summary && (
        <>
          <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
            <StatTile label="Runs" value={String(summary.totalRuns)} />
            <StatTile
              label="Tokens in / out"
              value={`${formatTokens(summary.totalInputTokens)} / ${formatTokens(
                summary.totalOutputTokens,
              )}`}
            />
            <StatTile
              label="Estimated cost"
              value={formatCost(summary.estimatedCostUsd)}
              note={
                summary.costIsPartial
                  ? "At least — some models have no price"
                  : `Prices as of ${summary.pricesAsOf}`
              }
            />
            <StatTile
              label="Failures"
              value={String(summary.failureCount)}
              note={summary.failureCount > 0 ? "Runs that errored" : undefined}
            />
          </div>

          <Card>
            <CardHeader>
              <CardTitle>Runs per day</CardTitle>
            </CardHeader>
            <CardContent>
              <DailyBars days={summary.daily} />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Most used prompts</CardTitle>
              <CardDescription>
                Prompt shortcuts only — the assistant and reminders are listed under
                Activity.
              </CardDescription>
            </CardHeader>
            <CardContent>
              {summary.topPrompts.length === 0 ? (
                <p className="text-muted-foreground text-sm">
                  No prompt shortcuts run in this period.
                </p>
              ) : (
                <div className="overflow-x-auto">
                  <table className="w-full min-w-[32rem] text-sm">
                    <thead>
                      <tr className="text-muted-foreground border-b text-left">
                        <th className="py-2 pr-4 font-medium">Prompt</th>
                        <th className="py-2 pr-4 text-right font-medium">Runs</th>
                        <th className="py-2 pr-4 text-right font-medium">Tokens</th>
                        <th className="py-2 text-right font-medium">Cost</th>
                      </tr>
                    </thead>
                    <tbody>
                      {summary.topPrompts.map((p) => (
                        <tr key={p.promptId} className="border-b last:border-0">
                          <td className="py-2 pr-4">
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="font-medium">{p.name || p.promptId}</span>
                              {p.failures > 0 && (
                                <Badge variant="outline">{p.failures} failed</Badge>
                              )}
                            </div>
                          </td>
                          <td className="py-2 pr-4 text-right tabular-nums">{p.runs}</td>
                          <td className="py-2 pr-4 text-right tabular-nums">
                            {formatTokens(p.inputTokens + p.outputTokens)}
                          </td>
                          <td className="py-2 text-right tabular-nums">
                            {formatCost(p.costUsd)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </CardContent>
          </Card>

          <div className="grid gap-6 xl:grid-cols-2">
            <Card>
              <CardHeader>
                <CardTitle>Activity</CardTitle>
                <CardDescription>Where the runs came from.</CardDescription>
              </CardHeader>
              <CardContent>
                {summary.bySource.length === 0 ? (
                  <p className="text-muted-foreground text-sm">Nothing recorded yet.</p>
                ) : (
                  <ul className="flex flex-col gap-2 text-sm">
                    {summary.bySource.map((s) => (
                      <li
                        key={s.source}
                        className="flex items-baseline justify-between gap-4 border-b pb-2 last:border-0 last:pb-0"
                      >
                        <span>{sourceLabel(s.source)}</span>
                        <span className="text-muted-foreground tabular-nums">
                          {s.runs} {s.runs === 1 ? "run" : "runs"}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle>By model</CardTitle>
                <CardDescription>Where the money goes.</CardDescription>
              </CardHeader>
              <CardContent>
                {summary.byModel.length === 0 ? (
                  <p className="text-muted-foreground text-sm">Nothing recorded yet.</p>
                ) : (
                  <ul className="flex flex-col gap-2 text-sm">
                    {summary.byModel.map((m) => (
                      <li
                        key={`${m.provider}/${m.model}`}
                        className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 border-b pb-2 last:border-0 last:pb-0"
                      >
                        <span className="flex flex-wrap items-center gap-2">
                          <span className="font-medium">{m.model}</span>
                          <span className="text-muted-foreground text-xs">
                            {providerLabel(m.provider)}
                          </span>
                          {!m.priced && <Badge variant="outline">No price</Badge>}
                        </span>
                        <span className="text-muted-foreground tabular-nums">
                          {formatTokens(m.inputTokens + m.outputTokens)} ·{" "}
                          {formatCost(m.costUsd)}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </CardContent>
            </Card>
          </div>

          <Card>
            <CardHeader>
              <CardTitle>Pricing</CardTitle>
              <CardDescription>
                What each model costs, used for the estimates above.
              </CardDescription>
            </CardHeader>
            <CardContent>
              {showPricing ? (
                <ModelPrices pricesAsOf={summary.pricesAsOf} />
              ) : (
                <Button variant="outline" size="sm" onClick={() => setShowPricing(true)}>
                  Edit model prices
                </Button>
              )}
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}

function StatTile({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <Card size="sm">
      <CardContent className="flex flex-col gap-1">
        <span className="text-muted-foreground text-sm">{label}</span>
        <span className="text-2xl font-semibold tabular-nums">{value}</span>
        {note && <span className="text-muted-foreground text-xs">{note}</span>}
      </CardContent>
    </Card>
  );
}
