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

import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  initWasm,
  connect,
  disconnect,
  setSyncHandlers,
  renderPageForPreview,
  getBinaryDocById,
  applyNodeEdit,
  parseQmdContentSync,
  vfsAddFile,
  vfsReadFile,
  getFileContent,
  diffToEditorChanges,
  applyEditorOperations,
  regenerateNestedBuffers,
} from '@quarto/preview-runtime';
import { Q2PreviewIframe } from '@quarto/preview-renderer/iframe/Q2PreviewIframe';
import { extractMetaString } from '@quarto/preview-renderer/framework';
import type { Diagnostic, Pass1Failure, PreviewNodeEditPayload } from '@quarto/preview-renderer/types/diagnostic';
import type { CaptureRef, FileEntry } from '@quarto/quarto-automerge-schema';
import { bootWithRetry, superviseReconnect } from './bootController';
import { BootLoadingScreen } from './components/BootLoadingScreen';
import { ForceRefreshButton } from './components/ForceRefreshButton';
import { PreviewDiagnosticsOverlay } from './components/PreviewDiagnosticsOverlay';
import { StaleCaptureOverlay } from './components/StaleCaptureOverlay';
import { pickInitialPage } from './pickInitialPage';

/**
 * Suffix appended to the document's title in the browser tab so a
 * `q2 preview` tab is distinguishable from the live / published page
 * the same author may have open in a sibling tab. Used both with a
 * doc title (`<title> (Quarto Preview)`) and as the standalone
 * fallback when no `meta.title` is set (`Quarto Preview`).
 */
const PREVIEW_TITLE_SUFFIX = 'Quarto Preview';

/**
 * Build the browser-tab title from the active doc's Pandoc-AST
 * `meta.title`. Returns `"<title> (Quarto Preview)"` when a title is
 * extractable, otherwise the bare `"Quarto Preview"` suffix. The
 * input is the parsed AST (`{ blocks, meta, ... }`) — callers pass
 * `JSON.parse(state.astJson)` and `null`/parse-error degrades to the
 * fallback.
 *
 * `extractMetaString` (from the framework) handles `MetaString`,
 * `MetaInlines`, and `MetaBlocks` — the three shapes pampa's JSON
 * writer emits for a YAML `title: …` scalar. Other Meta variants
 * (MetaBool, MetaList, MetaMap) coerce to `undefined` and we fall
 * back. Exported for the integration tests; not part of the SPA's
 * runtime public surface.
 */
export function buildPreviewTabTitle(ast: unknown): string {
  if (!ast || typeof ast !== 'object') return PREVIEW_TITLE_SUFFIX;
  const meta = (ast as { meta?: unknown }).meta;
  if (!meta || typeof meta !== 'object') return PREVIEW_TITLE_SUFFIX;
  const titleNode = (meta as Record<string, unknown>).title;
  const title = extractMetaString(titleNode);
  if (!title) return PREVIEW_TITLE_SUFFIX;
  return `${title} (${PREVIEW_TITLE_SUFFIX})`;
}

type BootState = 'loading' | 'ready' | 'error';

