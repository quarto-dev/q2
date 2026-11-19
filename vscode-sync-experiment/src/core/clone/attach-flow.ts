/**
 * Attach a folder to a project as a clone peer (GE-10): read the share's repo identity and registry, run the attach
 * check, keep `.quarto-hub/` out of `git status`, and write `project.json` with `mode: clone` and the identity.
 * The interim command, the headless peer, and (in GE-11) the Join command all call this.
 */
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import { asString, readIndexFields, type Sidecar, viewOf } from '@quarto/quarto-git-epochs';
import { excludeMetaDir } from '../git-exclude';
import { writeProjectMeta, type ProjectMeta } from '../meta';
import { findDoc, webSocketAdapter } from '../repo';
import { checkAttach, type AttachResult, type AttachShare } from './attach';

/** Read what attach needs from the share: the `repo` entry and the registry's `(path, blob)` pairs. */
export async function readShare(serverUrl: string, indexDocId: string): Promise<AttachShare> {
  const repo = new Repo({ network: [webSocketAdapter(serverUrl)] });
  try {
    const index = await findDoc<IndexDocument>(repo, indexDocId);
    const fields = readIndexFields(index.doc());
    const share: AttachShare = {};
    if (fields.repo) share.repo = fields.repo;
    if (fields.epochs) {
      const sidecar: DocHandle<Sidecar> = await findDoc<Sidecar>(repo, asString(fields.epochs));
      share.registry = viewOf(sidecar.doc()!).entries.map((e) => ({ path: e.path, blob: e.blob }));
    }
    return share;
  } finally {
    await repo.shutdown().catch(() => {});
  }
}

export type AttachFolderResult = AttachResult & { meta?: ProjectMeta };

/** Check `dir` against the project and, when it passes, bind it. A refusal writes nothing. */
export async function attachFolder(opts: { dir: string; serverUrl: string; indexDocId: string }): Promise<AttachFolderResult> {
  const share = await readShare(opts.serverUrl, opts.indexDocId);
  const result = await checkAttach({ dir: opts.dir, share });
  if (!result.ok) return result;
  await excludeMetaDir(result.projectRoot).catch(() => false);
  const meta: ProjectMeta = {
    indexDocId: opts.indexDocId,
    syncServer: opts.serverUrl,
    createdAt: new Date().toISOString(),
    mode: 'clone',
    repo: result.identity,
  };
  await writeProjectMeta(result.projectRoot, meta);
  return { ...result, meta };
}
