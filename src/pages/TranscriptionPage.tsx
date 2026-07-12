import { MicSettings } from "@/components/transcription/mic-settings";
import { ModelManager } from "@/components/transcription/model-manager";
import { TestRecorder } from "@/components/transcription/test-recorder";

export default function TranscriptionPage() {
  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Transcription</h1>
        <p className="text-muted-foreground mt-1">
          Whisper models, microphone and language settings.
        </p>
      </div>
      <ModelManager />
      <MicSettings />
      <TestRecorder />
    </div>
  );
}
