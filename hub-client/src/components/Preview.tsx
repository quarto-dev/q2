import { useState, useCallback, useRef, useEffect } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '../types/project';
import type { Diagnostic } from '../types/diagnostic';
import { initWasm, renderToHtml, isWasmReady } from '../services/wasmRenderer';
import { useScrollSync } from '../hooks/useScrollSync';
import { stripAnsi } from '../utils/stripAnsi';
import { PreviewErrorOverlay } from './PreviewErrorOverlay';
import DoubleBufferedIframe, { type DoubleBufferedIframeHandle } from './DoubleBufferedIframe';

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
  scrollSyncEnabled: boolean;
  editorRef: React.RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  editorReady: boolean;
  editorHasFocusRef: React.RefObject<boolean>;
  onFileChange: (file: FileEntry) => void;
  onOpenNewFileDialog: (initialFilename: string) => void;
  onDiagnosticsChange: (diagnostics: Diagnostic[]) => void;
  onWasmStatusChange?: (status: 'loading' | 'ready' | 'error', error: string | null) => void;
}

// Fallback for when WASM isn't ready yet
function renderFallback(content: string, message: string): string {
  return `
    <html>
      <head>
        <style>
          body {
            font-family: system-ui, -apple-system, sans-serif;
            padding: 24px;
            max-width: 800px;
            margin: 0 auto;
            line-height: 1.6;
            color: #333;
          }
          pre {
            background: #f4f4f4;
            padding: 16px;
            border-radius: 4px;
            overflow-x: auto;
          }
          code { font-family: 'SF Mono', Monaco, monospace; }
          .notice {
            padding: 12px;
            border-radius: 4px;
            margin-bottom: 16px;
          }
          .loading { background: #e3f2fd; }
          .error { background: #ffebee; color: #c62828; }
        </style>
      </head>
      <body>
        <div class="notice loading">
          ${message}
        </div>
        <pre><code>${content.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>
      </body>
    </html>
  `;
}

