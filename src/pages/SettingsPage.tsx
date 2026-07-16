import { useState } from "react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ShortcutInput } from "@/components/shortcut-input";
import { useSettings, type Settings } from "@/lib/settings-store";

interface ToggleRowProps {
  label: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

function ToggleRow({ label, description, checked, onChange }: ToggleRowProps) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <Label>{label}</Label>
        <p className="text-muted-foreground text-sm">{description}</p>
      </div>
      <Switch checked={checked} onCheckedChange={onChange} />
    </div>
  );
}

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
            Global combination that starts and stops dictation. Applied immediately.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ShortcutInput
            value={settings.dictationShortcut}
            onChange={(accel) => {
              if (accel) void safeUpdate({ dictationShortcut: accel });
            }}
            forDictation
            allowClear={false}
          />
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
