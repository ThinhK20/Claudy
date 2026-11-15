import { CircleAlert, CircleCheck, Info, TriangleAlert, X } from "lucide-react";
import { cn } from "@/lib/utils";

export type StatusTone = "error" | "warning" | "success" | "info";

interface InlineStatusProps {
  tone?: StatusTone;
  children: React.ReactNode;
  className?: string;
  /** Shows a small dismiss button when set. */
  onDismiss?: () => void;
}

const TONE_CLASS: Record<StatusTone, string> = {
  error: "text-destructive",
  warning: "text-warning",
  success: "text-success",
  info: "text-muted-foreground",
};

const TONE_ICON: Record<StatusTone, React.ElementType> = {
  error: CircleAlert,
  warning: TriangleAlert,
  success: CircleCheck,
  info: Info,
};

/**
 * The one way the app reports an outcome next to the control that caused it.
 * Errors are announced immediately (`role="alert"`); confirmations, warnings
 * and notes politely (`role="status"`).
 */
export function InlineStatus({
  tone = "error",
  children,
  className,
  onDismiss,
}: InlineStatusProps) {
  const Icon = TONE_ICON[tone];
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={cn("flex items-start gap-1.5 text-sm", TONE_CLASS[tone], className)}
    >
      <Icon className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
      <span className="min-w-0 flex-1 break-words whitespace-pre-wrap">{children}</span>
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss"
          className="hover:bg-muted focus-visible:ring-ring -my-0.5 flex size-6 shrink-0 items-center justify-center rounded-md outline-none focus-visible:ring-2"
        >
          <X className="size-3.5" aria-hidden="true" />
        </button>
      )}
    </div>
  );
}
