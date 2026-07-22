import { useCallback, useEffect, useRef, useState } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Check, Copy, Loader2, Mic, Paperclip, RotateCw, Square, Volume2, X } from "lucide-react";
import { ThemeProvider } from "@/components/theme-provider";
import { Button } from "@/components/ui/button";
import { LevelBars } from "@/components/transcription/level-bars";
import { useSettings } from "@/lib/settings-store";
import { insertAtCursor } from "@/lib/insert-at-cursor";
import { useMicTranscription } from "@/lib/use-mic-transcription";
import {
  activeProviderSupportsImages,
  askAssistant,
  assistantNewQuestion,
  closeAssistant,
  getAssistantPhase,
  onAssistantState,
  replayAssistantSpeech,
  resizeAssistantInput,
  setAssistantDialogOpen,
  stopAssistantSpeech,
  type AssistantPhase,
} from "@/lib/assistant-api";
import {
  fileToAttachment,
  imageFilesOnly,
  type Attachment,
} from "@/lib/image-attach";

export default function AssistantPage() {
  return (
    <ThemeProvider>
      <AssistantPanel />
    </ThemeProvider>
  );
}

function AssistantPanel() {
  const load = useSettings((s) => s.load);
  const panelCloseSecs = useSettings((s) => s.settings?.assistant.panelCloseSecs ?? 0);
  const keepOpenWhileSpeaking = useSettings(
    (s) => s.settings?.assistant.keepOpenWhileSpeaking ?? true,
  );
  const micDevice = useSettings((s) => s.settings?.micDevice ?? "");
  const hasSttModel = useSettings((s) => Boolean(s.settings?.model));

  const [phase, setPhase] = useState<AssistantPhase>("input");
  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [ttsError, setTtsError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [supportsImages, setSupportsImages] = useState(true);

  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const hasAttachments = attachments.length > 0;
  const imagesBlocked = hasAttachments && !supportsImages;

  useEffect(() => {
    load().catch(() => {});
    getAssistantPhase().then(setPhase).catch(() => {});
    const un = onAssistantState((s) => {
      setPhase(s.phase);
      if (s.question !== null) setQuestion(s.question);
      if (s.answer !== null) setAnswer(s.answer);
      setMessage(s.message);
      setTtsError(s.ttsError);
      // A fresh input drops the previous question's draft and attachments.
      if (s.phase === "input") {
        setDraft("");
        setAttachments([]);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [load]);

  // WebView2 doesn't always honor focus on a freshly shown window; retry on
  // the next frame after entering the input phase. Also re-load settings and
  // re-check image support on each open — this window outlives Settings-page
  // changes made in main.
  useEffect(() => {
    if (phase !== "input") return;
    load().catch(() => {});
    activeProviderSupportsImages()
      .then(setSupportsImages)
      .catch(() => setSupportsImages(true));
    const focus = () => textareaRef.current?.focus();
    focus();
    const raf = requestAnimationFrame(focus);
    return () => cancelAnimationFrame(raf);
  }, [phase, load]);

  // Grow the popup to fit the thumbnail row, shrink back when it's empty.
  useEffect(() => {
    if (phase !== "input") return;
    void resizeAssistantInput(hasAttachments);
  }, [phase, hasAttachments]);

  const addFiles = useCallback(async (files: File[]) => {
    const images = imageFilesOnly(files);
    if (images.length === 0) return;
    try {
      const added = await Promise.all(images.map(fileToAttachment));
      setAttachments((prev) => [...prev, ...added]);
    } catch {
      // A single unreadable file shouldn't wipe the box; ignore it.
    }
  }, []);

  const removeAttachment = useCallback((id: string) => {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  }, []);

  const onPaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const files = Array.from(e.clipboardData.items)
      .filter((it) => it.kind === "file" && it.type.startsWith("image/"))
      .map((it) => it.getAsFile())
      .filter((f): f is File => f !== null);
    if (files.length > 0) {
      e.preventDefault();
      void addFiles(files);
    }
  };

  const onDrop = (e: React.DragEvent<HTMLDivElement>) => {
    if (e.dataTransfer.files.length === 0) return;
    e.preventDefault();
    void addFiles(Array.from(e.dataTransfer.files));
  };

  const onDragOver = (e: React.DragEvent<HTMLDivElement>) => {
    if (Array.from(e.dataTransfer.items).some((it) => it.kind === "file")) {
      e.preventDefault();
    }
  };

  const onFilePicked = (e: React.ChangeEvent<HTMLInputElement>) => {
    void addFiles(e.target.files ? Array.from(e.target.files) : []);
    e.target.value = ""; // let the same file be picked again
  };

  // Open the native picker. The dialog steals focus, which would otherwise
  // trigger the Input-phase blur-dismiss, so we flag it in the backend first.
  // When the dialog closes (pick OR cancel) the window regains focus — that's a
  // reliable signal to clear the flag regardless of picker cancel-event support.
  const openPicker = useCallback(() => {
    void setAssistantDialogOpen(true);
    const clear = () => {
      void setAssistantDialogOpen(false);
      window.removeEventListener("focus", clear);
    };
    window.addEventListener("focus", clear);
    fileInputRef.current?.click();
  }, []);

  // Drop transcribed speech in at the caret and keep focus for editing — we
  // fill the box rather than auto-sending so a misheard word can be fixed.
  const insertTranscript = useCallback(
    (text: string) => {
      const el = textareaRef.current;
      const start = el?.selectionStart ?? draft.length;
      const end = el?.selectionEnd ?? draft.length;
      const next = insertAtCursor(draft, text, start, end);
      setDraft(next.text);
      requestAnimationFrame(() => {
        const node = textareaRef.current;
        if (!node) return;
        node.focus();
        node.setSelectionRange(next.caret, next.caret);
      });
    },
    [draft],
  );

  const mic = useMicTranscription({ device: micDevice, onText: insertTranscript });

  // Never leave the mic recording once we leave the input phase (submit/close).
  useEffect(() => {
    if (phase !== "input") mic.cancel();
  }, [phase, mic.cancel]);

  const canSend = (draft.trim().length > 0 || hasAttachments) && !imagesBlocked;

  const submit = useCallback(() => {
    const q = draft.trim();
    if (!q && attachments.length === 0) return;
    if (attachments.length > 0 && !supportsImages) return;
    void askAssistant(
      q,
      attachments.map((a) => ({ mediaType: a.mediaType, data: a.data })),
    );
  }, [draft, attachments, supportsImages]);

  const onInputKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      mic.cancel();
      void closeAssistant();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <div className="text-foreground flex h-screen flex-col overflow-hidden">
      {phase === "input" && (
        <div
          data-tauri-drag-region
          onDrop={onDrop}
          onDragOver={onDragOver}
          className="bg-popover border-border flex h-full flex-col gap-2 rounded-xl border p-3 shadow-lg"
        >
          {hasAttachments && (
            <div className="flex flex-wrap gap-2">
              {attachments.map((a) => (
                <div key={a.id} className="relative">
                  <img
                    src={a.previewUrl}
                    alt="Attached image"
                    className="border-border h-16 w-16 rounded-md border object-cover"
                  />
                  <button
                    type="button"
                    onClick={() => removeAttachment(a.id)}
                    aria-label="Remove image"
                    className="bg-background border-border text-foreground absolute -top-1.5 -right-1.5 rounded-full border p-0.5 shadow"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </div>
              ))}
            </div>
          )}
          <textarea
            ref={textareaRef}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onInputKeyDown}
            onPaste={onPaste}
            placeholder="Ask anything… paste or drop an image"
            className="placeholder:text-muted-foreground flex-1 resize-none bg-transparent text-sm outline-none"
          />
          {imagesBlocked && (
            <p className="text-destructive text-xs">
              The current model can't read images — remove them or switch provider in Settings.
            </p>
          )}
          {mic.error && <p className="text-destructive text-xs">{mic.error}</p>}
          <div className="text-muted-foreground flex items-center justify-between gap-2 text-xs">
            <span data-tauri-drag-region className="select-none">
              Enter to send · Shift+Enter for a new line · Esc to close
            </span>
            <div className="flex items-center gap-1">
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                multiple
                hidden
                onChange={onFilePicked}
              />
              <Button
                size="sm"
                variant={mic.state === "recording" ? "destructive" : "ghost"}
                aria-label={
                  !hasSttModel
                    ? "Select a speech model in Settings to use voice"
                    : mic.state === "recording"
                      ? "Stop recording"
                      : "Record voice prompt"
                }
                disabled={!hasSttModel || mic.state === "transcribing"}
                onClick={mic.toggle}
              >
                {mic.state === "transcribing" ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : mic.state === "recording" ? (
                  <LevelBars level={mic.level} />
                ) : (
                  <Mic className="h-4 w-4" />
                )}
              </Button>
              <Button size="sm" variant="ghost" aria-label="Attach image" onClick={openPicker}>
                <Paperclip className="h-4 w-4" />
              </Button>
              <Button size="sm" onClick={submit} disabled={!canSend}>
                Ask
              </Button>
            </div>
          </div>
        </div>
      )}

      {phase !== "input" && (
        <ResponsePanel
          phase={phase}
          question={question}
          answer={answer}
          message={message}
          ttsError={ttsError}
          panelCloseSecs={panelCloseSecs}
          keepOpenWhileSpeaking={keepOpenWhileSpeaking}
          onRetry={() => question.trim() && void askAssistant(question)}
        />
      )}
    </div>
  );
}

/// Compact markdown styling for the small floating panel; headings render as
/// bold text so a stray "#" can't blow up the layout.
const MD_COMPONENTS: Components = {
  p: ({ node: _, ...p }) => <p className="mb-2 last:mb-0" {...p} />,
  strong: ({ node: _, ...p }) => <strong className="font-semibold" {...p} />,
  ul: ({ node: _, ...p }) => <ul className="mb-2 list-disc pl-5" {...p} />,
  ol: ({ node: _, ...p }) => <ol className="mb-2 list-decimal pl-5" {...p} />,
  li: ({ node: _, ...p }) => <li className="mb-1" {...p} />,
  h1: ({ node: _, ...p }) => <p className="mt-2 mb-1 font-semibold" {...p} />,
  h2: ({ node: _, ...p }) => <p className="mt-2 mb-1 font-semibold" {...p} />,
  h3: ({ node: _, ...p }) => <p className="mt-2 mb-1 font-semibold" {...p} />,
  a: ({ node: _, ...p }) => <a className="underline" target="_blank" rel="noreferrer" {...p} />,
  code: ({ node: _, ...p }) => (
    <code className="bg-muted rounded px-1 font-mono text-xs" {...p} />
  ),
  pre: ({ node: _, ...p }) => (
    <pre className="bg-muted mb-2 overflow-x-auto rounded p-2 text-xs" {...p} />
  ),
  blockquote: ({ node: _, ...p }) => (
    <blockquote className="border-border mb-2 border-l-2 pl-2" {...p} />
  ),
};

interface ResponsePanelProps {
  phase: AssistantPhase;
  question: string;
  answer: string;
  message: string | null;
  ttsError: string | null;
  panelCloseSecs: number;
  keepOpenWhileSpeaking: boolean;
  onRetry: () => void;
}

function ResponsePanel({
  phase,
  question,
  answer,
  message,
  ttsError,
  panelCloseSecs,
  keepOpenWhileSpeaking,
  onRetry,
}: ResponsePanelProps) {
  const [copied, setCopied] = useState(false);
  const [hovering, setHovering] = useState(false);
  const [activity, setActivity] = useState(0);

  // Auto-close: while an answer is shown — and during speech when the user
  // opted out of keeping the panel open — paused on hover and reset on any
  // interaction. 0 disables it entirely. Closing mid-speech stops playback.
  useEffect(() => {
    const countdownActive =
      phase === "answering" || (phase === "speaking" && !keepOpenWhileSpeaking);
    if (!countdownActive) return;
    if (panelCloseSecs <= 0 || hovering) return;
    const t = setTimeout(() => void closeAssistant(), panelCloseSecs * 1000);
    return () => clearTimeout(t);
  }, [phase, panelCloseSecs, keepOpenWhileSpeaking, hovering, activity]);

  const copy = async () => {
    try {
      await writeText(answer);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard write can fail if another app holds it; ignore.
    }
  };

  // Hand a bottom-right corner drag to the OS to resize the window. The dragged
  // size is remembered in Rust (see lib.rs `Resized` handler) for the session.
  const startResize = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    void getCurrentWindow().startResizeDragging("SouthEast");
  };

  // The panel can be dragged larger only once an answer has responded (not while
  // still "Thinking…"), matching where the remembered-size capture is gated.
  const canResize = phase === "answering" || phase === "speaking" || phase === "error";

  return (
    <div
      className="bg-popover border-border relative flex h-full flex-col rounded-xl border shadow-lg"
      onMouseEnter={() => setHovering(true)}
      onMouseLeave={() => setHovering(false)}
      onKeyDownCapture={() => setActivity((n) => n + 1)}
      onScrollCapture={() => setActivity((n) => n + 1)}
    >
      <div
        data-tauri-drag-region
        className="border-border text-muted-foreground shrink-0 border-b px-3 py-2 text-xs"
      >
        <span data-tauri-drag-region className="line-clamp-2 select-none whitespace-pre-wrap">
          {question}
        </span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2 text-sm">
        {phase === "loading" && (
          <div className="text-muted-foreground flex items-center gap-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            <span>Thinking…</span>
          </div>
        )}
        {phase === "error" && (
          <p className="text-destructive whitespace-pre-wrap">
            {message ?? "Something went wrong"}
          </p>
        )}
        {(phase === "answering" || phase === "speaking") && (
          <Markdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
            {answer}
          </Markdown>
        )}
      </div>

      {ttsError && (
        <p className="text-muted-foreground border-border shrink-0 border-t px-3 py-1.5 text-xs">
          {ttsError}
        </p>
      )}

      <div className="border-border flex shrink-0 items-center justify-end gap-2 border-t px-3 py-2">
        {phase === "error" && (
          <Button variant="outline" size="sm" onClick={onRetry}>
            <RotateCw className="h-4 w-4" />
            Retry
          </Button>
        )}
        {phase === "speaking" && (
          <Button variant="ghost" size="sm" onClick={() => void stopAssistantSpeech()}>
            <Square className="h-4 w-4" />
            Stop
          </Button>
        )}
        {phase === "answering" && (
          <Button variant="ghost" size="sm" onClick={() => void replayAssistantSpeech().catch(() => {})}>
            <Volume2 className="h-4 w-4" />
            Replay
          </Button>
        )}
        {(phase === "answering" || phase === "speaking") && (
          <>
            <Button variant="ghost" size="sm" onClick={copy}>
              {copied ? (
                <Check className="h-4 w-4" />
              ) : (
                <Copy className="h-4 w-4" />
              )}
              {copied ? "Copied" : "Copy"}
            </Button>
            <Button variant="outline" size="sm" onClick={() => void assistantNewQuestion()}>
              <RotateCw className="h-4 w-4" />
              Ask another
            </Button>
          </>
        )}
        <Button variant="ghost" size="sm" onClick={() => void closeAssistant()}>
          <X className="h-4 w-4" />
          Close
        </Button>
      </div>

      {canResize && (
        <div
          onMouseDown={startResize}
          title="Drag to resize"
          className="text-muted-foreground/50 hover:text-muted-foreground absolute right-0 bottom-0 z-10 flex h-4 w-4 cursor-nwse-resize items-end justify-end p-[3px]"
        >
          <svg viewBox="0 0 10 10" className="h-full w-full" aria-hidden="true">
            <path
              d="M9 1 L1 9 M9 5 L5 9"
              stroke="currentColor"
              strokeWidth="1.25"
              strokeLinecap="round"
              fill="none"
            />
          </svg>
        </div>
      )}
    </div>
  );
}
