/**
 * Derive a safe single path segment / filename stem from a project's
 * human-readable name (its `description`).
 *
 * Used for two things that must stay in lock-step (see GH #147):
 *   - the download filename stem of an exported project ZIP, and
 *   - the top-level folder every entry inside that ZIP is nested under.
 *
 * The result is safe to use as one path segment on all platforms: spaces,
 * path separators, Windows-reserved characters (`< > : " / \ | ? *`) and
 * C0 control characters are collapsed to hyphens, and trailing dots/spaces
 * (illegal on Windows) are removed. An empty result falls back to
 * `"project"`, matching the historical download-filename fallback.
 */

// Character codes that are unsafe as a single path segment on some platform:
// the Windows-reserved set plus both path separators. (Control chars and the
// space are handled by the `code <= 0x20` check in `isHostile`.)
const RESERVED_CODES = new Set<number>([
  0x3c, // <
  0x3e, // >
  0x3a, // :
  0x22, // "
  0x2f, // /  (forward slash)
  0x5c, // \  (backslash)
  0x7c, // |
  0x3f, // ?
  0x2a, // *
]);

function isHostile(code: number): boolean {
  // code <= 0x20 covers all C0 control chars (0x00–0x1f) and the space (0x20).
  return code <= 0x20 || RESERVED_CODES.has(code);
}

export function projectFolderName(description: string | undefined): string {
  let out = '';
  for (const ch of description || '') {
    out += isHostile(ch.charCodeAt(0)) ? '-' : ch;
  }
  const cleaned = out
    .replace(/-+/g, '-') // collapse runs of hyphens
    .replace(/^-+/, '') // trim leading hyphens (e.g. from a leading slash)
    .replace(/[-. ]+$/, ''); // trim trailing hyphen/dot/space
  return cleaned || 'project';
}
