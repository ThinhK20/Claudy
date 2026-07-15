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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
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
  const [isKeyStored, setIsKeyStored] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [test, setTest] = useState<TestState>({ status: "idle", message: "" });

  const activeId = settings?.ai.activeProvider ?? "openai_compatible";
  const meta = PROVIDERS.find((p) => p.id === activeId) ?? PROVIDERS[0];

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

  const patchProvider = (patch: Partial<{ baseUrl: string; model: string }>) =>
    update({ ai: { ...settings.ai, [meta.settingsKey]: { ...cfg, ...patch } } });

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
          AI provider for prompt shortcuts.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Active provider</CardTitle>
          <CardDescription>Used by every prompt shortcut</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0">Provider</Label>
            <Select
              value={activeId}
              onValueChange={(v) =>
                update({ ai: { ...settings.ai, activeProvider: v as ProviderId } })
              }
            >
              <SelectTrigger className="flex-1">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PROVIDERS.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
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
              onClick={saveKey}
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
            Sends a one-word prompt through the full pipeline
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div>
            <Button onClick={runTest} disabled={test.status === "running"}>
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
