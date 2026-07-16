import { useCallback, useEffect, useState } from "react";
import { Copy, Download, Pencil, Play, Plus, Trash2, Upload } from "lucide-react";
import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import { EMPTY_PROMPT, PromptEditor } from "@/components/prompt-editor";
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

export default function PromptsPage() {
  const [prompts, setPrompts] = useState<Prompt[]>([]);
  const [search, setSearch] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [toDelete, setToDelete] = useState<Prompt | null>(null);
  const [editing, setEditing] = useState<Prompt | null>(null);
  const [report, setReport] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setPrompts(await listPrompts());
      setError(null);
    } catch (e: unknown) {
      setError(String(e));
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
      setError(String(e));
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
      setError(String(e));
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
      setError(String(e));
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Prompts</h1>
        <p className="text-muted-foreground mt-1">
          AI prompts that run on selected text via global shortcuts.
        </p>
      </div>

      <div className="flex items-center gap-3">
        <Input
          value={search}
          placeholder="Search prompts…"
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

      {error && <p className="text-destructive text-sm">{error}</p>}
      {report && <p className="text-muted-foreground text-sm">{report}</p>}

      <Card>
        <CardContent className="p-0">
          {visible.length === 0 && (
            <p className="text-muted-foreground p-6 text-sm">
              {prompts.length === 0 ? "No prompts yet." : "No prompts match your search."}
            </p>
          )}
          {visible.map((p) => (
            <div
              key={p.id}
              className="flex items-center gap-3 border-b px-4 py-3 last:border-b-0"
            >
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
                  disabled={!p.enabled}
                  onClick={() => void runPrompt(p.id)}
                >
                  <Play className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="Edit"
                  onClick={() => setEditing(p)}
                >
                  <Pencil className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="Duplicate"
                  onClick={() => duplicate(p)}
                >
                  <Copy className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  title="Delete"
                  onClick={() => setToDelete(p)}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>
          ))}
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
            <AlertDialogAction onClick={confirmDelete}>Delete</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <PromptEditor
        initial={editing}
        onClose={() => setEditing(null)}
        onSaved={() => void reload()}
      />
    </div>
  );
}
