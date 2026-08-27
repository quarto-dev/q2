/**
 * Pure decision logic for pasting images from the clipboard into the
 * source editor (bd-706b0ixu; design:
 * claude-notes/plans/2026-08-27-paste-image-clipboard.md).
 *
 * The paste path is silent — no dialog — so it is deliberately narrower
 * than the drag-and-drop/asset-dialog flows:
 *
 * - Raster images only. `image/svg+xml` is excluded because SVG can
 *   carry executable script; SVG ingestion stays available through the
 *   deliberate, visible dialog flows (§3c option S1; pipeline-wide SVG
 *   posture is bd-myoj9kp5).
 * - Payloads carrying meaningful `text/plain` pass through to Monaco's
 *   text paste. Excel/Sheets cell copies ship an `image/png` rendition
 *   of the cells *alongside* the TSV text — the user wants the text.
 *   The one text form that does NOT block take-over is the "filename
 *   rider" an OS file-copy adds (the pasted file's own name), which
 *   Monaco would otherwise insert as stray text. Offering the image
 *   rendition of mixed payloads is follow-up bd-yspyic32.
 * - Zero-size files pass through (degrade to Monaco's behavior, §D5).
 */

/** Raster MIME types accepted on the silent paste path. */
export const ACCEPTED_PASTE_IMAGE_TYPES: ReadonlySet<string> = new Set([
  'image/png',
  'image/jpeg',
  'image/gif',
  'image/webp',
  'image/avif',
]);

/** File extension (without dot) for each accepted MIME type. */
const EXTENSION_BY_MIME: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/avif': 'avif',
};

/** The subset of `File` the classifier needs (DOM-free for testing). */
export interface PastePayloadFile {
  name: string;
  type: string;
  size: number;
}

export interface PastePayload {
  /** `clipboardData.files`, reduced to name/type/size. */
  files: PastePayloadFile[];
  /** The clipboard's `text/plain` rendition (`''` when absent). */
  text: string;
}

export type PasteClassification = 'take-over' | 'pass-through';

/**
 * Decide whether the paste handler takes over a clipboard payload or
 * lets Monaco's own text-paste handling run.
 */
export function classifyPastePayload(payload: PastePayload): PasteClassification {
  const { files, text } = payload;
  if (files.length === 0) return 'pass-through';

  const allAcceptedImages = files.every(
    (f) => f.size > 0 && ACCEPTED_PASTE_IMAGE_TYPES.has(f.type)
  );
  if (!allAcceptedImages) return 'pass-through';

  if (!isFilenameRider(text, files)) return 'pass-through';

  return 'take-over';
}

/**
 * True when `text` carries no information beyond the pasted files
 * themselves: empty, a single pasted file's name, or the newline-joined
 * names. Anything else (Office/Excel text renditions, full paths) is
 * meaningful text and blocks take-over — the conservative direction,
 * since pass-through is today's behavior.
 */
function isFilenameRider(text: string, files: PastePayloadFile[]): boolean {
  const trimmed = text.trim();
  if (trimmed === '') return true;
  if (files.some((f) => f.name === trimmed)) return true;
  return trimmed === files.map((f) => f.name).join('\n');
}

/**
 * Auto-generated filename for a pasted image: `pasted-<hash8>.<ext>`.
 *
 * Content-hash naming is what makes the no-dialog flow safe under
 * concurrent CRDT peers: `createBinaryFile`'s existence check is
 * check-then-act against the local replica, so two peers claiming the
 * same index key with different content would race to last-writer-wins
 * and silently lose an image. Different content → different hash →
 * different key: no race. Identical concurrent content converges to
 * identical bytes whichever write wins.
 *
 * Returns null for a MIME type outside the accepted set.
 */
export function pastedImageFilename(hash: string, mimeType: string): string | null {
  const ext = EXTENSION_BY_MIME[mimeType];
  if (!ext) return null;
  return `pasted-${hash.slice(0, 8)}.${ext}`;
}

/**
 * Make selected text safe to use as markdown image alt text: collapse
 * whitespace runs (newlines would break the reference in
 * indentation-sensitive contexts) and escape square brackets.
 */
export function sanitizeAltText(text: string): string {
  return text
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/([[\]])/g, '\\$1');
}
