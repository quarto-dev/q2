/**
 * The hub project: what hub-client's sync client does on top of a repo.
 * On the repo wired into "repo" it follows the index document whose id is
 * wired into "projectid" (the localStorage node remembers it), keeps the
 * file list, mirrors every file into the WASM VFS, and retries files that
 * are slow to arrive. It needs no network of its own: the repo brings
 * whatever is attached to it, and the client no longer waits for a peer —
 * documents come from the repo's storage, or from a peer once one is
 * there. Offered on the "project" port.
 */

import { useMemo } from 'react';
import { Handle, Position } from '@xyflow/react';
import AutomergeLogo from './AutomergeLogo';
import { useHubProject } from '../hubProject';
import { useInput, useOutput, type IdStore, type ProjectPort, type RepoPort } from './ports';

export default function HubProjectNode() {
  const repoPort = useInput<RepoPort>('repo');
  const projectId = useInput<IdStore>('projectid');
  const indexDocId = projectId === null || projectId.id === null
    ? null
    : `automerge:${projectId.id.replace(/^automerge:/, '')}`;
  // The URL only names the server for the client's logs: the repo carries
  // the network. One connection per repo and project id.
  const url = repoPort !== null && indexDocId !== null ? (repoPort.url ?? '') : null;
  const { files, error } = useHubProject(null, url, indexDocId ?? undefined, repoPort?.repo, repoPort === null ? '' : 'repo');
  const connected = files.length > 0;

  const port = useMemo<ProjectPort | undefined>(
    () => (url === null || indexDocId === null || !connected ? undefined : { url, indexDocId, files }),
    [url, indexDocId, files, connected],
  );
  useOutput('project', port);

  const status = repoPort === null
    ? 'No repo connected'
    : projectId === null
      ? 'No project id connected'
      : indexDocId === null
        ? 'No project id remembered — enter one in the localStorage node'
        : error ?? (!connected
          ? `connecting…${repoPort.networkAttached ? '' : ' (no network on the repo — the index has to be in its storage)'}`
          : `${files.length} files · ${repoPort.networkAttached ? 'syncing' : 'offline'}`);

  return (
    <div className="cd-node cd-hubproject">
      <div className="cd-title cd-title-logo"><AutomergeLogo size={28} /> Project</div>
      <div className="cd-id">{indexDocId === null ? '—' : indexDocId.replace(/^automerge:/, '')}</div>
      <div className="cd-meta">{status}</div>
      <div className="cd-meta">sync client on the repo · index doc, file list, VFS mirror</div>
      <Handle type="target" position={Position.Top} id="repo" title="repo" />
      <Handle type="target" position={Position.Left} id="projectid" title="project id" />
      <Handle type="source" position={Position.Right} id="project" title="project" />
    </div>
  );
}
