/**
 * The preview runtime's connection to a hub project: a sync client of
 * its own (a second repo beside the Automerge node's) that follows the
 * index doc whose id is wired into "projectid" (the localStorage node
 * remembers it) over the network adapter wired into "adapter",
 * persists every document through the IndexedDB adapter wired into "idb"
 * (memory only without one), mirrors every file into the WASM VFS, and is
 * what the document editor, LSP and render work against. Offered on the
 * "project" port.
 */

import { useMemo } from 'react';
import { Handle, Position } from '@xyflow/react';
import AutomergeLogo from './AutomergeLogo';
import { useHubProject } from '../hubProject';
import { useInput, useOutput, type AdapterPort, type IdStore, type ProjectPort, type StorageAdapterPort } from './ports';

export default function HubProjectNode() {
  const adapter = useInput<AdapterPort>('adapter');
  const idb = useInput<StorageAdapterPort>('idb');
  const projectId = useInput<IdStore>('projectid');
  const indexDocId = projectId === null || projectId.id === null
    ? null
    : `automerge:${projectId.id.replace(/^automerge:/, '')}`;
  // Nothing to open without an adapter and a project id.
  const url = adapter !== null && indexDocId !== null ? adapter.url : null;
  // The storage is part of the connection: switching it reconnects.
  const key = `${adapter?.id ?? ''}\n${idb?.dbName ?? 'memory'}`;
  const { files, error } = useHubProject(null, url, adapter?.take, key, idb?.adapter, indexDocId ?? undefined);
  const port = useMemo<ProjectPort | undefined>(
    () => (url === null || indexDocId === null || files.length === 0 ? undefined : { url, indexDocId, files }),
    [url, indexDocId, files],
  );
  useOutput('project', port);

  const status = adapter === null
    ? 'No adapter connected'
    : projectId === null
      ? 'No project id connected'
      : indexDocId === null
        ? 'No project id remembered — enter one in the localStorage node'
        : error ?? (files.length === 0 ? 'connecting…' : `${files.length} files`);

  return (
    <div className="cd-node cd-hubproject">
      <div className="cd-title cd-title-logo"><AutomergeLogo size={28} /> Project</div>
      <div className="cd-id">{indexDocId === null ? '—' : indexDocId.replace(/^automerge:/, '')}</div>
      <div className="cd-meta">{status}</div>
      <div className="cd-meta">repo · {idb === null ? 'memory storage (no IndexedDB connected)' : `IndexedDB storage (${idb.dbName})`}</div>
      <Handle type="target" position={Position.Top} id="adapter" title="adapter" />
      <Handle type="target" position={Position.Left} id="projectid" title="project id" />
      <Handle type="target" position={Position.Bottom} id="idb" title="idb" />
      <Handle type="source" position={Position.Right} id="project" title="project" />
    </div>
  );
}
