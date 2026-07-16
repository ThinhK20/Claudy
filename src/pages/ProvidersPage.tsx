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
import { hasApiKey, setApiKey, testProvider } from "@/lib/ai-api";
import {
  useSettings,
  type AiSettings,
  type ProviderId,
} from "@/lib/settings-store";

interface ProviderMeta {
  id: ProviderId;
  settingsKey: keyof Omit<AiSettings, "activeProvider">;
  label: string;
  defaultBaseUrl: string;
  defaultModel: string;
  keyHint: string;
}

const PROVIDERS: ProviderMeta[] = [
  {
    id: "openai_compatible",
    settingsKey: "openaiCompatible",
    label: "OpenAI-compatible",
    defaultBaseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4o-mini",
    keyHint: "Optional for local servers (LM Studio, llama.cpp)",
  },
  {
    id: "ollama",
    settingsKey: "ollama",
    label: "Ollama",
    defaultBaseUrl: "http://localhost:11434",
    defaultModel: "llama3.2",
    keyHint: "Not used by Ollama",
  },
  {
    id: "anthropic",
    settingsKey: "anthropic",
    label: "Anthropic",
    defaultBaseUrl: "https://api.anthropic.com",
    defaultModel: "claude-sonnet-5",
    keyHint: "Required",
  },
  {
    id: "gemini",
    settingsKey: "gemini",
    label: "Google Gemini",
    defaultBaseUrl: "https://generativelanguage.googleapis.com",
    defaultModel: "gemini-2.5-flash",
    keyHint: "Required",
  },
];

interface TestState {
  status: "idle" | "running" | "ok" | "error";
  message: string;
}

export default function ProvidersPage() {
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);
  // null = "show the active provider's tab" (until the user picks one).
  const [selected, setSelected] = useState<ProviderId | null>(null);
  const [isKeyStored, setIsKeyStored] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [test, setTest] = useState<TestState>({ status: "idle", message: "" });

  const activeId = settings?.ai.activeProvider ?? "openai_compatible";
  const selectedId = selected ?? activeId;
  const meta = PROVIDERS.find((p) => p.id === selectedId) ?? PROVIDERS[0];

  useEffect(() => {
    setKeyDraft("");
    setKeyError(null);
    setTest({ status: "idle", message: "" });
    hasApiKey(meta.id)
      .then(setIsKeyStored)
      .catch((e: unknown) => setKeyError(String(e)));
  }, [meta.id]);

  if (!settings) return null;
  const cfg = settings.ai[meta.settingsKey];
  const isActive = meta.id === activeId;

  const patchProvider = (patch: Partial<{ baseUrl: string; model: string }>) =>
    update({ ai: { ...settings.ai, [meta.settingsKey]: { ...cfg, ...patch } } });

  const setActive = () =>
    update({ ai: { ...settings.ai, activeProvider: meta.id } });

  const saveKey = async () => {
    setKeyError(null);
    try {
      await setApiKey(meta.id, keyDraft); // empty draft = remove the key
      setKeyDraft("");
      setIsKeyStored(await hasApiKey(meta.id));
    } catch (e: unknown) {
      setKeyError(String(e));
    }
  };

  const runTest = async () => {
    setTest({ status: "running", message: "" });
    try {
      const reply = await testProvider(meta.id);
      setTest({ status: "ok", message: reply });
    } catch (e: unknown) {
      setTest({ status: "error", message: String(e) });
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Providers</h1>
        <p className="text-muted-foreground mt-1">
          AI provider for prompt shortcuts. Configure any provider; only the active one
          is used.
        </p>
      </div>

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
                Empty fields use the defaults shown as placeholders.
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
            <Label className="w-28 shrink-0">Base URL</Label>
            <Input
              key={`${meta.id}-baseUrl`}
              defaultValue={cfg.baseUrl}
              placeholder={meta.defaultBaseUrl}
              onBlur={(e) => patchProvider({ baseUrl: e.target.value.trim() })}
            />
          </div>
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0">Model</Label>
            <Input
              key={`${meta.id}-model`}
              defaultValue={cfg.model}
              placeholder={meta.defaultModel}
              onBlur={(e) => patchProvider({ model: e.target.value.trim() })}
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>API key</CardTitle>
          <CardDescription>
            Stored in the OS credential store, never in a file. {meta.keyHint}.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <Input
              type="password"
              value={keyDraft}
              placeholder={isKeyStored ? "•••••••• (stored)" : "Paste API key"}
              onChange={(e) => setKeyDraft(e.target.value)}
            />
            <Button
              variant="outline"
              size="sm"
              onClick={() => void saveKey()}
              disabled={!keyDraft && !isKeyStored}
            >
              {keyDraft || !isKeyStored ? "Save key" : "Remove key"}
            </Button>
            {isKeyStored && <Badge variant="secondary">Key stored</Badge>}
          </div>
          {keyError && <p className="text-destructive text-sm">{keyError}</p>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Connection test</CardTitle>
          <CardDescription>
            Sends a one-word prompt through the full pipeline for {meta.label}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div>
            <Button onClick={() => void runTest()} disabled={test.status === "running"}>
              {test.status === "running" ? "Testing…" : "Test connection"}
            </Button>
          </div>
          {test.status === "ok" && (
            <p className="text-sm text-green-600">Reply: {test.message}</p>
          )}
          {test.status === "error" && (
            <p className="text-destructive text-sm">{test.message}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
