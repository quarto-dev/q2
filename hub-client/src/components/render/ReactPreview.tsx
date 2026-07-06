import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import type { CSSProperties } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import type { Diagnostic, PreviewNodeEditPayload } from '@quarto/preview-renderer/types/diagnostic';
import type { ActorIdentity, CaptureRef } from '@quarto/preview-runtime';
import {
  parseQmdToAstWithAttribution,
  renderPageInProjectWithAttribution,
  renderPageForPreview,
  isWasmReady,
  incrementalWriteQmd,
  applyNodeEdit,
  parseQmdContentSync,
  getActorId,
  regenerateNestedBuffers,
} from '@quarto/preview-runtime';
import { pipelineKindForFormat } from '@quarto/preview-runtime';
import { useAttribution } from '../../hooks/useAttribution';
import { useActiveCaptureBytes } from '../../hooks/useActiveCaptureBytes';
import { stripAnsi } from '@quarto/preview-renderer/utils/stripAnsi';
import { PreviewErrorOverlay } from '@quarto/preview-renderer/overlays/PreviewErrorOverlay';
import { usePreference } from '../../hooks/usePreference';
import { useScrollSync } from '../../hooks/useScrollSync';
import type { Q2PreviewIframeHandle } from '@quarto/preview-renderer/iframe/Q2PreviewIframe';
import ReactRenderer from './ReactRenderer';

/**
 * P3.2: Referentially stable empty object returned by the nestedEditBuffers
 * memo when unlockNestingCursor is off (or inputs are unavailable). A module-
 * level constant avoids creating a new object on every render, which would
 * churn the iframe's UPDATE_AST effect even when nothing has changed.
 */
const EMPTY_NESTED_BUFFERS: Record<string, string> = {};

/**
 * P3.2: Pure gating helper for the nested-edit-buffer table.
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

// Preview pane state machine:
// START: Initial blank page
// ERROR_AT_START: Error page shown before any successful render
// GOOD: Successfully rendered HTML preview
// ERROR_FROM_GOOD: Error occurred after previous successful render (keep last good HTML, show overlay)
type PreviewState = 'START' | 'ERROR_AT_START' | 'GOOD' | 'ERROR_FROM_GOOD';

// Error info for the overlay
interface CurrentError {
  message: string;
  diagnostics?: Diagnostic[]; // Using intelligence Diagnostic type with range/position
}

interface PreviewProps {
  content: string;
  currentFile: FileEntry | null;
  files: FileEntry[];
  fileContents: Map<string, string>;
  scrollSyncEnabled: boolean;
  editorRef: React.RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  editorReady: boolean;
  editorHasFocusRef: React.RefObject<boolean>;
  onFileChange: (file: FileEntry, anchor?: string) => void;
  onOpenNewFileDialog: (initialFilename: string) => void;
  onDiagnosticsChange: (diagnostics: Diagnostic[]) => void;
  onAstChange?: (astJson: string | null) => void;
  currentSlideIndex?: number;
  onSlideChange?: (slideIndex: number) => void;
  onContentRewrite: (content: string) => void;
  /**
   * Register a deferred scroll-to-line with the parent (Editor.tsx) for
   * replay. Scrubbing history scrolls the preview to the changed line through
   * the same render-deferred mechanism as normal q2-preview editing, so it
   * lands once against the fresh DOM instead of racing the async render.
   */
  onRegisterReplayScroll?: (fn: (line: number) => void) => void;
  format: string; // 'q2-slides', 'q2-debug', or 'q2-preview'
  /**
   * Automerge actor → display identity (name + colour). Consumed
   * by `useAttribution` to fill the `identities` half of the
   * attribution payload. Falls back to `actor.slice(0, 8)` +
   * `fnv1aHex8`-derived colour for any actor without a profile
   * entry, so the Phase 6 producer invariant always holds.
   */
  identities?: Record<string, ActorIdentity>;
  /**
   * Path → recorded engine capture sidecar entry (bd-sfet3264). The
   * active document's entry (if any) points at a capture binary doc;
   * ReactPreview fetches its gzipped `EngineCapture[]` bytes and threads
   * them into the render so executed engine output is spliced into the
   * AST. Absent/empty ⇒ source-only rendering (today's behaviour).
   */
  captures?: Record<string, CaptureRef>;
  /**
   * Attribution overlay on/off. Session-only, owned by `Editor.tsx`
   * and driven by the toggle in the replay bar. When false,
   * `useAttribution` short-circuits and the WASM call falls through
   * to the byte-identical no-attribution path.
   */
  attributionOn: boolean;
  /**
   * Reports whether `useAttribution` is mid-build. The Attribution
   * pill animates its border while true so a long run-list build on
   * a large document gives visible feedback that work is happening.
   * Called once on mount with the current value and once on unmount
   * with `false` so the parent state never gets stuck.
   */
  onAttributionGeneratingChange?: (generating: boolean) => void;
}