interface PreviewAppState {
  boot: BootState;
  files: FileEntry[];
  activeFile: string | null;
  /**
   * Phase F.1 (bd-kw93.14): anchor (no leading `#`) the iframe should
   * scroll to after the next render commits. Set when the user clicks
   * a cross-page link with `#frag` or when popstate restores a hash;
   * forwarded to `Q2PreviewIframe` as `pendingAnchor`.
   *
   * Paired with `pendingAnchorEpoch` so subsequent edits to the same
   * page don't re-trigger the scroll: each navigation event bumps the
   * epoch, the iframe scrolls only when the epoch changes (per-tick
   * tracking via ref). Without the epoch, an edit-driven re-render
   * would jerk the user back to the original anchor every time.
   */
  pendingAnchor: string | null;
  pendingAnchorEpoch: number;
  astJson: string | null;
  /** Pre-pipeline AST retained for apply_node_edit (Phase 1/4). */
  untransformedAstJson: string | null;
  /**
   * The QMD source text that was used to produce the current render
   * generation (Task 3 / block-editing plan). The byte offsets in
   * `astJson` and `untransformedAstJson` belong to this snapshot.
   * Using the live VFS content at edit time would produce skewed
   * offsets when the user edits between renders.
   */
  renderedContent: string;
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
   * Phase D.6 (bd-kw93.12): dep set for the *current* `activeFile`,
   * fetched from `/api/preview/deps`. `null` means "unknown yet";
   * the filter in `onFileContent` falls back to pre-D.6 behaviour
   * (every change bumps `contentTick`) when null — fail-open is
   * correct because a missed re-render is the worse failure mode
   * here. Paths in the set are project-relative forward-slash
   * strings; `activeFile` itself is included in the set for a
   * single comparison call.
   */
  deps: Set<string> | null;
  /**
   * Boot-time failure (e.g. `/health` 5xx, samod connect throws). When
   * set, the SPA replaces the UI with `<PreviewErrorOverlay>` — there's
   * no previous render worth keeping. Distinct from `renderError`.
   */
  error: Error | null;
  /**
   * Render-pipeline status (bd-b9kzg, extends Phase D.4 /
   * bd-kw93.10). Carries both failures *and* warnings emitted by
   * the WASM render: a successful render with warnings populates
   * `warnings` (and leaves `failure` null), a failed render
   * populates `failure` plus whatever structured diagnostics the
   * render returned. The overlay surfaces are derived from this
   * shape in the render block below.
   *
   * Render *failures* are non-terminal: the iframe keeps showing
   * the last good `astJson` and the overlay is overlaid on top so
   * the user can see what broke without losing the prior render's
   * context. A subsequent successful render replaces this with a
   * clean status (or warnings-only if the new render had any).
   */
  render: RenderStatus;
  /**
   * Server-side diagnostics for the active page, fetched from
   * `GET /api/preview/diagnostics?page=<rel>` (bd-b9kzg). Sourced
   * from the per-server `DiagnosticSink` populated by the three
   * Phase 3 callsite migrations (`capture_driver`, `deps`,
   * `re_execute`). Distinct from the WASM render's own
   * `result.warnings` so the overlay can render them in their own
   * visual lane (the user knows where each diagnostic came from).
   */
  serverDiagnostics: Diagnostic[];
  /** Bumps on every onFileContent callback so the render effect re-fires. */
  contentTick: number;
  /**
   * Whether this preview session may edit documents (bd-ov4gqk3m).
   * Fetched once at boot from `GET /api/preview/config`; mirrors the
   * `--allow-edit` CLI flag. When false the entire edit surface is
   * disabled (`editingDisabled` on the iframe) and `handleSetAst`
   * drops any payload that arrives anyway. Defaults to false and
   * fails closed if the endpoint is missing or errors — a read-only
   * preview is the safe state.
   */
  allowEdit: boolean;
  /**
   * P3.2: nesting-cursor mode for nested blocks. Read once at boot from
   * `?nestingCursor=1` in the boot URL. When true the SPA passes
   * `unlockNestingCursor` to the iframe and runs `regenerateNestedBuffers`
   * on every render. Read-at-load only (no live toggle via the SPA).
   */
  nestingCursor: boolean;
  /**
   * Rich-text (tiptap) block editor (bd-sjb4pzx8). Now ON BY DEFAULT
   * (bd-q9lyghv2 follow-up): read once at boot from `?richText`, where only an
   * explicit `?richText=0` opts out. When true the SPA passes `richText` to the
   * iframe so editable paragraphs render the WYSIWYG editor. Read-at-load only.
   */
  richText: boolean;
  /**
   * IndexDocument V2 capture sidecar (Phase C.3) — path → CaptureRef
   * mapping. Populated by the server-side eager-capture driver (Phase
   * C.1) and read here by the render effect (Phase C.4) so the
   * WASM-side `EngineRegistry::with_replay` can stand in for the real
   * engine.
   */
  captures: Record<string, CaptureRef>;
  /**
   * bd-jit6pdwq Phase 2: boot-retry visibility. The boot controller
   * retries `connect()` without an attempt cap while `/health` says
   * the server is alive; these two fields keep that loop from looking
   * like a silent hang. 1-based attempt counter; `bootLastError` is
   * the previous attempt's failure (null on the first attempt).
   */
  bootAttempt: number;
  bootLastError: Error | null;
  /**
   * bd-jit6pdwq Phase 3: steady-state connection status, rendered as
   * a banner over the (still-shown) content. `reconnecting` while a
   * health-arbitrated reconnect is in flight; `server-gone` once
   * `/health` confirms the server is down (the supervision loop keeps
   * polling HTTP-only and recovers automatically if it returns).
   */
  connection: 'connected' | 'reconnecting' | 'server-gone';
}

/**
 * Render-pipeline status. The four fields cleanly cover the three
 * outcomes a render can have:
 *   - **Success, clean** → `failure: null`, all arrays empty.
 *   - **Success with warnings** → `failure: null`, `warnings`
 *     populated, possibly `pass1Failures` too.
 *   - **Failure** → `failure: { message }`, plus whatever
 *     structured `diagnostics` / `warnings` / `pass1Failures` the
 *     render returned.
 */
interface RenderStatus {
  failure: { message: string } | null;
  diagnostics: Diagnostic[];
  warnings: Diagnostic[];
  pass1Failures: Pass1Failure[];
}

const EMPTY_RENDER_STATUS: RenderStatus = {
  failure: null,
  diagnostics: [],
  warnings: [],
  pass1Failures: [],
};

const CONNECTION_BANNER_STYLE: React.CSSProperties = {
  position: 'absolute',
  top: 8,
  left: '50%',
  transform: 'translateX(-50%)',
  zIndex: 1000,
  padding: '6px 14px',
  borderRadius: 6,
  background: '#fff8e1',
  border: '1px solid #e0d4a8',
  color: '#6b5d2e',
  font: '13px -apple-system, Segoe UI, sans-serif',
  boxShadow: '0 1px 4px rgba(0,0,0,0.15)',
};

/**
 * Parse `?nestingCursor` from the boot URL (P3.2; default flipped in
 * bd-9x3zbuj8). The hierarchical block navigator (BreadcrumbChip) is now the
 * DEFAULT in q2 preview, matching hub-client (which defaults the
 * `unlockNestingCursor` preference on): only an explicit `?nestingCursor=0`
 * opts OUT; an absent param, `?nestingCursor=1`, or any other value keeps it
 * on. Read-at-load; the SPA does not react to URL changes after mount.
 *
 * Safe to default on globally: the navigator self-gates on an active edit
 * target, so a read-only preview (no `--allow-edit`) never shows it.
 */
export function parseNestingCursorParam(search: string): boolean {
  try {
    return new URLSearchParams(search).get('nestingCursor') !== '0';
  } catch {
    return true;
  }
}

/**
 * Parse `?richText` from the boot URL (bd-sjb4pzx8). The rich-text editor is now
 * the DEFAULT (bd-q9lyghv2 follow-up): only an explicit `?richText=0` opts OUT;
 * an absent param, `?richText=1`, or any other value keeps it on. Read-at-load;
 * the SPA does not react to URL changes after mount.
 */
export function parseRichTextParam(search: string): boolean {
  try {
    return new URLSearchParams(search).get('richText') !== '0';
  } catch {
    return true;
  }
}

