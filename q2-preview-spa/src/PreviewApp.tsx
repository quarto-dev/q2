/**
 * Top-level component for the q2-preview SPA.
 *
 * Phase A scope (bd-o5wd + bd-mflk):
 *   - fetch `index_document_id` from the hub's `GET /health`,
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
 * - `wsUrl` is derived from `window.location` rather than read from
 *   a server endpoint. The CLI always opens the SPA on the same
 *   host:port the websocket lives on, so this is the single source
 *   of truth.
 *
 * - `indexDocId` comes from `GET /health` rather than the URL
 *   fragment. The Phase A plan's Q-A3 originally chose URL-fragment
 *   carrier (mirroring hub-client's `#/share/...` pattern), but
 *   threading the docId through the CLI before serve-start turned
 *   out to require either pre-binding the listener + extracting
 *   ctx.index().document_id() or extending run_server's API. The hub
 *   already exposes `index_document_id` on `/health` for free
 *   (no auth required when auth_config is None, which is preview's
 *   default), so we use that. Net: simpler architecture, one extra
 *   round-trip on boot, no new server-side patterns introduced.
 */

import { useCallback, useEffect, useState } from 'react';
import {
  initWasm,
  connect,
  setSyncHandlers,
  renderPageForPreview,
  getBinaryDocById,
} from '@quarto/preview-runtime';
import { Q2PreviewIframe } from '@quarto/preview-renderer/iframe/Q2PreviewIframe';
import { PreviewErrorOverlay } from '@quarto/preview-renderer/overlays/PreviewErrorOverlay';
import type { CaptureRef, FileEntry } from '@quarto/quarto-automerge-schema';
import { ForceRefreshButton } from './components/ForceRefreshButton';
import { StaleCaptureOverlay } from './components/StaleCaptureOverlay';
import { pickInitialPage } from './pickInitialPage';

type BootState = 'loading' | 'ready' | 'error';

interface PreviewAppState {
  boot: BootState;
  files: FileEntry[];
  activeFile: string | null;
  astJson: string | null;
  /**
   * Three-way value matching `Q2PreviewIframe`'s `themeFingerprint`
   * contract (see its docstring): a string means "post this theme to
   * the iframe"; `null` means "clear the theme"; `undefined` means
   * "we have no opinion yet — keep the iframe's last-good theme".
   * Initialized as `undefined` so the pre-first-render iframe doesn't
   * receive an unintended `UPDATE_THEME { cssUrl: null }`.
   */
  themeFingerprint: string | null | undefined;
  /**
   * Boot-time failure (e.g. `/health` 5xx, samod connect throws). When
   * set, the SPA replaces the UI with `<PreviewErrorOverlay>` — there's
   * no previous render worth keeping. Distinct from `renderError`.
   */
  error: Error | null;
  /**
   * Phase D.4 (bd-kw93.10): render-pipeline failure (WASM
   * `renderPageForPreview` threw, or `result.success === false`).
   * Render errors are *non-terminal*: the iframe keeps showing the
   * last good `astJson` and `<PreviewErrorOverlay>` is overlaid on
   * top so the user can see what broke without losing the prior
   * render's context. A subsequent successful render clears this.
   */
  renderError: Error | null;
  /** Bumps on every onFileContent callback so the render effect re-fires. */
  contentTick: number;
  /**
   * IndexDocument V2 capture sidecar (Phase C.3) — path → CaptureRef
   * mapping. Populated by the server-side eager-capture driver (Phase
   * C.1) and read here by the render effect (Phase C.4) so the
   * WASM-side `EngineRegistry::with_replay` can stand in for the real
   * engine.
   */
  captures: Record<string, CaptureRef>;
}

const INITIAL_STATE: PreviewAppState = {
  boot: 'loading',
  files: [],
  activeFile: null,
  astJson: null,
  themeFingerprint: undefined,
  error: null,
  renderError: null,
  contentTick: 0,
  captures: {},
};

