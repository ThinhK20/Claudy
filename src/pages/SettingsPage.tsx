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
import { InlineStatus } from "@/components/inline-status";
import { Page, SettingsUnavailable } from "@/components/page";
import { SaveStatus } from "@/components/save-status";
import { ToggleRow } from "@/components/toggle-row";
import { errorMessage } from "@/lib/error-message";
import { useSettings, type Settings } from "@/lib/settings-store";
import { useSafeUpdate } from "@/lib/use-safe-update";
import { clearUsage } from "@/lib/usage-api";
import { isAutostartEnabled, setAutostart } from "@/lib/autostart-api";

const TITLE = "General";
const DESCRIPTION = "Appearance, startup and the local usage record.";

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

export default function SettingsPage() {
  const settings = useSettings((s) => s.settings);
  const { safeUpdate, feedback } = useSafeUpdate();
  const [autostartError, setAutostartError] = useState<string | null>(null);
  const [clearStatus, setClearStatus] = useState<
    { tone: "error" | "success"; text: string } | null
  >(null);
  // null until the first read resolves. Not in the settings store: the OS
  // owns this value, so it is read back from there rather than cached.
  const [autostart, setAutostartState] = useState<boolean | null>(null);

  useEffect(() => {
    void isAutostartEnabled()
      .then(setAutostartState)
      .catch((e: unknown) =>
        setAutostartError(`Couldn't read the startup setting: ${errorMessage(e)}`),
      );
  }, []);

  if (!settings) return <SettingsUnavailable title={TITLE} description={DESCRIPTION} />;

  // Same optimistic-then-reconcile shape as safeUpdate, but the truth is read
  // back from the OS instead of from the settings store.
  const toggleAutostart = async (on: boolean) => {
    setAutostartError(null);
    setAutostartState(on);
    try {
      await setAutostart(on);
    } catch (e: unknown) {
      setAutostartError(`Couldn't change the startup setting: ${errorMessage(e)}`);
      try {
        setAutostartState(await isAutostartEnabled());
      } catch {
        // Reading it back failed too; leave the switch as the user left it
        // rather than guessing, the error above already says what happened.
      }
    }
  };

  const doClearUsage = async () => {
    setClearStatus(null);
    try {
      await clearUsage();
      setClearStatus({ tone: "success", text: "Usage history cleared." });
    } catch (e: unknown) {
      setClearStatus({ tone: "error", text: `Couldn't clear history: ${errorMessage(e)}` });
    }
  };

  return (
    <Page title={TITLE} description={DESCRIPTION}>
      <Card>
        <CardHeader>
          <CardTitle>Appearance</CardTitle>
          <CardDescription>Light or dark colors for Claudy's windows.</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Theme</Label>
              <p className="text-muted-foreground text-sm">
                System follows your Windows light or dark setting.
              </p>
            </div>
            <Select
              value={settings.theme}
              onValueChange={(v) => void safeUpdate({ theme: v as Settings["theme"] })}
            >
              <SelectTrigger className="w-32" aria-label="Theme">
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
          <CardDescription>How Claudy delivers results and starts up.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-5">
          <div className="flex flex-col gap-2">
            <ToggleRow
              label="Start with Windows"
              description="Launch Claudy into the tray when you sign in, so your shortcuts work straight away."
              checked={autostart ?? false}
              disabled={autostart === null}
              onChange={(v) => void toggleAutostart(v)}
            />
            {autostartError && <InlineStatus>{autostartError}</InlineStatus>}
          </div>
          <ToggleRow
            label="Notifications"
            description="Show a desktop notification for results and errors."
            checked={settings.notificationsEnabled}
            onChange={(v) => void safeUpdate({ notificationsEnabled: v })}
          />
          <ToggleRow
            label="Auto-paste prompt results"
            description="Replace the selection with the result instead of only copying it. The result always stays on the clipboard."
            checked={settings.autoPaste}
            onChange={(v) => void safeUpdate({ autoPaste: v })}
          />
          <ToggleRow
            label="Start minimized"
            description="Start hidden in the tray instead of opening the main window."
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
            description="Log each run's time, prompt name, model, token counts and outcome."
            checked={settings.usage.enabled}
            onChange={(v) => void safeUpdate({ usage: { ...settings.usage, enabled: v } })}
          />
          <div className="flex items-center justify-between gap-4">
            <div>
              <Label>Keep history for</Label>
              <p className="text-muted-foreground text-sm">
                Older entries are deleted automatically.
              </p>
            </div>
            <Select
              value={String(settings.usage.retentionDays)}
              onValueChange={(v) =>
                void safeUpdate({ usage: { ...settings.usage, retentionDays: Number(v) } })
              }
            >
              <SelectTrigger className="w-36" aria-label="Keep history for">
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
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between gap-4">
              <div>
                <Label>Clear history</Label>
                <p className="text-muted-foreground text-sm">
                  Delete everything recorded so far.
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
                      variant="destructive"
                      onClick={() => void doClearUsage()}
                    >
                      Clear all
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            </div>
            {clearStatus && (
              <InlineStatus tone={clearStatus.tone}>{clearStatus.text}</InlineStatus>
            )}
          </div>
        </CardContent>
      </Card>

      <SaveStatus feedback={feedback} />
    </Page>
  );
}
