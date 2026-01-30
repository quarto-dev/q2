import { useState, useCallback, useRef, useEffect } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '../types/project';
import { isQmdFile } from '../types/project';
import type { Diagnostic } from '../types/diagnostic';
import { initWasm, parseQmdToAst, isWasmReady } from '../services/wasmRenderer';
import { stripAnsi } from '../utils/stripAnsi';
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
  scrollSyncEnabled: boolean;
  editorRef: React.RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  editorReady: boolean;
  editorHasFocusRef: React.RefObject<boolean>;
  onFileChange: (file: FileEntry, anchor?: string) => void;
  onOpenNewFileDialog: (initialFilename: string) => void;
  onDiagnosticsChange: (diagnostics: Diagnostic[]) => void;
  onWasmStatusChange?: (status: 'loading' | 'ready' | 'error', error: string | null) => void;
}

// Result of rendering QMD content to AST
type RenderResult = {
  success: true;
  astJson: string;
  diagnostics: Diagnostic[];
} | {
  success: false;
  error: string;
  diagnostics: Diagnostic[];
}

// Parse QMD content to AST using WASM
// Returns diagnostics and AST JSON string or error message
async function doRender(
  qmdContent: string,
  _options: { scrollSyncEnabled: boolean; documentPath?: string }
): Promise<RenderResult> {
  if (!isWasmReady()) {
    return {
      success: false,
      error: 'WASM renderer not ready',
      diagnostics: [],
    };
  }

  try {
    // Parse to AST
    const astJson = await parseQmdToAst(qmdContent);

    return {
      success: true,
      astJson,
      diagnostics: [],
    };
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

export default function ReactPreview({
  content,
  currentFile,
  files,
  scrollSyncEnabled,
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

  // Rendered AST JSON to display
  const [ast, setAst] = useState<string>('');

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

    const result = await doRender(qmdContent, { scrollSyncEnabled, documentPath });
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
    } else {
      // Set current error for overlay
      const currentState = previewStateRef.current;
      if (currentState === 'START' || currentState === 'ERROR_AT_START') {
        // No good render yet - show full error page
        setPreviewState('ERROR_AT_START');
        setAst(''); // Clear AST on error
      } else {
        // Was GOOD or ERROR_FROM_GOOD - keep last good AST, show overlay
        // DON'T update AST content
        setPreviewState('ERROR_FROM_GOOD');
      }
    }
  }, [scrollSyncEnabled, onDiagnosticsChange]);

  // Debounced render update
  const updatePreview = useCallback((newContent: string, documentPath?: string) => {
    if (renderTimeoutRef.current) {
      clearTimeout(renderTimeoutRef.current);
    }
    renderTimeoutRef.current = window.setTimeout(() => {
      doRenderWithStateManagement(newContent, documentPath);
    }, 20);
  }, [doRenderWithStateManagement]);

  // Re-render when content changes, WASM becomes ready, or scroll sync is toggled
  useEffect(() => {
    const filePath = currentFile?.path;

    // For non-QMD files, show a placeholder and clear diagnostics
    if (!isQmdFile(filePath)) {
      onDiagnosticsChange([]);
      setCurrentError(null);
      setPreviewState('START');
      setAst('');
      return;
    }

    // Pass document path as-is from Automerge (e.g., "index.qmd" or "docs/index.qmd").
    updatePreview(content, filePath);
  }, [content, updatePreview, wasmStatus, scrollSyncEnabled, currentFile?.path, onDiagnosticsChange]);

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
      <div className="pane preview-pane" style={{ overflow: 'scroll' }}>
        {ast && (previewState === 'GOOD' || previewState === 'ERROR_FROM_GOOD') ? (
          <ReactRenderer
            astJson={ast}
            currentFilePath={currentFile?.path ?? ''}
            onNavigateToDocument={handleNavigateToDocument}
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
        {/* Error overlay shown when error occurs after successful render */}
        <PreviewErrorOverlay
          error={currentError}
          visible={previewState === 'ERROR_FROM_GOOD'}
        />
      </div>
    </>
  );
}
