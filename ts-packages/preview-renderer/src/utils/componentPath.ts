/**
 * Resolve a `render-components` path entry to a project-root-relative
 * key suitable for `fileContents` lookup.
 *
 * Convention:
 * - A leading `/` means project-root-absolute. The slash is stripped.
 * - Otherwise the path is relative to the directory of `currentFilePath`.
 *
 * `currentFilePath` is itself project-root-relative without a leading slash
 * (matches `FileEntry.path`).
 */
export function resolveComponentPath(
  path: string,
  currentFilePath: string
): string {
  if (path.startsWith('/')) {
    return normalize(path.slice(1));
  }
  const lastSlash = currentFilePath.lastIndexOf('/');
  const dir = lastSlash >= 0 ? currentFilePath.slice(0, lastSlash + 1) : '';
  return normalize(dir + path);
}

function normalize(joined: string): string {
  const parts = joined.split('/').filter((p) => p && p !== '.');
  const out: string[] = [];
  for (const p of parts) {
    if (p === '..') out.pop();
    else out.push(p);
  }
  return out.join('/');
}
