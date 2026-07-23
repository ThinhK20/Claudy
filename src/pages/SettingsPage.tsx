import { useState } from "react";
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
import { ShortcutInput } from "@/components/shortcut-input";
import { ToggleRow } from "@/components/toggle-row";
import { useSettings, type Settings } from "@/lib/settings-store";

const THEMES: { value: Settings["theme"]; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const DICTATION_MODES: { value: Settings["dictationMode"]; label: string }[] = [
  { value: "hold", label: "Hold to talk" },
  { value: "toggle", label: "Press to toggle" },
];

export default function SettingsPage() {
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);
  const load = useSettings((s) => s.load);
  const [error, setError] = useState<string | null>(null);

  if (!settings) return null;

  // update() is optimistic; on rejection re-load so the UI shows reality.
  const safeUpdate = async (patch: Partial<Settings>) => {
    setError(null);
    try {
      await update(patch);
    } catch (e: unknown) {
      setError(String(e));
      try {
        await load();
      } catch (reloadError: unknown) {
        setError(`${String(e)} (could not refresh settings: ${String(reloadError)})`);
      }
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Settings</h1>
        <p className="text-muted-foreground mt-1">Application preferences.</p>
      </div>

      {error && <p className="text-destructive text-sm">{error}</p>}

      <Card>
        <CardHeader>
          <CardTitle>Dictation shortcut</CardTitle>
          <CardDescription>
            Global combination that runs dictation, and how it activates. Applied
            immediately.
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
              <SelectTrigger className="w-44">
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
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Appearance</CardTitle>
          <CardDescription>Color theme for the main window</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Theme</Label>
              <p className="text-muted-foreground text-sm">
                System follows your OS light/dark preference
              </p>
            </div>
            <Select
              value={settings.theme}
              onValueChange={(v) => void safeUpdate({ theme: v as Settings["theme"] })}
            >
              <SelectTrigger className="w-32">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {THEMES.map((t) => (
                  <SelectItem key={t.value} value={t.value}>
                    {t.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Behavior</CardTitle>
          <CardDescription>How Claudy delivers results and starts up</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          <ToggleRow
            label="Notifications"
            description="Show a desktop notification for results and errors"
            checked={settings.notificationsEnabled}
            onChange={(v) => void safeUpdate({ notificationsEnabled: v })}
          />
          <ToggleRow
            label="Restore clipboard after dictation"
            description="Put your previous clipboard content back after text is inserted"
            checked={settings.restoreClipboard}
            onChange={(v) => void safeUpdate({ restoreClipboard: v })}
          />
          <ToggleRow
            label="Auto-paste prompt results"
            description="Replace the selection with the result instead of only copying it. The result always stays on the clipboard."
            checked={settings.autoPaste}
            onChange={(v) => void safeUpdate({ autoPaste: v })}
          />
          <ToggleRow
            label="Start minimized"
            description="Start hidden in the tray instead of opening the main window"
            checked={settings.startMinimized}
            onChange={(v) => void safeUpdate({ startMinimized: v })}
          />
        </CardContent>
      </Card>
    </div>
  );
}
