import { useEffect, useState } from "react";
import { ArrowRight, Check, Circle, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import type { PageKey } from "@/components/app-sidebar";
import { hasApiKey } from "@/lib/ai-api";
import { useSettings } from "@/lib/settings-store";
import {
  isAutoItem,
  isSetupComplete,
  isSetupItemDone,
  readDismissed,
  readMarkedDone,
  SETUP_ITEM_IDS,
  writeDismissed,
  writeMarkedDone,
  type SetupFacts,
  type SetupItemId,
} from "@/lib/setup-checklist";
import { cn } from "@/lib/utils";

const ITEMS: Record<SetupItemId, { label: string; hint: string; page: PageKey }> = {
  "speech-model": {
    label: "Download a speech model",
    hint: "Needed for dictation and the voice button in the ask box.",
    page: "transcription",
  },
  "mic-test": {
    label: "Test your microphone",
    hint: "Check Claudy hears you before you rely on it.",
    page: "transcription",
  },
  provider: {
    label: "Add a provider key and test the connection",
    hint: "Prompt shortcuts, the assistant and reminders need an AI provider.",
    page: "providers",
  },
  shortcut: {
    label: "Try the dictation shortcut",
    hint: "Hold it in any app, speak, and release.",
    page: "transcription",
  },
};

/**
 * First-run checklist. Items backed by settings tick themselves; the others
 * can only be ticked by hand (or by the mic test), since nothing records them.
 * Hidden for good once dismissed or complete.
 */
export function SetupChecklist({ onNavigate }: { onNavigate: (page: PageKey) => void }) {
  const settings = useSettings((s) => s.settings);
  const [dismissed, setDismissed] = useState(readDismissed);
  const [marked, setMarked] = useState<SetupItemId[]>(readMarkedDone);
  const [hasProviderKey, setHasProviderKey] = useState<boolean | null>(null);

  const activeProvider = settings?.ai.activeProvider;
  useEffect(() => {
    if (!activeProvider || dismissed) return;
    let stale = false;
    // Ollama runs locally without a key, so there is nothing to add.
    if (activeProvider === "ollama") {
      setHasProviderKey(true);
      return;
    }
    hasApiKey(activeProvider)
      .then((stored) => {
        if (!stale) setHasProviderKey(stored);
      })
      .catch(() => {
        if (!stale) setHasProviderKey(null);
      });
    return () => {
      stale = true;
    };
  }, [activeProvider, dismissed]);

  if (!settings || dismissed) return null;

  const facts: SetupFacts = { hasSpeechModel: Boolean(settings.model), hasProviderKey };
  const markedSet = new Set<string>(marked);
  if (isSetupComplete(facts, markedSet)) return null;

  const doneCount = SETUP_ITEM_IDS.filter((id) => isSetupItemDone(id, facts, markedSet)).length;

  const toggleMarked = (id: SetupItemId) => {
    const next = marked.includes(id) ? marked.filter((m) => m !== id) : [...marked, id];
    setMarked(next);
    writeMarkedDone(next);
  };

  const dismiss = () => {
    setDismissed(true);
    writeDismissed();
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Get started</CardTitle>
        <CardDescription>
          {doneCount} of {SETUP_ITEM_IDS.length} done. A few steps to get Claudy working.
        </CardDescription>
        <CardAction>
          <Button variant="ghost" size="icon-sm" aria-label="Dismiss setup checklist" onClick={dismiss}>
            <X aria-hidden="true" />
          </Button>
        </CardAction>
      </CardHeader>
      <CardContent>
        <ul className="flex flex-col gap-1">
          {SETUP_ITEM_IDS.map((id) => {
            const item = ITEMS[id];
            const done = isSetupItemDone(id, facts, markedSet);
            const auto = isAutoItem(id);
            return (
              <li key={id} className="flex items-center gap-3 rounded-md py-1.5">
                {auto ? (
                  <span
                    className={cn(
                      "flex size-6 shrink-0 items-center justify-center",
                      done ? "text-success" : "text-muted-foreground",
                    )}
                  >
                    {done ? (
                      <Check className="size-4" aria-hidden="true" />
                    ) : (
                      <Circle className="size-4" aria-hidden="true" />
                    )}
                  </span>
                ) : (
                  <button
                    type="button"
                    role="checkbox"
                    aria-checked={done}
                    aria-label={`Mark "${item.label}" as done`}
                    onClick={() => toggleMarked(id)}
                    className={cn(
                      "focus-visible:ring-ring flex size-6 shrink-0 items-center justify-center rounded-md outline-none hover:bg-muted focus-visible:ring-2",
                      done ? "text-success" : "text-muted-foreground",
                    )}
                  >
                    {done ? (
                      <Check className="size-4" aria-hidden="true" />
                    ) : (
                      <Circle className="size-4" aria-hidden="true" />
                    )}
                  </button>
                )}
                <div className="min-w-0 flex-1">
                  <p className={cn("text-sm font-medium", done && "text-muted-foreground line-through")}>
                    {item.label}
                    {done && <span className="sr-only"> (done)</span>}
                  </p>
                  <p className="text-muted-foreground text-xs">{item.hint}</p>
                </div>
                {!done && (
                  <Button variant="ghost" size="sm" onClick={() => onNavigate(item.page)}>
                    Open
                    <ArrowRight aria-hidden="true" />
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      </CardContent>
    </Card>
  );
}
