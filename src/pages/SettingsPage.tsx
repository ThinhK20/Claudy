import { useEffect, useState } from "react";
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
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { ShortcutInput } from "@/components/shortcut-input";
import { ToggleRow } from "@/components/toggle-row";
import { useSettings, type Settings } from "@/lib/settings-store";
import { clearUsage } from "@/lib/usage-api";
import { isAutostartEnabled, setAutostart } from "@/lib/autostart-api";

const THEMES: { value: Settings["theme"]; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/** 0 = keep forever; a hard cap on the number of entries still applies. */
const RETENTIONS: { value: number; label: string }[] = [
  { value: 30, label: "30 days" },
  { value: 90, label: "90 days" },
  { value: 365, label: "1 year" },
  { value: 0, label: "Forever" },
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
  // null until the first read resolves. Not in the settings store: the OS
  // owns this value, so it is read back from there rather than cached.
  const [autostart, setAutostartState] = useState<boolean | null>(null);

  useEffect(() => {
    void isAutostartEnabled()
      .then(setAutostartState)
      .catch((e: unknown) => setError(String(e)));
  }, []);

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

  // Same optimistic-then-reconcile shape as safeUpdate, but the truth is read
  // back from the OS instead of from the settings store.
  const toggleAutostart = async (on: boolean) => {
    setError(null);
    setAutostartState(on);
    try {
      await setAutostart(on);
    } catch (e: unknown) {
      setError(String(e));
      try {
        setAutostartState(await isAutostartEnabled());
      } catch {
        // Reading it back failed too; leave the switch as the user left it
        // rather than guessing, the error above already says what happened.
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
            label="Start with Windows"
            description="Launch Claudy into the tray when you sign in, so your shortcuts work straight away"
            checked={autostart ?? false}
            disabled={autostart === null}
            onChange={(v) => void toggleAutostart(v)}
          />
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

      <Card>
        <CardHeader>
          <CardTitle>Usage stats</CardTitle>
          <CardDescription>
            A local record of what you run, behind the Usage page. It stays on this
            computer and stores metadata only — never your prompts, the text you
            select, or the answers.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          <ToggleRow
            label="Record usage stats"
            description="Log each run's time, prompt name, model, token counts and outcome"
            checked={settings.usage.enabled}
            onChange={(v) => void safeUpdate({ usage: { ...settings.usage, enabled: v } })}
          />
          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Keep history for</Label>
              <p className="text-muted-foreground text-sm">
                Older entries are deleted automatically
              </p>
            </div>
            <Select
              value={String(settings.usage.retentionDays)}
              onValueChange={(v) =>
                void safeUpdate({ usage: { ...settings.usage, retentionDays: Number(v) } })
              }
            >
              <SelectTrigger className="w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {RETENTIONS.map((r) => (
                  <SelectItem key={r.value} value={String(r.value)}>
                    {r.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Clear history</Label>
              <p className="text-muted-foreground text-sm">
                Delete everything recorded so far
              </p>
            </div>
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="outline">Clear all</Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Clear usage history?</AlertDialogTitle>
                  <AlertDialogDescription>
                    Every recorded run is deleted and the Usage page starts from
                    empty. This can't be undone.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction
                    onClick={() => {
                      void clearUsage().catch((e: unknown) => setError(String(e)));
                    }}
                  >
                    Clear all
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
