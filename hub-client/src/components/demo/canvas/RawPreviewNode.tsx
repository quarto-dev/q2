/**
 * The raw preview: the document wired into its "doc" port, parsed by the
 * WASM module wired into "wasm" (parse only — no transform pipeline, no
 * project) and shown by the q2-debug renderer, the React AST view in its
 * own iframe. It is the preview of the parse tree rather than of the
 * rendered page, which is what makes it useful next to the editor: it
 * needs no project, no render, and shows exactly what the parser saw.
 */

import { useEffect, useState } from 'react';
import { Handle, NodeResizer, Position } from '@xyflow/react';
import { parseQmdToAst } from '@quarto/preview-runtime';
import type { TextDocumentContent } from '@quarto/quarto-automerge-schema';
import { Q2DebugIframe } from '../../render/q2-debug/Q2DebugIframe';
import { useInput, type DocPort, type WasmPort } from './ports';

const DEBOUNCE_MS = 300;
const noop = () => {};

function Parsed({ text, path }: { text: string; path: string }) {
  // The last successful parse stays up while the next one is pending.
  const [parsed, setParsed] = useState<{ astJson: string; error: string | null } | null>(null);
  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void parseQmdToAst(text).then((result) => {
        if (cancelled) return;
        if (result.success && result.ast !== undefined) setParsed({ astJson: result.ast, error: null });
        else setParsed((prev) => ({ astJson: prev?.astJson ?? '', error: String(result.error ?? 'parse failed') }));
      });
    }, DEBOUNCE_MS);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [text]);

  if (parsed === null) return <div className="cd-empty">parsing…</div>;
  return (
    <>
      {parsed.error !== null && <div className="cd-preview-error">{parsed.error}</div>}
      {parsed.astJson !== '' && <Q2DebugIframe astJson={parsed.astJson} currentFilePath={path} setAst={noop} />}
    </>
  );
}

export default function RawPreviewNode() {
  const doc = useInput<DocPort<TextDocumentContent>>('doc');
  const wasm = useInput<WasmPort>('wasm');
  const ready = doc !== null && wasm !== null && wasm.ready;
  return (
    <div className="cd-node cd-rawpreview">
      <NodeResizer minWidth={320} minHeight={200} />
      <div className="cd-title">Raw preview (q2-debug)</div>
      <div className="cd-preview nodrag nowheel">
        {doc === null
          ? <div className="cd-empty">No doc connected to preview</div>
          : !ready
            ? <div className="cd-empty">No WASM connected to parse with</div>
            : <Parsed text={doc.doc.text} path={`${doc.id}.qmd`} />}
      </div>
      <Handle type="target" position={Position.Left} id="doc" title="doc" style={{ top: '35%' }} />
      <Handle type="target" position={Position.Left} id="wasm" title="wasm" style={{ top: '65%' }} />
    </div>
  );
}