/**
 * Fetch the project's index document id from the hub's `/health`
 * endpoint. Throws on network failure or if the response doesn't
 * carry an `index_document_id`.
 *
 * The hub stores doc IDs in the bare form (e.g. `4ByAxLmG…`) and
 * `/health` returns them that way. `@quarto/preview-runtime`'s
 * `connect()` expects the `automerge:<id>` form (same as
 * automerge-repo's `DocumentId`); see how hub-client normalizes the
 * incoming `shareRoute.indexDocId` in App.tsx for the same reason.
 * We normalize here so callers see a single consistent shape.
 */
async function fetchIndexDocId(loc: Location = window.location): Promise<string> {
  const healthUrl = `${loc.protocol}//${loc.host}/health`;
  const resp = await fetch(healthUrl);
  if (!resp.ok) {
    throw new Error(`GET /health returned ${resp.status} ${resp.statusText}`);
  }
  const body = (await resp.json()) as { index_document_id?: string };
  if (!body.index_document_id) {
    throw new Error(
      `/health response missing index_document_id; got: ${JSON.stringify(body)}`,
    );
  }
  return body.index_document_id.startsWith('automerge:')
    ? body.index_document_id
    : `automerge:${body.index_document_id}`;
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

  // Force-refresh trigger (bd-b5hf): bumping `contentTick` re-fires
  // the render useEffect. Reuses the same channel `onFileContent`
  // uses for sync-driven re-renders so there's one path through the
  // render pipeline regardless of who asks. Stable identity via
  // useCallback so the button doesn't re-mount on every state
  // update.
  const handleRefresh = useCallback(() => {
    setState((s) => ({ ...s, contentTick: s.contentTick + 1 }));
  }, []);

  // Boot once: WASM init + samod connect + initial file pick.
  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const indexDocId = await fetchIndexDocId();
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
          // Phase C.4: keep the capture sidecar in state so the render
          // effect can pick up server-recorded captures (writes by
          // Phase C.1) and route them into WASM replay.
          onCapturesChange: (captures) => {
            if (cancelled) return;
            setState((s) => ({
              ...s,
              captures,
              // Bump contentTick so the render effect re-fires; the
              // newly-recorded capture should now affect the rendered
              // AST for the active page.
              contentTick: s.contentTick + 1,
            }));
          },
          onError: (err) => {
            if (cancelled) return;
            setState((s) => ({ ...s, error: err, boot: 'error' }));
          },
        });

        // 5s peer wait: the q2-preview SPA always hits a fresh
        // ephemeral hub with no IndexedDB cache, so the underlying
        // 1ms "probe" default in quarto-sync-client would race the
        // samod handshake and `findDoc(indexDocId)` would fail with
        // "Document … is unavailable" on cold loads.
        const initialFiles = await connect(wsUrl, indexDocId, undefined, undefined, undefined, 5000);
        if (cancelled) return;

        // Phase D.2 (bd-kw93.13): seed `activeFile` from the boot
        // URL's `?page=<rel>` query if the CLI carried one through.
        // Falls back to firstQmd when missing/invalid — that's the
        // pre-D.2 Phase A behaviour preserved verbatim.
        const activeFile = pickInitialPage(
          typeof window !== 'undefined' ? window.location.search : '',
          initialFiles,
        );
        setState((s) => ({
          ...s,
          files: initialFiles,
          activeFile,
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
        // Phase C.4: look up the capture for the active page, fetch
        // the binary doc, and pass the gzipped JSON bytes through to
        // the WASM renderer. The renderer constructs a ReplayEngine
        // from them when present; absent ⇒ default registry (markdown
        // engine; code cells render as source).
        const captureRef = state.captures[state.activeFile!];
        let captureGzJson: Uint8Array | undefined;
        if (captureRef?.captureDocId) {
          const binaryDoc = await getBinaryDocById(captureRef.captureDocId);
          if (cancelled) return;
          captureGzJson = binaryDoc?.content;
        }

        const result = await renderPageForPreview(
          state.activeFile!,
          undefined,
          captureGzJson,
        );
        if (cancelled) return;
        if (result.success && result.ast_json !== undefined) {
          // Three-way themeFingerprint (mirrors hub-client's
          // `ReactPreview` mapping at line 119-125): a string means a
          // compiled theme exists; field absent means render succeeded
          // with no theme intended → explicit clear (`null`). The
          // render-failure branch below leaves the value untouched so
          // last-good styling survives transient errors.
          //
          // Phase D.4 (bd-kw93.10): a successful render also clears
          // `renderError` so the previous overlay (if any) goes away.
          setState((s) => ({
            ...s,
            astJson: result.ast_json ?? null,
            themeFingerprint: result.theme_fingerprint ?? null,
            renderError: null,
          }));
        } else {
          // Log the full result so we can diagnose surprises in the
          // browser console without a code change.
          console.error('renderPageInProject failed', {
            path: state.activeFile,
            result,
          });
          // Phase D.4: route into the non-terminal `renderError`
          // slot. We deliberately do NOT touch `boot` or `astJson` —
          // the iframe keeps showing the last-good render underneath
          // and the overlay surfaces the failure on top. Distinct
          // from boot errors, which DO replace the UI (no good
          // render exists to fall back to).
          setState((s) => ({
            ...s,
            renderError: new Error(
              result.error ?? `renderPageInProject failed: ${JSON.stringify(result)}`,
            ),
          }));
        }
      } catch (err) {
        if (cancelled) return;
        // Same non-terminal treatment as the non-success branch
        // above. A render that throws (e.g. malformed qmd hits the
        // WASM parser) overlays on top of the previous good render.
        setState((s) => ({
          ...s,
          renderError: err instanceof Error ? err : new Error(String(err)),
        }));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [state.activeFile, state.contentTick, state.captures]);

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

  // Phase D.4 (bd-kw93.10): if the *first* render failed (no good
  // astJson exists to fall back to), show the overlay terminal-style.
  // Subsequent failures with a prior good astJson take the
  // overlay-on-top branch further down.
  if (state.astJson === null && state.renderError) {
    return (
      <PreviewErrorOverlay
        error={{ message: state.renderError.message }}
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

  // The wrapper anchors the absolutely-positioned refresh button
  // (and any future floating chrome) so it overlays the iframe
  // without an extra flex/grid layer. `height: 100%` lets the
  // iframe still fill the SPA root (set by index.html).
  // Phase C.5: show the stale-capture overlay when the active page's
  // sidecar entry says staleness=true OR an in-flight re-execute is
  // running OR a previous re-execute errored. The previous capture
  // still drives the rendered preview underneath; the overlay just
  // surfaces the signal + the Re-execute action.
  const activeCapture: CaptureRef | undefined = state.captures[state.activeFile];
  const showStaleOverlay =
    activeCapture !== undefined &&
    (activeCapture.staleness === true ||
      activeCapture.state === 'running' ||
      activeCapture.state === 'error');

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%' }}>
      <Q2PreviewIframe
        astJson={state.astJson}
        currentFilePath={state.activeFile}
        themeFingerprint={state.themeFingerprint}
        setAst={noopSetAst}
      />
      {showStaleOverlay && (
        <StaleCaptureOverlay
          activePath={state.activeFile}
          state={activeCapture?.state}
          lastError={activeCapture?.lastError}
        />
      )}
      {/* Phase D.4 (bd-kw93.10): non-terminal render-error overlay.
          Shown collapsed so it doesn't hide the last-good render the
          user is looking at; click "Error" to expand for details. */}
      {state.renderError && (
        <PreviewErrorOverlay
          error={{ message: state.renderError.message }}
          visible
          collapsed
        />
      )}
      <ForceRefreshButton onRefresh={handleRefresh} />
    </div>
  );
}
