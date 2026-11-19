/**
 * Which published files differ from HEAD (GE-12, G24). The confirm lists them: they are shared as they are on disk, and
 * each is registered under its HEAD blob. Text compares after CRLF normalization (a Windows checkout of an LF blob is
 * not a change); a binary file compares by bytes. A plain folder has no HEAD, so nothing is dirty.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { contentKey, createGitProbe } from '@quarto/quarto-git-epochs/probe';
import type { Selection } from './select';

export async function dirtyFiles(dir: string, selection: Selection, opts: { gitPath?: string } = {}): Promise<string[]> {
  if (selection.kind !== 'git') return [];
  const probe = createGitProbe({ cwd: dir, ...(opts.gitPath ? { gitPath: opts.gitPath } : {}) });
  const withBlob = selection.files.filter((f) => f.blob);
  const blobs = await probe.catBlobs([...new Set(withBlob.map((f) => f.blob!))]);
  const dirty: string[] = [];
  for (const f of withBlob) {
    const head = blobs.get(f.blob!);
    if (!head) continue;
    const disk = await fs.readFile(path.join(dir, f.path));
    const same = f.binary ? Buffer.from(head).equals(disk) : contentKey(head) === contentKey(disk);
    if (!same) dirty.push(f.path);
  }
  return dirty.sort();
}
