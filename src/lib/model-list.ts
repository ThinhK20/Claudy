/**
 * The comma-separated model list behind the Providers page's "Fallback
 * models" field. Pure string work, kept out of `ai-api` so it stays testable
 * without a Tauri runtime.
 */

/**
 * Read what the user typed. Blanks are dropped, so a trailing comma or a
 * stray space can never become a request for a model named "".
 */
export function parseModelList(raw: string): string[] {
  return raw
    .split(",")
    .map((model) => model.trim())
    .filter((model) => model.length > 0);
}

/** The stored list as one editable line. */
export function formatModelList(models: string[]): string {
  return models.join(", ");
}