/**
 * P3.2: module-level empty object returned by the nestedEditBuffers memo
 * when nestingCursor is off. Referentially stable so the iframe effect dep
 * never churns on an off-render.
 */
const EMPTY_NESTED_BUFFERS: Record<string, string> = {};

/**
 * P3.2: Pure gating helper for the nested-edit-buffer table (SPA mirror of
 * hub-client's ReactPreview.computeNestedEditBuffers).
 *
 * Returns EMPTY_NESTED_BUFFERS (referentially stable) when:
 *   - `unlock` is false (flag is off — the WASM pass must not run), OR
 *   - `content` is absent/empty, OR
 *   - `ast` is absent/empty.
 *
 * The `regen` callback is required — pass `regenerateNestedBuffers` at
 * production call sites; pass a `vi.fn()` in tests. This avoids a
 * problematic default-parameter reference to the import (which breaks when
 * other test files mock @quarto/preview-runtime without the export).
 *
 * Exported so unit tests can call it directly, giving a genuine
 * fail-on-revert: removing `!unlock` from the guard causes the "flag off"
 * assertion to call regen and turn RED.
 */
export function computeNestedEditBuffers(
  unlock: boolean,
  content: string | undefined,
  ast: string | null | undefined,
  regen: (c: string, a: string) => Record<string, string>,
): Record<string, string> {
  if (!unlock || !content || !ast) {
    return EMPTY_NESTED_BUFFERS;
  }
  try {
    return regen(content, ast);
  } catch {
    return EMPTY_NESTED_BUFFERS;
  }
}

/**
 * Build the initial PreviewApp state. Called lazily via `useState(() => ...)`
 * so it runs at component-mount time (when `window.location.search` is already
 * set by the test harness or the real browser) rather than at module-import
 * time (when jsdom may not have applied test overrides yet).
 */
function buildInitialState(): PreviewAppState {
  return {
    boot: 'loading',
    files: [],
    activeFile: null,
    pendingAnchor: null,
    pendingAnchorEpoch: 0,
    astJson: null,
    untransformedAstJson: null,
    renderedContent: '',
    themeFingerprint: undefined,
    deps: null,
    error: null,
    render: EMPTY_RENDER_STATUS,
    serverDiagnostics: [],
    contentTick: 0,
    captures: {},
    allowEdit: false,
    nestingCursor: typeof window !== 'undefined'
      ? parseNestingCursorParam(window.location.search)
      : false,
    richText: typeof window !== 'undefined'
      ? parseRichTextParam(window.location.search)
      : false,
    bootAttempt: 1,
    bootLastError: null,
    connection: 'connected',
  };
}

/**
 * Phase D.6 (bd-kw93.12): decide whether a text-file change at
 * `changedPath` should trigger a re-render of the page named by
 * `activeFile`, given the cached dep set `deps`.
 *
 * The filter is intentionally narrow: it ONLY filters *source-file*
 * edits (`.qmd`, and `.md` since bd-6d2wj4zp Phase 5 — with hub-wide
 * extension-based sync, never-rendered `.md` like a README also watch,
 * and must not re-render the active page). Non-source files (CSS,
 * _quarto.yml, _metadata.yml, .tsx custom components, …) are
 * project-wide signals that affect rendering regardless of the active
 * page's include-shortcode set, so they always pass.
 *
 * Returns true (fail-open) when `deps` is null — the server response
 * hasn't landed yet, and we'd rather over-render than miss a change.
 * (The deps fetch seeds the set with the active page itself, so "edit
 * my own page" needs no special case here.)
 *
 * Exported for unit testing.
 */
export function shouldRerenderForTextChange(
  changedPath: string,
  activeFile: string | null,
  deps: Set<string> | null,
): boolean {
  if (!activeFile) return true;
  // Non-source edits always pass: they're either config (_quarto.yml,
  // _metadata.yml) or project-wide assets (CSS, custom components)
  // that the include-shortcode dep extractor doesn't track.
  const lower = changedPath.toLowerCase();
  if (!lower.endsWith('.qmd') && !lower.endsWith('.md')) return true;
  if (deps === null) return true;
  return deps.has(changedPath);
}

/**
 * Shape the structured render status + server-diagnostics feed
 * into props for `<PreviewDiagnosticsOverlay>` (bd-b9kzg).
 *
 * Visibility: shows whenever ANY feed has content (a failure, WASM
 * diagnostics, WASM warnings, sibling pass-1 failures, or
 * server-side diagnostics) — otherwise the overlay is hidden.
 *
 * Severity: "error" when the render carries an actual failure or
 * structured `diagnostics` (which the WASM only emits on failure);
 * "warning" for everything else (warnings-only, pass-1-only,
 * server-only).
 *
 * Error payload: when there's a failure or structured diagnostics,
 * builds the `{ message, diagnostics, pass1Failures }` shape the
 * overlay expects, merging `warnings` into the `diagnostics` array
 * so they render together. For warnings-only renders, builds a
 * payload with an empty message and warnings as the diagnostics
 * list. For server-only (no WASM-side content), returns `null` for
 * `error` — the overlay handles the server-only case via its
 * `serverDiagnostics` prop.
 */
