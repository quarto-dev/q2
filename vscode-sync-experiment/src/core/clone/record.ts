/**
 * The persisted git-epochs record (GE-10, G10): `.quarto-hub/git-epochs.json`. It holds what the startup rule needs
 * to tell a changed HEAD from an unchanged one: the HEAD last decided on, the blob id of every participating file at
 * that HEAD, and the paths left detached. Written atomically (temp file and rename), retrying a rename that Windows
 * refuses with EPERM while a watcher or scanner holds the target.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { StartupRecord } from '@quarto/quarto-git-epochs';
import { META_DIR } from '../ignore';

export const RECORD_FILE = 'git-epochs.json';
const VERSION = 1;

export type EpochRecord = StartupRecord & { version: number };

const file = (root: string) => path.join(root, META_DIR, RECORD_FILE);

/** The record, or null when it is missing or unreadable (the startup rule then treats this as a first run). */
export async function readRecord(root: string): Promise<EpochRecord | null> {
  try {
    const r = JSON.parse(await fs.readFile(file(root), 'utf8')) as EpochRecord;
    if (r.version !== VERSION || typeof r.head !== 'string' || typeof r.blobs !== 'object' || r.blobs === null) return null;
    return { version: VERSION, head: r.head, blobs: r.blobs, detached: Array.isArray(r.detached) ? r.detached : [] };
  } catch {
    return null;
  }
}

export async function writeRecord(root: string, rec: StartupRecord): Promise<void> {
  const target = file(root);
  await fs.mkdir(path.dirname(target), { recursive: true });
  const tmp = `${target}.${process.pid}.tmp`;
  const body: EpochRecord = { version: VERSION, head: rec.head, blobs: rec.blobs, detached: [...(rec.detached ?? [])].sort() };
  await fs.writeFile(tmp, JSON.stringify(body, null, 2) + '\n');
  for (let attempt = 0; ; attempt++) {
    try {
      await fs.rename(tmp, target);
      return;
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code;
      if ((code === 'EPERM' || code === 'EBUSY') && attempt < 5) {
        await new Promise((r) => setTimeout(r, 20 * (attempt + 1)));
        continue;
      }
      await fs.rm(tmp, { force: true }).catch(() => {});
      throw err;
    }
  }
}

export async function removeRecord(root: string): Promise<void> {
  await fs.rm(file(root), { force: true });
}