// Result of rendering QMD content to AST
type RenderResult = {
  success: true;
  astJson: string;
  /** Pre-pipeline (untransformed) AST JSON for apply_node_edit (Phase 1). */
  untransformedAstJson?: string;
  diagnostics: Diagnostic[];
  /**
   * Three-way theme fingerprint (Plan 2A item 11).
   *  - `string`: render succeeded with a theme; the parent posts
   *    `UPDATE_THEME` with this fingerprint.
   *  - `null`: render succeeded with no theme (e.g. user removed
   *    `theme:` YAML key); the parent posts an explicit clear.
   *  - field-omitted (`undefined` after destructuring): the upstream
   *    pipeline did not surface a theme — same effect as `null`.
   *
   * The render-failure case omits this field entirely; the
   * ReactPreview state then preserves last-good fingerprint across
   * transient errors (see `setThemeFingerprint` call site).
   */
  themeFingerprint?: string | null;
} | {
  success: false;
  error: string;
  diagnostics: Diagnostic[];
}

// Render QMD content to AST JSON for the iframe-based preview.
//
// Dispatches on `pipelineKindForFormat(format)`:
// - `'preview'` (q2-preview): calls
//   `renderPageInProjectWithAttribution(documentPath, undefined, attributionJson)`,
//   which runs the full q2-preview pipeline in WASM (shortcodes, Lua
//   filters, sectionize, crossref, sidebar/navbar metadata, etc.) and
//   returns the post-pipeline AST as JSON via `RenderResponse.ast_json`.
//   Requires a `documentPath` because the pipeline reads the file from
//   VFS and discovers project context from it.
// - any other format (q2-debug, q2-slides): calls
//   `parseQmdToAstWithAttribution(content, attributionJson)`, which is
//   path-less and skips the transform pipeline entirely.
//
// In both branches, when `attributionJson` is non-null the Rust pipeline
// installs `PreBuiltAttributionProvider` on the active-page ctx, runs
// `AttributionGenerateTransform` + `AttributionRenderTransform`, and
// the resulting AST carries `astContext.attribution` plus
// `astContext.attributionActors`. When `null`, the call is
// byte-identical to the unflagged path (Phase 0 test #10 for q2-debug;
// the no-provider baseline in
// `render_qmd_to_preview_ast_surfaces_attribution_when_provider_installed`
// for q2-preview).
//
// Returns diagnostics and an AST JSON string, or an error message.
async function doRender(
  qmdContent: string,
  options: {
    scrollSyncEnabled: boolean;
    documentPath?: string;
    format: string;
    attributionJson: string | null;
    // bd-sfet3264: gzipped-JSON `EngineCapture[]` for the active document
    // (fetched from the capture binary doc). When present, the q2-preview
    // pipeline splices the recorded engine output into the AST. `undefined`
    // renders code cells as source.
    captureGzJson?: Uint8Array;
  }
): Promise<RenderResult> {
  if (!isWasmReady()) {
    return {
      success: false,
      error: 'WASM renderer not ready',
      diagnostics: [],
    };
  }

  // Convergence (bd-vwp4y5ku): `format: revealjs` renders through the
  // q2-preview pipeline too — via `renderPageForPreview`, the same WASM
  // entry the `q2 preview` SPA uses. That entry applies `quarto preview`'s
  // format substitution (`revealjs` → the `q2-slides` pseudo-format), so
  // the full transform pipeline runs (incl. `CompileThemeCssStage`) and
  // the returned AST carries the document's compiled reveal theme +
  // `theme_fingerprint`. Without it, revealjs fell through to the
  // parse-only path below — a raw AST the hand-rolled deck rendered with
  // reveal's stock `white.css` (uppercase headings, centered content).
  const isSlidesPreview = options.format === 'revealjs';

  if (isSlidesPreview || pipelineKindForFormat(options.format) === 'preview') {
    if (!options.documentPath) {
      return {
        success: false,
        error: 'preview render requires a document path (reads the file from VFS)',
        diagnostics: [],
      };
    }

    // q2-preview threads author attribution through
    // `renderPageInProjectWithAttribution` (`attributionJson` is produced
    // by `useAttribution`; `null` is byte-identical to the no-attribution
    // path). The revealjs slides path uses `renderPageForPreview`, which
    // performs the preview format substitution but does not yet thread
    // attribution — slides have no attribution overlay today (follow-up).
    const result = isSlidesPreview
      ? await renderPageForPreview(options.documentPath, undefined, options.captureGzJson)
      : await renderPageInProjectWithAttribution(
          options.documentPath,
          undefined,
          options.attributionJson,
          options.captureGzJson,
        );
    const allDiagnostics: Diagnostic[] = [
      ...(result.diagnostics ?? []),
      ...(result.warnings ?? []),
    ];

    if (result.success) {
      const astJson = result.ast_json;
      if (astJson === undefined) {
        return {
          success: false,
          error: 'preview render succeeded but produced no ast_json — backend bug',
          diagnostics: allDiagnostics,
        };
      }
      // Three-way themeFingerprint mapping:
      //   - present string ⇒ theme present, pass through
      //   - field absent on RenderResponse ⇒ render succeeded with no
      //     theme intended ⇒ explicit clear (`null`)
      // The render-failure branch below omits the field entirely so
      // last-good fingerprint state is preserved.
      const themeFingerprint = result.theme_fingerprint ?? null;
      return {
        success: true,
        astJson,
        untransformedAstJson: result.untransformed_ast_json,
        diagnostics: allDiagnostics,
        themeFingerprint,
      };
    } else {
      const errorMsg =
        typeof result.error === 'string'
          ? result.error
          : JSON.stringify(result.error, null, 2) || 'Unknown error';
      return {
        success: false,
        diagnostics: allDiagnostics,
        error: errorMsg,
      };
    }
  }

  // q2-debug / q2-slides path: parse-only, no transform pipeline.
  // Attribution provider is installed when attributionJson is non-null.
  const result = await parseQmdToAstWithAttribution(
    qmdContent,
    options.attributionJson,
  );

  // Collect all diagnostics from both success and error paths
  const allDiagnostics: Diagnostic[] = [
    ...(result.diagnostics ?? []),
    ...(result.warnings ?? []),
  ];

  if (result.success) {
    return {
      success: true,
      astJson: result.ast,
      diagnostics: allDiagnostics,
    };
  } else {
    const errorMsg =
      typeof result.error === 'string'
        ? result.error
        : JSON.stringify(result.error, null, 2) || 'Unknown error';

    return {
      success: false,
      diagnostics: allDiagnostics,
      error: errorMsg,
    };
  }
}

