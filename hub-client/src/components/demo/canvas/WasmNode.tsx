/**
 * The quarto WASM module and its virtual filesystem. The hub project
 * wired into "project" fills the VFS with the project's files as they
 * sync; the LSP and the render read them from there. Offered on the
 * "wasm" port once the module is initialised.
 */

import { useEffect, useMemo, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { initWasm, isWasmReady, vfsListFiles } from '@quarto/preview-runtime';
import { useInput, useOutput, type ProjectPort, type WasmPort } from './ports';

const POLL_MS = 1000;

export default function WasmNode() {
  const project = useInput<ProjectPort>('project');
  const [ready, setReady] = useState(isWasmReady());
  const [files, setFiles] = useState<string[]>([]);

  // Initialise once a project is wired in, then follow the VFS.
  useEffect(() => {
    if (project === null) return;
    let cancelled = false;
    const poll = () => {
      if (cancelled || !isWasmReady()) return;
      const listed = vfsListFiles();
      const next = listed.success ? (listed.files ?? []) : [];
      setFiles((prev) => (prev.length === next.length && prev.every((f, i) => f === next[i]) ? prev : next));
    };
    void initWasm().then(() => { if (!cancelled) { setReady(true); poll(); } });
    const timer = window.setInterval(poll, POLL_MS);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [project]);

  const port = useMemo<WasmPort | undefined>(
    () => (project === null || !ready ? undefined : { ready, files }),
    [project, ready, files],
  );
  useOutput('wasm', port);

  const status = project === null
    ? 'No project connected'
    : !ready ? 'initialising…' : `${files.length} files in VFS`;

  return (
    <div className="cd-node cd-wasm">
      <div className="cd-title">WASM</div>
      <div className="cd-meta">quarto-hub-client.wasm · {status}</div>
      <Handle type="target" position={Position.Left} id="project" title="project" />
      <Handle type="source" position={Position.Right} id="wasm" title="wasm" />
    </div>
  );
}
