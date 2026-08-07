/**
 * Phase D.2 (bd-kw93.13): pick the page to show when the SPA first
 * mounts.
 *
 * The CLI carries the user's intended page (e.g. `q2 preview
 * posts/intro.qmd`) into the boot URL as `?page=<rel-path>`. This
 * helper:
 *
 *   1. Reads the `page` query parameter.
 *   2. Validates that the requested path is in `files`.
 *   3. Returns it on hit.
 *   4. Falls through to the first `.qmd` file in the index on miss /
 *      missing query / unrecognised path.
 *
 * Keeping the helper pure (string + files in, string|null out) makes
 * it unit-testable without spinning up a samod / WebSocket harness.
 */
export interface FileLike {
  path: string;
}

export function pickInitialPage(
  search: string,
  files: readonly FileLike[],
): string | null {
  const requested = parsePageQuery(search);
  if (requested && files.some((f) => f.path === requested)) {
    return requested;
  }
  // Fall-through: first .qmd in the discovered index, as the SPA
  // has done since Phase A. This is also the path taken when the
  // CLI didn't pass a `?page=` hint at all. `.md` is only a
  // last-resort fallback (bd-6d2wj4zp Phase 5): with extension-based
  // sync the index may contain never-rendered `.md` (a README), while
  // a `.qmd` is always deliberate content.
  const firstQmd = files.find((f) => f.path.endsWith('.qmd'));
  if (firstQmd) return firstQmd.path;
  const firstMd = files.find((f) => f.path.endsWith('.md'));
  return firstMd ? firstMd.path : null;
}

function parsePageQuery(search: string): string | null {
  // `URLSearchParams` accepts `?...` directly so we don't have to
  // strip the leading `?` ourselves.
  if (!search) return null;
  let params: URLSearchParams;
  try {
    params = new URLSearchParams(search);
  } catch {
    return null;
  }
  const raw = params.get('page');
  if (!raw) return null;
  // Reject empty strings (could happen with `?page=`) and reject any
  // attempt to escape the project via `..` segments. The CLI never
  // emits paths with `..`; defensive checks here keep a hand-crafted
  // URL from confusing the activeFile lookup.
  const trimmed = raw.trim();
  if (!trimmed) return null;
  if (trimmed.split('/').some((seg) => seg === '..' || seg === '.')) {
    return null;
  }
  return trimmed;
}
