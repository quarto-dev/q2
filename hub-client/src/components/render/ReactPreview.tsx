import { useState, useCallback, useRef, useEffect } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '../../types/project';
import type { Diagnostic } from '../../types/diagnostic';
import { parseQmdToAst, renderPageInProject, isWasmReady, incrementalWriteQmd } from '../../services/wasmRenderer';
import { pipelineKindForFormat } from '../../utils/pipelineKind';
import { stripAnsi } from '../../utils/stripAnsi';
import { PreviewErrorOverlay } from './PreviewErrorOverlay';
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
// - `'preview'` (q2-preview): calls `renderPageInProject(documentPath)`,
//   which runs the full q2-preview pipeline in WASM (shortcodes, Lua
//   filters, sectionize, crossref, sidebar/navbar metadata, etc.) and
//   returns the post-pipeline AST as JSON via `RenderResponse.ast_json`.
//   Requires a `documentPath` because the pipeline reads the file from
//   VFS and discovers project context from it.
// - any other format (q2-debug, q2-slides): calls `parseQmdToAst(content)`,
//   which is path-less and skips the transform pipeline entirely — the
//   raw parse-only AST.
//
// Returns diagnostics and an AST JSON string, or an error message.
async function doRender(
  qmdContent: string,
  options: { scrollSyncEnabled: boolean; documentPath?: string; format: string }
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

    const result = await renderPageInProject(options.documentPath);
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
  const result = await parseQmdToAst(qmdContent);

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
}: PreviewProps) {
  // Preview state machine for error handling
  const [previewState, setPreviewState] = useState<PreviewState>('START');
  const [currentError, setCurrentError] = useState<CurrentError | null>(null);
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

    const result = await doRender(qmdContent, { scrollSyncEnabled, documentPath, format });
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
  }, [scrollSyncEnabled, onDiagnosticsChange, onAstChange, format]);

  // Immediate render update (no debounce)
  const updatePreview = useCallback((newContent: string, documentPath?: string) => {
    if (renderTimeoutRef.current) {
      clearTimeout(renderTimeoutRef.current);
    }
    doRenderWithStateManagement(newContent, documentPath);
  }, [doRenderWithStateManagement]);

  // Re-render when content changes or scroll sync is toggled
  useEffect(() => {
    // Pass document path as-is from Automerge (e.g., "index.qmd" or "docs/index.qmd").
    updatePreview(content, currentFile?.path);
  }, [content, updatePreview, scrollSyncEnabled, currentFile?.path, onDiagnosticsChange]);

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
      />
    </div>
  );
}
