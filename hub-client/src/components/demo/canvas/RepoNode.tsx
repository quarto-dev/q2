/**
 * An Automerge repo, as hub-client has exactly one of: the object that
 * holds documents (as handles) and owns the adapters. The network adapter
 * wired into "adapter" is attached while the edge is there and detached
 * when it goes; the repo and its documents stay, just offline. The storage
 * adapter wired into "idb" is where the repo persists; a repo's storage is
 * fixed when it is built, so this one is built with a stand-in that
 * forwards to whatever is attached (and keeps nothing when nothing is) —
 * on attach, what the store already holds is merged in and the documents
 * are written to it. Offered on the "repo" port: the Project follows its
 * index document in it, and a Document is a handle in it.
 */

import { useEffect, useMemo, useState } from 'react';
import { Handle, Position, useNodeId } from '@xyflow/react';
import { next as A } from '@automerge/automerge';
import { Repo, type NetworkAdapter, type StorageAdapterInterface, type StorageKey } from '@automerge/automerge-repo';
import AutomergeLogo from './AutomergeLogo';
import { useInput, useOutput, type AdapterPort, type RepoPort, type StorageAdapterPort } from './ports';

const POLL_MS = 1000;

/** The repo's storage: whatever adapter is attached right now, else nothing. */
class SwitchableStorage implements StorageAdapterInterface {
  attached: StorageAdapterInterface | null = null;
  load(key: StorageKey) { return this.attached?.load(key) ?? Promise.resolve(undefined); }
  save(key: StorageKey, data: Uint8Array) { return this.attached?.save(key, data) ?? Promise.resolve(); }
  remove(key: StorageKey) { return this.attached?.remove(key) ?? Promise.resolve(); }
  loadRange(prefix: StorageKey) { return this.attached?.loadRange(prefix) ?? Promise.resolve([]); }
  removeRange(prefix: StorageKey) { return this.attached?.removeRange(prefix) ?? Promise.resolve(); }
}

// One repo per node for the page's lifetime (survives React's dev double-mount).
const repos = new Map<string, { repo: Repo; storage: SwitchableStorage }>();

function getRepo(nodeId: string): { repo: Repo; storage: SwitchableStorage } {
  let entry = repos.get(nodeId);
  if (entry === undefined) {
    const storage = new SwitchableStorage();
    entry = { repo: new Repo({ network: [], storage }), storage };
    repos.set(nodeId, entry);
  }
  return entry;
}

export default function RepoNode() {
  const nodeId = useNodeId() ?? 'repo';
  const { repo } = getRepo(nodeId);
  const adapter = useInput<AdapterPort>('adapter');
  const idb = useInput<StorageAdapterPort>('idb');
  const [peers, setPeers] = useState(0);
  const [handles, setHandles] = useState(0);

  // Network: attach the wired-in adapter; detach it when the edge goes.
  useEffect(() => {
    if (adapter === null) return;
    const network: NetworkAdapter = adapter.take();
    repo.networkSubsystem.addNetworkAdapter(network);
    return () => {
      repo.networkSubsystem.removeNetworkAdapter(network);
      network.disconnect();
    };
  }, [adapter, repo]);

  // Storage: attach the wired-in adapter, merge in what it holds for the
  // documents the repo has, and write them to it; detach when the edge goes.
  useEffect(() => {
    if (idb === null) return;
    const { storage } = getRepo(nodeId);
    storage.attached = idb.adapter;
    let cancelled = false;
    void (async () => {
      for (const handle of Object.values(repo.handles)) {
        const chunks = await idb.adapter.loadRange([handle.documentId]);
        if (cancelled) return;
        for (const chunk of chunks) {
          if (chunk.data !== undefined) handle.update((d) => A.loadIncremental(d, chunk.data as Uint8Array));
        }
        const latest = handle.doc();
        if (latest !== undefined) await idb.adapter.save([handle.documentId, 'snapshot', `attach-${Date.now()}`], A.save(latest));
      }
    })();
    return () => {
      cancelled = true;
      if (storage.attached === idb.adapter) storage.attached = null;
    };
  }, [idb, repo, nodeId]);

  // What the repo holds and sees, for the footer.
  useEffect(() => {
    const poll = () => {
      setPeers(repo.peers.length);
      setHandles(Object.keys(repo.handles).length);
    };
    poll();
    const timer = window.setInterval(poll, POLL_MS);
    return () => window.clearInterval(timer);
  }, [repo]);

  const port = useMemo<RepoPort>(
    () => ({ repo, url: adapter?.url ?? null, networkAttached: adapter !== null, storageAttached: idb !== null }),
    [repo, adapter, idb],
  );
  useOutput('repo', port);

  return (
    <div className="cd-node cd-repo">
      <div className="cd-title cd-title-logo"><AutomergeLogo size={28} /> Repo</div>
      <div className="cd-meta">{handles} document{handles === 1 ? '' : 's'} held · {peers} peer{peers === 1 ? '' : 's'}</div>
      <div className="cd-meta">
        {idb === null ? 'no storage attached' : `storage: IndexedDB (${idb.dbName})`} · {adapter === null ? 'no network attached' : 'network attached'}
      </div>
      <Handle type="target" position={Position.Left} id="adapter" title="adapter" />
      <Handle type="target" position={Position.Left} id="idb" title="idb" />
      <Handle type="source" position={Position.Right} id="repo" title="repo" />
    </div>
  );
}
