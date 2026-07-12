import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ModelInfo {
  id: string;
  label: string;
  diskSize: string;
  downloaded: boolean;
}

export type DownloadStatus = "downloading" | "verifying" | "done" | "error" | "cancelled";

export interface DownloadProgress {
  id: string;
  downloaded: number;
  total: number;
  status: DownloadStatus;
  message: string | null;
}

export interface TranscriptionResult {
  text: string;
  durationMs: number;
}

export const listModels = (): Promise<ModelInfo[]> => invoke("list_models");
export const downloadModel = (id: string): Promise<void> => invoke("download_model", { id });
export const cancelModelDownload = (id: string): Promise<void> =>
  invoke("cancel_model_download", { id });
export const deleteModel = (id: string): Promise<void> => invoke("delete_model", { id });
export const getModelsDir = (): Promise<string> => invoke("get_models_dir");
export const listAudioDevices = (): Promise<string[]> => invoke("list_audio_devices");
export const startCapture = (device: string): Promise<void> => invoke("start_capture", { device });
export const stopCapture = (): Promise<void> => invoke("stop_capture");
export const stopCaptureAndTranscribe = (): Promise<TranscriptionResult> =>
  invoke("stop_capture_and_transcribe");

export const onDownloadProgress = (
  cb: (progress: DownloadProgress) => void,
): Promise<UnlistenFn> =>
  listen<DownloadProgress>("model-download-progress", (event) => cb(event.payload));

export const onMicLevel = (cb: (level: number) => void): Promise<UnlistenFn> =>
  listen<{ level: number }>("mic-level", (event) => cb(event.payload.level));
