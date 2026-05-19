import { useState, useCallback, useRef, useEffect } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import type { ActorIdentity } from '@quarto/preview-runtime';
import {
  parseQmdToAstWithAttribution,
  renderPageInProjectWithAttribution,
  isWasmReady,
  incrementalWriteQmd,
} from '@quarto/preview-runtime';
import { pipelineKindForFormat } from '../../utils/pipelineKind';
import { useAttribution } from '../../hooks/useAttribution';
import { stripAnsi } from '@quarto/preview-renderer/utils/stripAnsi';
import { PreviewErrorOverlay } from '@quarto/preview-renderer/overlays/PreviewErrorOverlay';
import { usePreference } from '../../hooks/usePreference';
import ReactRenderer from './ReactRenderer';

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
   * Authorship overlay on/off. Session-only, owned by `Editor.tsx`
   * and driven by the toggle in the replay bar. When false,
   * `useAttribution` short-circuits and the WASM call falls through
   * to the byte-identical no-attribution path.
   */
  authorshipOn: boolean;
  /**
   * Reports whether `useAttribution` is mid-build. The Authorship
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
  }
): Promise<RenderResult> {
  if (!isWasmReady()) {
    return {
      success: false,
      error: 'WASM renderer not ready',
      diagnostics: [],
    };
  }

  if (pipelineKindForFormat(options.format) === 'preview') {
    if (!options.documentPath) {
      return {
        success: false,
        error: 'q2-preview requires a document path (renderPageInProject reads from VFS)',
        diagnostics: [],
      };
    }

    // Phase 3 — q2-preview attribution wiring. `attributionJson` is
    // produced by `useAttribution` and threaded through `doRender`'s
    // options exactly as for q2-debug (line 175 below). When it's
    // `null`, the WASM call falls through to the byte-identical
    // no-attribution path; otherwise the active-page ctx receives a
    // `PreBuiltAttributionProvider` and the resulting AST JSON
    // carries `astContext.attribution*` for `<Ast>` to surface as
    // per-author backgrounds and tooltips.
    const result = await renderPageInProjectWithAttribution(
      options.documentPath,
      undefined,
      options.attributionJson,
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
          error: 'q2-preview render succeeded but produced no ast_json — backend bug',
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

export default function ReactPreview({
  content,
  currentFile,
  files,
  fileContents,
  scrollSyncEnabled,
  onFileChange,
  onOpenNewFileDialog,
  onDiagnosticsChange,
  onAstChange,
  currentSlideIndex,
  onSlideChange,
  onContentRewrite,
  format,
  identities,
  authorshipOn,
  onAttributionGeneratingChange,
}: PreviewProps) {
  // Preview state machine for error handling
  const [previewState, setPreviewState] = useState<PreviewState>('START');
  const [currentError, setCurrentError] = useState<CurrentError | null>(null);
  // Persist the error-overlay collapsed state in localStorage. The
  // overlay itself is package-internal in @quarto/preview-renderer and
  // takes the value via props (controlled component).
  const [errorOverlayCollapsed, setErrorOverlayCollapsed] = usePreference('errorOverlayCollapsed');
  // Track previewState in a ref for use in callbacks
  const previewStateRef = useRef<PreviewState>('START');
  useEffect(() => {
    previewStateRef.current = previewState;
  }, [previewState]);

  // Rendered AST JSON to display
  const [ast, setAst] = useState<string>('');

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
  // `enabled` is false (Authorship toggle off), in which case the
  // payload stays `null` and the WASM call falls through to the
  // byte-identical no-attribution path.
  //
  // `enabled` is driven by the session-only `authorshipOn` prop owned
  // by `Editor.tsx`, surfaced as the Authorship toggle in the replay
  // bar. `identities` is the Automerge actor → display-name/colour
  // table threaded down from `Editor.tsx`; missing entries fall back
  // to the hook's `(actor.slice(0, 8), actorColor(fnv1aHex8(actor)))`
  // so the Phase 6 producer invariant always holds.
  const { payload: attributionPayload, generating: attributionGenerating } =
    useAttribution({
      enabled: authorshipOn,
      filePath: currentFile?.path ?? null,
      sourceText: content,
      identities: identities ?? {},
    });

  // Surface the hook's generating flag to the Authorship pill in the
  // replay bar. Cleanup emits `false` on unmount so switching to a
  // non-q2 file (where ReactPreview tears down) clears the indicator.
  useEffect(() => {
    onAttributionGeneratingChange?.(attributionGenerating);
    return () => onAttributionGeneratingChange?.(false);
  }, [attributionGenerating, onAttributionGeneratingChange]);

  // Debounce rendering
  const renderTimeoutRef = useRef<number | null>(null);
  const lastContentRef = useRef<string>('');

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
      // Update rendered AST
      setAst(result.astJson);
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
  }, [scrollSyncEnabled, onDiagnosticsChange, onAstChange, format, attributionPayload]);

  // Immediate render update (no debounce)
  const updatePreview = useCallback((newContent: string, documentPath?: string) => {
    if (renderTimeoutRef.current) {
      clearTimeout(renderTimeoutRef.current);
    }
    doRenderWithStateManagement(newContent, documentPath);
  }, [doRenderWithStateManagement]);

  // Re-render when content changes, scroll sync is toggled, or a new
  // attribution payload arrives.
  useEffect(() => {
    // Pass document path as-is from Automerge (e.g., "index.qmd" or "docs/index.qmd").
    updatePreview(content, currentFile?.path);
  }, [
    content,
    updatePreview,
    scrollSyncEnabled,
    currentFile?.path,
    onDiagnosticsChange,
    attributionPayload,
  ]);

  // Reset preview state when file changes
  useEffect(() => {
    setPreviewState('START');
    setCurrentError(null);
  }, [currentFile?.path]);

  // Handler for AST modifications - converts AST back to QMD and updates content.
  //
  // q2-preview is **read-only in v1** (Plan 1 §"Multi-plan contract:
  // read-only mode lifts at Plan 7"). The post-pipeline AST diverges
  // from source enough that a naive incrementalWriteQmd would
  // corrupt the qmd; Plan 7 lifts this guard once the writer's
  // round-trip machinery understands q2-preview's transform shapes
  // (Synthetic / Derived / atomic CustomNodes). Component-driven
  // edits (kanban drag, comment buttons in Plan 2) call this and
  // silently no-op with a console.warn — that is the accepted
  // post-Plan-2 UX gap until Plan 7 ships.
  const handleSetAst = useCallback((newAst: any) => {
    if (pipelineKindForFormat(format) === 'preview') {
      console.warn('q2-preview is read-only in v1; AST edit dropped (Plan 7 lifts this guard)');
      return;
    }
    try {
      const newQmd = incrementalWriteQmd(content, newAst);
      onContentRewrite(newQmd);
    } catch (err) {
      console.error('Failed to write AST back to QMD:', err);
    }
  }, [content, onContentRewrite, format]);

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', position: 'relative' }}>
      <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
        {ast && (previewState === 'GOOD' || previewState === 'ERROR_FROM_GOOD') ? (
          <ReactRenderer
            astJson={ast}
            currentFilePath={currentFile?.path ?? ''}
            files={files}
            fileContents={fileContents}
            onNavigateToDocument={handleNavigateToDocument}
            setAst={handleSetAst}
            currentSlideIndex={currentSlideIndex}
            onSlideChange={onSlideChange}
            format={format}
            themeFingerprint={themeFingerprint}
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
        error={currentError}
        visible={previewState === 'ERROR_FROM_GOOD'}
        collapsed={errorOverlayCollapsed}
        onToggleCollapsed={setErrorOverlayCollapsed}
      />
    </div>
  );
}
