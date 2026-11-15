import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Page, SettingsUnavailable } from "@/components/page";
import { SaveStatus } from "@/components/save-status";
import { ShortcutInput } from "@/components/shortcut-input";
import { ToggleRow } from "@/components/toggle-row";
import { SystemPromptEditor } from "@/components/assistant/system-prompt-editor";
import { VoiceModelManager } from "@/components/assistant/voice-model-manager";
import { WakeWordCard } from "@/components/assistant/wake-word-settings";
import { useSettings, type WakeWordSettings } from "@/lib/settings-store";
import { useSafeUpdate } from "@/lib/use-safe-update";

const TITLE = "Assistant";
const DESCRIPTION =
  "An ask box opened by a global shortcut or your voice, with an optional spoken answer.";

const AUTO_CLOSE_OPTIONS: { value: number; label: string }[] = [
  { value: 0, label: "Never" },
  { value: 10, label: "10 seconds" },
  { value: 15, label: "15 seconds" },
  { value: 30, label: "30 seconds" },
  { value: 60, label: "60 seconds" },
];

const VOICE_OPTIONS: { value: string; label: string }[] = [
  { value: "af_heart", label: "Heart (US, female)" },
  { value: "af_bella", label: "Bella (US, female)" },
  { value: "af_nicole", label: "Nicole (US, female)" },
  { value: "af_sarah", label: "Sarah (US, female)" },
  { value: "am_adam", label: "Adam (US, male)" },
  { value: "am_michael", label: "Michael (US, male)" },
  { value: "am_puck", label: "Puck (US, male)" },
  { value: "bf_emma", label: "Emma (UK, female)" },
  { value: "bf_isabella", label: "Isabella (UK, female)" },
  { value: "bm_george", label: "George (UK, male)" },
  { value: "bm_lewis", label: "Lewis (UK, male)" },
];

const SPEED_OPTIONS: number[] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

export default function AssistantSettingsPage() {
  const settings = useSettings((s) => s.settings);
  const { safeUpdate, feedback } = useSafeUpdate();

  if (!settings) return <SettingsUnavailable title={TITLE} description={DESCRIPTION} />;

  return (
    <Page title={TITLE} description={DESCRIPTION}>
      <Card>
        <CardHeader>
          <CardTitle>Behavior</CardTitle>
          <CardDescription>How the ask box opens and how the answer panel closes.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          <div>
            <Label>Shortcut</Label>
            <p className="text-muted-foreground mb-2 text-sm">
              The global key combination that opens the ask box at your cursor.
            </p>
            <ShortcutInput
              value={settings.assistant.shortcut}
              onChange={(accel) => {
                if (accel)
                  void safeUpdate({
                    assistant: { ...settings.assistant, shortcut: accel },
                  });
              }}
              forAssistant
              allowClear={false}
            />
          </div>
          <ToggleRow
            label="Search the web automatically"
            description="Let supported providers (Anthropic, Gemini) look things up online to answer. Ignored by providers without web search."
            checked={settings.assistant.autoWebSearch}
            onChange={(v) =>
              void safeUpdate({
                assistant: { ...settings.assistant, autoWebSearch: v },
              })
            }
          />
          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Auto-close answer</Label>
              <p className="text-muted-foreground text-sm">
                Hide the answer panel after this long. A bar shows the time left; hovering, scrolling or using the keyboard in the panel pauses it.
              </p>
            </div>
            <Select
              value={String(settings.assistant.panelCloseSecs)}
              onValueChange={(v) =>
                void safeUpdate({
                  assistant: { ...settings.assistant, panelCloseSecs: Number(v) },
                })
              }
            >
              <SelectTrigger className="w-36" aria-label="Auto-close answer">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {AUTO_CLOSE_OPTIONS.map((o) => (
                  <SelectItem key={o.value} value={String(o.value)}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      <WakeWordCard
        onChange={(patch: Partial<WakeWordSettings>) =>
          void safeUpdate({
            assistant: {
              ...settings.assistant,
              wakeWord: { ...settings.assistant.wakeWord, ...patch },
            },
          })
        }
      />

      <SystemPromptEditor />

      <Card>
        <CardHeader>
          <CardTitle>Voice</CardTitle>
          <CardDescription>Spoken answers using the local voice model.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          <ToggleRow
            label="Speak answers aloud"
            description="Read the answer out loud using the local voice model."
            checked={settings.assistant.autoSpeak}
            onChange={(v) =>
              void safeUpdate({ assistant: { ...settings.assistant, autoSpeak: v } })
            }
          />

          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Voice</Label>
              <p className="text-muted-foreground text-sm">Which voice reads answers.</p>
            </div>
            <Select
              value={settings.assistant.ttsVoice}
              onValueChange={(v) =>
                void safeUpdate({ assistant: { ...settings.assistant, ttsVoice: v } })
              }
            >
              <SelectTrigger className="w-44" aria-label="Voice">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {VOICE_OPTIONS.map((o) => (
                  <SelectItem key={o.value} value={o.value}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Speech speed</Label>
              <p className="text-muted-foreground text-sm">Playback rate for spoken answers.</p>
            </div>
            <Select
              value={String(settings.assistant.speechSpeed)}
              onValueChange={(v) =>
                void safeUpdate({ assistant: { ...settings.assistant, speechSpeed: Number(v) } })
              }
            >
              <SelectTrigger className="w-28" aria-label="Speech speed">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SPEED_OPTIONS.map((s) => (
                  <SelectItem key={s} value={String(s)}>
                    {s.toFixed(2)}×
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Volume</Label>
              <p className="text-muted-foreground text-sm">
                {Math.round(settings.assistant.volume * 100)}%
              </p>
            </div>
            <Slider
              className="w-44"
              aria-label="Volume"
              min={0}
              max={1}
              step={0.05}
              value={[settings.assistant.volume]}
              onValueChange={([v]) =>
                void safeUpdate({ assistant: { ...settings.assistant, volume: v } })
              }
            />
          </div>

          <ToggleRow
            label="Keep the answer panel open while speaking"
            description="Pause the auto-close timer until the spoken answer finishes."
            checked={settings.assistant.keepOpenWhileSpeaking}
            onChange={(v) =>
              void safeUpdate({
                assistant: { ...settings.assistant, keepOpenWhileSpeaking: v },
              })
            }
          />

          <VoiceModelManager />
        </CardContent>
      </Card>

      <SaveStatus feedback={feedback} />
    </Page>
  );
}
