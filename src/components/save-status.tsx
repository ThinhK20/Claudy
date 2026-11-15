import { InlineStatus } from "@/components/inline-status";
import type { SaveFeedback } from "@/lib/use-safe-update";

/**
 * Outcome of the page's auto-saved fields, pinned to the bottom of the window
 * so it is visible next to whichever control was just changed, however far
 * the page is scrolled. The live regions stay mounted so screen readers pick
 * up each change.
 */
export function SaveStatus({ feedback }: { feedback: SaveFeedback }) {
  return (
    <div className="pointer-events-none fixed right-4 bottom-4 z-40 flex max-w-sm flex-col items-end gap-2">
      {feedback.error && (
        <div className="bg-popover pointer-events-auto rounded-lg border p-3 shadow-md">
          <InlineStatus onDismiss={feedback.dismissError}>{feedback.error}</InlineStatus>
        </div>
      )}
      <div role="status" aria-live="polite">
        {feedback.saved && !feedback.error && (
          <span className="bg-popover text-success inline-flex rounded-md border px-2.5 py-1 text-xs font-medium shadow-sm">
            Saved
          </span>
        )}
      </div>
    </div>
  );
}
