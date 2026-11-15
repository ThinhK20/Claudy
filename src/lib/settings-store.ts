import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "@/lib/error-message";
import type { ModelPrice } from "@/lib/usage-api";
import type { VocabRule } from "@/lib/vocabulary-api";

export type ProviderId = "openai_compatible" | "ollama" | "anthropic" | "gemini";

export interface ProviderSettings {
  baseUrl: string;
  model: string;
  /** Tried in order when `model` is overloaded. Empty = the built-in chain. */
  fallbackModels: string[];
}

export interface AiSettings {
  activeProvider: ProviderId;
  /** Off restores the old behavior: an overloaded model just fails. */
  modelFallbackEnabled: boolean;
  openaiCompatible: ProviderSettings;
  ollama: ProviderSettings;
  anthropic: ProviderSettings;
  gemini: ProviderSettings;
}

/** How forgiving the wake-phrase match is against what whisper actually heard. */
export type WakeSensitivity = "strict" | "balanced" | "lenient";

export interface WakeWordSettings {
  enabled: boolean;
  phrase: string;
  /** Whisper model used for detection only — kept small on purpose. */
  model: string;
  sensitivity: WakeSensitivity;
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
  wakeWord: WakeWordSettings;
}

/** "hold" = hold to talk, release to transcribe (a quick tap latches it). */
export type DictationMode = "hold" | "toggle";

/** The local usage log behind the Usage page. Metadata only, never sent
 *  anywhere — see the Privacy section of the README. */
export interface UsageSettings {
  enabled: boolean;
  /** Events older than this are dropped on the next write. 0 = keep forever. */
  retentionDays: number;
  /** Per-model price overrides for the cost estimate; empty = bundled table. */
  modelPrices: ModelPrice[];
}

/** Corrections applied to every finished transcript, before the text reaches
 *  the app you're typing into. See `src-tauri/src/vocabulary.rs`. */
export interface VocabularySettings {
  /** Master switch — turns every rule off without deleting the list. */
  enabled: boolean;
  rules: VocabRule[];
}

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
  usage: UsageSettings;
  vocabulary: VocabularySettings;
}

interface SettingsState {
  settings: Settings | null;
  /** True while a load is in flight. */
  loading: boolean;
  /** Why the last load failed; cleared by the next successful one. Pages show
   *  it (with Retry) when there are no settings to render at all. */
  loadError: string | null;
  /** Rejects on failure too, so callers that reconcile after a failed save
   *  can tell; the error is kept in `loadError` either way. */
  load: () => Promise<void>;
  update: (patch: Partial<Settings>) => Promise<void>;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: null,
  loading: false,
  loadError: null,
  load: async () => {
    set({ loading: true });
    try {
      const settings = await invoke<Settings>("get_settings");
      set({ settings, loading: false, loadError: null });
    } catch (e: unknown) {
      set({ loading: false, loadError: errorMessage(e) });
      throw e;
    }
  },
  update: async (patch) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...patch };
    set({ settings: next }); // optimistic
    await invoke("update_settings", { settings: next });
  },
}));
