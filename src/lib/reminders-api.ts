import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ReminderStatus = "pending" | "fired" | "done";

export interface Reminder {
  id: string;
  /** Exactly what the user typed — never rewritten. */
  text: string;
  /** `text` with the time phrase stripped; what the notification shows. */
  title: string;
  /** RFC3339 with offset, e.g. "2026-07-24T16:32:00+07:00". */
  dueAt: string;
  createdAt: string;
  /** RFC3339; "" until it fires. */
  firedAt: string;
  status: ReminderStatus;
}

/** Result of the local-only parse behind the Remind Mode chip. */
export interface ReminderPreview {
  title: string;
  /** RFC3339, or null when the local parser found no time in the text. */
  dueAt: string | null;
}

/** Local parser only — safe to call on every keystroke, never hits the network. */
export const previewReminder = (text: string): Promise<ReminderPreview> =>
  invoke("preview_reminder", { text });

/** The Enter path in Remind Mode. Falls back to one AI call when the local
 *  parser finds no time, so this can take a couple of seconds and can reject. */
export const createReminderFromText = (text: string): Promise<Reminder> =>
  invoke("create_reminder_from_text", { text });

export const listReminders = (): Promise<Reminder[]> => invoke("list_reminders");

/** Upsert. An empty `id` creates; returns the stored reminder. */
export const saveReminder = (reminder: Reminder): Promise<Reminder> =>
  invoke("save_reminder", { reminder });

export const deleteReminder = (id: string): Promise<void> =>
  invoke("delete_reminder", { id });

/** Re-arm a reminder `minutes` from now, whatever its current status. */
export const snoozeReminder = (id: string, minutes: number): Promise<Reminder> =>
  invoke("snooze_reminder", { id, minutes });

export const completeReminder = (id: string): Promise<void> =>
  invoke("complete_reminder", { id });

/** Fired by the backend after every mutation and every fire, so an open list
 *  refreshes without polling. */
export const onRemindersChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("reminders-changed", () => cb());
