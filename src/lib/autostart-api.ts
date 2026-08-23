import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

/**
 * Launch-at-login, kept deliberately outside the settings store: the OS is the
 * source of truth. The user can clear the entry from Task Manager, and an
 * installer can rewrite it, so a mirrored boolean in settings.json would
 * eventually lie. Every read goes back to the OS.
 */
export async function isAutostartEnabled(): Promise<boolean> {
  return isEnabled();
}

export async function setAutostart(on: boolean): Promise<void> {
  if (on) await enable();
  else await disable();
}