export function computeOverlayInputs(
  render: RenderStatus,
  serverDiagnostics: Diagnostic[],
): {
  visible: boolean;
  severity: 'error' | 'warning';
  error: {
    message: string;
    diagnostics?: Diagnostic[];
    pass1Failures?: Pass1Failure[];
  } | null;
} {
  const hasFailure = render.failure !== null;
  const hasDiagnostics = render.diagnostics.length > 0;
  const hasWarnings = render.warnings.length > 0;
  const hasPass1Failures = render.pass1Failures.length > 0;
  const hasServerDiagnostics = serverDiagnostics.length > 0;

  const visible =
    hasFailure ||
    hasDiagnostics ||
    hasWarnings ||
    hasPass1Failures ||
    hasServerDiagnostics;
  if (!visible) {
    return { visible: false, severity: 'warning', error: null };
  }

  const severity: 'error' | 'warning' =
    hasFailure || hasDiagnostics ? 'error' : 'warning';

  // Server-only (no WASM-side payload at all) — return error=null
  // and rely on the overlay's serverDiagnostics-only render path.
  const hasWasmSidePayload =
    hasFailure || hasDiagnostics || hasWarnings || hasPass1Failures;
  if (!hasWasmSidePayload) {
    return { visible: true, severity, error: null };
  }

  return {
    visible: true,
    severity,
    error: {
      message: render.failure?.message ?? '',
      diagnostics: [...render.diagnostics, ...render.warnings],
      pass1Failures: render.pass1Failures,
    },
  };
}

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
 * Fetch the session's edit permission from `GET /api/preview/config`
 * (bd-ov4gqk3m). Mirrors the CLI's `--allow-edit` flag. Fails closed:
 * any network error, non-2xx status, or malformed body yields `false`
 * — a read-only preview is always the safe state, and the server
 * independently refuses disk write-back when the flag is off.
 */
