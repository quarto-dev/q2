import { useState, useCallback, useRef, useEffect } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '../../types/project';
import type { Diagnostic } from '../../types/diagnostic';
import { renderToHtml, isWasmReady, setScrollSyncEnabled } from '../../services/wasmRenderer';
import { getFileContent, getBinaryFileContent } from '../../services/automergeSync';
import { useScrollSync } from '../../hooks/useScrollSync';
import { useSelectionSync } from '../../hooks/useSelectionSync';
import { PreviewErrorOverlay } from './PreviewErrorOverlay';
import MorphIframe, { type MorphIframeHandle } from './MorphIframe';
import { ErrorView } from './PreviewStaticInfoViews';

// Preview pane state machine:
// START: Initial blank page
// ERROR_AT_START: Error page shown before any successful render
// GOOD: Successfully rendered HTML preview
// ERROR_FROM_GOOD: Error occurred after previous successful render (keep last good HTML, show overlay)
type PreviewState = 'START' | 'ERROR_AT_START' | 'GOOD' | 'ERROR_FROM_GOOD';

// Error info for the overlay
interface CurrentError {
  message: string;
  diagnostics?: Diagnostic[];
}

interface PreviewProps {
  content: string;
  currentFile: FileEntry | null;
  files: FileEntry[];
  /**
   * Phase 9 (Decision 6): a fresh `Map` identity is produced on
   * every Automerge edit (`App.tsx::setFileContents`). Passing it
   * as a `useEffect` dep is what makes the project-aware preview
   * re-render when *any* sibling file changes — a sibling's title
   * edit can shift the active page's sidebar HTML, and the user
   * expects to see that live.
   */
  fileContents: Map<string, string>;
  scrollSyncEnabled: boolean;
  editorRef: React.RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  editorReady: boolean;
  editorHasFocusRef: React.RefObject<boolean>;
  onFileChange: (file: FileEntry, anchor?: string) => void;
  onOpenNewFileDialog: (initialFilename: string) => void;
  onDiagnosticsChange: (diagnostics: Diagnostic[]) => void;
  /** Callback to register scrollToLine function for external use */
  onRegisterScrollToLine?: (fn: (line: number) => void) => void;
  /** Callback to register setScrollRatio function for external use */
  onRegisterSetScrollRatio?: (fn: (ratio: number) => void) => void;
}

// Result of rendering QMD content
type RenderResult = {
  success: true;
  html: string;
  diagnostics: Diagnostic[];
} | {
  success: false;
  error: string;
  diagnostics: Diagnostic[];
}

