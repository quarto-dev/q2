/**
 * The render pipeline, run on the file shown by the editor wired into its
 * "editor" port, in the WASM module wired into "wasm": after each edit
 * (debounced) the project render — the same queued render the preview
 * uses — processes the page against the module's VFS, and whatever it
 * reports (parse errors, links to files that don't exist, unresolved
 * cross-references, unknown shortcodes, ...) is set as markers on the
 * editor it came from, so the underlines appear there. Unwire the editor
 * and they go.
 */

import { useEffect, useMemo, useState } from 'react';
import { diagnosticsToMarkers } from '../../../utils/diagnosticToMonaco';
import { Handle, Position } from '@xyflow/react';
import { useRenderDiagnostics } from '../../../hooks/useRenderDiagnostics';
import { useInput, type EditorPort, type WasmPort } from './ports';

export default function RenderNode() {
  const editor = useInput<EditorPort>('editor');
  const wasm = useInput<WasmPort>('wasm');
  const port = wasm !== null && wasm.ready ? editor : null;
  const path = port?.path ?? null;

  // Edits are tracked per Monaco instance; a freshly published one reads as-is.
  const [edited, setEdited] = useState<{ port: EditorPort; content: string } | null>(null);
  useEffect(() => {
    if (port === null) return;
    const model = port.editor.getModel();
    if (!model) return;
    const sub = model.onDidChangeContent(() => setEdited({ port, content: model.getValue() }));
    return () => sub.dispose();
  }, [port]);
  const content = edited !== null && edited.port === port ? edited.content : (port?.editor.getModel()?.getValue() ?? '');

  const diagnostics = useRenderDiagnostics(path, content, port !== null);

  // Diagnostics → the editor's markers; cleared when either side goes away.
  const markers = useMemo(() => diagnosticsToMarkers(diagnostics).markers, [diagnostics]);
  useEffect(() => {
    if (port === null) return;
    const model = port.editor.getModel();
    if (!model) return;
    port.monaco.editor.setModelMarkers(model, 'quarto', markers);
    return () => { port.monaco.editor.setModelMarkers(model, 'quarto', []); };
  }, [port, markers]);

  const errors = diagnostics.filter((d) => d.kind === 'error').length;
  const warnings = diagnostics.length - errors;

  return (
    <div className="cd-node cd-render">
      <div className="cd-title">Render</div>
      {editor === null
        ? <div className="cd-empty">No editor connected</div>
        : port === null
          ? <div className="cd-empty">No WASM connected to render with</div>
        : (
          <div className="cd-kv">
            <div className="cd-row"><span>errors</span><span>{errors}</span></div>
            <div className="cd-row"><span>warnings</span><span>{warnings}</span></div>
            <div className="cd-meta">project render of {port.path}</div>
          </div>
        )}
      <Handle type="target" position={Position.Left} id="editor" title="editor" style={{ top: '35%' }} />
      <Handle type="target" position={Position.Left} id="wasm" title="wasm" style={{ top: '65%' }} />
    </div>
  );
}
