import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { InlineStatus } from "@/components/inline-status";
import { Page, SettingsUnavailable } from "@/components/page";
import { SaveStatus } from "@/components/save-status";
import { ToggleRow } from "@/components/toggle-row";
import { hasApiKey, setApiKey, testProvider } from "@/lib/ai-api";
import { errorMessage } from "@/lib/error-message";
import { formatModelList, parseModelList } from "@/lib/model-list";
import {
  useSettings,
  type AiSettings,
  type ProviderId,
  type ProviderSettings,
} from "@/lib/settings-store";
import { useSafeUpdate } from "@/lib/use-safe-update";

const TITLE = "Providers";
const DESCRIPTION =
  "The AI service behind prompt shortcuts, the assistant and reminders. Configure any provider; only the active one is used.";

/** The `AiSettings` keys that hold a provider block, as opposed to the
    scalar switches sitting beside them. Derived rather than hand-listed so a
    new switch cannot silently widen it. */
type ProviderKey = {
  [K in keyof AiSettings]: AiSettings[K] extends ProviderSettings ? K : never;
}[keyof AiSettings];

interface ProviderMeta {
  id: ProviderId;
  settingsKey: ProviderKey;
  label: string;
  defaultBaseUrl: string;
  defaultModel: string;
  /** Mirrors `ai::fallback::builtin_chain` in Rust; shown as the placeholder
      so the user can see what an empty field will actually do. Display only —
      the backend never reads it. */
  defaultFallbacks: string[];
  keyHint: string;
}

const PROVIDERS: ProviderMeta[] = [
  {
    id: "openai_compatible",
    settingsKey: "openaiCompatible",
    label: "OpenAI-compatible",
    defaultBaseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4o-mini",
    defaultFallbacks: [],
    keyHint: "Optional for local servers (LM Studio, llama.cpp)",
  },
  {
    id: "ollama",
    settingsKey: "ollama",
    label: "Ollama",
    defaultBaseUrl: "http://localhost:11434",
    defaultModel: "llama3.2",
    defaultFallbacks: [],
    keyHint: "Not used by Ollama",
  },
  {
    id: "anthropic",
    settingsKey: "anthropic",
    label: "Anthropic",
    defaultBaseUrl: "https://api.anthropic.com",
    defaultModel: "claude-sonnet-5",
    defaultFallbacks: ["claude-sonnet-5", "claude-haiku-4-5-20251001"],
    keyHint: "Required",
  },
  {
    id: "gemini",
    settingsKey: "gemini",
    label: "Google Gemini",
    defaultBaseUrl: "https://generativelanguage.googleapis.com",
    defaultModel: "gemini-2.5-flash",
    defaultFallbacks: ["gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-2.0-flash"],
    keyHint: "Required",
  },
  {
    id: "deepseek",
    settingsKey: "deepseek",
    label: "DeepSeek",
    defaultBaseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-flash",
    defaultFallbacks: ["deepseek-flash", "deepseek-v4-pro"],
    keyHint: "Required",
  },
];

interface TestState {
  status: "idle" | "running" | "ok" | "error";
  message: string;
}

interface KeyStatus {
  tone: "error" | "success";
  text: string;
}

