import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { useSettings } from "@/lib/settings-store";
import {
  cancelModelDownload,
  deleteModel,
  downloadModel,
  getModelsDir,
  listModels,
  onDownloadProgress,
  type DownloadProgress,
  type ModelInfo,
} from "@/lib/stt-api";

export function ModelManager() {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [modelsDir, setModelsDir] = useState("");
  const [progress, setProgress] = useState<Record<string, DownloadProgress | undefined>>({});
  const [error, setError] = useState<string | null>(null);
  const activeModel = useSettings((s) => s.settings?.model ?? "");
  const updateSettings = useSettings((s) => s.update);

  const refresh = useCallback(async () => {
    try {
      setModels(await listModels());
      setModelsDir(await getModelsDir());
    } catch (e: unknown) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    let unlisten: (() => void) | undefined;
    onDownloadProgress((p) => {
      setProgress((prev) => ({ ...prev, [p.id]: p }));
      if (p.status === "done") refresh();
      if (p.status === "error") setError(p.message ?? "Download failed");
    }).then((un) => {
      unlisten = un;
    });
    return () => unlisten?.();
  }, [refresh]);

  const handleDownload = async (id: string) => {
    setError(null);
    try {
      await downloadModel(id);
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  const handleDelete = async (id: string) => {
    setError(null);
    try {
      if (activeModel === id) await updateSettings({ model: "" });
      await deleteModel(id);
      setProgress((prev) => ({ ...prev, [id]: undefined }));
      await refresh();
    } catch (e: unknown) {
      setError(String(e));
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Models</CardTitle>
        <CardDescription>
          Whisper models are stored in <code className="text-xs">{modelsDir}</code>
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {error && <p className="text-destructive text-sm">{error}</p>}
        {models.map((m) => {
          const p = progress[m.id];
          const isDownloading = p?.status === "downloading" || p?.status === "verifying";
          return (
            <div key={m.id} className="flex items-center gap-3 rounded-md border p-3">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-medium">{m.label}</span>
                  <span className="text-muted-foreground text-xs">{m.diskSize}</span>
                  {activeModel === m.id && <Badge>Active</Badge>}
                </div>
                {isDownloading && p && (
                  <div className="mt-2 flex items-center gap-2">
                    <Progress
                      value={p.total > 0 ? (p.downloaded / p.total) * 100 : 0}
                      className="h-2"
                    />
                    <span className="text-muted-foreground w-20 shrink-0 text-xs">
                      {p.status === "verifying"
                        ? "Verifying…"
                        : `${Math.round((p.downloaded / Math.max(p.total, 1)) * 100)}%`}
                    </span>
                  </div>
                )}
              </div>
              {isDownloading ? (
                <Button variant="outline" size="sm" onClick={() => cancelModelDownload(m.id)}>
                  Cancel
                </Button>
              ) : m.downloaded ? (
                <div className="flex gap-2">
                  {activeModel !== m.id && (
                    <Button size="sm" onClick={() => updateSettings({ model: m.id })}>
                      Use
                    </Button>
                  )}
                  <Button variant="outline" size="sm" onClick={() => handleDelete(m.id)}>
                    Delete
                  </Button>
                </div>
              ) : (
                <Button size="sm" onClick={() => handleDownload(m.id)}>
                  Download
                </Button>
              )}
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}