async function fetchAllowEdit(loc: Location = window.location): Promise<boolean> {
  try {
    const resp = await fetch(`${loc.protocol}//${loc.host}/api/preview/config`);
    if (!resp.ok) return false;
    const body = (await resp.json()) as { allowEdit?: unknown };
    return body.allowEdit === true;
  } catch {
    return false;
  }
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


export default function PreviewApp() {
  // Lazy initializer: reads window.location.search at mount time so tests can
  // set window.location before rendering (module-level INITIAL_STATE would be
  // evaluated at import time, before the test harness applies its overrides).
  const [state, setState] = useState<PreviewAppState>(buildInitialState);

  // Force-refresh trigger (bd-b5hf): bumping `contentTick` re-fires
  // the render useEffect. Reuses the same channel `onFileContent`
  // uses for sync-driven re-renders so there's one path through the
  // render pipeline regardless of who asks. Stable identity via
  // useCallback so the button doesn't re-mount on every state
  // update.
  const handleRefresh = useCallback(() => {
    setState((s) => ({ ...s, contentTick: s.contentTick + 1 }));
  }, []);

  // Phase 4 (target-incremental-writes) + bd-ov4gqk3m: real setAst
  // handler for q2-preview node edits. Calls apply_node_edit with the
  // retained untransformedAst, splices the resulting QMD into the
  // Automerge document (which the server persists to disk — the CLI
  // ran with --allow-edit if we got here), writes the result to the
  // VFS as an optimistic local update, and bumps contentTick to
  // trigger re-render.
  //
  // Read-only sessions (no --allow-edit) never get here through the
  // UI — `editingDisabled` keeps the edit surface inert — but a stray
  // payload is dropped anyway as defense in depth.
  const handleSetAst = useCallback(
    (payload: any) => {
      const edit = payload as PreviewNodeEditPayload;
      if (!edit.__isPreviewNodeEdit) return;
      if (!state.allowEdit) return;
      if (!state.untransformedAstJson || !state.activeFile) return;
      // Use the content snapshot captured at render time so byte offsets are
      // guaranteed to match astJson (live VFS content may have drifted).
      if (!state.renderedContent) return;
      try {
        let modifiedSubtreeJson: string;
        if (edit.channel === 'text') {
          // Text channel: parse raw QMD → Pandoc JSON → apply_node_edit.
          const parseResult = parseQmdContentSync(edit.newText);
          if (!parseResult.success || !parseResult.ast) {
            console.error('parse_qmd_content failed in PreviewApp:', parseResult.error);
            return;
          }
          modifiedSubtreeJson = parseResult.ast;
        } else {
          // Subtree channel: render-component already produced Pandoc JSON.
          modifiedSubtreeJson = edit.modifiedSubtreeJson;
        }
        const newQmd = applyNodeEdit(
          state.renderedContent,
          state.untransformedAstJson,
          edit.destinationSourceInfoJson,
          modifiedSubtreeJson,
        );
        // Write the edit into the Automerge document so it syncs to the
        // server (and from there to disk). The diff base must be the
        // Automerge-side content — same reasoning as hub-client's
        // handleContentRewrite. The change echoes back through
        // onFileContent with identical text; re-render is idempotent.
        const oldContent = getFileContent(state.activeFile) ?? '';
        const changes = diffToEditorChanges(oldContent, newQmd);
        if (changes.length > 0) {
          applyEditorOperations(state.activeFile, changes);
        }
        // Optimistic local update so the re-render is immediate rather
        // than waiting for the sync round-trip.
        vfsAddFile(state.activeFile, newQmd);
        setState((s) => ({ ...s, contentTick: s.contentTick + 1 }));
      } catch (err) {
        console.error('apply_node_edit failed in PreviewApp:', err);
      }
    },
    [state.allowEdit, state.untransformedAstJson, state.activeFile, state.renderedContent],
  );

  // Phase F.1 (bd-kw93.14): the iframe posts NAVIGATE_TO_DOCUMENT
  // when the user clicks a cross-page artifact-rooted `.html` link.
  // Update activeFile + pendingAnchor and push a fresh history entry
  // so the browser's back/forward walks the in-SPA navigation.
  // History entries are keyed only on (page, anchor) — same-page
  // same-anchor clicks are no-ops to avoid duplicate stack entries.
  const handleNavigate = useCallback((path: string, anchor: string | null) => {
    setState((s) => {
      // No-op when same page + same anchor + no scroll requested
      // (would be a duplicate history push). When the anchor is
      // present, even a same-page click bumps the epoch so the
      // iframe re-scrolls (a user clicking the same anchor twice
      // is a deliberate "take me back to that section" signal).
      if (s.activeFile === path && s.pendingAnchor === anchor && anchor === null) {
        return s;
      }
      return {
        ...s,
        activeFile: path,
        pendingAnchor: anchor,
        pendingAnchorEpoch: anchor ? s.pendingAnchorEpoch + 1 : s.pendingAnchorEpoch,
      };
    });
    // Build the URL the same way `pickInitialPage` reads it back so
    // a popstate restore + a fresh-tab boot land on the same state.
    const params = new URLSearchParams();
    params.set('page', path);
    const newSearch = `?${params.toString()}`;
    const newHash = anchor ? `#${anchor}` : '';
    if (window.location.search !== newSearch || window.location.hash !== newHash) {
      const newUrl = `${window.location.pathname}${newSearch}${newHash}`;
      window.history.pushState(null, '', newUrl);
    }
  }, []);

  // Phase F.1 (bd-kw93.14): browser back/forward fires popstate; map
  // the restored URL back to (activeFile, pendingAnchor) using the
  // same parsing the boot path uses. Falls back gracefully when the
  // URL points at a page no longer in the project (e.g. file got
  // renamed mid-session) — `pickInitialPage` reverts to firstQmd.
  useEffect(() => {
    const onPopState = () => {
      setState((s) => {
        if (s.boot !== 'ready' || s.files.length === 0) return s;
        const restored = pickInitialPage(window.location.search, s.files);
        const restoredAnchor = window.location.hash
          ? window.location.hash.slice(1)
          : null;
        if (restored === s.activeFile && restoredAnchor === s.pendingAnchor && !restoredAnchor) {
          return s;
        }
        return {
          ...s,
          activeFile: restored,
          pendingAnchor: restoredAnchor,
          // Bump on any popstate that carries an anchor so the
          // iframe re-scrolls. Same logic as `handleNavigate`.
          pendingAnchorEpoch: restoredAnchor
            ? s.pendingAnchorEpoch + 1
            : s.pendingAnchorEpoch,
        };
      });
    };
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, []);

  // bd-jit6pdwq Phase 3: close the sync connection when the tab goes
  // away. A socket in CONNECTING state occupies Firefox's per-IP
  // WebSocket handshake queue browser-wide; explicitly disconnecting
  // on pagehide aborts it instead of leaving it to time out — so a
  // closing/navigating preview tab can't stall other localhost tabs.
  useEffect(() => {
    const onPageHide = () => {
      void disconnect();
    };
    window.addEventListener('pagehide', onPageHide);
    return () => window.removeEventListener('pagehide', onPageHide);
  }, []);

  // Boot once: WASM init + samod connect + initial file pick.
  useEffect(() => {
    let cancelled = false;
    // bd-jit6pdwq Phase 3: bridge between the sync client's
    // onConnectionChange callback and the supervision loop below.
    // `notifyConnection` is non-null only while the loop is waiting
    // for a drop; `releaseConnectionWait` unblocks that wait on
    // unmount so the async closure can exit.
    let notifyConnection: ((connected: boolean) => void) | null = null;
    let releaseConnectionWait: (() => void) | null = null;

    void (async () => {
      try {
        // Fail-fast probe: a dead server at boot is an immediate,
        // explicit error (there is nothing to retry toward). The doc
        // id itself is re-fetched per connect attempt below.
        await fetchIndexDocId();
        const wsUrl = deriveWsUrl();
        // bd-ov4gqk3m: learn whether this session may edit documents.
        // Fail-closed helper — never throws, defaults to read-only.
        const allowEdit = await fetchAllowEdit();

        await initWasm();

        setSyncHandlers({
          onFilesChange: (files) => {
            if (cancelled) return;
            setState((s) => ({ ...s, files }));
          },
          onFileContent: (path: string) => {
            if (cancelled) return;
            // Phase D.6 filter: read `activeFile` + `deps` via the
            // setState callback so the filter sees the *latest*
            // values (the closure was set up at boot time and would
            // otherwise capture stale state).
            setState((s) => {
              if (!shouldRerenderForTextChange(path, s.activeFile, s.deps)) {
                return s;
              }
              return { ...s, contentTick: s.contentTick + 1 };
            });
          },
          // Phase D.3 (bd-kw93.9): binary docs (images, SVGs,
          // anything not text-shaped) sync through samod on a
          // separate channel from text. Without this handler an
          // edit to e.g. `assets/logo.svg` would land in the binary
          // doc but the SPA would never re-render. Bump the same
          // `contentTick` as text changes so downstream effects
          // pick the change up uniformly.
          onBinaryContent: () => {
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
          // bd-jit6pdwq Phase 3: the supervision loop (below) listens
          // for drops on an established connection. Events fired while
          // no waiter is armed (boot phase, mid-reconnect) go nowhere
          // by design — those phases own their connection state.
          onConnectionChange: (connected) => {
            if (cancelled) return;
            notifyConnection?.(connected);
          },
          onError: (err) => {
            if (cancelled) return;
            // bd-jit6pdwq: while booting or reconnecting, connection
            // errors belong to the boot/supervision retry loops (each
            // failed connect() fires this callback too) — surfacing
            // them here would turn a retryable failure into a terminal
            // error screen. Otherwise errors surface as before.
            setState((s) =>
              s.boot === 'loading' || s.connection !== 'connected'
                ? s
                : { ...s, error: err, boot: 'error' },
            );
          },
        });

        // bd-jit6pdwq Phase 2: health-arbitrated boot. The WebSocket
        // gets patience (peerTimeoutMs: Infinity — Firefox serializes
        // WS opening handshakes per IP address browser-wide, so a hung
        // handshake in another tab can stall ours past any finite
        // budget); HTTP /health decides liveness (that queue can't
        // stall it). Memory storage: preview hubs are ephemeral, the
        // IndexedDB cache can never hit, and its open would otherwise
        // gate sending the WS join. See
        // claude-notes/research/2026-06-11-firefox-ws-handshake-serialization.md
        const healthUrl = `${window.location.protocol}//${window.location.host}/health`;
        const checkHealth = async (): Promise<boolean> => {
          try {
            const resp = await fetch(healthUrl);
            return resp.ok;
          } catch {
            return false;
          }
        };
        // Re-fetch the index doc id on EVERY attempt: q2 preview keeps
        // samod state in a per-process temp dir, so a server restart
        // (same port) serves a brand-new index document — reconnecting
        // with a stale id would retry "unavailable" forever. /health
        // is health-gate-required before any attempt, so this adds no
        // new failure mode. (Found by the Phase 5 kill/restart check;
        // `indexDocId` from the initial fetch above is only used to
        // fail fast when the server is down at boot.)
        const connectWithPreviewOptions = async () => {
          const currentDocId = await fetchIndexDocId();
          return connect(wsUrl, currentDocId, undefined, undefined, undefined, {
            peerTimeoutMs: Infinity,
            storage: 'memory',
          });
        };

        const initialFiles = await bootWithRetry({
          connect: connectWithPreviewOptions,
          checkHealth,
          onStatus: (status) => {
            if (cancelled || status.phase !== 'connecting') return;
            setState((s) => ({
              ...s,
              bootAttempt: status.attempt,
              bootLastError: status.lastError,
            }));
          },
          isCancelled: () => cancelled,
          // Server declared gone ⇒ tear the abandoned attempt's
          // adapter down so it stops retrying the dead port.
          teardown: () => disconnect(),
        });
        if (cancelled || initialFiles === null) return;

        // Phase D.2 (bd-kw93.13): seed `activeFile` from the boot
        // URL's `?page=<rel>` query if the CLI carried one through.
        // Falls back to firstQmd when missing/invalid — that's the
        // pre-D.2 Phase A behaviour preserved verbatim.
        const activeFile = pickInitialPage(
          typeof window !== 'undefined' ? window.location.search : '',
          initialFiles,
        );
        // Phase F.1 (bd-kw93.14): if the boot URL carries a hash
        // (e.g. `?page=docs/api.qmd#install`), seed pendingAnchor so
        // the first render scrolls into the named section. The iframe
        // does the scrolling once the AST is committed.
        const initialHash = (typeof window !== 'undefined' && window.location.hash)
          ? window.location.hash.slice(1)
          : '';
        setState((s) => ({
          ...s,
          files: initialFiles,
          activeFile,
          pendingAnchor: initialHash || null,
          // Boot epoch is 1 only when there's actually an anchor to
          // scroll to; staying at 0 means "no scroll has been
          // requested," which the iframe ignores.
          pendingAnchorEpoch: initialHash ? 1 : 0,
          allowEdit,
          boot: 'ready',
        }));

        // bd-jit6pdwq Phase 3: steady-state supervision. When the
        // established connection drops, tear the adapter down (stops
        // automerge-repo's retry-every-5s-forever loop and closes the
        // socket — a stale tab must not occupy Firefox's per-IP WS
        // handshake queue) and reconnect under /health arbitration:
        // alive ⇒ reconnect now; dead ⇒ banner + cheap HTTP polling
        // (1 s → 30 s cap) until it returns. Same-port restarts
        // recover without a manual reload.
        for (;;) {
          const dropped = await new Promise<boolean>((resolve) => {
            releaseConnectionWait = () => resolve(false);
            notifyConnection = (connected) => {
              if (!connected) resolve(true);
            };
          });
          notifyConnection = null;
          if (cancelled || !dropped) return;

          setState((s) => ({ ...s, connection: 'reconnecting' }));
          await disconnect();
          if (cancelled) return;

          const refreshedFiles = await superviseReconnect({
            connect: connectWithPreviewOptions,
            checkHealth,
            onStatus: (status) => {
              if (cancelled) return;
              setState((s) => ({
                ...s,
                connection:
                  status.phase === 'server-gone' ? 'server-gone' : 'reconnecting',
              }));
            },
            isCancelled: () => cancelled,
            // Gone phase must be HTTP-only: drop the abandoned
            // attempt's adapter (it would retry the dead port
            // every 5 s otherwise).
            teardown: () => disconnect(),
          });
          if (cancelled || refreshedFiles === null) return;

          setState((s) => ({
            ...s,
            connection: 'connected',
            files: refreshedFiles,
            // Content may have changed while we were offline; re-fire
            // the render effect through the usual channel.
            contentTick: s.contentTick + 1,
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
      releaseConnectionWait?.();
    };
  }, []);

  // Phase D.6 (bd-kw93.12): fetch the dep set for the active page
  // from `/api/preview/deps`. Re-fetch whenever the active file
  // itself changes (different page), or whenever its content was
  // just edited (a new include shortcode might have been added or
  // removed). The dep set is consumed by `onFileContent`'s filter
  // above; null = unknown ⇒ fail-open.
  useEffect(() => {
    if (!state.activeFile) return;
    let cancelled = false;
    const activePath = state.activeFile;
    void (async () => {
      try {
        const resp = await fetch(
          `/api/preview/deps?page=${encodeURIComponent(activePath)}`,
        );
        if (cancelled) return;
        if (!resp.ok) {
          // Fail-open: leave `deps` as null so the filter accepts
          // everything (pre-D.6 behaviour). A 400 here means the
          // server hasn't indexed the page yet; the next refetch
          // (driven by the activeFile/contentTick deps below) will
          // try again.
          console.warn(
            `deps fetch returned ${resp.status} for ${activePath}; filter falls open`,
          );
          return;
        }
        const body = (await resp.json()) as { deps?: string[] };
        if (cancelled) return;
        const list = body.deps ?? [];
        // Include the active page itself in the set so the filter
        // doesn't need a special-case for "edit my own page."
        const set = new Set<string>([activePath, ...list]);
        setState((s) =>
          s.activeFile === activePath ? { ...s, deps: set } : s,
        );
      } catch (e) {
        // Network errors etc. — fail-open, just log.
        console.warn(
          `deps fetch threw for ${activePath}: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [state.activeFile, state.contentTick]);

  // bd-b9kzg: fetch the server-side diagnostic list for the
  // active page. Same trigger as the dep fetch above (activeFile
  // change + every contentTick bump) so a fresh diagnostic from
  // the capture driver / re_execute / deps path becomes visible
  // shortly after the file change that produced it. Fail-open:
  // a fetch failure leaves `serverDiagnostics` at its prior
  // value with a console warning — losing visibility on a
  // diagnostic is annoying, but blocking the render on a
  // transient network blip would be worse.
  useEffect(() => {
    if (!state.activeFile) return;
    let cancelled = false;
    const activePath = state.activeFile;
    void (async () => {
      try {
        const resp = await fetch(
          `/api/preview/diagnostics?page=${encodeURIComponent(activePath)}`,
        );
        if (cancelled) return;
        if (!resp.ok) {
          console.warn(
            `diagnostics fetch returned ${resp.status} for ${activePath}; leaving prior list in place`,
          );
          return;
        }
        const body = (await resp.json()) as { diagnostics?: Diagnostic[] };
        if (cancelled) return;
        const list = body.diagnostics ?? [];
        setState((s) =>
          s.activeFile === activePath ? { ...s, serverDiagnostics: list } : s,
        );
      } catch (e) {
        console.warn(
          `diagnostics fetch threw for ${activePath}: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [state.activeFile, state.contentTick]);

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

        // Task 3 (block-editing): snapshot the VFS content for the
        // active file immediately before the render so the captured
        // text is byte-for-byte what the WASM renderer reads. The byte
        // offsets in the resulting astJson / untransformedAstJson
        // belong to this snapshot, not to any later live-edit content.
        const vfsSnap = vfsReadFile(state.activeFile!);
        const capturedContent = vfsSnap.success && vfsSnap.content ? vfsSnap.content : '';

        const result = await renderPageForPreview(
          state.activeFile!,
          undefined,
          captureGzJson,
        );
        if (cancelled) return;
        // Phase D.3 (bd-kw93.9) + bd-0mji: a test-only render
        // counter on `window`. Lets Playwright / SPA tests assert
        // "this edit reached the SPA and produced a (re-)render"
        // without inferring through DOM diffs. Production builds
        // include this; it's a single integer increment, no
        // measurable cost. Counts completed render attempts
        // (success or non-success result), not effect firings.
        if (typeof window !== 'undefined') {
          const w = window as unknown as { __renderTicks?: number };
          w.__renderTicks = (w.__renderTicks ?? 0) + 1;
        }
        if (result.success && result.ast_json !== undefined) {
          // Three-way themeFingerprint (mirrors hub-client's
          // `ReactPreview` mapping at line 119-125): a string means a
          // compiled theme exists; field absent means render succeeded
          // with no theme intended → explicit clear (`null`). The
          // render-failure branch below leaves the value untouched so
          // last-good styling survives transient errors.
          //
          // bd-b9kzg: a successful render still carries `warnings` and
          // `pass1_failures` — the overlay surfaces these even on
          // success, in warning mode. `failure` and `diagnostics`
          // (which only appear on failures) are cleared so the
          // previous failure overlay (if any) goes away.
          setState((s) => ({
            ...s,
            astJson: result.ast_json ?? null,
            untransformedAstJson: result.untransformed_ast_json ?? null,
            renderedContent: capturedContent,
            themeFingerprint: result.theme_fingerprint ?? null,
            render: {
              failure: null,
              diagnostics: [],
              warnings: result.warnings ?? [],
              pass1Failures: result.pass1_failures ?? [],
            },
          }));
        } else {
          // Log the full result so we can diagnose surprises in the
          // browser console without a code change.
          console.error('renderPageInProject failed', {
            path: state.activeFile,
            result,
          });
          // bd-b9kzg: route into the non-terminal `render.failure`
          // slot AND thread the structured diagnostics through. We
          // deliberately do NOT touch `boot` or `astJson` — the iframe
          // keeps showing the last-good render underneath and the
          // overlay surfaces the failure (plus its diagnostics +
          // sibling pass-1 failures) on top. Distinct from boot
          // errors, which DO replace the UI (no good render exists
          // to fall back to).
          setState((s) => ({
            ...s,
            render: {
              failure: {
                message:
                  result.error ??
                  `renderPageInProject failed: ${JSON.stringify(result)}`,
              },
              diagnostics: result.diagnostics ?? [],
              warnings: result.warnings ?? [],
              pass1Failures: result.pass1_failures ?? [],
            },
          }));
        }
      } catch (err) {
        if (cancelled) return;
        // Phase D.3: bump the render counter on the catch path too
        // so "an edit triggered a render attempt" stays a reliable
        // signal even when the render threw.
        if (typeof window !== 'undefined') {
          const w = window as unknown as { __renderTicks?: number };
          w.__renderTicks = (w.__renderTicks ?? 0) + 1;
        }
        // Same non-terminal treatment as the non-success branch
        // above. A render that throws (e.g. malformed qmd hits the
        // WASM parser) overlays on top of the previous good render.
        // The structured diagnostics arrays are empty here because
        // we never got a `RenderResponse` to read them from — only
        // the thrown error's message is available.
        const message = err instanceof Error ? err.message : String(err);
        setState((s) => ({
          ...s,
          render: {
            failure: { message },
            diagnostics: [],
            warnings: [],
            pass1Failures: [],
          },
        }));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [state.activeFile, state.contentTick, state.captures]);

  // bd-iuzmk: set the browser-tab title from the active AST's
  // `meta.title`. Runs after every successful render so a live-edited
  // title surfaces in the tab without a separate channel. Failure
  // modes (parse error, missing meta) fall back to the bare "Quarto
  // Preview" suffix — same as today's hard-coded title. No cleanup:
  // the next document switch / edit re-fires this effect; on unmount,
  // the tab is closed anyway.
  useEffect(() => {
    if (state.astJson === null) return;
    let ast: unknown = null;
    try {
      ast = JSON.parse(state.astJson);
    } catch {
      ast = null;
    }
    document.title = buildPreviewTabTitle(ast);
  }, [state.astJson]);

  // P3.2: gated nested buffer table. Delegates to computeNestedEditBuffers
  // (the exported pure helper) so the gating logic has a single testable
  // source of truth. Returns EMPTY_NESTED_BUFFERS when nestingCursor is off
  // or either render input is missing, keeping the iframe dep stable.
  const nestedEditBuffers = useMemo(
    () =>
      computeNestedEditBuffers(
        state.nestingCursor,
        state.renderedContent,
        state.untransformedAstJson,
        regenerateNestedBuffers,
      ),
    [state.nestingCursor, state.renderedContent, state.untransformedAstJson],
  );

  // ── Render ────────────────────────────────────────────────────────────

  // bd-b9kzg: derive overlay inputs from the structured render
  // status + server-diagnostics feed. Both feeds share the same
  // overlay; the overlay decides how to lay them out via `severity`
  // and `serverDiagnostics` props.
  const overlayInputs = computeOverlayInputs(state.render, state.serverDiagnostics);

  if (state.boot === 'error' && state.error) {
    return (
      <PreviewDiagnosticsOverlay
        error={{ message: state.error.message }}
        visible
        collapsed={false}
        severity="error"
      />
    );
  }

  // bd-b9kzg (extends Phase D.4 / bd-kw93.10): if the *first*
  // render failed (no good astJson exists to fall back to), show
  // the overlay terminal-style. Subsequent failures with a prior
  // good astJson take the overlay-on-top branch further down. The
  // structured diagnostics + pass1Failures + serverDiagnostics
  // flow through too — the user sees the full picture.
  if (state.astJson === null && state.render.failure) {
    return (
      <PreviewDiagnosticsOverlay
        error={overlayInputs.error}
        visible
        collapsed={false}
        severity={overlayInputs.severity}
        serverDiagnostics={state.serverDiagnostics}
      />
    );
  }

  if (state.boot === 'loading' || !state.activeFile || state.astJson === null) {
    return (
      <BootLoadingScreen
        attempt={state.bootAttempt}
        lastError={state.bootLastError}
      />
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
        // Phase F.1 (bd-kw93.14): forward project file paths +
        // pending-anchor so the iframe link handler recognises
        // artifact-rooted `.html` clicks and the iframe scrolls to
        // the cross-page anchor after committing the new AST.
        projectFilePaths={state.files.map((f) => f.path)}
        pendingAnchor={state.pendingAnchor}
        pendingAnchorEpoch={state.pendingAnchorEpoch}
        onNavigateToDocument={handleNavigate}
        setAst={handleSetAst}
        // Task 3 (block-editing): QMD source snapshot whose byte
        // offsets match astJson. Forwarded to the iframe so it can
        // slice source bytes without skew.
        renderedContent={state.renderedContent}
        // Plan 2a: pre-pipeline AST for the structural editability
        // gate. In lockstep with astJson + renderedContent.
        untransformedAstJson={state.untransformedAstJson}
        // bd-ov4gqk3m: without --allow-edit the preview is read-only —
        // no edit affordance renders inside the iframe at all.
        editingDisabled={!state.allowEdit}
        // P3.2: nesting-cursor mode + per-key nested buffers.
        unlockNestingCursor={state.nestingCursor}
        // Rich-text editor (bd-sjb4pzx8): on by default; `?richText=0` opts out.
        richText={state.richText}
        nestedEditBuffers={nestedEditBuffers}
      />
      {showStaleOverlay && (
        <StaleCaptureOverlay
          activePath={state.activeFile}
          state={activeCapture?.state}
          lastError={activeCapture?.lastError}
        />
      )}
      {/* bd-b9kzg (extends Phase D.4): non-terminal diagnostics
          overlay. The overlay defaults to its own internal
          collapsed state (true) when the `collapsed` prop is
          omitted — this lets the user click the indicator to
          expand/collapse without PreviewApp having to thread a
          piece of UI state through. Decision 2: collapsed by
          default keeps the surface quiet so warnings don't
          disrupt the rendered preview. Surfaces three feeds
          (failure + WASM diagnostics, WASM warnings, server-side
          sink). */}
      {overlayInputs.visible && (
        <PreviewDiagnosticsOverlay
          error={overlayInputs.error}
          visible
          severity={overlayInputs.severity}
          serverDiagnostics={state.serverDiagnostics}
        />
      )}
      {/* bd-jit6pdwq Phase 3: connection status banner. The rendered
          content stays up underneath — a dropped connection is not a
          render failure. server-gone recovers automatically when
          /health answers again (same-port restart). */}
      {state.connection !== 'connected' && (
        <div role="status" style={CONNECTION_BANNER_STYLE}>
          {state.connection === 'reconnecting'
            ? 'Reconnecting to the preview server…'
            : 'Preview server stopped. Restart `q2 preview` — this page reconnects automatically.'}
        </div>
      )}
      <ForceRefreshButton onRefresh={handleRefresh} />
    </div>
  );
}
