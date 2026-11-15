import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "@/lib/error-message";
import { useSettings, type Settings } from "@/lib/settings-store";

/** How long the "Saved" confirmation stays up after an auto-save. */
const SAVED_VISIBLE_MS = 2000;

export interface SaveFeedback {
  error: string | null;
  saved: boolean;
  dismissError: () => void;
}

/**
 * Auto-save for settings pages. `update()` is optimistic, so a rejected save
 * re-loads to show reality again. Reports a short "Saved" on success and
 * keeps the error up until the next save or a dismiss.
 */
export function useSafeUpdate(): {
  safeUpdate: (patch: Partial<Settings>) => Promise<void>;
  feedback: SaveFeedback;
} {
  const update = useSettings((s) => s.update);
  const load = useSettings((s) => s.load);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    },
    [],
  );

  const safeUpdate = useCallback(
    async (patch: Partial<Settings>) => {
      setError(null);
      setSaved(false);
      if (timerRef.current) clearTimeout(timerRef.current);
      try {
        await update(patch);
        setSaved(true);
        timerRef.current = setTimeout(() => setSaved(false), SAVED_VISIBLE_MS);
      } catch (e: unknown) {
        const message = errorMessage(e);
        setError(`Couldn't save: ${message}`);
        try {
          await load();
        } catch (reloadError: unknown) {
          setError(
            `Couldn't save: ${message} (could not refresh settings: ${errorMessage(reloadError)})`,
          );
        }
      }
    },
    [update, load],
  );

  const dismissError = useCallback(() => setError(null), []);

  return { safeUpdate, feedback: { error, saved, dismissError } };
}
