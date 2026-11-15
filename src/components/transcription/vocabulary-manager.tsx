import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowRight, Download, Plus, Trash2, Upload } from "lucide-react";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { InlineStatus } from "@/components/inline-status";
import { ToggleRow } from "@/components/toggle-row";
import { errorMessage } from "@/lib/error-message";
import { useSettings } from "@/lib/settings-store";
import {
  exportVocabulary,
  importVocabulary,
  previewVocabulary,
  type VocabRule,
} from "@/lib/vocabulary-api";

/** Mirrors `MAX_RULES` / `MAX_RULE_CHARS` in `src-tauri/src/vocabulary.rs`. */
const MAX_RULES = 200;
const MAX_RULE_CHARS = 200;

/** How long typing settles before the preview re-runs. Long enough not to
 *  invoke on every keystroke, short enough to still feel live. */
const PREVIEW_DEBOUNCE_MS = 200;

const JSON_FILTER = [{ name: "JSON", extensions: ["json"] }];

export function VocabularyManager() {
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);

  // Local draft of the list so typing stays smooth; committed to settings on
  // blur and on every structural change (add/delete/toggle). Same approach as
  // the price editor, which has the same "don't write on every keystroke"
  // problem.
  const [rules, setRules] = useState<VocabRule[]>([]);
  const [sample, setSample] = useState("");
  const [preview, setPreview] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<VocabRule | null>(null);

  const stored = settings?.vocabulary.rules;

  // Re-sync when the stored list changes identity (first load, or an import
  // that wrote settings behind our back).
  useEffect(() => {
    if (stored) setRules(stored);
  }, [stored]);

  const save = useCallback(
    async (next: VocabRule[]) => {
      if (!settings) return;
      setRules(next); // optimistic, so a row never flickers back
      try {
        await update({ vocabulary: { ...settings.vocabulary, rules: next } });
        setError(null);
      } catch (e: unknown) {
        setError(`Couldn't save your rules: ${errorMessage(e)}`);
      }
    },
    [settings, update],
  );

  // Debounced live preview, run against the DRAFT rules so a rule can be seen
  // working before it is committed. Sequence-guarded: a slow invoke must never
  // overwrite the result of a newer one.
  const previewSeq = useRef(0);
  useEffect(() => {
    if (!sample.trim()) {
      setPreview("");
      return;
    }
    const seq = ++previewSeq.current;
    const timer = setTimeout(() => {
      previewVocabulary(sample, rules)
        .then((out) => {
          if (seq === previewSeq.current) setPreview(out);
        })
        .catch((e: unknown) => {
          if (seq === previewSeq.current) setError(`Preview failed: ${errorMessage(e)}`);
        });
    }, PREVIEW_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [sample, rules]);

  if (!settings) return null;

  const addRule = () => {
    if (rules.length >= MAX_RULES) {
      setError(`You've reached the limit of ${MAX_RULES} rules.`);
      return;
    }
    // Draft only, not saved: a blank rule changes nothing, and saving here
    // would write an empty row every time the button is pressed.
    setRules([
      ...rules,
      { id: crypto.randomUUID(), from: "", to: "", enabled: true, matchCase: false },
    ]);
  };

  const editRule = (id: string, patch: Partial<VocabRule>) =>
    setRules(rules.map((r) => (r.id === id ? { ...r, ...patch } : r)));

  const commitRule = (id: string, patch: Partial<VocabRule>) =>
    void save(rules.map((r) => (r.id === id ? { ...r, ...patch } : r)));

  const removeRule = (id: string) => void save(rules.filter((r) => r.id !== id));

  // A blank draft row holds nothing worth confirming; a real rule does.
  const requestDelete = (id: string) => {
    const rule = rules.find((r) => r.id === id);
    if (!rule) return;
    if (!rule.from.trim() && !rule.to.trim()) removeRule(id);
    else setToDelete(rule);
  };

  const setEnabled = async (enabled: boolean) => {
    setError(null);
    try {
      await update({ vocabulary: { ...settings.vocabulary, enabled } });
    } catch (e: unknown) {
      setError(`Couldn't save: ${errorMessage(e)}`);
    }
  };

  const doExport = async () => {
    setError(null);
    setReport(null);
    try {
      const path = await saveFile({
        defaultPath: "claudy-vocabulary.json",
        filters: JSON_FILTER,
      });
      if (!path) return; // user cancelled
      const count = await exportVocabulary(path);
      setReport(`Exported ${count} rule${count === 1 ? "" : "s"}.`);
    } catch (e: unknown) {
      setError(`Export failed: ${errorMessage(e)}`);
    }
  };

  const doImport = async () => {
    setError(null);
    setReport(null);
    try {
      const path = await openFile({ multiple: false, filters: JSON_FILTER });
      if (typeof path !== "string") return; // user cancelled
      const r = await importVocabulary(path);
      // The command wrote settings directly, so re-read rather than trusting
      // the local draft.
      await useSettings.getState().load();
      setReport(
        `Imported: ${r.added} added, ${r.updated} updated.` +
          (r.skipped ? ` ${r.skipped} skipped — the list is full.` : ""),
      );
    } catch (e: unknown) {
      setError(`Import failed: ${errorMessage(e)}`);
    }
  };

  const activeCount = rules.filter((r) => r.enabled && r.from.trim()).length;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Custom vocabulary</CardTitle>
        <CardDescription>
          Names, jargon and product names are what speech models get wrong most.
          Tell Claudy how to spell them and every transcript is corrected before it
          reaches the app you're typing into.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        <ToggleRow
          label="Apply corrections"
          description="Turn off to see raw transcripts without deleting your rules."
          checked={settings.vocabulary.enabled}
          onChange={(enabled) => void setEnabled(enabled)}
        />

        {error && <InlineStatus onDismiss={() => setError(null)}>{error}</InlineStatus>}
        {report && <InlineStatus tone="success">{report}</InlineStatus>}

        {rules.length === 0 ? (
          <p className="text-muted-foreground text-sm">
            No rules yet. Add one to turn what Claudy hears — say, "get hub" — into
            what you meant: "GitHub".
          </p>
        ) : (
          // Radix throws unless a Root has a Provider above it, and these are
          // the app's first tooltips — so the provider lives here rather than
          // at the app root, where nothing else would yet need it.
          <TooltipProvider>
            <div className="flex flex-col gap-2">
              {/* Column headers, so the two inputs aren't a guessing game. */}
              <div className="text-muted-foreground flex items-center gap-2 text-xs">
                <span className="flex-1">When Claudy hears</span>
                <span className="w-4" aria-hidden="true" />
                <span className="flex-1">write this instead</span>
                <span className="w-[8.5rem]" />
              </div>
              {rules.map((rule) => (
                <RuleRow
                  key={rule.id}
                  rule={rule}
                  onEdit={editRule}
                  onCommit={commitRule}
                  onDelete={requestDelete}
                />
              ))}
            </div>
          </TooltipProvider>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" onClick={addRule}>
            <Plus className="size-4" />
            Add rule
          </Button>
          <Button variant="outline" size="sm" onClick={() => void doImport()}>
            <Upload className="size-4" />
            Import
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void doExport()}
            disabled={rules.length === 0}
          >
            <Download className="size-4" />
            Export
          </Button>
          <span className="text-muted-foreground ml-auto text-xs tabular-nums">
            {activeCount} active of {rules.length}
          </span>
        </div>

        <div className="flex flex-col gap-2 border-t pt-4">
          <Label htmlFor="vocab-sample">Try it</Label>
          <Textarea
            id="vocab-sample"
            rows={2}
            placeholder="Paste something Claudy misheard and watch your rules fix it."
            value={sample}
            onChange={(e) => setSample(e.target.value)}
          />
          {sample.trim() && (
            <div className="bg-muted/50 rounded-md border p-3 text-sm">
              {/* Runs the same Rust matcher dictation uses, so what shows here
                  is exactly what would have been typed. */}
              {preview === sample ? (
                <span className="text-muted-foreground">
                  No rule matched — this text would be typed unchanged.
                </span>
              ) : (
                <span className="whitespace-pre-wrap">{preview}</span>
              )}
            </div>
          )}
        </div>
      </CardContent>

      <ConfirmDialog
        open={toDelete !== null}
        onOpenChange={(open) => {
          if (!open) setToDelete(null);
        }}
        title="Delete this rule?"
        description={`"${toDelete?.from ?? ""}" will no longer be corrected to "${toDelete?.to ?? ""}". This can't be undone.`}
        confirmLabel="Delete"
        onConfirm={() => {
          if (toDelete) removeRule(toDelete.id);
          setToDelete(null);
        }}
      />
    </Card>
  );
}

function RuleRow({
  rule,
  onEdit,
  onCommit,
  onDelete,
}: {
  rule: VocabRule;
  onEdit: (id: string, patch: Partial<VocabRule>) => void;
  onCommit: (id: string, patch: Partial<VocabRule>) => void;
  onDelete: (id: string) => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <Input
        aria-label="Heard as"
        placeholder="get hub"
        maxLength={MAX_RULE_CHARS}
        className="h-9 flex-1"
        value={rule.from}
        onChange={(e) => onEdit(rule.id, { from: e.target.value })}
        onBlur={(e) => onCommit(rule.id, { from: e.target.value })}
      />
      <ArrowRight className="text-muted-foreground size-4 shrink-0" aria-hidden="true" />
      <Input
        aria-label="Written as"
        placeholder="GitHub"
        maxLength={MAX_RULE_CHARS}
        className="h-9 flex-1"
        value={rule.to}
        onChange={(e) => onEdit(rule.id, { to: e.target.value })}
        onBlur={(e) => onCommit(rule.id, { to: e.target.value })}
      />

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant={rule.matchCase ? "secondary" : "ghost"}
            size="sm"
            aria-label="Match case"
            aria-pressed={rule.matchCase}
            className="w-9 shrink-0 font-mono"
            onClick={() => onCommit(rule.id, { matchCase: !rule.matchCase })}
          >
            Aa
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {rule.matchCase
            ? "Matching this exact capitalisation only"
            : "Matching any capitalisation"}
        </TooltipContent>
      </Tooltip>

      <Tooltip>
        <TooltipTrigger asChild>
          {/* Wrapped in a span: Radix needs a single DOM node it can attach a
              hover target to, and Switch renders a button of its own. */}
          <span className="shrink-0">
            <Switch
              aria-label="Rule enabled"
              checked={rule.enabled}
              onCheckedChange={(enabled) => onCommit(rule.id, { enabled })}
            />
          </span>
        </TooltipTrigger>
        <TooltipContent>{rule.enabled ? "Rule is on" : "Rule is off"}</TooltipContent>
      </Tooltip>

      <Button
        variant="ghost"
        size="sm"
        aria-label="Delete rule"
        className="shrink-0"
        onClick={() => onDelete(rule.id)}
      >
        <Trash2 className="size-4" />
      </Button>
    </div>
  );
}
