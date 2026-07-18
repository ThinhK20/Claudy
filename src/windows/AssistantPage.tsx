import { useCallback, useEffect, useRef, useState } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy, Loader2, RotateCw, Square, Volume2, X } from "lucide-react";
import { ThemeProvider } from "@/components/theme-provider";
import { Button } from "@/components/ui/button";
import { useSettings } from "@/lib/settings-store";
import {
  askAssistant,
  assistantNewQuestion,
  closeAssistant,
  getAssistantPhase,
  onAssistantState,
  replayAssistantSpeech,
  stopAssistantSpeech,
  type AssistantPhase,
} from "@/lib/assistant-api";

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

  const [phase, setPhase] = useState<AssistantPhase>("input");
  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [ttsError, setTtsError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    load().catch(() => {});
    getAssistantPhase().then(setPhase).catch(() => {});
    const un = onAssistantState((s) => {
      setPhase(s.phase);
      if (s.question !== null) setQuestion(s.question);
      if (s.answer !== null) setAnswer(s.answer);
      setMessage(s.message);
      setTtsError(s.ttsError);
      if (s.phase === "input") setDraft("");
    });
    return () => {
      un.then((f) => f());
    };
  }, [load]);

  // WebView2 doesn't always honor focus on a freshly shown window; retry on
  // the next frame after entering the input phase. Also re-load settings on
  // each open — this window outlives Settings-page changes made in main.
  useEffect(() => {
    if (phase !== "input") return;
    load().catch(() => {});
    const focus = () => textareaRef.current?.focus();
    focus();
    const raf = requestAnimationFrame(focus);
    return () => cancelAnimationFrame(raf);
  }, [phase, load]);

  const submit = useCallback(() => {
    const q = draft.trim();
    if (!q) return;
    void askAssistant(q);
  }, [draft]);

  const onInputKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
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
          className="bg-popover border-border flex h-full flex-col gap-2 rounded-xl border p-3 shadow-lg"
        >
          <textarea
            ref={textareaRef}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onInputKeyDown}
            placeholder="Ask anything…"
            className="placeholder:text-muted-foreground flex-1 resize-none bg-transparent text-sm outline-none"
          />
          <div className="text-muted-foreground flex items-center justify-between text-xs">
            <span data-tauri-drag-region className="select-none">
              Enter to send · Shift+Enter for a new line · Esc to close
            </span>
            <Button size="sm" onClick={submit} disabled={!draft.trim()}>
              Ask
            </Button>
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

  return (
    <div
      className="bg-popover border-border flex h-full flex-col rounded-xl border shadow-lg"
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
    </div>
  );
}
