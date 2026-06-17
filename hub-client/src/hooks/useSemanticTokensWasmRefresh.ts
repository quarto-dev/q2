import { useEffect, useRef } from 'react';
import { refreshSemanticTokens } from '../services/monacoProviders';

/**
 * Force an immediate semantic-tokens re-tokenise once WASM finishes
 * initializing.
 *
 * The editor-mount refresh (see `useIntelligenceProviders`) can fire before the
 * WASM highlighter is ready — on a cold start with the first file already
 * loaded, the mount-time fetch returns no tokens and the Monarch base shows
 * until the next edit. Re-firing on the loading→ready transition paints the
 * correct colours as soon as the highlighter is available.
 *
 * Fires exactly once, only after the editor is mounted (so the providers are
 * registered) and WASM has reached 'ready' — never on 'error', and not again on
 * later re-renders. Subsequent file opens are handled by the mount-time refresh.
 *
 * @param wasmStatus - Current WASM initialization status
 * @param editorReady - Whether the editor has mounted (providers registered)
 */
export function useSemanticTokensWasmRefresh(
  wasmStatus: 'loading' | 'ready' | 'error',
  editorReady: boolean
): void {
  const firedRef = useRef(false);
  useEffect(() => {
    if (wasmStatus === 'ready' && editorReady && !firedRef.current) {
      firedRef.current = true;
      refreshSemanticTokens();
    }
  }, [wasmStatus, editorReady]);
}
