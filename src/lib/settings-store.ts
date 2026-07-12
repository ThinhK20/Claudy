import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export interface Settings {
  theme: "light" | "dark" | "system";
  language: string;
  model: string;
  micDevice: string;
  dictationShortcut: string;
  keepModelWarm: boolean;
  restoreClipboard: boolean;
  autoPaste: boolean;
  notificationsEnabled: boolean;
  startMinimized: boolean;
  modelsDirOverride: string;
}

interface SettingsState {
  settings: Settings | null;
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  load: async () => {
    set({ settings: await invoke<Settings>("get_settings") });
  },
  update: async (patch) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...patch };
    set({ settings: next }); // optimistic
    await invoke("update_settings", { settings: next });
  },
}));
