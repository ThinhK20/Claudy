import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { InlineStatus } from "@/components/inline-status";
import { ToggleRow } from "@/components/toggle-row";
import { errorMessage } from "@/lib/error-message";
import { useSettings, type Settings } from "@/lib/settings-store";
import { markSetupItemDone } from "@/lib/setup-checklist";
import { listAudioDevices, onMicLevel, startCapture, stopCapture } from "@/lib/stt-api";

const DEFAULT_DEVICE = "__default__";

const LANGUAGES = [
  { code: "auto", label: "Auto-detect" },
  { code: "en", label: "English" },
  { code: "vi", label: "Vietnamese" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
  { code: "zh", label: "Chinese" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "es", label: "Spanish" },
  { code: "pt", label: "Portuguese" },
];

export function MicSettings() {
  const [devices, setDevices] = useState<string[]>([]);
  const [isTesting, setIsTesting] = useState(false);
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const isTestingRef = useRef(false);
  // Guards the async gap in toggleTest: unmounting mid-subscribe would
  // otherwise leave the mic-level listener registered with no way to drop it.
  const mountedRef = useRef(true);
  const settings = useSettings((s) => s.settings);
  const updateSettings = useSettings((s) => s.update);

  useEffect(() => {
    listAudioDevices()
      .then(setDevices)
      .catch((e: unknown) => setError(`Couldn't list microphones: ${errorMessage(e)}`));
    return () => {
      mountedRef.current = false;
      unlistenRef.current?.();
      if (isTestingRef.current) stopCapture().catch(() => {});
    };
  }, []);

  if (!settings) return null;

  const save = async (patch: Partial<Settings>) => {
    setError(null);
    try {
      await updateSettings(patch);
    } catch (e: unknown) {
      setError(`Couldn't save: ${errorMessage(e)}`);
    }
  };

  const toggleTest = async () => {
    setError(null);
    try {
      if (isTesting) {
        await stopCapture();
        unlistenRef.current?.();
        unlistenRef.current = null;
        setIsTesting(false);
        isTestingRef.current = false;
        setLevel(0);
      } else {
        const unlisten = await onMicLevel(setLevel);
        if (!mountedRef.current) {
          // Unmounted while subscribing; nothing was captured yet, so drop the
          // listener and leave the audio slot alone.
          unlisten();
          return;
        }
        unlistenRef.current = unlisten;
        await startCapture(settings.micDevice);
        setIsTesting(true);
        isTestingRef.current = true;
        markSetupItemDone("mic-test");
      }
    } catch (e: unknown) {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setIsTesting(false);
      isTestingRef.current = false;
      setError(`Mic test failed: ${errorMessage(e)}`);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Microphone and language</CardTitle>
        <CardDescription>
          Which microphone to record from and the language you speak.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex items-center gap-3">
          <Label className="w-28 shrink-0">Microphone</Label>
          <Select
            value={settings.micDevice || DEFAULT_DEVICE}
            onValueChange={(v) => void save({ micDevice: v === DEFAULT_DEVICE ? "" : v })}
          >
            <SelectTrigger className="flex-1" aria-label="Microphone">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={DEFAULT_DEVICE}>System default</SelectItem>
              {devices.map((d) => (
                <SelectItem key={d} value={d}>
                  {d}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button variant="outline" size="sm" onClick={() => void toggleTest()}>
            {isTesting ? "Stop test" : "Test mic"}
          </Button>
        </div>
        {isTesting && (
          <div
            role="meter"
            aria-label="Microphone level"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(Math.min(level * 400, 100))}
            className="bg-muted h-2 w-full overflow-hidden rounded-full"
          >
            <div
              className="bg-success h-full transition-[width] duration-75"
              style={{ width: `${Math.min(level * 400, 100)}%` }}
            />
          </div>
        )}
        {error && <InlineStatus onDismiss={() => setError(null)}>{error}</InlineStatus>}
        <div className="flex items-center gap-3">
          <Label className="w-28 shrink-0">Language</Label>
          <Select
            value={settings.language}
            onValueChange={(v) => void save({ language: v })}
          >
            <SelectTrigger className="flex-1" aria-label="Language">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LANGUAGES.map((l) => (
                <SelectItem key={l.code} value={l.code}>
                  {l.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <ToggleRow
          label="Keep the speech model loaded"
          description="Dictation starts faster after the first time, but the model stays in memory between uses."
          checked={settings.keepModelWarm}
          onChange={(v) => void save({ keepModelWarm: v })}
        />
      </CardContent>
    </Card>
  );
}