// Exported for unit test. SPURIOUS = the edit round-tripped to identical QMD.
export function classifyCommitOutcome(
  newQmd: string,
  renderedContent: string,
): 'change' | 'spurious' {
  return newQmd === renderedContent ? 'spurious' : 'change';
}

/**
 * G22: commit-status indicator — a translucent "glass" status dot in the
 * preview's bottom-right corner that glows out of the page and fades to nothing.
 *
 *   pending  → amber  (commit going out)
 *   change   → green  (real diff written)
 *   spurious → blue   (no-op round-trip — the false-dirty bug class)
 *   error    → (not lit here; the error pill takes over the corner)
 *
 * Modern / non-skeuomorphic: no bezel, no fixture, no persistence. The dot is
 * defined by LIGHT not by a housing — a colored radial bloom, a thin colored
 * glass rim, and a `backdrop-filter` refraction — so it reads on a white page
 * now and on a dark background later. Idle → opacity 0 (gone). Lit states bloom
 * in (scale + fade) and dim back out; only the attention state (blue) pulses.
 */
const COMMIT_BULB_CSS = `
@keyframes q2-bulb-pulse {
  0%, 100% { opacity: 0.84; }
  50% { opacity: 1; }
}
.q2-commit-bulb {
  position: absolute;
  /* Sits in the bottom-right corner where the error pill appears, so on error
     the pill visually replaces the bulb. */
  bottom: 20px;
  right: 20px;
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: radial-gradient(circle at 50% 45%, var(--c-bright) 0%, var(--c-soft) 58%, transparent 100%);
  border: 1px solid var(--c-rim);
  backdrop-filter: blur(3px) saturate(1.4);
  -webkit-backdrop-filter: blur(3px) saturate(1.4);
  box-shadow: 0 0 var(--c-glow-blur) var(--c-glow-spread) var(--c-glow);
  opacity: var(--bulb-opacity);
  transform: scale(var(--bulb-scale));
  transition:
    opacity 300ms ease,
    box-shadow 220ms ease,
    background 220ms ease,
    border-color 220ms ease,
    transform 300ms cubic-bezier(0.2, 0.7, 0.3, 1.3);
  pointer-events: none;
  z-index: 50;
  will-change: opacity, transform;
}
/* Gentle, slow breath — only the attention state (blue) pulses. */
.q2-commit-bulb[data-pulse="1"] { animation: q2-bulb-pulse 1.5s ease-in-out infinite; }
`;

