// Pure request-path normalization. Mirrors `normalize_request_path_str` in
// crates/quarto-core/src/pandoc_request/path.rs: `/` separators, no `.`/`..`/empty
// components, verbatim (`\\?\`) and UNC prefixes understood, upper-case drive letter.
// String-based so the Windows cases are testable on every OS.

export function normalizeRequestPath(raw: string): string {
  let s = raw.replaceAll('\\', '/');
  if (s.startsWith('//?/UNC/')) s = '//' + s.slice('//?/UNC/'.length);
  else if (s.startsWith('//?/')) s = s.slice('//?/'.length);

  let prefix = '';
  let rest = s;
  const drive = /^([A-Za-z]):(?=\/|$)/.exec(s);
  if (drive) {
    prefix = drive[1].toUpperCase() + ':';
    rest = s.slice(2);
  } else if (s.startsWith('//')) {
    const [server = '', share = '', ...tail] = s.slice(2).split('/');
    if (server === '') {
      rest = s.replace(/^\/+/, '');
    } else {
      prefix = `//${server}/${share}`;
      rest = tail.join('/');
    }
  }

  const rooted = prefix !== '' || s.startsWith('/');
  const parts: string[] = [];
  for (const comp of rest.split('/')) {
    if (comp === '' || comp === '.') continue;
    if (comp === '..') {
      if (parts.length > 0 && parts[parts.length - 1] !== '..') parts.pop();
      else if (!rooted) parts.push('..');
    } else parts.push(comp);
  }
  const body = parts.join('/');
  return rooted ? `${prefix}/${body}` : body;
}

/** A request path: absolute (`/...` or `X:/...`), no backslash, already normalized. */
export function isNormalizedAbsolute(p: string): boolean {
  return /^(\/|[A-Za-z]:\/)/.test(p) && !p.includes('\\') && normalizeRequestPath(p) === p;
}

/** `path` equals `root` or lies under it, component-wise (`/tmpfoo` is not under `/tmp`). */
export function isUnder(path: string, root: string): boolean {
  const r = root.endsWith('/') ? root.slice(0, -1) : root;
  return path === r || path.startsWith(r + '/');
}

/** Split an absolute path into components (drive prefixes are not mountable). */
export function components(p: string): string[] {
  return p.split('/').filter((c) => c !== '');
}
