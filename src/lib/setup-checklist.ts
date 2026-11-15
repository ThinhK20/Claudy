/** The first-run checklist on the Prompts page. */
export const SETUP_ITEM_IDS = ["speech-model", "mic-test", "provider", "shortcut"] as const;

export type SetupItemId = (typeof SETUP_ITEM_IDS)[number];

/** What the frontend can actually observe. `null` = not known yet. */
export interface SetupFacts {
  hasSpeechModel: boolean;
  hasProviderKey: boolean | null;
}

/** Items whose state lives in settings tick themselves; the rest can only be
 *  marked by hand, because nothing records them. */
const AUTO_ITEMS: ReadonlySet<SetupItemId> = new Set(["speech-model", "provider"]);

export function isAutoItem(id: SetupItemId): boolean {
  return AUTO_ITEMS.has(id);
}

export function isSetupItemDone(
  id: SetupItemId,
  facts: SetupFacts,
  markedDone: ReadonlySet<string>,
): boolean {
  switch (id) {
    case "speech-model":
      return facts.hasSpeechModel;
    case "provider":
      return facts.hasProviderKey === true;
    default:
      return markedDone.has(id);
  }
}

export function isSetupComplete(facts: SetupFacts, markedDone: ReadonlySet<string>): boolean {
  return SETUP_ITEM_IDS.every((id) => isSetupItemDone(id, facts, markedDone));
}

const MARKED_KEY = "claudy.setup.marked";
const DISMISSED_KEY = "claudy.setup.dismissed";

/** Per-machine UI memory only; storage can be unavailable, so every access
 *  is guarded and a failure just means the checklist forgets. */
export function readMarkedDone(): SetupItemId[] {
  try {
    return parseStoredIds(localStorage.getItem(MARKED_KEY));
  } catch {
    return [];
  }
}

export function writeMarkedDone(ids: readonly SetupItemId[]): void {
  try {
    localStorage.setItem(MARKED_KEY, JSON.stringify(ids));
  } catch {
    // Not remembered; the checklist still works for this session.
  }
}

/** Called where the frontend sees the step happen, e.g. a working mic test. */
export function markSetupItemDone(id: SetupItemId): void {
  const current = readMarkedDone();
  if (!current.includes(id)) writeMarkedDone([...current, id]);
}

export function readDismissed(): boolean {
  try {
    return localStorage.getItem(DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

export function writeDismissed(): void {
  try {
    localStorage.setItem(DISMISSED_KEY, "1");
  } catch {
    // Not remembered; it hides for this session only.
  }
}

/** Reads the hand-marked ids back from storage, ignoring anything unexpected. */
export function parseStoredIds(raw: string | null): SetupItemId[] {
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((v): v is SetupItemId =>
      (SETUP_ITEM_IDS as readonly unknown[]).includes(v),
    );
  } catch {
    return [];
  }
}