export default function ProvidersPage() {
  const settings = useSettings((s) => s.settings);
  const { safeUpdate, feedback } = useSafeUpdate();
  // null = "show the active provider's tab" (until the user picks one).
  const [selected, setSelected] = useState<ProviderId | null>(null);
  const [isKeyStored, setIsKeyStored] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [keyStatus, setKeyStatus] = useState<KeyStatus | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [test, setTest] = useState<TestState>({ status: "idle", message: "" });

  const activeId = settings?.ai.activeProvider ?? "openai_compatible";
  const selectedId = selected ?? activeId;
  const meta = PROVIDERS.find((p) => p.id === selectedId) ?? PROVIDERS[0];

  useEffect(() => {
    let stale = false;
    setKeyDraft("");
    setKeyStatus(null);
    setTest({ status: "idle", message: "" });
    hasApiKey(meta.id)
      .then((stored) => {
        if (!stale) setIsKeyStored(stored);
      })
      .catch((e: unknown) => {
        if (!stale)
          setKeyStatus({
            tone: "error",
            text: `Couldn't check for a stored key: ${errorMessage(e)}`,
          });
      });
    return () => {
      stale = true;
    };
  }, [meta.id]);

  if (!settings) return <SettingsUnavailable title={TITLE} description={DESCRIPTION} />;
  const cfg = settings.ai[meta.settingsKey];
  const isActive = meta.id === activeId;

  const patchProvider = (
    patch: Partial<{ baseUrl: string; model: string; fallbackModels: string[] }>,
  ) => void safeUpdate({ ai: { ...settings.ai, [meta.settingsKey]: { ...cfg, ...patch } } });

  const setActive = () =>
    void safeUpdate({ ai: { ...settings.ai, activeProvider: meta.id } });

  // An empty key removes the stored one, so Save and Remove share a command.
  const writeKey = async (key: string) => {
    setKeyStatus(null);
    try {
      await setApiKey(meta.id, key);
      setKeyDraft("");
      setIsKeyStored(await hasApiKey(meta.id));
      setKeyStatus({ tone: "success", text: key ? "Key saved." : "Key removed." });
    } catch (e: unknown) {
      setKeyStatus({
        tone: "error",
        text: `Couldn't ${key ? "save" : "remove"} the key: ${errorMessage(e)}`,
      });
    }
  };

  const runTest = async () => {
    setTest({ status: "running", message: "" });
    try {
      const reply = await testProvider(meta.id);
      setTest({ status: "ok", message: reply });
    } catch (e: unknown) {
      setTest({ status: "error", message: errorMessage(e) });
    }
  };

  return (
    <Page title={TITLE} description={DESCRIPTION}>
      <Tabs value={meta.id} onValueChange={(v) => setSelected(v as ProviderId)}>
        <TabsList>
          {PROVIDERS.map((p) => (
            <TabsTrigger key={p.id} value={p.id} className="gap-2">
              {p.label}
              {p.id === activeId && <Badge variant="secondary">Active</Badge>}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-4">
            <div>
              <CardTitle>{meta.label}</CardTitle>
              <CardDescription>
                Empty fields use the defaults shown in grey.
              </CardDescription>
            </div>
            {isActive ? (
              <Badge>Active provider</Badge>
            ) : (
              <Button variant="outline" size="sm" onClick={() => void setActive()}>
                Set as active
              </Button>
            )}
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          {/* key remounts on provider switch so defaultValue re-seeds; commit
              on blur — per-keystroke updates would write settings.json each key */}
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0" htmlFor="provider-base-url">
              Base URL
            </Label>
            <Input
              id="provider-base-url"
              key={`${meta.id}-baseUrl`}
              defaultValue={cfg.baseUrl}
              placeholder={meta.defaultBaseUrl}
              onBlur={(e) => patchProvider({ baseUrl: e.target.value.trim() })}
            />
          </div>
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0" htmlFor="provider-model">
              Model
            </Label>
            <Input
              id="provider-model"
              key={`${meta.id}-model`}
              defaultValue={cfg.model}
              placeholder={meta.defaultModel}
              onBlur={(e) => patchProvider({ model: e.target.value.trim() })}
            />
          </div>
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0" htmlFor="provider-fallbacks">
              Fallback models
            </Label>
            <Input
              id="provider-fallbacks"
              key={`${meta.id}-fallbackModels`}
              defaultValue={formatModelList(cfg.fallbackModels)}
              placeholder={
                meta.defaultFallbacks.length > 0
                  ? formatModelList(meta.defaultFallbacks)
                  : "none — add models to enable fallback"
              }
              onBlur={(e) => patchProvider({ fallbackModels: parseModelList(e.target.value) })}
            />
          </div>
          <p className="text-muted-foreground text-sm">
            Comma-separated, tried in order when {meta.label} reports the model is
            overloaded.{" "}
            {meta.defaultFallbacks.length > 0
              ? "Leave empty to use the built-in list shown in the field."
              : "Leave empty for no fallback."}
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Overload handling</CardTitle>
          <CardDescription>
            At peak hours providers reject requests with &ldquo;the model is
            overloaded&rdquo;. Applies to every provider, not just {meta.label}.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ToggleRow
            label="Switch models automatically when overloaded"
            description="Retries with the next fallback model and tells you which one answered. The overloaded model is skipped for a few minutes, then preferred again."
            checked={settings.ai.modelFallbackEnabled}
            onChange={(checked) =>
              void safeUpdate({ ai: { ...settings.ai, modelFallbackEnabled: checked } })
            }
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>API key</CardTitle>
          <CardDescription>
            Stored in the Windows credential store, never in a file. {meta.keyHint}.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <Input
              type="password"
              aria-label={`${meta.label} API key`}
              value={keyDraft}
              placeholder={isKeyStored ? "•••••••• (stored)" : "Paste API key"}
              onChange={(e) => setKeyDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && keyDraft) void writeKey(keyDraft);
              }}
            />
            <Button size="sm" onClick={() => void writeKey(keyDraft)} disabled={!keyDraft}>
              {isKeyStored ? "Replace key" : "Save key"}
            </Button>
            {isKeyStored && (
              <Button variant="outline" size="sm" onClick={() => setConfirmRemove(true)}>
                Remove
              </Button>
            )}
            {isKeyStored && <Badge variant="secondary">Key stored</Badge>}
          </div>
          {keyStatus && <InlineStatus tone={keyStatus.tone}>{keyStatus.text}</InlineStatus>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Connection test</CardTitle>
          <CardDescription>
            Sends a one-word prompt through the full pipeline for {meta.label}.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div>
            <Button onClick={() => void runTest()} disabled={test.status === "running"}>
              {test.status === "running" ? "Testing…" : "Test connection"}
            </Button>
          </div>
          {test.status === "ok" && (
            <InlineStatus tone="success">Connected. Reply: {test.message}</InlineStatus>
          )}
          {test.status === "error" && <InlineStatus>{test.message}</InlineStatus>}
        </CardContent>
      </Card>

      <ConfirmDialog
        open={confirmRemove}
        onOpenChange={setConfirmRemove}
        title={`Remove the ${meta.label} API key?`}
        description="The key is deleted from the Windows credential store. Requests to this provider fail until you add a key again."
        confirmLabel="Remove key"
        onConfirm={() => void writeKey("")}
      />

      <SaveStatus feedback={feedback} />
    </Page>
  );
}
