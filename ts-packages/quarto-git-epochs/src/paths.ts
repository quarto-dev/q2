/** Path helpers that handle Windows-shaped inputs on every platform. Browser-safe: no `node:path`. */

export const toForwardSlashes = (p: string): string => p.replace(/\\/g, '/');

/**
 * The project-relative, `/`-separated path of `file` under `root`, or null when `file` is outside `root`.
 * Accepts POSIX paths, drive letters (compared case-insensitively), UNC prefixes, and mixed separators.
 */
export function relativeProjectPath(root: string, file: string): string | null {
  const split = (p: string) => toForwardSlashes(p).split('/').filter((s, i, a) => s !== '' || i === 0 || i === a.length - 1);
  const r = split(root).filter((s) => s !== '');
  const f = split(file).filter((s) => s !== '');
  const windowsShaped = /^[a-zA-Z]:/.test(toForwardSlashes(root)) || toForwardSlashes(root).startsWith('//');
  const eq = (a: string, b: string) => (windowsShaped ? a.toLowerCase() === b.toLowerCase() : a === b);
  if (f.length < r.length) return null;
  for (let i = 0; i < r.length; i++) if (!eq(r[i]!, f[i]!)) return null;
  return f.slice(r.length).join('/');
}
