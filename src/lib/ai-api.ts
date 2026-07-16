import { invoke } from "@tauri-apps/api/core";
import type { ProviderId } from "@/lib/settings-store";

// --- API keys (Task 2) — the key is write-only; it never comes back. ---

export const setApiKey = (provider: ProviderId, key: string): Promise<void> =>
  invoke("set_api_key", { provider, key });

export const hasApiKey = (provider: ProviderId): Promise<boolean> =>
  invoke("has_api_key", { provider });

export const deleteApiKey = (provider: ProviderId): Promise<void> =>
  invoke("delete_api_key", { provider });

// --- Providers (Task 4) ---

/** Cheapest full round trip: endpoint + key + model. Resolves to the model's reply. */
export const testProvider = (providerId: ProviderId): Promise<string> =>
  invoke("test_provider", { providerId });

// --- Prompts (Tasks 5/7) — consumed by the Phase 5 prompt manager. ---

export interface Prompt {
  id: string;
  name: string;
  template: string;
  shortcut: string;
  enabled: boolean;
}

export const listPrompts = (): Promise<Prompt[]> => invoke("list_prompts");

/** Upsert; pass id: "" to create. Resolves to the stored prompt (id filled in). */
export const savePrompt = (prompt: Prompt): Promise<Prompt> =>
  invoke("save_prompt", { prompt });

export const deletePrompt = (id: string): Promise<void> =>
  invoke("delete_prompt", { id });

export const runPrompt = (id: string): Promise<void> => invoke("run_prompt", { id });

// --- Import / export (Phase 5) ---

export interface ImportReport {
  added: number;
  updated: number;
  /** Entries dropped entirely, with reasons. */
  skipped: string[];
  /** Entries imported after an adjustment (e.g. invalid shortcut cleared). */
  warnings: string[];
}

/** Write all prompts to `path` as a JSON array; resolves to the count. */
export const exportPrompts = (path: string): Promise<number> =>
  invoke("export_prompts", { path });

/** Merge prompts from a JSON export at `path` (upsert by id). */
export const importPrompts = (path: string): Promise<ImportReport> =>
  invoke("import_prompts", { path });
