import { useState, useCallback, useRef, useEffect } from 'react';
import MonacoEditor from '@monaco-editor/react';
import type { ProjectEntry, FileEntry } from '../types/project';
import { initWasm, renderToHtml, isWasmReady } from '../services/wasmRenderer';
import './Editor.css';

interface Props {
  project: ProjectEntry;
  files: FileEntry[];
  fileContents: Map<string, string>;
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

export default function Editor({ project, files, fileContents, onDisconnect, onContentChange }: Props) {
  const [currentFile, setCurrentFile] = useState<FileEntry | null>(selectDefaultFile(files));

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
        // Wrap the rendered HTML in a full document with styles
        const html = `
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
                h1, h2, h3, h4, h5, h6 {
                  margin-top: 1.5em;
                  margin-bottom: 0.5em;
                }
                p { margin: 1em 0; }
                pre {
                  background: #f4f4f4;
                  padding: 16px;
                  border-radius: 4px;
                  overflow-x: auto;
                }
                code {
                  font-family: 'SF Mono', Monaco, Consolas, monospace;
                  font-size: 0.9em;
                }
                :not(pre) > code {
                  background: #f0f0f0;
                  padding: 2px 6px;
                  border-radius: 3px;
                }
                ul, ol { margin: 1em 0; padding-left: 2em; }
                li { margin: 0.25em 0; }
                blockquote {
                  margin: 1em 0;
                  padding-left: 1em;
                  border-left: 4px solid #ddd;
                  color: #666;
                }
                a { color: #0066cc; }
                table {
                  border-collapse: collapse;
                  width: 100%;
                  margin: 1em 0;
                }
                th, td {
                  border: 1px solid #ddd;
                  padding: 8px 12px;
                  text-align: left;
                }
                th { background: #f4f4f4; }
              </style>
            </head>
            <body>${result.html}</body>
          </html>
        `;
        setPreviewHtml(html);
      } else {
        const errorMsg = typeof result.error === 'string'
          ? result.error
          : JSON.stringify(result.error, null, 2) || 'Unknown error';
        setPreviewHtml(renderError(qmdContent, errorMsg, result.diagnostics));
      }
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : JSON.stringify(err, null, 2);
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

  // Update content when file selection changes or fileContents updates
  useEffect(() => {
    if (currentFile) {
      const newContent = fileContents.get(currentFile.path);
      if (newContent !== undefined && newContent !== content) {
        setContent(newContent);
      }
    }
  }, [currentFile, fileContents]);

  // Update currentFile when files list changes (e.g., on initial load)
  useEffect(() => {
    if (!currentFile && files.length > 0) {
      setCurrentFile(selectDefaultFile(files));
    }
  }, [files, currentFile]);

  const handleEditorChange = (value: string | undefined) => {
    if (value !== undefined && currentFile) {
      setContent(value);
      onContentChange(currentFile.path, value);
    }
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
          <span className={`sync-status ${wasmStatus === 'ready' ? 'connected' : 'disconnected'}`}>
            {wasmStatus === 'loading' && 'Loading WASM...'}
            {wasmStatus === 'ready' && 'Ready'}
            {wasmStatus === 'error' && 'WASM Error'}
          </span>
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
          />
        </div>
      </main>
    </div>
  );
}