export function CommitStatusBulb({
  status,
}: {
  status: 'idle' | 'pending' | 'change' | 'spurious' | 'error';
}) {
  // RGB triples + per-state "loudness" — green (a normal change) is quiet; blue
  // (spurious) glows harder, pulses, and lingers so it pulls the eye. Derived
  // from one hue so it carries to light and dark backgrounds.
  //
  // 'error' is intentionally NOT lit here: on error the bulb hands off to the
  // error pill (PreviewErrorOverlay), which appears in the same corner — see the
  // render below where the bulb is suppressed while a commit error is showing.
  const PALETTE: Record<
    'pending' | 'change' | 'spurious',
    { rgb: string; glowA: number; blur: string; spread: string; pulse: boolean; title: string }
  > = {
    pending: { rgb: '255,160,30', glowA: 0.42, blur: '9px', spread: '0px', pulse: false, title: 'Committing…' },
    change: { rgb: '40,200,100', glowA: 0.36, blur: '8px', spread: '0px', pulse: false, title: 'Change committed' },
    spurious: { rgb: '60,140,255', glowA: 0.6, blur: '13px', spread: '1px', pulse: true, title: 'No change (spurious edit)' },
  };
  const on = status === 'pending' || status === 'change' || status === 'spurious';
  const p = on ? PALETTE[status] : null;
  const rgb = p?.rgb ?? '128,128,128';
  return (
    <>
      <style>{COMMIT_BULB_CSS}</style>
      <div
        className="q2-commit-bulb"
        data-pulse={p?.pulse ? '1' : undefined}
        title={p?.title}
        style={
          {
            '--c-bright': `rgba(${rgb},0.92)`,
            '--c-soft': `rgba(${rgb},0.30)`,
            '--c-rim': `rgba(${rgb},0.55)`,
            '--c-glow': `rgba(${rgb},${p?.glowA ?? 0})`,
            '--c-glow-blur': p?.blur ?? '0px',
            '--c-glow-spread': p?.spread ?? '0px',
            // No persistence: idle fades to nothing and shrinks slightly so lit
            // states bloom back in.
            '--bulb-opacity': on ? 1 : 0,
            '--bulb-scale': on ? 1 : 0.55,
          } as CSSProperties
        }
      />
    </>
  );
}

