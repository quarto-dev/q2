/**
 * The quarto WASM module (wasm-quarto-hub-client): the Rust pipeline,
 * parser and LSP compiled to WebAssembly, loaded once into the page.
 * Offered on the "wasm" port once initialised, to the virtual filesystem
 * it holds and to anything that parses or renders with it.
 */

import { useEffect, useMemo, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { initWasm, isWasmReady } from '@quarto/preview-runtime';
import { WasmLogo } from './icons';
import { useOutput, type WasmPort } from './ports';

export default function WasmNode() {
  const [ready, setReady] = useState(isWasmReady());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void initWasm()
      .then(() => { if (!cancelled) setReady(true); })
      .catch((err: unknown) => { if (!cancelled) setError(String(err)); });
    return () => { cancelled = true; };
  }, []);

  const port = useMemo<WasmPort | undefined>(() => (ready ? { ready } : undefined), [ready]);
  useOutput('wasm', port);

  return (
    <div className="cd-node cd-wasm">
      <WasmLogo />
      <div className="cd-title">q2 wasm</div>
      <div className="cd-meta">wasm-quarto-hub-client · {error ?? (ready ? 'ready' : 'initialising…')}</div>
      <Handle type="source" position={Position.Right} id="wasm" title="wasm" />
    </div>
  );
}
