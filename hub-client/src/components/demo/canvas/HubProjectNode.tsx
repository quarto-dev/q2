/**
 * The preview runtime's connection to the hub project: a sync client of
 * its own (a second repo beside the Automerge node's) that follows the
 * project's index doc on the server wired into "server", mirrors every
 * file into the WASM VFS, and is what the document editor, LSP and render
 * work against. Offered on the "project" port.
 */

import { useMemo } from 'react';
import { Handle, Position } from '@xyflow/react';
import { INDEX_DOC_ID, useHubProject } from '../hubProject';
import { useInput, useOutput, type ProjectPort, type ServerPort } from './ports';

export default function HubProjectNode() {
  const server = useInput<ServerPort>('server');
  const url = server?.url ?? null;
  const { files, error } = useHubProject(null, url);
  const port = useMemo<ProjectPort | undefined>(
    () => (url === null || files.length === 0 ? undefined : { url, indexDocId: INDEX_DOC_ID, files }),
    [url, files],
  );
  useOutput('project', port);

  const status = server === null
    ? 'No server connected'
    : error ?? (files.length === 0 ? 'connecting…' : `${files.length} files`);

  return (
    <div className="cd-node cd-hubproject">
      <div className="cd-title">Hub project</div>
      <div className="cd-id">{INDEX_DOC_ID.replace(/^automerge:/, '')}</div>
      <div className="cd-meta">{status}</div>
      <Handle type="target" position={Position.Top} id="server" title="server" />
      <Handle type="source" position={Position.Right} id="project" title="project" />
    </div>
  );
}
