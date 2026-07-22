// Turn pasted / dropped / picked image files into base64 attachments for the
// quick-ask box. Kept free of React and Tauri so the pure parts are testable.

export interface Attachment {
  /** Stable key for React lists and removal. */
  id: string;
  /** Full data URL, used directly as the thumbnail `<img>` src. */
  previewUrl: string;
  /** MIME type, e.g. "image/png" or "image/jpeg" after downscaling. */
  mediaType: string;
  /** Base64 payload without the `data:...;base64,` prefix. */
  data: string;
}

/** Longest edge (px) we send; larger screenshots are downscaled to this. */
export const MAX_DIMENSION = 1600;
const DOWNSCALE_MIME = "image/jpeg";
const DOWNSCALE_QUALITY = 0.85;

/** Split a `data:<mime>;base64,<data>` URL into its MIME type and raw base64. */
export function splitDataUrl(dataUrl: string): { mediaType: string; data: string } {
  const match = /^data:([^;,]+);base64,(.+)$/.exec(dataUrl);
  if (!match) {
    throw new Error("Unsupported image data (expected a base64 data URL)");
  }
  return { mediaType: match[1], data: match[2] };
}

function readAsDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error ?? new Error("Failed to read image"));
    reader.readAsDataURL(file);
  });
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("Failed to decode image"));
    img.src = src;
  });
}

/**
 * Downscale an image whose longest edge exceeds {@link MAX_DIMENSION}, re-encoding
 * as JPEG to keep the base64 payload well under provider size limits (hi-DPI
 * screenshots are multi-megabyte). Returns a data URL. On any decode/redraw
 * failure it returns the input unchanged so attachment never hard-fails here.
 */
export async function downscaleImage(dataUrl: string): Promise<string> {
  let img: HTMLImageElement;
  try {
    img = await loadImage(dataUrl);
  } catch {
    return dataUrl;
  }
  const { width, height } = img;
  if (width <= MAX_DIMENSION && height <= MAX_DIMENSION) {
    return dataUrl;
  }
  const scale = MAX_DIMENSION / Math.max(width, height);
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return dataUrl;
  }
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL(DOWNSCALE_MIME, DOWNSCALE_QUALITY);
}

let counter = 0;
function nextId(): string {
  counter += 1;
  return `att-${Date.now()}-${counter}`;
}

/** Convert one image `File`/`Blob` (paste, drop, or picker) into an Attachment. */
export async function fileToAttachment(file: File | Blob): Promise<Attachment> {
  const raw = await readAsDataUrl(file);
  const previewUrl = await downscaleImage(raw);
  const { mediaType, data } = splitDataUrl(previewUrl);
  return { id: nextId(), previewUrl, mediaType, data };
}

/** Keep only image files from a FileList / array (drop and picker inputs). */
export function imageFilesOnly(files: ArrayLike<File>): File[] {
  return Array.from(files).filter((f) => f.type.startsWith("image/"));
}
