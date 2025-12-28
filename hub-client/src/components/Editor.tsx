import { useState, useCallback, useRef, useEffect } from 'react';
import MonacoEditor from '@monaco-editor/react';
import type * as Monaco from 'monaco-editor';
import type { ProjectEntry, FileEntry } from '../types/project';
import type { Patch } from '../services/automergeSync';
import { initWasm, renderToHtml, isWasmReady } from '../services/wasmRenderer';
import { useIframePostProcessor } from '../hooks/useIframePostProcessor';
import { usePresence } from '../hooks/usePresence';
import { patchesToMonacoEdits } from '../utils/patchToMonacoEdits';
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

// Error display HTML
function renderError(content: string, error: string, diagnostics?: string[]): string {
  const diagHtml = diagnostics?.length
    ? `<ul>${diagnostics.map(d => `<li>${d}</li>`).join('')}</ul>`
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
          .error ul { margin: 8px 0 0 0; padding-left: 20px; }
        </style>
      </head>
      <body>
        <div class="error">
          <strong>Render Error:</strong> ${error}
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
  const [previewHtml, setPreviewHtml] = useState<string>('');
  const [wasmStatus, setWasmStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [wasmError, setWasmError] = useState<string | null>(null);
  const iframeRef = useRef<HTMLIFrameElement>(null);

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
  const { handleLoad } = useIframePostProcessor(iframeRef, {
    currentFilePath: currentFile?.path ?? '',
    onQmdLinkClick: handleQmdLinkClick,
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

  // Render function that uses WASM when available
  const doRender = useCallback(async (qmdContent: string) => {
    lastContentRef.current = qmdContent;

    if (!isWasmReady()) {
      setPreviewHtml(renderFallback(qmdContent, 'Loading WASM renderer...'));
      return;
    }

    try {
      const result = await renderToHtml(qmdContent);

      // Check if content changed while we were rendering
      if (qmdContent !== lastContentRef.current) {
        return;
      }

      if (result.success) {
        // The render pipeline now produces complete HTML with CSS links.
        // The useIframePostProcessor hook will replace CSS links with
        // data URIs after the iframe loads.
        setPreviewHtml(result.html);
      } else {
        const errorMsg =
          typeof result.error === 'string'
            ? result.error
            : JSON.stringify(result.error, null, 2) || 'Unknown error';
        setPreviewHtml(renderError(qmdContent, errorMsg, result.diagnostics));
      }
    } catch (err) {
      const errorMsg =
        err instanceof Error ? err.message : JSON.stringify(err, null, 2);
      setPreviewHtml(renderError(qmdContent, errorMsg));
    }
  }, []);

  // Debounced render update
  const updatePreview = useCallback((newContent: string) => {
    if (renderTimeoutRef.current) {
      clearTimeout(renderTimeoutRef.current);
    }
    renderTimeoutRef.current = window.setTimeout(() => {
      doRender(newContent);
    }, 300);
  }, [doRender]);

  // Re-render when content changes or WASM becomes ready
  useEffect(() => {
    updatePreview(content);
  }, [content, updatePreview, wasmStatus]);

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
  const handleEditorMount = (editor: Monaco.editor.IStandaloneCodeEditor) => {
    editorRef.current = editor;
    onPresenceEditorMount(editor);
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const file = files.find(f => f.path === e.target.value);
    if (file) {
      setCurrentFile(file);
      const fileContent = fileContents.get(file.path);
      setContent(fileContent ?? '');
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
        <button className="disconnect-btn" onClick={onDisconnect}>
          Disconnect
        </button>
      </header>

      {wasmError && (
        <div className="wasm-error-banner">
          Failed to load WASM: {wasmError}
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
          <iframe
            ref={iframeRef}
            srcDoc={previewHtml}
            title="Preview"
            sandbox="allow-same-origin"
            onLoad={handleLoad}
          />
        </div>
      </main>
    </div>
  );
}
