import { useCallback, useEffect, useState } from "react";
import { Copy, Download, Loader2, Pencil, Play, Plus, Trash2, Upload } from "lucide-react";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import type { PageProps } from "@/components/app-sidebar";
import { InlineStatus } from "@/components/inline-status";
import { Page } from "@/components/page";
import { EMPTY_PROMPT, PromptEditor } from "@/components/prompt-editor";
import { SetupChecklist } from "@/components/setup-checklist";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  deletePrompt,
  exportPrompts,
  importPrompts,
  listPrompts,
  runPrompt,
  savePrompt,
  type Prompt,
} from "@/lib/ai-api";
import { errorMessage } from "@/lib/error-message";

export default function PromptsPage({ onNavigate }: PageProps) {
  const [prompts, setPrompts] = useState<Prompt[]>([]);
  const [search, setSearch] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<Prompt | null>(null);
  const [editing, setEditing] = useState<Prompt | null>(null);
  const [report, setReport] = useState<string | null>(null);
  // Per-row "Run now" state: which prompts are in flight, and why one failed.
  const [running, setRunning] = useState<ReadonlySet<string>>(new Set());
  const [runErrors, setRunErrors] = useState<Record<string, string>>({});

  const reload = useCallback(async () => {
    try {
      setPrompts(await listPrompts());
      setError(null);
    } catch (e: unknown) {
      setError(`Couldn't load prompts: ${errorMessage(e)}`);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const query = search.trim().toLowerCase();
  const visible = prompts.filter(
    (p) =>
      !query ||
      p.name.toLowerCase().includes(query) ||
      p.template.toLowerCase().includes(query),
  );

  const mutate = async (op: () => Promise<unknown>) => {
    try {
      await op();
      await reload();
    } catch (e: unknown) {
      setError(errorMessage(e));
    }
  };

  const clearRunError = (id: string) =>
    setRunErrors((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });

  // `run_prompt` only starts the run (the result and its errors arrive as a
  // notification), so this covers the hand-off: a rejected invoke is shown on
  // the row instead of being dropped.
  const runNow = async (p: Prompt) => {
    setRunning((prev) => new Set(prev).add(p.id));
    clearRunError(p.id);
    try {
      await runPrompt(p.id);
    } catch (e: unknown) {
      setRunErrors((prev) => ({ ...prev, [p.id]: `Run failed: ${errorMessage(e)}` }));
    } finally {
      setRunning((prev) => {
        const next = new Set(prev);
        next.delete(p.id);
        return next;
      });
    }
  };

  const toggleEnabled = (p: Prompt) =>
    mutate(() => savePrompt({ ...p, enabled: !p.enabled }));

  // Draft, not save: the copy opens in the editor. Its shortcut is cleared —
  // it would instantly conflict with its source.
  const duplicate = (p: Prompt) =>
    setEditing({ ...p, id: "", name: `${p.name} (copy)`, shortcut: "" });

  const confirmDelete = () => {
    if (toDelete) void mutate(() => deletePrompt(toDelete.id));
    setToDelete(null);
  };

  const JSON_FILTER = [{ name: "JSON", extensions: ["json"] }];

  const doExport = async () => {
    setError(null);
    setReport(null);
    try {
      const path = await saveFile({
        defaultPath: "claudy-prompts.json",
        filters: JSON_FILTER,
      });
      if (!path) return; // user cancelled
      const count = await exportPrompts(path);
      setReport(`Exported ${count} prompt${count === 1 ? "" : "s"}.`);
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
      const r = await importPrompts(path);
      await reload();
      const notes = [...r.warnings, ...r.skipped];
      setReport(
        `Imported: ${r.added} added, ${r.updated} updated.` +
          (notes.length ? ` ${notes.join("; ")}` : ""),
      );
    } catch (e: unknown) {
      setError(`Import failed: ${errorMessage(e)}`);
    }
  };

  return (
    <Page
      title="Prompts"
      description="AI prompts that run on the text you select, each from its own global shortcut."
    >
      <SetupChecklist onNavigate={onNavigate} />

      <div className="flex items-center gap-3">
        <Input
          value={search}
          placeholder="Search prompts…"
          aria-label="Search prompts"
          onChange={(e) => setSearch(e.target.value)}
          className="max-w-sm"
        />
        <Button onClick={() => setEditing(EMPTY_PROMPT)}>
          <Plus className="h-4 w-4" />
          New prompt
        </Button>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="outline" onClick={() => void doImport()}>
            <Upload className="h-4 w-4" />
            Import
          </Button>
          <Button variant="outline" onClick={() => void doExport()}>
            <Download className="h-4 w-4" />
            Export
          </Button>
        </div>
      </div>

      {error && <InlineStatus onDismiss={() => setError(null)}>{error}</InlineStatus>}
      {report && (
        <InlineStatus tone="success" onDismiss={() => setReport(null)}>
          {report}
        </InlineStatus>
      )}

      <Card>
        <CardContent className="p-0">
          {visible.length === 0 && (
            <p className="text-muted-foreground p-6 text-sm">
              {prompts.length === 0 ? "No prompts yet." : "No prompts match your search."}
            </p>
          )}
          {visible.map((p) => {
            const isRunning = running.has(p.id);
            const runError = runErrors[p.id];
            return (
              <div key={p.id} className="border-b px-4 py-3 last:border-b-0">
                <div className="flex items-center gap-3">
                  <Switch
                    checked={p.enabled}
                    onCheckedChange={() => void toggleEnabled(p)}
                    aria-label={`Enable ${p.name}`}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate font-medium">{p.name}</span>
                      {p.shortcut && (
                        <Badge variant="secondary" className="font-mono">
                          {p.shortcut}
                        </Badge>
                      )}
                    </div>
                    <p className="text-muted-foreground truncate text-sm">{p.template}</p>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      title="Run now (uses the current clipboard/selection)"
                      aria-label={isRunning ? `Running ${p.name}` : `Run ${p.name} now`}
                      aria-busy={isRunning}
                      disabled={!p.enabled || isRunning}
                      onClick={() => void runNow(p)}
                    >
                      {isRunning ? (
                        <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
                      ) : (
                        <Play className="h-4 w-4" aria-hidden="true" />
                      )}
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title="Edit"
                      aria-label={`Edit ${p.name}`}
                      onClick={() => setEditing(p)}
                    >
                      <Pencil className="h-4 w-4" aria-hidden="true" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title="Duplicate"
                      aria-label={`Duplicate ${p.name}`}
                      onClick={() => duplicate(p)}
                    >
                      <Copy className="h-4 w-4" aria-hidden="true" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title="Delete"
                      aria-label={`Delete ${p.name}`}
                      onClick={() => setToDelete(p)}
                    >
                      <Trash2 className="h-4 w-4" aria-hidden="true" />
                    </Button>
                  </div>
                </div>
                {runError && (
                  <InlineStatus className="mt-2" onDismiss={() => clearRunError(p.id)}>
                    {runError}
                  </InlineStatus>
                )}
              </div>
            );
          })}
        </CardContent>
      </Card>

      <AlertDialog
        open={toDelete !== null}
        onOpenChange={(open) => {
          if (!open) setToDelete(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{toDelete?.name}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This removes the prompt and releases its global shortcut. This cannot be
              undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={confirmDelete}>
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <PromptEditor
        initial={editing}
        onClose={() => setEditing(null)}
        onSaved={() => void reload()}
      />
    </Page>
  );
}