export default function ReactPreview({
  content,
  currentFile,
  files,
  fileContents,
  scrollSyncEnabled,
  editorRef,
  editorReady,
  editorHasFocusRef,
  onFileChange,
  onOpenNewFileDialog,
  onDiagnosticsChange,
  onAstChange,
  currentSlideIndex,
  onSlideChange,
  onContentRewrite,
  onRegisterReplayScroll,
  format,
  identities,
  captures,
  attributionOn,
  onAttributionGeneratingChange,
}: PreviewProps) {
  // Preview state machine for error handling
  const [previewState, setPreviewState] = useState<PreviewState>('START');
  const [currentError, setCurrentError] = useState<CurrentError | null>(null);
  // Persist the error-overlay collapsed state in localStorage. The
  // overlay itself is package-internal in @quarto/preview-renderer and
  // takes the value via props (controlled component).
  const [errorOverlayCollapsed, setErrorOverlayCollapsed] = usePreference('errorOverlayCollapsed');
  // P3.2: nesting-cursor mode preference (default-ON). When true, the
  // nestedEditBuffers memo below calls regenerateNestedBuffers to produce
  // a per-siKey clean QMD buffer table; when off, that WASM pass is
  // completely unreachable.
  const [unlockNestingCursor] = usePreference('unlockNestingCursor');
  // bd-j1nto6eq: rich-text editor preference (default-ON), forwarded to the
  // q2-preview iframe so editable paragraphs/headings open the tiptap editor
  // instead of the monospaced textarea.
  const [richText] = usePreference('richText');
  // Track previewState in a ref for use in callbacks
  const previewStateRef = useRef<PreviewState>('START');
  useEffect(() => {
    previewStateRef.current = previewState;
  }, [previewState]);

  // Compound render-result state: AST JSON, pre-pipeline AST, and the
  // QMD content that was used to produce this render generation.
  // Keeping them in one object guarantees they are always in sync —
  // the byte offsets in astJson / untransformedAstJson belong to
  // renderedContent and nowhere else.
  const [rendered, setRendered] = useState<{
    astJson: string | null;
    untransformedAstJson: string | null;
    renderedContent: string;
  }>({ astJson: null, untransformedAstJson: null, renderedContent: '' });

  // P3.2: gated nested buffer table. Delegates to computeNestedEditBuffers
  // (the exported pure helper) so the gating logic has a single testable
  // source of truth. Returns EMPTY_NESTED_BUFFERS (a module-level constant)
  // when unlockNestingCursor is off or either render input is missing, keeping
  // the iframe effect dep referentially stable across renders.
  const nestedEditBuffers = useMemo(
    () =>
      computeNestedEditBuffers(
        unlockNestingCursor ?? true,
        rendered.renderedContent,
        rendered.untransformedAstJson,
        regenerateNestedBuffers,
      ),
    [unlockNestingCursor, rendered.renderedContent, rendered.untransformedAstJson],
  );

  // Scroll sync (editor ↔ preview). Mirrors `Preview.tsx`: the q2-preview
  // iframe exposes `scrollToLine` / `getScrollRatio` via this handle, and
  // forwards its own scroll/click events through `handlePreviewScroll` /
  // `handlePreviewClick`. Only the q2-preview format wires this; other
  // React formats (q2-debug, slides) leave the handle unattached.
  const previewScrollRef = useRef<Q2PreviewIframeHandle>(null);

  const { handlePreviewScroll, handlePreviewClick, handleAstRendered, scrollToLineDeferred } = useScrollSync({
    editorRef,
    scrollPreviewToLine: (line: number) => {
      previewScrollRef.current?.scrollToLine(line);
    },
    getPreviewScrollRatio: () => {
      return previewScrollRef.current?.getScrollRatio() ?? null;
    },
    enabled: scrollSyncEnabled && editorReady,
    editorHasFocusRef,
    deferToRender: true,
  });

  // Register the deferred scroll-to-line with the parent (Editor.tsx) so
  // replay scrubbing scrolls the preview to the changed line through the same
  // render-deferred mechanism as normal q2-preview editing.
  useEffect(() => {
    onRegisterReplayScroll?.((line: number) => {
      scrollToLineDeferred(line);
    });
  }, [onRegisterReplayScroll, scrollToLineDeferred]);

  // Three-way theme fingerprint (Plan 2A item 11):
  //   `undefined` → pre-first-render or render failed; iframe keeps
  //                 last-good styling.
  //   `null`      → render succeeded with no theme intended; iframe
  //                 clears its `<link data-q2-theme>`.
  //   string      → render succeeded with a theme of this fingerprint.
  // Render failures intentionally do not call `setThemeFingerprint`,
  // preserving the last-good value across transient errors so that
  // editing in the YAML doesn't strip Bootstrap mid-edit.
  const [themeFingerprint, setThemeFingerprint] = useState<
    string | null | undefined
  >(undefined);

  // Phase 5 — q2-debug attribution producer wiring.
  //
  // `useAttribution` returns the JSON payload (`{ runs, identities }`)
  // for `parseQmdToAstWithAttribution`. The hook short-circuits when
  // `enabled` is false (Attribution toggle off), in which case the
  // payload stays `null` and the WASM call falls through to the
  // byte-identical no-attribution path.
  //
  // `enabled` is driven by the session-only `attributionOn` prop owned
  // by `Editor.tsx`, surfaced as the Attribution toggle in the replay
  // bar. `identities` is the Automerge actor → display-name/colour
  // table threaded down from `Editor.tsx`; missing entries fall back
  // to the hook's `(actor.slice(0, 8), actorColor(fnv1aHex8(actor)))`
  // so the Phase 6 producer invariant always holds.
  const { payload: attributionPayload, generating: attributionGenerating } =
    useAttribution({
      enabled: attributionOn,
      filePath: currentFile?.path ?? null,
      sourceText: content,
      identities: identities ?? {},
    });

  // Surface the hook's generating flag to the Attribution pill in the
  // replay bar. Cleanup emits `false` on unmount so switching to a
  // non-q2 file (where ReactPreview tears down) clears the indicator.
  useEffect(() => {
    onAttributionGeneratingChange?.(attributionGenerating);
    return () => onAttributionGeneratingChange?.(false);
  }, [attributionGenerating, onAttributionGeneratingChange]);

  // bd-sfet3264 (Phase 1D): recorded engine capture for the active document.
  //
  // The `captures` sidecar (threaded down from App.tsx) maps each path to a
  // CaptureRef pointing at a capture binary doc. `useActiveCaptureBytes` fetches
  // the active file's capture bytes so `doRender` can splice the recorded engine
  // output into the AST; a freshly-arrived capture updates `captureBytes`, a
  // render input below, so the preview re-renders to show executed output. The
  // same hook feeds the default `format: html` renderer (`Preview`).
  const captureBytes = useActiveCaptureBytes(captures, currentFile?.path);

  // Debounce rendering
  const renderTimeoutRef = useRef<number | null>(null);
  const lastContentRef = useRef<string>('');

  // G22: commit-status indicator. Every commit channel (text / subtree /
  // nesting) funnels through handleSetAst, so we classify the outcome there and
  // drive ONE bulb overlay from a single status state — no per-site plumbing.
  const [commitStatus, setCommitStatus] = useState<
    'idle' | 'pending' | 'change' | 'spurious' | 'error'
  >('idle');
  const commitStatusTimerRef = useRef<number | null>(null);
  const commitPendingSinceRef = useRef<number>(0);
  // G22: the message for a rejected commit, surfaced in the existing
  // PreviewErrorOverlay. Persists until the next SUCCESSFUL commit.
  const [commitError, setCommitError] = useState<string | null>(null);

  // Light the bulb amber when a commit goes out.
  const beginCommitStatus = useCallback(() => {
    if (commitStatusTimerRef.current !== null) clearTimeout(commitStatusTimerRef.current);
    commitPendingSinceRef.current = Date.now();
    setCommitStatus('pending');
  }, []);

  // applyNodeEdit is synchronous, so hold 'pending' a minimum so it is visible,
  // then flip to the result colour, then auto-clear to idle.
  const settleCommitStatus = useCallback(
    (result: 'change' | 'spurious' | 'error') => {
      const MIN_PENDING_MS = 200;
      // Loudness via dwell time: a normal change blinks briefly; a spurious /
      // rejected commit lingers longer so it draws the eye.
      const HOLD_MS = { change: 450, spurious: 1100, error: 1700 }[result];
      const elapsed = Date.now() - commitPendingSinceRef.current;
      const delay = Math.max(0, MIN_PENDING_MS - elapsed);
      if (commitStatusTimerRef.current !== null) clearTimeout(commitStatusTimerRef.current);
      commitStatusTimerRef.current = window.setTimeout(() => {
        setCommitStatus(result);
        commitStatusTimerRef.current = window.setTimeout(() => {
          setCommitStatus('idle');
          commitStatusTimerRef.current = null;
        }, HOLD_MS);
      }, delay);
    },
    [],
  );

  useEffect(() => () => {
    if (commitStatusTimerRef.current !== null) clearTimeout(commitStatusTimerRef.current);
  }, []);

  // Handler for cross-document navigation
  const handleNavigateToDocument = useCallback(
    (targetPath: string, anchor: string | null) => {
      const file = files.find(
        (f) => f.path === targetPath || '/' + f.path === targetPath
      );

      if (file) {
        // Existing file - switch to it
        onFileChange(file, anchor ?? undefined);
      } else {
        // Non-existent file - open create dialog with pre-filled name
        // Strip leading slash for the dialog
        const filename = targetPath.startsWith('/') ? targetPath.slice(1) : targetPath;
        onOpenNewFileDialog(filename);
      }
    },
    [files, onFileChange, onOpenNewFileDialog]
  );

  // Render function that uses WASM when available
  // Implements state machine transitions for error handling:
  // - On success: always transition to GOOD, swap to new content
  // - On error from START/ERROR_AT_START: show full error page
  // - On error from GOOD/ERROR_FROM_GOOD: keep last good AST, show overlay
  const doRenderWithStateManagement = useCallback(async (qmdContent: string, documentPath?: string) => {
    lastContentRef.current = qmdContent;

    const result = await doRender(qmdContent, {
      scrollSyncEnabled,
      documentPath,
      format,
      attributionJson: attributionPayload,
      captureGzJson: captureBytes,
    });
    if (qmdContent !== lastContentRef.current) return;

    // Update diagnostics
    onDiagnosticsChange(result.diagnostics);
    setCurrentError(result.success ? null : {
      message: result.error!,
      diagnostics: result.diagnostics,
    });

    if (result.success) {
      // Success: transition to GOOD state from any state
      setPreviewState('GOOD');
      // Update compound render-result state: AST, pre-pipeline AST, and
      // the QMD content snapshot whose byte offsets match these ASTs.
      setRendered({
        astJson: result.astJson,
        untransformedAstJson: result.untransformedAstJson ?? null,
        renderedContent: qmdContent,
      });
      // Apply theme fingerprint if the render produced one. Only the
      // success branch calls this — render failures preserve the
      // last-good fingerprint across transient errors (Plan 2A item
      // 11 three-way semantics).
      if (result.themeFingerprint !== undefined) {
        setThemeFingerprint(result.themeFingerprint);
      }
      // Notify parent of AST change
      onAstChange?.(result.astJson);
    } else {
      // Set current error for overlay
      const currentState = previewStateRef.current;
      if (currentState === 'START' || currentState === 'ERROR_AT_START') {
        // No good render yet - show full error page
        setPreviewState('ERROR_AT_START');
        // setAst(''); // Clear AST on error
        onAstChange?.(null);
      } else {
        // Was GOOD or ERROR_FROM_GOOD - keep last good AST, show overlay
        // DON'T update AST content
        setPreviewState('ERROR_FROM_GOOD');
      }
    }
  }, [scrollSyncEnabled, onDiagnosticsChange, onAstChange, format, attributionPayload, captureBytes]);

  // Immediate render update (no debounce)
  const updatePreview = useCallback((newContent: string, documentPath?: string) => {
    if (renderTimeoutRef.current) {
      clearTimeout(renderTimeoutRef.current);
    }
    doRenderWithStateManagement(newContent, documentPath);
  }, [doRenderWithStateManagement]);

  // Re-render when the active page's content changes, a sibling file's
  // content changes, scroll sync is toggled, or a new attribution payload
  // arrives.
  //
  // `fileContents` (bd-4jjckvwt) gets a fresh Map identity on every
  // Automerge edit (App.tsx), so depending on it makes a sibling edit —
  // most importantly `_brand.yml` / `_quarto.yml` / `_metadata.yml`
  // arriving via sync — re-render the active document. Without it, a
  // brand/theme change underneath the deck never recompiled the theme CSS
  // until a manual reload. This matches the HTML path's effect in
  // `Preview.tsx` (Phase 9 Decision 6). `updatePreview` here is
  // debounce-free, but `doRenderWithStateManagement`'s `lastContentRef`
  // guard discards stale in-flight results, so a burst settles on the
  // latest content.
  useEffect(() => {
    // Pass document path as-is from Automerge (e.g., "index.qmd" or "docs/index.qmd").
    updatePreview(content, currentFile?.path);
  }, [
    content,
    fileContents,
    updatePreview,
    scrollSyncEnabled,
    currentFile?.path,
    onDiagnosticsChange,
    attributionPayload,
    captureBytes,
  ]);

  // Reset preview state when file changes
  useEffect(() => {
    setPreviewState('START');
    setCurrentError(null);
  }, [currentFile?.path]);

  // Handler for AST modifications - converts AST back to QMD and updates content.
  //
  // For q2-preview: expects a PreviewNodeEditPayload carrying the
  // destination SourceInfo value and modified subtree; routes through
  // apply_node_edit using the retained untransformedAst (Phase 4).
  //
  // For other formats (q2-debug, q2-slides): expects a full PandocAST
  // and calls incrementalWriteQmd as before.
  const handleSetAst = useCallback(
    (newAst: any) => {
      if (pipelineKindForFormat(format) === 'preview') {
        const edit = newAst as PreviewNodeEditPayload;
        if (!edit.__isPreviewNodeEdit) {
          console.warn(
            'q2-preview setAst: expected PreviewNodeEditPayload; got',
            newAst,
          );
          return;
        }
        if (!rendered.untransformedAstJson) {
          console.warn(
            'q2-preview setAst: no untransformedAstJson retained; render first',
          );
          return;
        }
        // G22: a commit is going out — light the bulb amber.
        beginCommitStatus();
        try {
          let modifiedSubtreeJson: string;
          if (edit.channel === 'text') {
            // Text channel: parse raw QMD → Pandoc JSON → apply_node_edit.
            const parseResult = parseQmdContentSync(edit.newText);
            if (!parseResult.success || !parseResult.ast) {
              console.error('parse_qmd_content failed:', parseResult.error);
              settleCommitStatus('error'); // G22: parse rejected
              setCommitError(`Edit could not be parsed: ${parseResult.error ?? 'unknown error'}`);
              return;
            }
            modifiedSubtreeJson = parseResult.ast;
          } else {
            // Subtree channel: render-component already produced Pandoc JSON.
            modifiedSubtreeJson = edit.modifiedSubtreeJson;
          }
          // Use rendered.renderedContent so byte offsets match the AST's
          // generation, not the live editor content (which may have changed).
          const newQmd = applyNodeEdit(
            rendered.renderedContent,
            rendered.untransformedAstJson,
            edit.destinationSourceInfoJson,
            modifiedSubtreeJson,
          );
          // G22: a commit that round-trips to identical QMD is a SPURIOUS edit
          // (the false-dirty bug class) — surface it instead of silently writing.
          settleCommitStatus(classifyCommitOutcome(newQmd, rendered.renderedContent));
          setCommitError(null); // G22: a successful commit clears the prior error
          onContentRewrite(newQmd);
        } catch (err) {
          console.error('apply_node_edit failed:', err);
          settleCommitStatus('error'); // G22: apply rejected
          setCommitError(`Edit could not be applied: ${err instanceof Error ? err.message : String(err)}`);
        }
        return;
      }
      try {
        const newQmd = incrementalWriteQmd(content, newAst);
        onContentRewrite(newQmd);
      } catch (err) {
        console.error('Failed to write AST back to QMD:', err);
      }
    },
    [content, onContentRewrite, format, rendered.untransformedAstJson, rendered.renderedContent, beginCommitStatus, settleCommitStatus],
  );

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', position: 'relative' }}>
      {/* Bulb lives in the same bottom-right corner as the error pill; while a
          commit error is showing, the pill replaces the bulb (bulb → idle). */}
      <CommitStatusBulb status={commitError ? 'idle' : commitStatus} />
      <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
        {rendered.astJson && (previewState === 'GOOD' || previewState === 'ERROR_FROM_GOOD') ? (
          <ReactRenderer
            astJson={rendered.astJson}
            currentFilePath={currentFile?.path ?? ''}
            files={files}
            fileContents={fileContents}
            onNavigateToDocument={handleNavigateToDocument}
            setAst={handleSetAst}
            currentSlideIndex={currentSlideIndex}
            onSlideChange={onSlideChange}
            format={format}
            themeFingerprint={themeFingerprint}
            renderedContent={rendered.renderedContent}
            untransformedAstJson={rendered.untransformedAstJson}
            currentActor={getActorId()}
            unlockNestingCursor={unlockNestingCursor}
            richText={richText}
            nestedEditBuffers={nestedEditBuffers}
            scrollHandleRef={previewScrollRef}
            onPreviewScroll={handlePreviewScroll}
            onPreviewClick={handlePreviewClick}
            onAstRendered={handleAstRendered}
          />
        ) : previewState === 'ERROR_AT_START' && currentError ? (
          <div style={{ padding: '20px', color: 'red' }}>
            <strong>Render Error</strong>
            <pre style={{ marginTop: '10px', whiteSpace: 'pre-wrap' }}>
              {stripAnsi(currentError.message)}
            </pre>
          </div>
        ) : (
          <div style={{ padding: '20px', color: '#666' }}>
            Loading preview...
          </div>
        )}
      </div>
      {/* Error overlay shown when error occurs after successful render */}
      <PreviewErrorOverlay
        error={currentError ?? (commitError ? { message: commitError } : null)}
        visible={previewState === 'ERROR_FROM_GOOD' || commitError != null}
        collapsed={errorOverlayCollapsed}
        onToggleCollapsed={setErrorOverlayCollapsed}
      />
    </div>
  );
}
