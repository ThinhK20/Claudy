import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export type ProviderId = "openai_compatible" | "ollama" | "anthropic" | "gemini";

export interface ProviderSettings {
  baseUrl: string;
  model: string;
}

export interface AiSettings {
  activeProvider: ProviderId;
  openaiCompatible: ProviderSettings;
  ollama: ProviderSettings;
  anthropic: ProviderSettings;
  gemini: ProviderSettings;
}

export interface AssistantSettings {
  shortcut: string;
  ttsVoice: string;
  speechSpeed: number;
  volume: number;
  autoSpeak: boolean;
  autoWebSearch: boolean;
  panelCloseSecs: number;
  keepOpenWhileSpeaking: boolean;
  customSystemPrompt: string; // "" = no system prompt (default behavior)
}

/** "hold" = hold to talk, release to transcribe (a quick tap latches it). */
export type DictationMode = "hold" | "toggle";

export interface Settings {
  theme: "light" | "dark" | "system";
  language: string;
  model: string;
  micDevice: string;
  dictationShortcut: string;
  dictationMode: DictationMode;
  keepModelWarm: boolean;
  restoreClipboard: boolean;
  autoPaste: boolean;
  notificationsEnabled: boolean;
  startMinimized: boolean;
  modelsDirOverride: string;
  ai: AiSettings;
  assistant: AssistantSettings;
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
