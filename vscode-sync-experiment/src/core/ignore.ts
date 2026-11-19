/**
 * The ignore rule for a synced project folder (G23, extending D35).
 *
 * Never synced in either direction: device-local state, render output, and
 * tooling. A nested `.git` or `.quarto-hub` at any depth is ignored too, and
 * so is a subfolder that is bound to a project of its own (it has its own
 * `.quarto-hub/project.json`), so a nested mirror folder is never swallowed by
 * a parent that is itself synced.
 *
 * Pure: the caller passes the set of known bound subfolders.
 */

export const META_DIR = '.quarto-hub';

const IGNORED_TOP_LEVEL = new Set([META_DIR, '.vscode', '.git', '.quarto', '_site', '_book', '_freeze', 'node_modules', '.DS_Store']);
/** Ignored as a path segment at any depth. */
const IGNORED_ANYWHERE = new Set(['.git', META_DIR, '.DS_Store']);

/** Normalize a project-relative path to `/` separators. */
export function toSlashPath(rel: string): string {
  return rel.replace(/\\/g, '/');
}

/**
 * True if the project-relative path `rel` is never synced.
 * `boundSubfolders` holds the project-relative paths (slash form) of
 * subfolders that carry their own `.quarto-hub/project.json`.
 */
export function isIgnoredPath(rel: string, boundSubfolders: ReadonlySet<string> = new Set()): boolean {
  const norm = toSlashPath(rel);
  if (norm === '') return true;
  const segments = norm.split('/');
  const top = segments[0] ?? '';
  if (IGNORED_TOP_LEVEL.has(top) || top.startsWith(META_DIR)) return true;
  if (segments.some((seg) => IGNORED_ANYWHERE.has(seg))) return true;
  for (let i = 1; i <= segments.length; i++) {
    if (boundSubfolders.has(segments.slice(0, i).join('/'))) return true;
  }
  return false;
}
