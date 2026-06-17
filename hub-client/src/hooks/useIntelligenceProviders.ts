import { useCallback, useEffect } from 'react';
import type * as Monaco from 'monaco-editor';
import {
  registerIntelligenceProviders,
  disposeIntelligenceProviders,
  refreshSemanticTokens,
} from '../services/monacoProviders';

/**
 * Own the lifecycle of the global Monaco intelligence providers (document
 * symbols, folding ranges, semantic tokens).
 *
 * The providers are registered once (registration is idempotent) and disposed
 * once when the editor unmounts. The disposal is deliberately on its own
 * mount-only effect: it must NOT be coupled to anything that changes per
 * `currentFile`. A previous version wired `disposeIntelligenceProviders()` into
 * an effect whose deps tracked a `currentFile`-dependent callback, so
 * opening/switching a file could dispose the providers with no remount to
 * re-register them — leaving the editor with no semantic-tokens provider
 * (links' `[`/`]` rendered by the Monarch base only, hence mismatched) until a
 * full page reload.
 *
 * The editor remounts per file (MonacoEditor is keyed on the path), so this
 * handler also runs on every open. Rather than re-register (which would
 * reschedule the token fetch behind Monaco's adaptive debounce), it fires
 * `refreshSemanticTokens()` to force an immediate re-tokenise of the
 * freshly-attached model — the correct colours appear without a ~300ms wait.
 *
 * @param getCurrentFilePath - Stable getter for the active VFS path
 * @returns An editor-mount handler that registers the providers
 */
export function useIntelligenceProviders(
  getCurrentFilePath: () => string | null
): (monaco: typeof Monaco) => void {
  const onEditorMount = useCallback(
    (monaco: typeof Monaco) => {
      registerIntelligenceProviders(monaco, getCurrentFilePath);
      // Force the freshly-attached model to tokenise now, not after the debounce.
      refreshSemanticTokens();
    },
    [getCurrentFilePath]
  );

  useEffect(() => {
    return () => {
      disposeIntelligenceProviders();
    };
  }, []);

  return onEditorMount;
}
