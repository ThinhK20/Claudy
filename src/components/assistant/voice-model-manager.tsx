import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import {
  deleteTtsModel,
  downloadTtsModel,
  ttsModelStatus,
  type TtsAssetInfo,
} from "@/lib/assistant-api";
import { cancelModelDownload, onDownloadProgress, type DownloadProgress } from "@/lib/stt-api";

const TTS_IDS = ["kokoro-model", "kokoro-voices"];

export function VoiceModelManager() {
  const [assets, setAssets] = useState<TtsAssetInfo[]>([]);
  const [progress, setProgress] = useState<Record<string, DownloadProgress | undefined>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setAssets(await ttsModelStatus());
    } catch (e: unknown) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    let unlisten: (() => void) | undefined;
    onDownloadProgress((p) => {
      if (!TTS_IDS.includes(p.id)) return;
      setProgress((prev) => ({ ...prev, [p.id]: p }));
      if (p.status === "done") refresh();
      if (p.status === "error") {
        setError(p.message ?? "Download failed");
        setBusy(false);
      }
      if (p.status === "cancelled") setBusy(false);
    }).then((un) => {
      unlisten = un;
    });
    return () => unlisten?.();
  }, [refresh]);

  const allDownloaded = assets.length > 0 && assets.every((a) => a.downloaded);
  const active = assets.find((a) => {
    const p = progress[a.id];
    return p?.status === "downloading" || p?.status === "verifying";
  });

  const handleDownload = async () => {
    setError(null);
    setBusy(true);
    try {
      for (const a of assets) {
        if (!a.downloaded) await downloadTtsModel(a.id);
      }
      await refresh();
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async () => {
    setError(null);
    try {
      for (const a of assets) {
        if (a.downloaded) await deleteTtsModel(a.id);
      }
      setProgress({});
      await refresh();
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  const activeProgress = active ? progress[active.id] : undefined;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-4">
        <div>
          <p className="text-sm font-medium">Voice model</p>
          <p className="text-muted-foreground text-sm">
            Local Kokoro neural voice (~115 MB). Required to speak answers aloud.
          </p>
        </div>
        {allDownloaded ? (
          <Button variant="outline" size="sm" onClick={handleDelete}>
            Delete
          </Button>
        ) : active ? (
          <Button variant="outline" size="sm" onClick={() => void cancelModelDownload(active.id)}>
            Cancel
          </Button>
        ) : (
          <Button size="sm" onClick={handleDownload} disabled={busy}>
            Download voice model
          </Button>
        )}
      </div>

      {active && activeProgress && (
        <div className="flex items-center gap-2">
          <Progress
            value={
              activeProgress.total > 0
                ? (activeProgress.downloaded / activeProgress.total) * 100
                : 0
            }
            className="h-2"
          />
          <span className="text-muted-foreground w-24 shrink-0 text-xs">
            {activeProgress.status === "verifying"
              ? "Verifying…"
              : `${active.label.split(" ")[0]} ${Math.round(
                  (activeProgress.downloaded / Math.max(activeProgress.total, 1)) * 100,
                )}%`}
          </span>
        </div>
      )}

      {error && <p className="text-destructive text-sm">{error}</p>}
    </div>
  );
}
