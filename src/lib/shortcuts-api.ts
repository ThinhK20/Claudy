import { invoke } from "@tauri-apps/api/core";

export interface ShortcutCheck {
  ok: boolean;
  message: string;
}

/**
 * Validate an accelerator and check it against existing bindings
 * (dictation + enabled prompt shortcuts). Conflicts are warnings, not
 * hard errors — the backend skips conflicting bindings at sync time.
 */
export const checkShortcut = (
  accel: string,
  opts: { excludePromptId?: string; forDictation?: boolean; forAssistant?: boolean } = {},
): Promise<ShortcutCheck> =>
  invoke("check_shortcut", {
    accel,
    excludePromptId: opts.excludePromptId ?? null,
    forDictation: opts.forDictation ?? false,
    forAssistant: opts.forAssistant ?? false,
  });

/** Release every global shortcut so a recorder can capture live combos. */
export const suspendGlobalShortcuts = (): Promise<void> =>
  invoke("suspend_global_shortcuts");

/** Re-register all global shortcuts from stored settings/prompts. */
export const resumeGlobalShortcuts = (): Promise<void> =>
  invoke("resume_global_shortcuts");