// Error display HTML
function renderError(content: string, error: string, diagnostics?: string[]): string {
  // Strip ANSI codes and escape HTML
  const cleanError = stripAnsi(error)
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');

  const diagHtml = diagnostics?.length
    ? `<ul>${diagnostics.map(d => `<li>${stripAnsi(d).replace(/</g, '&lt;').replace(/>/g, '&gt;')}</li>`).join('')}</ul>`
    : '';

  return `
    <html>
      <head>
        <style>
          body {
            font-family: system-ui, -apple-system, sans-serif;
            padding: 24px;
            max-width: 800px;
            margin: 0 auto;
            line-height: 1.6;
            color: #333;
          }
          pre {
            background: #f4f4f4;
            padding: 16px;
            border-radius: 4px;
            overflow-x: auto;
          }
          code { font-family: 'SF Mono', Monaco, monospace; }
          .error {
            background: #ffebee;
            color: #c62828;
            padding: 12px;
            border-radius: 4px;
            margin-bottom: 16px;
          }
          .error-message {
            font-family: 'SF Mono', Monaco, 'Cascadia Code', 'Fira Code', monospace;
            font-size: 13px;
            line-height: 1.4;
            white-space: pre;
            overflow-x: auto;
            margin-top: 8px;
          }
          .error ul { margin: 8px 0 0 0; padding-left: 20px; }
        </style>
      </head>
      <body>
        <div class="error">
          <strong>Render Error</strong>
          <div class="error-message">${cleanError}</div>
          ${diagHtml}
        </div>
        <pre><code>${content.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>
      </body>
    </html>
  `;
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
// Render QMD content to HTML using WASM
// Returns diagnostics and HTML string or error message
async function doRender(
  qmdContent: string,
  options: { scrollSyncEnabled: boolean }
): Promise<RenderResult> {
  if (!isWasmReady()) {
    return {
      success: true,
      html: renderFallback(qmdContent, 'Loading WASM renderer...'),
      diagnostics: [],
    };
  }

  try {
    // Enable source location tracking when scroll sync is enabled
    const result = await renderToHtml(qmdContent, {
      sourceLocation: options.scrollSyncEnabled,
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
  scrollSyncEnabled,
  editorRef,
  editorReady,
  editorHasFocusRef,
  onFileChange,
  onOpenNewFileDialog,
  onDiagnosticsChange,
  onWasmStatusChange,
}: PreviewProps) {
  const [wasmStatus, setWasmStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [wasmError, setWasmError] = useState<string | null>(null);

  // Notify parent when WASM status changes
  useEffect(() => {
    onWasmStatusChange?.(wasmStatus, wasmError);
  }, [wasmStatus, wasmError, onWasmStatusChange]);



  // Preview state machine for error handling
  const [previewState, setPreviewState] = useState<PreviewState>('START');
  const [currentError, setCurrentError] = useState<CurrentError | null>(null);
  // Track previewState in a ref for use in callbacks
  const previewStateRef = useRef<PreviewState>('START');
  useEffect(() => {
    previewStateRef.current = previewState;
  }, [previewState]);

  // Ref to DoubleBufferedIframe to access its imperative methods
  const doubleBufferedIframeRef = useRef<DoubleBufferedIframeHandle>(null);

  // Rendered HTML to display in iframe
  const [renderedHtml, setRenderedHtml] = useState<string>('');

  // Debounce rendering
  const renderTimeoutRef = useRef<number | null>(null);
  const lastContentRef = useRef<string>('');

  // Initialize WASM on mount
  useEffect(() => {
    let cancelled = false;

    async function init() {
      try {
        setWasmStatus('loading');
        await initWasm();
        if (!cancelled) {
          setWasmStatus('ready');
        }
      } catch (err) {
        if (!cancelled) {
          setWasmStatus('error');
          setWasmError(err instanceof Error ? err.message : String(err));
        }
      }
    }

    init();
    return () => { cancelled = true; };
  }, []);

  // Handler for cross-document navigation from DoubleBufferedIframe
  const handleNavigateToDocument = useCallback(
    (targetPath: string, _anchor: string | null) => {
      const file = files.find(
        (f) => f.path === targetPath || '/' + f.path === targetPath
      );

      if (file) {
        // Existing file - switch to it
        // DoubleBufferedIframe will handle the anchor scrolling after swap
        onFileChange(file);
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

  // Render function that uses WASM when available
  // Implements state machine transitions for error handling:
  // - On success: always transition to GOOD, swap to new content
  // - On error from START/ERROR_AT_START: show full error page
  // - On error from GOOD/ERROR_FROM_GOOD: keep last good HTML, show overlay
  const doRenderWithStateManagement = useCallback(async (qmdContent: string) => {
    lastContentRef.current = qmdContent;

    const result = await doRender(qmdContent, { scrollSyncEnabled });
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
      // Update rendered HTML
      setRenderedHtml(result.html);
    } else {
      // Set current error for overlay
      const currentState = previewStateRef.current;
      if (currentState === 'START' || currentState === 'ERROR_AT_START') {
        // No good render yet - show full error page
        setPreviewState('ERROR_AT_START');
        setRenderedHtml(renderError(qmdContent, result.error));
      } else {
        // Was GOOD or ERROR_FROM_GOOD - keep last good HTML, show overlay
        // DON'T update HTML content
        setPreviewState('ERROR_FROM_GOOD');
      }
    }
  }, [scrollSyncEnabled, onDiagnosticsChange]);

  // Debounced render update
  const updatePreview = useCallback((newContent: string) => {
    if (renderTimeoutRef.current) {
      clearTimeout(renderTimeoutRef.current);
    }
    renderTimeoutRef.current = window.setTimeout(() => {
      doRenderWithStateManagement(newContent);
    }, 20);
  }, [doRenderWithStateManagement]);

  // Re-render when content changes, WASM becomes ready, or scroll sync is toggled
  useEffect(() => {
    updatePreview(content);
  }, [content, updatePreview, wasmStatus, scrollSyncEnabled]);

  // Reset preview state when file changes
  useEffect(() => {
    setPreviewState('START');
    setCurrentError(null);
  }, [currentFile?.path]);

  return (
    <>
      {wasmError && (
        <div className="wasm-error-banner">
          Failed to load WASM: {wasmError}
        </div>
      )}
      <div className="pane preview-pane">
        <DoubleBufferedIframe
          ref={doubleBufferedIframeRef}
          html={renderedHtml}
          currentFilePath={currentFile?.path ?? ''}
          onNavigateToDocument={handleNavigateToDocument}
          onScroll={handlePreviewScroll}
          onClick={handlePreviewClick}
        />
        {/* Error overlay shown when error occurs after successful render */}
        <PreviewErrorOverlay
          error={currentError}
          visible={previewState === 'ERROR_FROM_GOOD'}
        />
      </div>
    </>
  );
}
