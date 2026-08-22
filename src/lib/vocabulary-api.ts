import { invoke } from "@tauri-apps/api/core";

/** One "when you hear `from`, write `to`" correction.
 *  Mirrors `VocabRule` in `src-tauri/src/vocabulary.rs`. */
export interface VocabRule {
  id: string;
  /** What whisper tends to produce. Matched whole-word. */
  from: string;
  /** What to write instead. Empty deletes the match (filler words). */
  to: string;
  enabled: boolean;
  /** Exact-case match only, and `to` written verbatim — for rules like "it" -> "IT". */
  matchCase: boolean;
}

/** Mirrors `vocabulary::ImportReport`. */
export interface VocabImportReport {
  added: number;
  updated: number;
  /** Dropped because the list was already at the limit. */
  skipped: number;
}

/** Runs draft rules through the real Rust matcher so the "Try it" box can
 *  never disagree with what dictation will actually do. Deliberately not
 *  reimplemented in TypeScript — one matcher, no drift. */
export const previewVocabulary = (text: string, rules: VocabRule[]): Promise<string> =>
  invoke("preview_vocabulary", { text, rules });

export const importVocabulary = (path: string): Promise<VocabImportReport> =>
  invoke("import_vocabulary", { path });

/** Resolves to the number of rules written. */
export const exportVocabulary = (path: string): Promise<number> =>
  invoke("export_vocabulary", { path });
