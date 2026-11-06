/**
 * The hub-client file tree over the hub project wired into "project".
 * Picking a file offers its doc id on the "docid" port, so an Automerge
 * node wired to it opens that file's document; an Automerge node that
 * already has an id selects the matching file here (through the port's
 * `set`). Until something is picked, the project's `a.qmd` (else its
 * first file) is the selection.
 */

import { useMemo, useState } from 'react';
import { Handle, NodeResizer, Position } from '@xyflow/react';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import FileTree from '../../FileTree';
import { FILE_PATH } from '../hubProject';
import { useInput, useOutput, type IdStore, type ProjectPort } from './ports';

const bareId = (id: string) => id.replace(/^automerge:/, '');
const noop = () => {};

export default function FileExplorerNode() {
  const project = useInput<ProjectPort>('project');
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const files = project?.files ?? [];
  const picked = selectedId === null ? null : (files.find((f) => bareId(f.docId) === selectedId) ?? null);
  const selected = picked ?? files.find((f) => f.path === FILE_PATH) ?? files[0] ?? null;

  const store = useMemo<IdStore | undefined>(() => {
    if (project === null) return undefined;
    return {
      id: selected === null ? null : bareId(selected.docId),
      set: (id) => setSelectedId(bareId(id)),
    };
  }, [project, selected]);
  useOutput('docid', store);

  const onSelect = (file: FileEntry) => setSelectedId(bareId(file.docId));

  return (
    <div className="cd-node cd-explorer">
      <NodeResizer minWidth={220} minHeight={160} />
      <div className="cd-title">File explorer</div>
      <div className="cd-tree nodrag nowheel">
        {project === null
          ? <div className="cd-empty">No hub project connected</div>
          : <FileTree files={files} currentFile={selected} onSelectFile={onSelect} onUploadFiles={noop} />}
      </div>
      <div className="cd-meta">{selected === null ? 'pick a file' : `${selected.path} · ${bareId(selected.docId)}`}</div>
      <Handle type="target" position={Position.Left} id="project" title="project" />
      <Handle type="source" position={Position.Bottom} id="docid" title="doc id" />
    </div>
  );
}
