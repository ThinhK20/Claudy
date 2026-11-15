import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Textarea } from "@/components/ui/textarea";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { InlineStatus } from "@/components/inline-status";
import { errorMessage } from "@/lib/error-message";
import { useSettings } from "@/lib/settings-store";

/** Mirrored by MAX_SYSTEM_PROMPT_CHARS in src-tauri/src/config.rs. */
export const MAX_SYSTEM_PROMPT_CHARS = 10_000;

const PLACEHOLDER =
  "You are a concise software engineering assistant. Prefer practical examples, explain trade-offs, and answer in Markdown.";

/**
 * Draft-based editor for the assistant's custom system prompt. Unlike the
 * page's auto-saving toggles, a free-text prompt needs explicit Save/Reset
 * so half-typed instructions never reach the provider.
 */
export function SystemPromptEditor() {
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);
  const load = useSettings((s) => s.load);

  const saved = settings?.assistant.customSystemPrompt ?? "";
  const [draft, setDraft] = useState(saved);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [justSaved, setJustSaved] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);

  // Re-seed when the stored value changes underneath us (reload, other
  // window) — but not on the failed-save revert, so a rejected save never
  // wipes the text the user just typed.
  const skipNextReseedRef = useRef(false);
  useEffect(() => {
    if (skipNextReseedRef.current) {
      skipNextReseedRef.current = false;
      return;
    }
    setDraft(saved);
  }, [saved]);

  if (!settings) return null;

  const trimmed = draft.trim();
  // Unicode scalar count, matching Rust's chars().count() in update_settings.
  const charCount = Array.from(draft).length;
  const isDirty = trimmed !== saved;
  const isWhitespaceOnly = draft.length > 0 && trimmed === "";
  const isOverMax = charCount > MAX_SYSTEM_PROMPT_CHARS;

  const persist = async (value: string) => {
    setSaving(true);
    setError(null);
    setJustSaved(false);
    try {
      await update({ assistant: { ...settings.assistant, customSystemPrompt: value } });
      setJustSaved(true);
    } catch (e: unknown) {
      setError(`Couldn't save: ${errorMessage(e)}`);
      skipNextReseedRef.current = true; // keep the draft through the revert
      try {
        await load(); // update() is optimistic; show reality again
      } catch (reloadError: unknown) {
        skipNextReseedRef.current = false; // store unchanged, nothing to skip
        setError(
          `Couldn't save: ${errorMessage(e)} (could not refresh settings: ${errorMessage(reloadError)})`,
        );
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Custom system prompt</CardTitle>
        <CardDescription>
          Sent as the system instruction with every assistant question — personality, tone,
          language, format. Leave empty for the default behavior. Does not affect dictation
          prompts.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <Textarea
          rows={8}
          className="max-h-64 min-h-32 overflow-y-auto font-mono text-sm"
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
            setJustSaved(false);
          }}
          aria-label="Custom system prompt"
          placeholder={PLACEHOLDER}
          aria-invalid={isWhitespaceOnly || isOverMax}
        />
        <div className="flex items-center justify-between gap-4">
          <p className={`text-xs ${isOverMax ? "text-destructive" : "text-muted-foreground"}`}>
            {charCount.toLocaleString()} / {MAX_SYSTEM_PROMPT_CHARS.toLocaleString()}
          </p>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={draft === ""}
              onClick={() => void navigator.clipboard.writeText(draft)}
            >
              Copy
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={saving || (saved === "" && draft === "")}
              onClick={() => {
                // Nothing saved yet: only an unsaved draft is lost, no need to ask.
                if (saved === "") setDraft("");
                else setConfirmReset(true);
              }}
            >
              Reset
            </Button>
            <Button
              size="sm"
              disabled={saving || !isDirty || isWhitespaceOnly || isOverMax}
              onClick={() => void persist(trimmed)}
            >
              {saving ? "Saving…" : "Save"}
            </Button>
          </div>
        </div>
        {isWhitespaceOnly && (
          <InlineStatus>
            The prompt contains only whitespace — use Reset to return to the default behavior.
          </InlineStatus>
        )}
        {isOverMax && (
          <InlineStatus>
            The prompt exceeds {MAX_SYSTEM_PROMPT_CHARS.toLocaleString()} characters — shorten it
            to save.
          </InlineStatus>
        )}
        {error && <InlineStatus>{error}</InlineStatus>}
        {justSaved && !error && !isDirty && <InlineStatus tone="success">Saved.</InlineStatus>}
      </CardContent>

      <ConfirmDialog
        open={confirmReset}
        onOpenChange={setConfirmReset}
        title="Reset the system prompt?"
        description="Your saved prompt is deleted and the assistant goes back to its default behavior. This can't be undone."
        confirmLabel="Reset"
        onConfirm={() => {
          setDraft("");
          void persist("");
        }}
      />
    </Card>
  );
}
