import { useState, useCallback, useRef, useEffect } from 'react';
import MonacoEditor from '@monaco-editor/react';
import type * as Monaco from 'monaco-editor';
import type { ProjectEntry, FileEntry } from '../types/project';
import type { Patch } from '../services/automergeSync';
import type { Diagnostic } from '../types/diagnostic';
import { initWasm, renderToHtml, isWasmReady, vfsReadFile } from '../services/wasmRenderer';
import { useIframePostProcessor } from '../hooks/useIframePostProcessor';
import { usePresence } from '../hooks/usePresence';
import { useScrollSync } from '../hooks/useScrollSync';
import { patchesToMonacoEdits } from '../utils/patchToMonacoEdits';
import { diagnosticsToMarkers } from '../utils/diagnosticToMonaco';
import './Editor.css';

interface Props {
  project: ProjectEntry;
  files: FileEntry[];
  fileContents: Map<string, string>;
  filePatches: Map<string, Patch[]>;
  onDisconnect: () => void;
  onContentChange: (path: string, content: string) => void;
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

// Strip ANSI escape codes from text
function stripAnsi(text: string): string {
  // Match ANSI escape sequences: ESC [ ... m (SGR sequences)
  // This covers color codes like \x1b[31m, \x1b[38;5;246m, \x1b[0m, etc.
  return text.replace(/\x1b\[[0-9;]*m/g, '');
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

// Select the best default file: prefer index.qmd, then first .qmd, then first file
function selectDefaultFile(files: FileEntry[]): FileEntry | null {
  if (files.length === 0) return null;

  // Prefer index.qmd at root
  const indexQmd = files.find(f => f.path === 'index.qmd');
  if (indexQmd) return indexQmd;

  // Then any .qmd file
  const anyQmd = files.find(f => f.path.endsWith('.qmd'));
  if (anyQmd) return anyQmd;

  // Fall back to first file
  return files[0];
}

export default function Editor({ project, files, fileContents, filePatches, onDisconnect, onContentChange }: Props) {
  const [currentFile, setCurrentFile] = useState<FileEntry | null>(selectDefaultFile(files));

  // Monaco editor instance ref
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);

  // Flag to prevent local changes from echoing back during remote edits
  const applyingRemoteRef = useRef(false);

  // Presence for collaborative cursors
  const { remoteUsers, userCount, onEditorMount: onPresenceEditorMount } = usePresence(currentFile?.path ?? null);

  // Get content from fileContents map, or use default for new files
  const getContent = useCallback((file: FileEntry | null): string => {
    if (!file) return '';
    return fileContents.get(file.path) ?? '';
  }, [fileContents]);

  const [content, setContent] = useState<string>(getContent(currentFile));
  const [wasmStatus, setWasmStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [wasmError, setWasmError] = useState<string | null>(null);

  // Double-buffered iframes to prevent flash during updates
  const iframeARef = useRef<HTMLIFrameElement>(null);
  const iframeBRef = useRef<HTMLIFrameElement>(null);
  const [activeIframe, setActiveIframe] = useState<'A' | 'B'>('A');
  const activeIframeRef = useRef<'A' | 'B'>('A'); // Ref for use in callbacks
  const [iframeAHtml, setIframeAHtml] = useState<string>('');
  const [iframeBHtml, setIframeBHtml] = useState<string>('');
  // Track if we're waiting for inactive iframe to load before swapping
  const [swapPending, setSwapPending] = useState(false);
  // iframeRef points to the currently active iframe (for scroll sync and post-processing)
  const iframeRef = activeIframe === 'A' ? iframeARef : iframeBRef;
  const inactiveIframeRef = activeIframe === 'A' ? iframeBRef : iframeARef;

  // Keep ref in sync with state
  useEffect(() => {
    activeIframeRef.current = activeIframe;
  }, [activeIframe]);

  // Diagnostics state for Monaco markers
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [unlocatedErrors, setUnlocatedErrors] = useState<Diagnostic[]>([]);

  // Scroll sync state (enabled by default)
  const [scrollSyncEnabled, setScrollSyncEnabled] = useState(true);
  // Track if editor has focus (to prevent scroll feedback loop)
  const editorHasFocusRef = useRef(false);
  // Track when editor is mounted (for scroll sync initialization)
  const [editorReady, setEditorReady] = useState(false);
  const [iframeLoadCount, setIframeLoadCount] = useState(0);

  // Monaco instance ref for setting markers
  const monacoRef = useRef<typeof Monaco | null>(null);

  // Handler for .qmd link clicks in the preview
  const handleQmdLinkClick = useCallback(
    (targetPath: string) => {
      const file = files.find(
        (f) => f.path === targetPath || '/' + f.path === targetPath
      );
      if (file) {
        setCurrentFile(file);
      }
    },
    [files]
  );

  // Post-process iframe content after render (replace CSS links with data URIs)
  // Note: We pass the active iframeRef here, but we'll also process inactive iframe manually
  const { handleLoad: handlePostProcess } = useIframePostProcessor(iframeRef, {
    currentFilePath: currentFile?.path ?? '',
    onQmdLinkClick: handleQmdLinkClick,
  });

  // Handler for when the inactive iframe finishes loading new content
  const handleInactiveIframeLoad = useCallback(() => {
    // Only process if we're waiting for a swap
    if (!swapPending) return;

    const activeIframeEl = iframeRef.current;
    const inactiveIframeEl = inactiveIframeRef.current;

    // Save scroll position from currently active iframe
    let scrollPos: { x: number; y: number } | null = null;
    if (activeIframeEl?.contentWindow) {
      scrollPos = {
        x: activeIframeEl.contentWindow.scrollX,
        y: activeIframeEl.contentWindow.scrollY,
      };
    }

    // Post-process the inactive iframe (CSS data URIs, link handlers)
    if (inactiveIframeEl?.contentDocument) {
      const doc = inactiveIframeEl.contentDocument;
      // Inline the post-processing logic for the inactive iframe
      doc.querySelectorAll('link[rel="stylesheet"]').forEach((link) => {
        const href = link.getAttribute('href');
        if (href?.startsWith('/.quarto/')) {
          const result = vfsReadFile(href);
          if (result.success && result.content) {
            const dataUri = `data:text/css;base64,${btoa(result.content)}`;
            link.setAttribute('href', dataUri);
          }
        }
      });
    }

    // Swap: make inactive become active
    setActiveIframe((prev) => (prev === 'A' ? 'B' : 'A'));
    setSwapPending(false);
    setIframeLoadCount((n) => n + 1);

    // Restore scroll position to the now-active iframe (after swap, inactiveIframeRef is now the visible one)
    // We need to do this after React re-renders, so use setTimeout
    if (scrollPos) {
      setTimeout(() => {
        // After swap, the previously inactive iframe is now active
        const nowActiveIframe = inactiveIframeEl;
        if (nowActiveIframe?.contentWindow) {
          nowActiveIframe.contentWindow.scrollTo(scrollPos!.x, scrollPos!.y);
        }
      }, 0);
    }
  }, [swapPending, iframeRef, inactiveIframeRef]);

  // Handler for active iframe load (used for initial load and post-processing)
  const handleActiveIframeLoad = useCallback(() => {
    handlePostProcess();
    setIframeLoadCount((n) => n + 1);
  }, [handlePostProcess]);

  // Scroll synchronization between editor and preview
  useScrollSync({
    editorRef,
    iframeRef,
    enabled: scrollSyncEnabled && editorReady,
    iframeLoadCount,
    editorHasFocusRef,
  });

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

  // Helper to set HTML on the inactive iframe (uses ref for current active state)
  const setInactiveHtml = useCallback((html: string) => {
    if (activeIframeRef.current === 'A') {
      setIframeBHtml(html);
    } else {
      setIframeAHtml(html);
    }
  }, []);

  // Render function that uses WASM when available
  const doRender = useCallback(async (qmdContent: string) => {
    lastContentRef.current = qmdContent;

    console.log('[doRender] scrollSyncEnabled:', scrollSyncEnabled);

    if (!isWasmReady()) {
      // For initial load before WASM is ready, load into inactive iframe and swap
      setInactiveHtml(renderFallback(qmdContent, 'Loading WASM renderer...'));
      setSwapPending(true);
      setDiagnostics([]);
      return;
    }

    try {
      // Enable source location tracking when scroll sync is enabled
      console.log('[doRender] calling renderToHtml with sourceLocation:', scrollSyncEnabled);
      const result = await renderToHtml(qmdContent, {
        sourceLocation: scrollSyncEnabled,
      });

      // Check if content changed while we were rendering
      if (qmdContent !== lastContentRef.current) {
        return;
      }

      // Collect all diagnostics from both success and error paths
      const allDiagnostics: Diagnostic[] = [
        ...(result.diagnostics ?? []),
        ...(result.warnings ?? []),
      ];
      setDiagnostics(allDiagnostics);

      if (result.success) {
        // Load new content into inactive iframe (will swap on load)
        setInactiveHtml(result.html);
        setSwapPending(true);
      } else {
        const errorMsg =
          typeof result.error === 'string'
            ? result.error
            : JSON.stringify(result.error, null, 2) || 'Unknown error';
        // Show error in preview pane
        setInactiveHtml(renderError(qmdContent, errorMsg));
        setSwapPending(true);
      }
    } catch (err) {
      const errorMsg =
        err instanceof Error ? err.message : JSON.stringify(err, null, 2);
      setInactiveHtml(renderError(qmdContent, errorMsg));
      setSwapPending(true);
      setDiagnostics([]);
    }
  }, [scrollSyncEnabled, setInactiveHtml]);

  // Debounced render update
  const updatePreview = useCallback((newContent: string) => {
    if (renderTimeoutRef.current) {
      clearTimeout(renderTimeoutRef.current);
    }
    renderTimeoutRef.current = window.setTimeout(() => {
      doRender(newContent);
    }, 300);
  }, [doRender]);

  // Re-render when content changes, WASM becomes ready, or scroll sync is toggled
  useEffect(() => {
    updatePreview(content);
  }, [content, updatePreview, wasmStatus, scrollSyncEnabled]);

  // Apply Monaco markers when diagnostics change
  useEffect(() => {
    if (!editorRef.current || !monacoRef.current) {
      return;
    }

    const model = editorRef.current.getModel();
    if (!model) {
      return;
    }

    const { markers, unlocatedDiagnostics } = diagnosticsToMarkers(diagnostics);
    monacoRef.current.editor.setModelMarkers(model, 'quarto', markers);
    setUnlocatedErrors(unlocatedDiagnostics);
  }, [diagnostics]);

  // Sync local content state with external Automerge state.
  // Uses incremental edits when patches are available to preserve cursor position.
  // Note: setState in effect is intentional here - we're syncing with external state (Automerge).
  useEffect(() => {
    if (!currentFile) return;

    const newContent = fileContents.get(currentFile.path);
    if (newContent === undefined || newContent === content) return;

    const patches = filePatches.get(currentFile.path) ?? [];

    // If we have patches and the editor is mounted, apply incremental edits
    if (patches.length > 0 && editorRef.current) {
      const edits = patchesToMonacoEdits(patches, content);

      if (edits.length > 0) {
        // Mark that we're applying remote changes to prevent echo
        applyingRemoteRef.current = true;
        editorRef.current.executeEdits('remote-sync', edits);
        applyingRemoteRef.current = false;
      }

      // Sync local state with the new content
      setContent(newContent);
    } else {
      // Fallback: full content replacement (initial load, file switch, no patches)
      setContent(newContent);
    }
    // Note: `content` is intentionally NOT in the dependency array.
    // We only want to sync when external data changes, not on local edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentFile, fileContents, filePatches]);

  // Update currentFile when files list changes (e.g., on initial load)
  // Note: setState in effect is intentional - syncing with external file list
  useEffect(() => {
    if (!currentFile && files.length > 0) {
       
      setCurrentFile(selectDefaultFile(files));
    }
  }, [files, currentFile]);

  const handleEditorChange = (value: string | undefined) => {
    // Skip echo when applying remote changes
    if (applyingRemoteRef.current) return;

    if (value !== undefined && currentFile) {
      setContent(value);
      onContentChange(currentFile.path, value);
    }
  };

  // Capture Monaco editor instance on mount
  const handleEditorMount = (editor: Monaco.editor.IStandaloneCodeEditor, monaco: typeof Monaco) => {
    editorRef.current = editor;
    monacoRef.current = monaco;
    onPresenceEditorMount(editor);

    // Track editor focus state for scroll sync
    editor.onDidFocusEditorText(() => {
      editorHasFocusRef.current = true;
    });
    editor.onDidBlurEditorText(() => {
      editorHasFocusRef.current = false;
    });

    // Signal that editor is ready for scroll sync
    setEditorReady(true);
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const file = files.find(f => f.path === e.target.value);
    if (file) {
      setCurrentFile(file);
      const fileContent = fileContents.get(file.path);
      setContent(fileContent ?? '');
      // Clear diagnostics when switching files
      setDiagnostics([]);
      setUnlocatedErrors([]);
    }
  };

  return (
    <div className="editor-container">
      <header className="editor-header">
        <div className="project-info">
          <h1>{project.description}</h1>
          <div className="status-indicators">
            <span className={`sync-status ${wasmStatus === 'ready' ? 'connected' : 'disconnected'}`}>
              {wasmStatus === 'loading' && 'Loading WASM...'}
              {wasmStatus === 'ready' && 'Ready'}
              {wasmStatus === 'error' && 'WASM Error'}
            </span>
            {userCount > 0 && (
              <span className="user-count" title={remoteUsers.map(u => u.userName).join(', ')}>
                {userCount} other{userCount === 1 ? '' : 's'} here
              </span>
            )}
          </div>
        </div>
        <div className="file-selector">
          <select value={currentFile?.path || ''} onChange={handleFileChange}>
            {files.length === 0 ? (
              <option value="">No files</option>
            ) : (
              files.map(file => (
                <option key={file.path} value={file.path}>{file.path}</option>
              ))
            )}
          </select>
          <button className="new-file-btn" title="New file">+</button>
        </div>
        <div className="toolbar-actions">
          <label className="scroll-sync-toggle" title="Sync editor and preview scroll positions">
            <input
              type="checkbox"
              checked={scrollSyncEnabled}
              onChange={(e) => setScrollSyncEnabled(e.target.checked)}
            />
            <span>Scroll sync</span>
          </label>
          <button className="disconnect-btn" onClick={onDisconnect}>
            Disconnect
          </button>
        </div>
      </header>

      {wasmError && (
        <div className="wasm-error-banner">
          Failed to load WASM: {wasmError}
        </div>
      )}

      {unlocatedErrors.length > 0 && (
        <div className="diagnostics-banner">
          {unlocatedErrors.map((diag, i) => (
            <div key={i} className={`diagnostic-item diagnostic-${diag.kind}`}>
              {diag.code && <span className="diagnostic-code">[{diag.code}]</span>}
              <span className="diagnostic-title">{diag.title}</span>
              {diag.problem && <span className="diagnostic-problem">: {diag.problem}</span>}
            </div>
          ))}
        </div>
      )}

      <main className="editor-main">
        <div className="pane editor-pane">
          <MonacoEditor
            height="100%"
            language="markdown"
            theme="vs-dark"
            value={content}
            onChange={handleEditorChange}
            onMount={handleEditorMount}
            options={{
              minimap: { enabled: false },
              fontSize: 14,
              lineNumbers: 'on',
              wordWrap: 'on',
              padding: { top: 16 },
              scrollBeyondLastLine: false,
            }}
          />
        </div>
        <div className="pane preview-pane">
          {/* Double-buffered iframes: one visible, one loading in background */}
          <iframe
            ref={iframeARef}
            srcDoc={iframeAHtml}
            title="Preview A"
            sandbox="allow-same-origin"
            onLoad={activeIframe === 'A' ? handleActiveIframeLoad : handleInactiveIframeLoad}
            className={activeIframe === 'A' ? 'preview-active' : 'preview-hidden'}
          />
          <iframe
            ref={iframeBRef}
            srcDoc={iframeBHtml}
            title="Preview B"
            sandbox="allow-same-origin"
            onLoad={activeIframe === 'B' ? handleActiveIframeLoad : handleInactiveIframeLoad}
            className={activeIframe === 'B' ? 'preview-active' : 'preview-hidden'}
          />
        </div>
      </main>
    </div>
  );
}
