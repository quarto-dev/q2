/**
 * Top-level component for the q2-preview SPA.
 *
 * Phase A scope (bd-o5wd):
 *   - read `indexDocId` from `window.location.hash` (`#/preview/<id>`),
 *   - boot the WASM module + samod sync via `@quarto/preview-runtime`,
 *   - pick the first .qmd in the project as the active page,
 *   - render that page through `<Q2PreviewIframe>`,
 *   - re-render when any synced file's content changes.
 *
 * Out of scope (Phase A): URL-driven file selection beyond the first
 * .qmd, code-cell execution (Phase C), the force-refresh button
 * (bd-b5hf / A.6), real error UX beyond the connection error path.
 *
 * Decisions worth surfacing here:
 *
 * - `setAst` on Q2PreviewIframe is a no-op for now. The iframe takes
 *   it as a required prop because Phase 2 of q2-preview anticipated a
 *   WYSIWYG round-trip (the iframe asks the parent to update the
 *   AST). The SPA doesn't have an editor to round-trip into yet, so a
 *   no-op is correct.
 *
 * - `wsUrl` is derived from `window.location` rather than carried in
 *   the fragment (per Q-A3 of the Phase A plan). The CLI always opens
 *   the SPA on the same host:port the websocket lives on, so this is
 *   a single source of truth.
 */

import { useEffect, useState } from 'react';
import {
  initWasm,
  connect,
  setSyncHandlers,
  renderPageInProject,
} from '@quarto/preview-runtime';
import { Q2PreviewIframe } from '@quarto/preview-renderer/iframe/Q2PreviewIframe';
import { PreviewErrorOverlay } from '@quarto/preview-renderer/overlays/PreviewErrorOverlay';
import type { FileEntry } from '@quarto/quarto-automerge-schema';

type BootState = 'loading' | 'ready' | 'error';

interface PreviewAppState {
  boot: BootState;
  files: FileEntry[];
  activeFile: string | null;
  astJson: string | null;
  error: Error | null;
  /** Bumps on every onFileContent callback so the render effect re-fires. */
  contentTick: number;
}

const INITIAL_STATE: PreviewAppState = {
  boot: 'loading',
  files: [],
  activeFile: null,
  astJson: null,
  error: null,
  contentTick: 0,
};

/**
 * Parse `window.location.hash` for `#/preview/<indexDocId>`. Returns
 * `null` when the fragment is missing or malformed.
 */
function parseIndexDocId(hash: string): string | null {
  // `#/preview/automerge:abc123` → `automerge:abc123`. Tolerate the
  // bare-hash form `#/preview/abc123` too (test fixtures use both).
  const m = hash.match(/^#\/preview\/(.+)$/);
  return m ? m[1] : null;
}

/**
 * Derive the websocket URL from the page location. The CLI serves the
 * SPA on the same host:port that hosts the samod ws endpoint, so we
 * just swap the scheme.
 */
function deriveWsUrl(loc: Location = window.location): string {
  const wsScheme = loc.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${wsScheme}//${loc.host}/ws`;
}

/** No-op `setAst` until WYSIWYG mode is wired (post-Phase-A). */
const noopSetAst = () => {
  /* deliberately empty */
};

export default function PreviewApp() {
  const [state, setState] = useState<PreviewAppState>(INITIAL_STATE);

  // Boot once: WASM init + samod connect + initial file pick.
  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const indexDocId = parseIndexDocId(window.location.hash);
        if (!indexDocId) {
          throw new Error(
            `No indexDocId in URL fragment. Expected ` +
              `#/preview/<indexDocId>; got "${window.location.hash}".`,
          );
        }
        const wsUrl = deriveWsUrl();

        await initWasm();

        setSyncHandlers({
          onFilesChange: (files) => {
            if (cancelled) return;
            setState((s) => ({ ...s, files }));
          },
          onFileContent: () => {
            if (cancelled) return;
            setState((s) => ({ ...s, contentTick: s.contentTick + 1 }));
          },
          onError: (err) => {
            if (cancelled) return;
            setState((s) => ({ ...s, error: err, boot: 'error' }));
          },
        });

        const initialFiles = await connect(wsUrl, indexDocId);
        if (cancelled) return;

        const firstQmd = initialFiles.find((f) => f.path.endsWith('.qmd'));
        setState((s) => ({
          ...s,
          files: initialFiles,
          activeFile: firstQmd?.path ?? null,
          boot: 'ready',
        }));
      } catch (err) {
        if (cancelled) return;
        setState((s) => ({
          ...s,
          error: err instanceof Error ? err : new Error(String(err)),
          boot: 'error',
        }));
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  // Render the active page whenever it (or its content) changes.
  useEffect(() => {
    if (!state.activeFile) return;
    let cancelled = false;
    void (async () => {
      try {
        const result = await renderPageInProject(state.activeFile!);
        if (cancelled) return;
        if (result.success && result.ast_json !== undefined) {
          setState((s) => ({ ...s, astJson: result.ast_json ?? null }));
        } else {
          setState((s) => ({
            ...s,
            error: new Error(result.error ?? 'renderPageInProject failed'),
            boot: 'error',
          }));
        }
      } catch (err) {
        if (cancelled) return;
        setState((s) => ({
          ...s,
          error: err instanceof Error ? err : new Error(String(err)),
          boot: 'error',
        }));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [state.activeFile, state.contentTick]);

  // ── Render ────────────────────────────────────────────────────────────

  if (state.boot === 'error' && state.error) {
    return (
      <PreviewErrorOverlay
        error={{ message: state.error.message }}
        visible
        collapsed={false}
      />
    );
  }

  if (state.boot === 'loading' || !state.activeFile || state.astJson === null) {
    return (
      <div
        style={{
          padding: 24,
          color: '#666',
          font: '14px -apple-system, Segoe UI, sans-serif',
        }}
      >
        Initializing q2-preview…
      </div>
    );
  }

  return (
    <Q2PreviewIframe
      astJson={state.astJson}
      currentFilePath={state.activeFile}
      setAst={noopSetAst}
    />
  );
}
