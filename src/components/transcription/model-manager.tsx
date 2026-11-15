import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { ConfirmDialog } from "@/components/confirm-dialog";
import { InlineStatus } from "@/components/inline-status";
import { errorMessage } from "@/lib/error-message";
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
  const [toDelete, setToDelete] = useState<ModelInfo | null>(null);
  const activeModel = useSettings((s) => s.settings?.model ?? "");
  const updateSettings = useSettings((s) => s.update);

  const refresh = useCallback(async () => {
    try {
      setModels(await listModels());
      setModelsDir(await getModelsDir());
    } catch (e: unknown) {
      setError(`Couldn't list speech models: ${errorMessage(e)}`);
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
      setError(`Download failed: ${errorMessage(e)}`);
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
      setError(`Couldn't delete the model: ${errorMessage(e)}`);
    }
  };

  const useModel = async (id: string) => {
    setError(null);
    try {
      await updateSettings({ model: id });
    } catch (e: unknown) {
      setError(`Couldn't switch models: ${errorMessage(e)}`);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Speech models</CardTitle>
        <CardDescription>
          Whisper speech models turn your voice into text on this computer. Larger
          models are more accurate but slower. Stored in{" "}
          <code className="text-xs break-all">{modelsDir}</code>.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {error && <InlineStatus onDismiss={() => setError(null)}>{error}</InlineStatus>}
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
                    <Button size="sm" onClick={() => void useModel(m.id)}>
                      Use
                    </Button>
                  )}
                  <Button variant="outline" size="sm" onClick={() => setToDelete(m)}>
                    Delete
                  </Button>
                </div>
              ) : (
                <Button size="sm" onClick={() => void handleDownload(m.id)}>
                  Download
                </Button>
              )}
            </div>
          );
        })}
      </CardContent>

      <ConfirmDialog
        open={toDelete !== null}
        onOpenChange={(open) => {
          if (!open) setToDelete(null);
        }}
        title={`Delete the ${toDelete?.label ?? ""} speech model?`}
        description={
          toDelete && activeModel === toDelete.id
            ? `Dictation uses this model now, so it stops working until you pick another. To use it again you'd re-download ${toDelete.diskSize}.`
            : `The file is removed from disk. To use it again you'd re-download ${toDelete?.diskSize ?? "it"}.`
        }
        confirmLabel="Delete"
        onConfirm={() => {
          if (toDelete) void handleDelete(toDelete.id);
          setToDelete(null);
        }}
      />
    </Card>
  );
}
