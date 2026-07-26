import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useSettings } from "@/lib/settings-store";
import {
  listModelPrices,
  onUsageChanged,
  type ModelPrice,
  type ModelPriceRow,
} from "@/lib/usage-api";
import { providerLabel } from "@/lib/usage-format";

/** Reject a half-typed or nonsensical price rather than storing it — the Rust
 *  side ignores unusable overrides anyway, so keeping one would show a cost
 *  that doesn't match the setting. */
const usable = (n: number): boolean => Number.isFinite(n) && n >= 0;

export function ModelPrices({ pricesAsOf }: { pricesAsOf: string }) {
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);
  const [rows, setRows] = useState<ModelPriceRow[]>([]);
  const [showAll, setShowAll] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setRows(await listModelPrices());
      setError(null);
    } catch (e: unknown) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  // A new model joins the list the first time it runs.
  useEffect(() => {
    const unlisten = onUsageChanged(() => void reload());
    return () => {
      unlisten.then((f) => f());
    };
  }, [reload]);

  const overrides = settings?.usage.modelPrices ?? [];

  const save = async (modelPrices: ModelPrice[]) => {
    if (!settings) return;
    try {
      await update({ usage: { ...settings.usage, modelPrices } });
      setError(null);
    } catch (e: unknown) {
      setError(String(e));
    }
    // Re-read so `overridden` / `priced` reflect what actually got stored.
    await reload();
  };

  const setPrice = async (model: string, input: number, output: number) => {
    if (!usable(input) || !usable(output)) return;
    await save([
      ...overrides.filter((p) => p.model.toLowerCase() !== model.toLowerCase()),
      { model, inputPerMtok: input, outputPerMtok: output },
    ]);
  };

  const clearPrice = (model: string) =>
    save(overrides.filter((p) => p.model.toLowerCase() !== model.toLowerCase()));

  const visible = showAll ? rows : rows.filter((r) => r.inUse || r.overridden);

  return (
    <div className="flex flex-col gap-3">
      <p className="text-muted-foreground text-sm">
        Bundled prices were last checked in {pricesAsOf} and are list prices per
        million tokens. Providers change them without notice — edit any row to use
        your own. Ollama always counts as free.
      </p>
      {error && <p className="text-destructive text-sm">{error}</p>}

      {visible.length === 0 ? (
        <p className="text-muted-foreground text-sm">
          No models used yet. Run a prompt, or show the full list to set a price in
          advance.
        </p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[34rem] text-sm">
            <thead>
              <tr className="text-muted-foreground border-b text-left">
                {/* Model takes the slack so the price columns stay snug against
                    their inputs instead of drifting apart. */}
                <th className="w-full py-2 pr-4 font-medium">Model</th>
                <th className="w-28 py-2 pr-4 text-right font-medium whitespace-nowrap">
                  Input $/M
                </th>
                <th className="w-28 py-2 pr-4 text-right font-medium whitespace-nowrap">
                  Output $/M
                </th>
                <th className="py-2 font-medium" />
              </tr>
            </thead>
            <tbody>
              {visible.map((row) => (
                <PriceRow key={row.model} row={row} onSet={setPrice} onClear={clearPrice} />
              ))}
            </tbody>
          </table>
        </div>
      )}

      <div>
        <Button variant="outline" size="sm" onClick={() => setShowAll((v) => !v)}>
          {showAll ? "Show only models in use" : `Show all ${rows.length} models`}
        </Button>
      </div>
    </div>
  );
}

function PriceRow({
  row,
  onSet,
  onClear,
}: {
  row: ModelPriceRow;
  onSet: (model: string, input: number, output: number) => Promise<void>;
  onClear: (model: string) => Promise<void>;
}) {
  // Local draft so typing "1." or clearing the field doesn't write a broken
  // value on every keystroke; committed on blur.
  const [input, setInput] = useState(String(row.inputPerMtok));
  const [output, setOutput] = useState(String(row.outputPerMtok));

  useEffect(() => {
    setInput(String(row.inputPerMtok));
    setOutput(String(row.outputPerMtok));
  }, [row.inputPerMtok, row.outputPerMtok]);

  const commit = () => void onSet(row.model, Number(input), Number(output));

  return (
    <tr className="border-b last:border-0">
      <td className="py-2 pr-4">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-medium">{row.model}</span>
          {row.inUse && <Badge variant="secondary">In use</Badge>}
          {!row.priced && <Badge variant="outline">No price</Badge>}
          {row.overridden && <Badge>Custom</Badge>}
          {row.provider && (
            <span className="text-muted-foreground text-xs">
              {providerLabel(row.provider)}
            </span>
          )}
        </div>
      </td>
      <td className="py-2 pr-4">
        <div className="flex justify-end">
          <Input
            type="number"
            min="0"
            step="0.01"
            inputMode="decimal"
            aria-label={`Input price per million tokens for ${row.model}`}
            className="h-8 w-24 text-right tabular-nums"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onBlur={commit}
          />
        </div>
      </td>
      <td className="py-2 pr-4">
        <div className="flex justify-end">
          <Input
            type="number"
            min="0"
            step="0.01"
            inputMode="decimal"
            aria-label={`Output price per million tokens for ${row.model}`}
            className="h-8 w-24 text-right tabular-nums"
            value={output}
            onChange={(e) => setOutput(e.target.value)}
            onBlur={commit}
          />
        </div>
      </td>
      <td className="py-2 text-right">
        {row.overridden && (
          <Button variant="ghost" size="sm" onClick={() => void onClear(row.model)}>
            Reset
          </Button>
        )}
      </td>
    </tr>
  );
}
