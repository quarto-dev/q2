/**
 * Rebind a bound folder to a new project session (GE-10, G19): after a confirm, stop the running sync, delete the
 * Automerge store and the record, and rewrite `project.json`. A clone first re-verifies its repository against the new
 * project and refuses, deleting nothing, on a mismatch. The first start after a rebind has no record, so the startup
 * rule runs arrival for every participating file and merges nothing from disk.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { META_DIR } from '../ignore';
import { readProjectMeta, writeProjectMeta, type ProjectMeta } from '../meta';
import { verifyIdentity } from './attach';
import { readShare } from './attach-flow';
import { removeRecord } from './record';

export type RebindResult = { ok: true; meta: ProjectMeta } | { ok: false; reason: 'not-bound' | 'declined' | 'identity-mismatch'; message: string };

export interface RebindOptions {
  root: string;
  indexDocId: string;
  /** Defaults to the folder's current server. */
  serverUrl?: string;
  /** Asks the user; the store and the record are deleted only after a yes. */
  confirm(): Promise<boolean>;
  /** Stop the running sync (and close its storage) before anything is deleted: on Windows deleting an open store fails with EBUSY. */
  shutdown?(): Promise<void>;
}

export async function rebindFolder(o: RebindOptions): Promise<RebindResult> {
  const current = await readProjectMeta(o.root);
  if (!current) return { ok: false, reason: 'not-bound', message: `${o.root} is not bound to a project.` };
  const serverUrl = o.serverUrl ?? current.syncServer;
  let repo = current.repo;
  if (current.mode === 'clone') {
    const share = await readShare(serverUrl, o.indexDocId);
    if (!share.repo) return { ok: false, reason: 'identity-mismatch', message: 'The new project was not shared from a git repository.' };
    const { createGitProbe } = await import('@quarto/quarto-git-epochs/probe');
    const probe = createGitProbe({ cwd: o.root });
    await probe.ready().catch(() => {});
    const v = await verifyIdentity(probe, share.repo, share.registry);
    if (!v.ok) return { ok: false, reason: 'identity-mismatch', message: v.message };
    repo = share.repo;
  }
  if (!(await o.confirm())) return { ok: false, reason: 'declined', message: 'Rebind cancelled.' };
  await o.shutdown?.();
  await fs.rm(path.join(o.root, META_DIR, 'automerge'), { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  await removeRecord(o.root);
  const meta: ProjectMeta = { ...current, indexDocId: o.indexDocId, syncServer: serverUrl, createdAt: new Date().toISOString(), ...(repo ? { repo } : {}) };
  await writeProjectMeta(o.root, meta);
  return { ok: true, meta };
}
