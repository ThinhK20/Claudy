const FALLBACK = "Something went wrong.";

/**
 * A readable sentence from whatever a rejected promise carried. Tauri commands
 * reject with plain strings; JS failures with Error objects, whose String()
 * form would show a stray "Error:" prefix to the user.
 */
export function errorMessage(e: unknown): string {
  let text: string;
  if (typeof e === "string") {
    text = e;
  } else if (e instanceof Error) {
    text = e.message;
  } else if (
    e !== null &&
    typeof e === "object" &&
    typeof (e as { message?: unknown }).message === "string"
  ) {
    text = (e as { message: string }).message;
  } else if (e === null || e === undefined) {
    text = "";
  } else {
    text = String(e);
  }
  return text.trim() ? text : FALLBACK;
}
