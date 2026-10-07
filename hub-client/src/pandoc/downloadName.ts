/**
 * The name of a downloaded file: the document's stem plus the output's extension, made safe
 * to hand to `a.download` and to the user's file system.
 *
 * The document path is user data (a collaborator can name a file anything), so the stem
 * drops directory parts, control characters (C0, DEL, C1), bidirectional controls and
 * invisible formatting characters (which can disguise an extension: `evil‮xcod.docx`),
 * characters that are reserved on Windows, leading dots, and trailing dots and spaces.
 */

const FALLBACK_STEM = 'document';
const MAX_STEM_CODE_POINTS = 120;

// Code point ranges removed from a name: C0 and DEL, C1, Arabic letter mark, zero-width and
// bidi marks (200B-200F), bidi embeddings/overrides (202A-202E), word joiner through bidi
// isolates (2060-2069), BOM. Ranges, not a regex literal, so no control or invisible
// character appears in the source.
const UNSAFE_RANGES: readonly (readonly [number, number])[] = [
  [0x0000, 0x001f],
  [0x007f, 0x009f],
  [0x061c, 0x061c],
  [0x200b, 0x200f],
  [0x202a, 0x202e],
  [0x2060, 0x2069],
  [0xfeff, 0xfeff],
];

function stripUnsafe(s: string): string {
  let out = '';
  for (const ch of s) {
    const cp = ch.codePointAt(0)!;
    if (!UNSAFE_RANGES.some(([lo, hi]) => cp >= lo && cp <= hi)) out += ch;
  }
  return out;
}

// Reserved on Windows, plus path separators.
const RESERVED = /[<>:"/\\|?*]/g;
const WINDOWS_DEVICE = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])$/i;

function sanitizeStem(raw: string): string {
  let stem = stripUnsafe(raw).replace(RESERVED, '_');
  stem = stem.replace(/^\.+/, '').replace(/[. ]+$/, '').trim();
  if (stem === '' || WINDOWS_DEVICE.test(stem)) return FALLBACK_STEM;
  const points = Array.from(stem);
  if (points.length > MAX_STEM_CODE_POINTS) stem = points.slice(0, MAX_STEM_CODE_POINTS).join('').replace(/[. ]+$/, '');
  return stem === '' ? FALLBACK_STEM : stem;
}

/** `docPath` is the document's path (either separator); `extension` has no dot. */
export function sanitizeDownloadName(docPath: string, extension: string): string {
  const base = docPath.split(/[/\\]/).pop() ?? '';
  const stem = sanitizeStem(base.replace(/\.[^.]*$/, ''));
  const ext = extension.replace(/[^A-Za-z0-9]/g, '') || 'out';
  return `${stem}.${ext}`;
}
