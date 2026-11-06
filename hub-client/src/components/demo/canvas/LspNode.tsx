/**
 * Language intelligence for the editor wired into its "editor" port: the
 * hub-client providers (outline symbols, folding ranges, semantic tokens)
 * registered on its Monaco — quarto-lsp-core in the WASM module wired into
 * "wasm", analysing the file in that module's VFS. Shows what the analysis
 * found. Diagnostics are the render node's job.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { useIntelligence } from '../../../hooks/useIntelligence';
import { useIntelligenceProviders } from '../../../hooks/useIntelligenceProviders';
import { disposeIntelligenceProviders } from '../../../services/monacoProviders';
import { useInput, type EditorPort, type WasmPort } from './ports';

export default function LspNode() {
  const editor = useInput<EditorPort>('editor');
  const wasm = useInput<WasmPort>('wasm');
  const port = wasm !== null && wasm.ready ? editor : null;
  const path = port?.path ?? null;

  // The providers resolve the file through this getter at request time.
  const pathRef = useRef(path);
  useEffect(() => { pathRef.current = path; }, [path]);
  const getCurrentFilePath = useCallback(() => pathRef.current, []);
  const registerProviders = useIntelligenceProviders(getCurrentFilePath, 'ready', port !== null);

  // Register on each Monaco the editor node publishes; let go on disconnect.
  useEffect(() => {
    if (port === null) return;
    registerProviders(port.monaco);
    return () => disposeIntelligenceProviders();
  }, [port, registerProviders]);

  // Re-analyse after every edit (debounced in the hook).
  const [version, setVersion] = useState(0);
  useEffect(() => {
    if (port === null) return;
    const model = port.editor.getModel();
    if (!model) return;
    const sub = model.onDidChangeContent(() => setVersion((v) => v + 1));
    return () => sub.dispose();
  }, [port]);
  const { symbols, foldingRanges, loading, refresh } = useIntelligence({ path, enableSymbols: true, enableFoldingRanges: true });
  useEffect(() => { refresh(); }, [version, refresh]);

  return (
    <div className="cd-node cd-lsp">
      <div className="cd-title">LSP</div>
      {editor === null
        ? <div className="cd-empty">No editor connected</div>
        : port === null
          ? <div className="cd-empty">No WASM connected to analyse with</div>
        : (
          <div className="cd-kv">
            <div className="cd-row"><span>symbols</span><span>{symbols.length}</span></div>
            <div className="cd-row"><span>folding ranges</span><span>{foldingRanges.length}</span></div>
            <div className="cd-meta">{loading ? 'analysing…' : `quarto-lsp-core on ${port.path}`}</div>
          </div>
        )}
      <Handle type="target" position={Position.Left} id="editor" title="editor" />
      <Handle type="target" position={Position.Bottom} id="wasm" title="wasm" />
    </div>
  );
}
