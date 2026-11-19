/**
 * Wait until a server holds every given doc at the heads this repo holds (GE-12, G24). The extension uses a raw `Repo`,
 * not a `SyncClient`, so it cannot call `drainOutbound`; this is the same delivery proof on the same primitives: some
 * storage-backed peer's last confirmed heads (`getSyncInfo`, updated on each sync message) equal the handle's heads.
 * Event-driven on `remote-heads` and `peer`, bounded by `ms`, never rejects.
 */
import type { DocHandle, Repo } from '@automerge/automerge-repo/slim';

type StorageId = Parameters<DocHandle<unknown>['getSyncInfo']>[0];

export interface HeadsWait {
  ok: boolean;
  /** Doc ids the server has not confirmed at the current heads. */
  pending: string[];
}

const sameHeads = (a: readonly string[], b: readonly string[]) => a.length === b.length && [...a].sort().every((h, i) => h === [...b].sort()[i]);

function storageIds(repo: Repo): Set<StorageId> {
  const ids = new Set<StorageId>();
  for (const meta of Object.values(repo.peerMetadataByPeerId ?? {})) if (meta?.storageId) ids.add(meta.storageId);
  return ids;
}

function delivered(repo: Repo, h: DocHandle<unknown>): boolean {
  if (!h.isReady()) return true; // never loaded: nothing of ours to deliver
  const heads = h.heads() as string[];
  for (const id of storageIds(repo)) {
    const info = h.getSyncInfo(id);
    if (info && sameHeads(info.lastHeads as string[], heads)) return true;
  }
  return false;
}

export async function waitForServerHeads(repo: Repo, handles: DocHandle<unknown>[], ms: number): Promise<HeadsWait> {
  const pending = () => handles.filter((h) => !delivered(repo, h)).map((h) => String(h.documentId));
  if (pending().length > 0) {
    await new Promise<void>((resolve) => {
      const cleanups: (() => void)[] = [];
      let done = false;
      const finish = () => {
        if (done) return;
        done = true;
        cleanups.forEach((c) => c());
        resolve();
      };
      const timer = setTimeout(finish, ms);
      cleanups.push(() => clearTimeout(timer));
      const recheck = () => {
        if (pending().length === 0) finish();
      };
      for (const h of handles) {
        h.on('remote-heads', recheck);
        cleanups.push(() => h.off('remote-heads', recheck));
      }
      repo.networkSubsystem.on('peer', recheck);
      cleanups.push(() => repo.networkSubsystem.off('peer', recheck));
    });
  }
  const left = pending();
  return { ok: left.length === 0, pending: left };
}
