import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { ShortcutInput } from "@/components/shortcut-input";
import { savePrompt, type Prompt } from "@/lib/ai-api";

export const EMPTY_PROMPT: Prompt = {
  id: "",
  name: "",
  template: "",
  shortcut: "",
  enabled: true,
};

interface PromptEditorProps {
  /** null = closed. id "" = create mode (also used for duplicate drafts). */
  initial: Prompt | null;
  onClose: () => void;
  onSaved: () => void;
}

export function PromptEditor({ initial, onClose, onSaved }: PromptEditorProps) {
  const [draft, setDraft] = useState<Prompt>(EMPTY_PROMPT);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Re-seed the form every time the dialog opens with a new subject.
  useEffect(() => {
    if (initial) {
      setDraft(initial);
      setError(null);
      setSaving(false);
    }
  }, [initial]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await savePrompt(draft);
      onSaved();
      onClose();
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog
      open={initial !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      {/* Fixed 60vw x 80vh editor; the template row absorbs the spare height. */}
      <DialogContent className="h-[80vh] grid-rows-[auto_minmax(0,1fr)_auto] sm:max-w-[60vw]">
        <DialogHeader>
          <DialogTitle>{initial?.id ? "Edit prompt" : "New prompt"}</DialogTitle>
          <DialogDescription>
            Placeholders: {"{{selected_text}}"}, {"{{clipboard}}"}, {"{{date}}"},{" "}
            {"{{time}}"}. Unknown placeholders pass through verbatim.
          </DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-col gap-4">
          <div className="flex flex-col gap-2">
            <Label htmlFor="prompt-name">Name</Label>
            <Input
              id="prompt-name"
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              placeholder="Fix grammar & spelling"
            />
          </div>
          <div className="flex min-h-0 flex-1 flex-col gap-2">
            <Label htmlFor="prompt-template">Template</Label>
            <Textarea
              id="prompt-template"
              rows={6}
              className="min-h-0 flex-1"
              value={draft.template}
              onChange={(e) => setDraft({ ...draft, template: e.target.value })}
              placeholder={"Correct the grammar of the following text:\n\n{{selected_text}}"}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label>Global shortcut</Label>
            <ShortcutInput
              value={draft.shortcut}
              onChange={(accel) => setDraft({ ...draft, shortcut: accel })}
              excludePromptId={draft.id || undefined}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="prompt-enabled">Enabled</Label>
            <Switch
              id="prompt-enabled"
              checked={draft.enabled}
              onCheckedChange={(enabled) => setDraft({ ...draft, enabled })}
            />
          </div>
          {error && <p className="text-destructive text-sm">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() => void save()}
            disabled={saving || !draft.name.trim() || !draft.template.trim()}
          >
            {saving ? "Saving…" : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