// Render a VFS document to HTML using WASM
// The document content must already be in the VFS via Automerge sync.
// Scroll sync (source-location) is controlled via runtime metadata, not per-render.
async function doRender(
  documentPath: string,
  projectFilePaths: readonly string[],
): Promise<RenderResult> {
  // Caller should check isWasmReady() before calling this
  if (!isWasmReady()) {
    return {
      success: false,
      error: 'WASM not ready',
      diagnostics: [],
    };
  }

  try {
    const result = await renderToHtml({
      documentPath: documentPath,
      userGrammars: {
        files: projectFilePaths,
        getBinaryContent: async (path) =>
          getBinaryFileContent(path)?.content ?? null,
        getTextContent: async (path) => getFileContent(path),
      },
    });

    // Collect all diagnostics from both success and error paths
    const allDiagnostics: Diagnostic[] = [
      ...(result.diagnostics ?? []),
      ...(result.warnings ?? []),
    ];

    if (result.success) {
      return {
        success: true,
        html: result.html,
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
  } catch (err) {
    const errorMsg =
      err instanceof Error ? err.message : JSON.stringify(err, null, 2);

    return {
      success: false,
      diagnostics: [],
      error: errorMsg,
    };
  }
}

export default function Preview({
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
  onRegisterScrollToLine,
  onRegisterSetScrollRatio,
}: PreviewProps) {
  // Preview state machine for error handling
  const [previewState, setPreviewState] = useState<PreviewState>('START');
  const [currentError, setCurrentError] = useState<CurrentError | null>(null);
  // Track previewState in a ref for use in callbacks
  const previewStateRef = useRef<PreviewState>('START');
  useEffect(() => {
    previewStateRef.current = previewState;
  }, [previewState]);

  // Ref to MorphIframe to access its imperative methods
  const doubleBufferedIframeRef = useRef<MorphIframeHandle>(null);

  // Register scroll functions with parent for external control
  useEffect(() => {
    onRegisterScrollToLine?.((line: number) => {
      doubleBufferedIframeRef.current?.scrollToLine(line);
    });
    onRegisterSetScrollRatio?.((ratio: number) => {
      doubleBufferedIframeRef.current?.setScrollRatio(ratio);
    });
  }, [onRegisterScrollToLine, onRegisterSetScrollRatio]);

  // Rendered HTML to display in iframe
  const [renderedHtml, setRenderedHtml] = useState<string>('');

  // Debounce rendering
  const renderTimeoutRef = useRef<number | null>(null);
  const lastContentRef = useRef<string>('');

  // Handler for cross-document navigation from DoubleBufferedIframe
  const handleNavigateToDocument = useCallback(
    (targetPath: string, anchor: string | null) => {
      const file = files.find(
        (f) => f.path === targetPath || '/' + f.path === targetPath
      );

      if (file) {
        // Existing file - switch to it
        // DoubleBufferedIframe will handle the anchor scrolling after swap
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

  // Scroll synchronization between editor and preview
  const { handlePreviewScroll, handlePreviewClick } = useScrollSync({
    editorRef,
    scrollPreviewToLine: (line: number) => {
      doubleBufferedIframeRef.current?.scrollToLine(line);
    },
    getPreviewScrollRatio: () => {
      return doubleBufferedIframeRef.current?.getScrollRatio() ?? null;
    },
    enabled: scrollSyncEnabled && editorReady,
    editorHasFocusRef,
  });

  // Selection synchronization between preview and editor
  const { handlePreviewSelection } = useSelectionSync({
    editorRef,
    previewRef: doubleBufferedIframeRef,
    enabled: scrollSyncEnabled && editorReady,
  });

  // Set scroll sync via runtime metadata when the prop changes
  useEffect(() => {
    if (isWasmReady()) {
      setScrollSyncEnabled(scrollSyncEnabled);
    }
  }, [scrollSyncEnabled]);

  // Render function that uses WASM when available
  // Implements state machine transitions for error handling:
  // - On success: always transition to GOOD, swap to new content
  // - On error from START/ERROR_AT_START: show full error page
  // - On error from GOOD/ERROR_FROM_GOOD: keep last good HTML, show overlay
  const doRenderWithStateManagement = useCallback(async (qmdContent: string, documentPath: string) => {
    lastContentRef.current = qmdContent;

    // Don't render if WASM isn't ready - component will show fallback
    if (!isWasmReady()) {
      return;
    }

    // Pass the current project file list so the render can auto-discover
    // any user-defined tree-sitter grammars under `_quarto/grammars/*`.
    // The discovery step is pure + cache-backed so this is ~free when
    // grammars haven't changed since the last render.
    const projectFilePaths = files.map((f) => f.path);
    const result = await doRender(documentPath, projectFilePaths);
    if (qmdContent !== lastContentRef.current) return;

    // Update diagnostics
    onDiagnosticsChange(result.diagnostics);

    if (result.success) {
      // Normal success: transition to GOOD state from any state
      setPreviewState('GOOD');
      setCurrentError(null);
      // Update rendered HTML
      setRenderedHtml(result.html);
    } else {
      // Set current error for overlay
      setCurrentError({
        message: result.error,
        diagnostics: result.diagnostics,
      });

      const currentState = previewStateRef.current;
      if (currentState === 'START' || currentState === 'ERROR_AT_START') {
        // No good render yet - show full error page
        setPreviewState('ERROR_AT_START');
      } else {
        // Was GOOD or ERROR_FROM_GOOD - keep last good HTML, show overlay
        // DON'T update HTML content
        setPreviewState('ERROR_FROM_GOOD');
      }
    }
  }, [onDiagnosticsChange, files]);

  // Debounced render update
  const updatePreview = useCallback((newContent: string, documentPath: string) => {
    if (renderTimeoutRef.current) {
      clearTimeout(renderTimeoutRef.current);
    }
    renderTimeoutRef.current = window.setTimeout(() => {
      doRenderWithStateManagement(newContent, documentPath);
    }, 20);
  }, [doRenderWithStateManagement]);

  // Re-render when the active page's content changes OR when any
  // sibling file's content changes (Phase 9 Decision 6). For
  // single-file projects this is identical to "active content
  // changed" — the map only contains the active file. For website
  // projects, a sibling title edit shifts the rendered sidebar
  // HTML, so the active page must re-render.
  //
  // The `fileContents` Map gets a fresh identity on every Automerge
  // edit (App.tsx:370-377), so depending on it lets React fire this
  // effect without us doing any change detection ourselves. The
  // 20ms debounce inside `updatePreview` absorbs burst edits.
  useEffect(() => {
    const filePath = currentFile?.path;

    // PreviewRouter filters non-QMD files, so filePath should always be valid here
    if (!filePath) {
      return;
    }

    // Pass document path as-is from Automerge (e.g., "index.qmd" or "docs/index.qmd").
    // The WASM layer will use VFS path normalization to resolve relative paths correctly.
    updatePreview(content, filePath);
  }, [content, fileContents, updatePreview, currentFile?.path]);

  // Reset preview state when file changes
  useEffect(() => {
    setPreviewState('START');
    setCurrentError(null);
  }, [currentFile?.path]);

  return (
    <div style={{ width: '100%', height: '100%', position: 'relative' }}>
      {previewState === 'ERROR_AT_START' && currentError ? (
        // Error page (no good render yet)
        <ErrorView content={content} error={currentError.message} diagnostics={currentError.diagnostics} />
      ) : (
        // Normal iframe with morphing
        <>
          <MorphIframe
            ref={doubleBufferedIframeRef}
            qmdContent={content}
            html={renderedHtml}
            currentFilePath={currentFile?.path ?? ''}
            onNavigateToDocument={handleNavigateToDocument}
            onScroll={handlePreviewScroll}
            onClick={handlePreviewClick}
            onSelectionChange={handlePreviewSelection}
          />
          {/* Error overlay shown when error occurs after successful render */}
          <PreviewErrorOverlay
            error={currentError}
            visible={previewState === 'ERROR_FROM_GOOD'}
          />
        </>
      )}
    </div>
  );
}
