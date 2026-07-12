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
import { Switch } from "@/components/ui/switch";
import { useSettings } from "@/lib/settings-store";
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
  const settings = useSettings((s) => s.settings);
  const updateSettings = useSettings((s) => s.update);

  useEffect(() => {
    listAudioDevices()
      .then(setDevices)
      .catch((e: unknown) => setError(String(e)));
    return () => {
      unlistenRef.current?.();
      if (isTestingRef.current) stopCapture().catch(() => {});
    };
  }, []);

  if (!settings) return null;

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
        unlistenRef.current = await onMicLevel(setLevel);
        await startCapture(settings.micDevice);
        setIsTesting(true);
        isTestingRef.current = true;
      }
    } catch (e: unknown) {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setIsTesting(false);
      isTestingRef.current = false;
      setError(String(e));
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Microphone & Language</CardTitle>
        <CardDescription>Input device and transcription language</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {error && <p className="text-destructive text-sm">{error}</p>}
        <div className="flex items-center gap-3">
          <Label className="w-28 shrink-0">Microphone</Label>
          <Select
            value={settings.micDevice || DEFAULT_DEVICE}
            onValueChange={(v) =>
              updateSettings({ micDevice: v === DEFAULT_DEVICE ? "" : v })
            }
          >
            <SelectTrigger className="flex-1">
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
          <Button variant="outline" size="sm" onClick={toggleTest}>
            {isTesting ? "Stop test" : "Test mic"}
          </Button>
        </div>
        {isTesting && (
          <div className="bg-muted h-2 w-full overflow-hidden rounded-full">
            <div
              className="h-full bg-green-500 transition-[width] duration-75"
              style={{ width: `${Math.min(level * 400, 100)}%` }}
            />
          </div>
        )}
        <div className="flex items-center gap-3">
          <Label className="w-28 shrink-0">Language</Label>
          <Select
            value={settings.language}
            onValueChange={(v) => updateSettings({ language: v })}
          >
            <SelectTrigger className="flex-1">
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
        <div className="flex items-center gap-3">
          <Label className="w-28 shrink-0" htmlFor="keep-warm">
            Keep model warm
          </Label>
          <Switch
            id="keep-warm"
            checked={settings.keepModelWarm}
            onCheckedChange={(v) => updateSettings({ keepModelWarm: v })}
          />
          <span className="text-muted-foreground text-xs">
            Faster repeat transcriptions; uses more memory
          </span>
        </div>
      </CardContent>
    </Card>
  );
}
