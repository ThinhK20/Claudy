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
import { Page, SettingsUnavailable } from "@/components/page";
import { SaveStatus } from "@/components/save-status";
import { ShortcutInput } from "@/components/shortcut-input";
import { ToggleRow } from "@/components/toggle-row";
import { MicSettings } from "@/components/transcription/mic-settings";
import { ModelManager } from "@/components/transcription/model-manager";
import { TestRecorder } from "@/components/transcription/test-recorder";
import { VocabularyManager } from "@/components/transcription/vocabulary-manager";
import { useSettings, type Settings } from "@/lib/settings-store";
import { useSafeUpdate } from "@/lib/use-safe-update";

const TITLE = "Dictation";
const DESCRIPTION =
  "Speak into any app: the shortcut, speech models, microphone and corrections.";

const DICTATION_MODES: { value: Settings["dictationMode"]; label: string }[] = [
  { value: "hold", label: "Hold to talk" },
  { value: "toggle", label: "Press to toggle" },
];

export default function TranscriptionPage() {
  const settings = useSettings((s) => s.settings);
  const { safeUpdate, feedback } = useSafeUpdate();

  if (!settings) return <SettingsUnavailable title={TITLE} description={DESCRIPTION} />;

  return (
    <Page title={TITLE} description={DESCRIPTION}>
      <ModelManager />

      <Card>
        <CardHeader>
          <CardTitle>Dictation shortcut</CardTitle>
          <CardDescription>
            The global key combination that starts dictation, and how it activates.
            Changes apply immediately.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          <ShortcutInput
            value={settings.dictationShortcut}
            onChange={(accel) => {
              if (accel) void safeUpdate({ dictationShortcut: accel });
            }}
            forDictation
            allowClear={false}
          />
          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Activation</Label>
              <p className="text-muted-foreground text-sm">
                {settings.dictationMode === "hold"
                  ? "Hold while you talk, release to insert. A quick tap keeps recording until you press again."
                  : "Press once to start, press again to stop."}
              </p>
            </div>
            <Select
              value={settings.dictationMode}
              onValueChange={(v) =>
                void safeUpdate({ dictationMode: v as Settings["dictationMode"] })
              }
            >
              <SelectTrigger className="w-44" aria-label="Activation">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {DICTATION_MODES.map((m) => (
                  <SelectItem key={m.value} value={m.value}>
                    {m.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <ToggleRow
            label="Restore clipboard after dictation"
            description="Put your previous clipboard content back after the text is inserted."
            checked={settings.restoreClipboard}
            onChange={(v) => void safeUpdate({ restoreClipboard: v })}
          />
        </CardContent>
      </Card>

      <MicSettings />
      <VocabularyManager />
      <TestRecorder />

      <SaveStatus feedback={feedback} />
    </Page>
  );
}
