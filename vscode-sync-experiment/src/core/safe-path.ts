/**
 * One validator for every path that comes from a document (G36).
 *
 * A project index is written by other people. Its keys become paths on this
 * machine, so a hostile or merely odd key (`../x`, `.git/hooks/pre-commit`,
 * an absolute path, a Windows alternate data stream) must never reach the
 * disk. `safeJoin` is called before any write, `rm`, `mkdir`, or `rmdir` whose
 * path derives from a doc; a rejected key is skipped and logged by the caller,
 * never thrown past it.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { isIgnoredPath } from './ignore';

export class UnsafePathError extends Error {
  constructor(
    readonly rel: string,
    reason: string,
  ) {
    super(`unsafe path ${JSON.stringify(rel)}: ${reason}`);
    this.name = 'UnsafePathError';
  }
}

function isInside(root: string, target: string): boolean {
  const rel = path.relative(root, target);
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel));
}

/** The real path of the deepest ancestor of `target` that exists. */
async function realpathOfExistingAncestor(target: string): Promise<string> {
  let current = target;
  for (;;) {
    try {
      return await fs.realpath(current);
    } catch {
      const parent = path.dirname(current);
      if (parent === current) return current;
      current = parent;
    }
  }
}

/**
 * Join the doc-derived `rel` (slash form) onto `root`, or throw
 * `UnsafePathError`. The result is inside `root` after symlinks and Windows
 * junctions on existing ancestors are resolved.
 */
export async function safeJoin(root: string, rel: string): Promise<string> {
  if (rel === '' || rel.includes('\u0000')) throw new UnsafePathError(rel, 'empty or contains NUL');
  if (rel.includes('\\')) throw new UnsafePathError(rel, 'contains a backslash');
  if (rel.startsWith('/') || /^[A-Za-z]:/.test(rel)) throw new UnsafePathError(rel, 'absolute');

  const norm = path.posix.normalize(rel);
  if (norm === '.' || norm === '..' || norm.startsWith('../') || norm.startsWith('/')) {
    throw new UnsafePathError(rel, 'escapes the project root');
  }
  for (const seg of norm.split('/')) {
    if (seg.includes(':')) throw new UnsafePathError(rel, 'a segment contains ":"');
    if (/[. ]$/.test(seg)) throw new UnsafePathError(rel, 'a segment ends in a dot or a space');
  }
  // `.git`, `.quarto-hub`, `.vscode`, and the other never-synced entries, in
  // any letter case (Windows and macOS filesystems are case-insensitive).
  if (isIgnoredPath(norm) || isIgnoredPath(norm.toLowerCase())) throw new UnsafePathError(rel, 'a reserved name');

  const target = path.join(root, ...norm.split('/'));
  if (!isInside(root, target)) throw new UnsafePathError(rel, 'escapes the project root');
  const realRoot = await fs.realpath(root).catch(() => root);
  const realAncestor = await realpathOfExistingAncestor(target);
  if (!isInside(realRoot, realAncestor)) throw new UnsafePathError(rel, 'resolves outside the project root');
  return target;
}
