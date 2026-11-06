/**
 * Language intelligence for the editor wired into its "editor" port: the
 * hub-client providers (outline symbols, folding ranges, semantic tokens)
 * registered on its Monaco — quarto-lsp-core reading
 * the file it edits from the virtual filesystem wired into "vfs". Shows
 * what the analysis found. Diagnostics are the render node's job.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { useIntelligence } from '../../../hooks/useIntelligence';
import { useIntelligenceProviders } from '../../../hooks/useIntelligenceProviders';
import { disposeIntelligenceProviders } from '../../../services/monacoProviders';
import { useInput, type EditorPort, type VfsPort } from './ports';

export default function LspNode() {
  const editor = useInput<EditorPort>('editor');
  const vfs = useInput<VfsPort>('vfs');
  const port = vfs !== null ? editor : null;
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
        : vfs === null
          ? <div className="cd-empty">No VFS connected to read from</div>
        : (
          <div className="cd-kv">
            <div className="cd-row"><span>symbols</span><span>{symbols.length}</span></div>
            <div className="cd-row"><span>folding ranges</span><span>{foldingRanges.length}</span></div>
            <div className="cd-meta">{loading ? 'analysing…' : `quarto-lsp-core on ${path}`}</div>
          </div>
        )}
      <Handle type="target" position={Position.Left} id="editor" title="editor" style={{ top: '35%' }} />
      <Handle type="target" position={Position.Left} id="vfs" title="vfs" style={{ top: '65%' }} />
    </div>
  );
}
