import * as path from 'node:path';
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import { NodeFSStorageAdapter } from '@automerge/automerge-repo-storage-nodefs';
import { META_DIR } from './ignore';

/** In Node the browser adapter's isomorphic-ws import resolves to `ws`. */
export function webSocketAdapter(url: string): BrowserWebSocketClientAdapter {
  return new BrowserWebSocketClientAdapter(url);
}

/** The project's repo: storage under `.quarto-hub/automerge`, optionally online. */
export function makeRepo(root: string, syncServer: string | null): Repo {
  return new Repo({
    storage: new NodeFSStorageAdapter(path.join(root, META_DIR, 'automerge')),
    network: syncServer ? [webSocketAdapter(syncServer)] : [],
  });
}

export async function findDoc<T>(repo: Repo, docId: string): Promise<DocHandle<T>> {
  const handle = await repo.find<T>(`automerge:${docId}` as never, { signal: AbortSignal.timeout(30_000) });
  await handle.whenReady();
  return handle;
}
