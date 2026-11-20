/**
 * The virtual filesystem inside the WASM module wired into "wasm": the
 * files quarto's pipeline reads. The project wired into "project" mirrors
 * every project file into it as they sync; the file wired into "file" (the
 * editor's text, under its name) is written on every change. Lists what
 * it holds and shows any file's contents. Offered on the "vfs" port to the
 * LSP and the render, which read from it.
 */

import { useEffect, useMemo, useState } from 'react';
import { Handle, NodeResizer, Position } from '@xyflow/react';
import { vfsAddFile, vfsListFiles, vfsReadFile } from '@quarto/preview-runtime';
import { useInput, useOutput, type FilePort, type ProjectPort, type VfsPort, type WasmPort } from './ports';

const POLL_MS = 1000;

export default function VfsNode() {
  const wasm = useInput<WasmPort>('wasm');
  const project = useInput<ProjectPort>('project');
  const file = useInput<FilePort>('file');
  const ready = wasm !== null && wasm.ready;
  const [files, setFiles] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState<string | null>(null);

  // The wired-in file, written on every change.
  useEffect(() => {
    if (file === null || !ready) return;
    vfsAddFile(file.path, file.text);
  }, [file, ready]);

  // Follow the filesystem: the list, and the selected file's contents.
  useEffect(() => {
    if (!ready) return;
    const poll = () => {
      const listed = vfsListFiles();
      const next = (listed.success ? (listed.files ?? []) : []).slice().sort();
      setFiles((prev) => (prev.length === next.length && prev.every((f, i) => f === next[i]) ? prev : next));
    };
    poll();
    const timer = window.setInterval(poll, POLL_MS);
    return () => window.clearInterval(timer);
  }, [ready]);
  useEffect(() => {
    if (!ready || selected === null) return;
    const read = () => {
      const result = vfsReadFile(selected);
      setContent(result.success ? (result.content ?? '') : null);
    };
    read();
    const timer = window.setInterval(read, POLL_MS);
    return () => window.clearInterval(timer);
  }, [ready, selected, file]);

  const port = useMemo<VfsPort | undefined>(() => (ready ? { files } : undefined), [ready, files]);
  useOutput('vfs', port);

  const shown = selected !== null && files.includes(selected) ? selected : null;

  return (
    <div className="cd-node cd-vfs">
      <NodeResizer minWidth={260} minHeight={180} />
      <div className="cd-title">VFS</div>
      <div className="cd-vfs-body nodrag nowheel">
        {!ready
          ? <div className="cd-empty">No WASM connected</div>
          : files.length === 0
            ? <div className="cd-empty">empty{project === null ? ' — no project connected to fill it' : ''}</div>
            : (
              <>
                <ul className="cd-vfs-files">
                  {files.map((f) => (
                    <li key={f} className={f === shown ? 'selected' : ''} onClick={() => setSelected(f === shown ? null : f)}>{f}</li>
                  ))}
                </ul>
                {shown !== null && <pre className="cd-vfs-content">{content ?? '(binary or unreadable)'}</pre>}
              </>
            )}
      </div>
      <div className="cd-meta">
        {ready ? `${files.length} file${files.length === 1 ? '' : 's'}` : 'inside the WASM module'}
        {file !== null && ready ? ` · ← ${file.path}` : ''}
      </div>
      <Handle type="target" position={Position.Left} id="wasm" title="wasm" />
      <Handle type="target" position={Position.Top} id="project" title="project" style={{ left: '35%' }} />
      <Handle type="target" position={Position.Top} id="file" title="file" style={{ left: '65%' }} />
      <Handle type="source" position={Position.Right} id="vfs" title="vfs" />
    </div>
  );
}
