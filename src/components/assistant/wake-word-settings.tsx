import { useCallback, useEffect, useState } from "react";
import { Ear, TriangleAlert } from "lucide-react";
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
import { ToggleRow } from "@/components/toggle-row";
import { listModels, type ModelInfo } from "@/lib/stt-api";
import { checkWakePhrase, onWakeState, wakeWordListening } from "@/lib/wake-api";
import {
  useSettings,
  type WakeSensitivity,
  type WakeWordSettings,
} from "@/lib/settings-store";

/** Matches the live-preview debounce used by the ask box's time parser. */
const PHRASE_DEBOUNCE_MS = 300;

const SENSITIVITY_OPTIONS: { value: WakeSensitivity; label: string }[] = [
  { value: "strict", label: "Strict" },
  { value: "balanced", label: "Balanced" },
  { value: "lenient", label: "Lenient" },
];

interface WakeWordCardProps {
  /** Persist a change; the parent owns error handling and the reload on failure. */
  onChange: (patch: Partial<WakeWordSettings>) => void;
}

export function WakeWordCard({ onChange }: WakeWordCardProps) {
  const wake = useSettings((s) => s.settings?.assistant.wakeWord);

  const [models, setModels] = useState<ModelInfo[]>([]);
  const [phrase, setPhrase] = useState(wake?.phrase ?? "");
  const [phraseWarning, setPhraseWarning] = useState("");
  const [listening, setListening] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The store loads asynchronously, so adopt the saved phrase once it arrives
  // (and after a rejected save resets it).
  const savedPhrase = wake?.phrase;
  useEffect(() => {
    if (savedPhrase !== undefined) setPhrase(savedPhrase);
  }, [savedPhrase]);

  useEffect(() => {
    listModels().then(setModels).catch(() => setModels([]));
    wakeWordListening().then(setListening).catch(() => {});
    let unlisten: (() => void) | undefined;
    onWakeState((s) => {
      setListening(s.phase === "listening" || s.phase === "triggered");
      setError(s.phase === "error" ? s.message : null);
    }).then((un) => {
      unlisten = un;
    });
    return () => unlisten?.();
  }, []);

  // Ask Rust for the advice so the rule lives in exactly one place.
  useEffect(() => {
    const timer = setTimeout(() => {
      checkWakePhrase(phrase)
        .then(setPhraseWarning)
        .catch(() => setPhraseWarning(""));
    }, PHRASE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [phrase]);

  // Save on blur rather than per keystroke: every save restarts the listener.
  const commitPhrase = useCallback(() => {
    const next = phrase.trim();
    if (!next || next === wake?.phrase) {
      setPhrase(wake?.phrase ?? "");
      return;
    }
    onChange({ phrase: next });
  }, [phrase, wake?.phrase, onChange]);

  if (!wake) return null;

  const downloaded = models.filter((m) => m.downloaded);
  const modelMissing = models.length > 0 && !downloaded.some((m) => m.id === wake.model);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          Wake word
          {listening && (
            <span className="text-muted-foreground inline-flex items-center gap-1 text-xs font-normal">
              <Ear className="h-3.5 w-3.5" />
              listening
            </span>
          )}
        </CardTitle>
        <CardDescription>
          Say a phrase out loud to open the ask box with the microphone already
          running — no keyboard needed.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        <ToggleRow
          label="Listen for a wake word"
          description="Keeps the microphone open while Claudy is idle. Audio stays in memory, is never saved or sent anywhere, and a small speech model runs only when it hears talking — expect some CPU use in noisy rooms."
          checked={wake.enabled}
          onChange={(enabled) => onChange({ enabled })}
        />

        <div>
          <Label htmlFor="wake-phrase">Phrase</Label>
          <p className="text-muted-foreground mb-2 text-sm">
            What you say to open the ask box.
          </p>
          <Input
            id="wake-phrase"
            value={phrase}
            placeholder="hey claudy"
            onChange={(e) => setPhrase(e.target.value)}
            onBlur={commitPhrase}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
            }}
          />
          {phraseWarning && (
            <p className="text-muted-foreground mt-2 flex items-start gap-1.5 text-sm">
              <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              {phraseWarning}
            </p>
          )}
        </div>

        <div className="flex items-center justify-between gap-4">
          <div>
            <Label>Detection model</Label>
            <p className="text-muted-foreground text-sm">
              Runs every time you speak nearby, so keep it small. Separate from
              the model used for dictation.
            </p>
          </div>
          <Select
            value={wake.model}
            onValueChange={(model) => onChange({ model })}
            disabled={downloaded.length === 0}
          >
            <SelectTrigger className="w-52">
              <SelectValue placeholder="No models downloaded" />
            </SelectTrigger>
            <SelectContent>
              {downloaded.map((m) => (
                <SelectItem key={m.id} value={m.id}>
                  {m.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {modelMissing && (
          <p className="text-destructive text-sm">
            {downloaded.length === 0
              ? "No speech models are downloaded yet — get one on the Transcription page before turning this on."
              : `Model "${wake.model}" is not downloaded. Pick one above, or download it on the Transcription page.`}
          </p>
        )}

        <div className="flex items-center justify-between gap-4">
          <div>
            <Label>Sensitivity</Label>
            <p className="text-muted-foreground text-sm">
              How close the words have to be. Raise it if Claudy misses you,
              lower it if it opens on its own.
            </p>
          </div>
          <Select
            value={wake.sensitivity}
            onValueChange={(v) => onChange({ sensitivity: v as WakeSensitivity })}
          >
            <SelectTrigger className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SENSITIVITY_OPTIONS.map((o) => (
                <SelectItem key={o.value} value={o.value}>
                  {o.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {error && <p className="text-destructive text-sm">{error}</p>}
      </CardContent>
    </Card>
  );
}
