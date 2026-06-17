import { useCallback, useEffect, useRef } from 'react';
import type * as Monaco from 'monaco-editor';
import {
  registerIntelligenceProviders,
  disposeIntelligenceProviders,
  refreshSemanticTokens,
} from '../services/monacoProviders';

/**
 * Lifecycle of the global Monaco intelligence providers (symbols, folding,
 * semantic tokens). Returns an editor-mount handler.
 *
 * Registration is idempotent; disposal is unmount-only — coupling it to
 * `currentFile` once left a file switch with no provider until a page reload.
 * `refreshSemanticTokens()` forces an immediate re-tokenise (Monaco wires a
 * provider's `onDidChange` to `schedule(0)`, skipping its ~300ms debounce); we
 * fire it on every mount (the editor remounts per file) and once on WASM
 * `loading → ready` (the mount refresh can precede a cold-start highlighter).
 */
export function useIntelligenceProviders(
  getCurrentFilePath: () => string | null,
  wasmStatus: 'loading' | 'ready' | 'error',
  editorReady: boolean
): (monaco: typeof Monaco) => void {
  const onEditorMount = useCallback(
    (monaco: typeof Monaco) => {
      registerIntelligenceProviders(monaco, getCurrentFilePath);
      refreshSemanticTokens();
    },
    [getCurrentFilePath]
  );

  useEffect(() => () => disposeIntelligenceProviders(), []);

  // Re-tokenise once, when WASM is ready and the editor is mounted.
  const wasmRefreshFiredRef = useRef(false);
  useEffect(() => {
    if (wasmStatus === 'ready' && editorReady && !wasmRefreshFiredRef.current) {
      wasmRefreshFiredRef.current = true;
      refreshSemanticTokens();
    }
  }, [wasmStatus, editorReady]);

  return onEditorMount;
}
