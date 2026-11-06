/**
 * The hub project, as hub-client holds it: the sync client's repo, which
 * follows the index doc whose id is wired into "projectid" (the
 * localStorage node remembers it) and holds a handle for every file in
 * it. It persists through the IndexedDB adapter wired into "idb" (memory
 * only without one) and mirrors every file into the WASM VFS. The network
 * adapter wired into "adapter" is attached to the repo while the edge is
 * there and detached when it goes — the repo and its documents stay, just
 * offline, the way the app behaves when the server goes away. Offered on
 * the "project" port; documents are handles drawn from it.
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { getRepo } from '@quarto/preview-runtime';
import type { NetworkAdapter } from '@automerge/automerge-repo';
import AutomergeLogo from './AutomergeLogo';
import { getConnectedAdapter, useHubProject } from '../hubProject';
import { useInput, useOutput, type AdapterPort, type IdStore, type ProjectPort, type StorageAdapterPort } from './ports';

export default function HubProjectNode() {
  const adapter = useInput<AdapterPort>('adapter');
  const idb = useInput<StorageAdapterPort>('idb');
  const projectId = useInput<IdStore>('projectid');
  const indexDocId = projectId === null || projectId.id === null
    ? null
    : `automerge:${projectId.id.replace(/^automerge:/, '')}`;
  // The first connection needs an adapter (the index doc has to arrive
  // over something) and a project id. Afterwards the adapter edge only
  // attaches and detaches the network; the connection and its storage are
  // the constant part, so neither is in the connection key.
  const [lastUrl, setLastUrl] = useState<string | null>(null);
  if (adapter !== null && adapter.url !== lastUrl) setLastUrl(adapter.url);
  const url = indexDocId !== null ? (adapter?.url ?? lastUrl) : null;
  const key = `${url ?? ''}\n${idb?.dbName ?? 'memory'}`;
  const { files, error } = useHubProject(null, url, adapter?.take, key, idb?.adapter, indexDocId ?? undefined);
  const connected = files.length > 0;

  // Network: whatever adapter is wired in is on the repo; nothing is when
  // the edge is gone. The adapter `connect` was made with is used once;
  // after a detach, a fresh instance is taken (the old one's disconnect
  // is terminal).
  const consumedRef = useRef<NetworkAdapter | null>(null);
  useEffect(() => {
    if (!connected || adapter === null) return;
    const repo = getRepo();
    if (repo === null) return;
    const viaConnect = getConnectedAdapter();
    const instance = viaConnect !== null && consumedRef.current !== viaConnect ? viaConnect : adapter.take();
    if (viaConnect !== null) consumedRef.current = viaConnect;
    if (instance !== viaConnect) repo.networkSubsystem.addNetworkAdapter(instance);
    return () => {
      repo.networkSubsystem.removeNetworkAdapter(instance);
      instance.disconnect();
    };
  }, [adapter, connected]);
  const port = useMemo<ProjectPort | undefined>(
    () => (url === null || indexDocId === null || files.length === 0 ? undefined : { url, indexDocId, files }),
    [url, indexDocId, files],
  );
  useOutput('project', port);

  const status = projectId === null
    ? 'No project id connected'
    : indexDocId === null
      ? 'No project id remembered — enter one in the localStorage node'
      : url === null
        ? 'No adapter connected to fetch the project over'
        : error ?? (!connected ? 'connecting…' : `${files.length} files · ${adapter === null ? 'offline' : 'syncing'}`);

  return (
    <div className="cd-node cd-hubproject">
      <div className="cd-title cd-title-logo"><AutomergeLogo size={28} /> Project</div>
      <div className="cd-id">{indexDocId === null ? '—' : indexDocId.replace(/^automerge:/, '')}</div>
      <div className="cd-meta">{status}</div>
      <div className="cd-meta">sync client repo · {idb === null ? 'memory storage (no IndexedDB connected)' : `IndexedDB storage (${idb.dbName})`}</div>
      <Handle type="target" position={Position.Top} id="adapter" title="adapter" />
      <Handle type="target" position={Position.Left} id="projectid" title="project id" />
      <Handle type="target" position={Position.Bottom} id="idb" title="idb" />
      <Handle type="source" position={Position.Right} id="project" title="project" />
    </div>
  );
}
