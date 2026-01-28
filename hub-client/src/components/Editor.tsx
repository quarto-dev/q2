import { useState, useCallback, useRef, useEffect } from 'react';
import MonacoEditor from '@monaco-editor/react';
import type * as Monaco from 'monaco-editor';
import type { ProjectEntry, FileEntry } from '../types/project';
import { isBinaryExtension } from '../types/project';
import {
  createFile,
  createBinaryFile,
  deleteFile,
  renameFile,
} from '../services/automergeSync';
import type { Diagnostic } from '../types/diagnostic';
import { initWasm, renderToHtml, isWasmReady, parseQmdToAst } from '../services/wasmRenderer';
import { registerIntelligenceProviders, disposeIntelligenceProviders } from '../services/monacoProviders';
import { processFileForUpload } from '../services/resourceService';
import { useIframePostProcessor } from '../hooks/useIframePostProcessor';
import { postProcessIframe } from '../utils/iframePostProcessor';
import { usePresence } from '../hooks/usePresence';
import { useScrollSync } from '../hooks/useScrollSync';
import { usePreference } from '../hooks/usePreference';
import { useIntelligence } from '../hooks/useIntelligence';
import { diffToMonacoEdits } from '../utils/diffToMonacoEdits';
import { diagnosticsToMarkers } from '../utils/diagnosticToMonaco';
import { stripAnsi } from '../utils/stripAnsi';
import { PreviewErrorOverlay } from './PreviewErrorOverlay';
import FileSidebar from './FileSidebar';
import NewFileDialog from './NewFileDialog';
import MinimalHeader from './MinimalHeader';
import SidebarTabs from './SidebarTabs';
import OutlinePanel from './OutlinePanel';
import ProjectTab from './tabs/ProjectTab';
import StatusTab from './tabs/StatusTab';
import SettingsTab from './tabs/SettingsTab';
import AboutTab from './tabs/AboutTab';
import './Editor.css';
import { extractHeadings } from './example-ast-usage';
import ReactAstRenderer from './ReactAstRenderer';

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

export default function Editor({ project, files, fileContents, onDisconnect, onContentChange }: Props) {
  const [currentFile, setCurrentFile] = useState<FileEntry | null>(selectDefaultFile(files));

  // Monaco editor instance ref
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);

  // Track current file path in a ref for Monaco providers (they need stable callbacks)
  const currentFilePathRef = useRef<string | null>(currentFile?.path ?? null);

  // Flag to prevent local changes from echoing back during remote edits
  const applyingRemoteRef = useRef(false);

  // Presence for collaborative cursors
  const { remoteUsers, userCount, onEditorMount: onPresenceEditorMount } = usePresence(currentFile?.path ?? null);

  // Intelligence for document outline
  const {
    symbols,
    loading: intelligenceLoading,
    error: intelligenceError,
    refresh: refreshIntelligence,
  } = useIntelligence({
    path: currentFile?.path ?? null,
    enableSymbols: true,
  });

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
  const [ast, setAst] = useState<string>('');
  // Track if we're waiting for inactive iframe to load before swapping
  const [swapPending, setSwapPending] = useState(false);
  // iframeRef points to the currently active iframe (for scroll sync and post-processing)
  const iframeRef = activeIframe === 'A' ? iframeARef : iframeBRef;
  const inactiveIframeRef = activeIframe === 'A' ? iframeBRef : iframeARef;

  // Keep ref in sync with state
  useEffect(() => {
    activeIframeRef.current = activeIframe;
  }, [activeIframe]);

  // Keep current file path ref in sync for Monaco providers
  useEffect(() => {
    currentFilePathRef.current = currentFile?.path ?? null;
  }, [currentFile]);

  // Diagnostics state for Monaco markers
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [unlocatedErrors, setUnlocatedErrors] = useState<Diagnostic[]>([]);

  // Preview state machine for error handling
  const [previewState, setPreviewState] = useState<PreviewState>('START');
  const [currentError, setCurrentError] = useState<CurrentError | null>(null);
  // Track previewState in a ref for use in callbacks
  const previewStateRef = useRef<PreviewState>('START');
  useEffect(() => {
    previewStateRef.current = previewState;
  }, [previewState]);

  // Scroll sync state (persisted in localStorage)
  const [scrollSyncEnabled, setScrollSyncEnabled] = usePreference('scrollSyncEnabled');
  // Track if editor has focus (to prevent scroll feedback loop)
  const editorHasFocusRef = useRef(false);
  // Track when editor is mounted (for scroll sync initialization)
  const [editorReady, setEditorReady] = useState(false);
  const [iframeLoadCount, setIframeLoadCount] = useState(0);

  // Monaco instance ref for setting markers
  const monacoRef = useRef<typeof Monaco | null>(null);

  // New file dialog state
  const [showNewFileDialog, setShowNewFileDialog] = useState(false);
  const [pendingUploadFiles, setPendingUploadFiles] = useState<File[]>([]);
  // Initial filename for new file dialog (e.g., from clicking a link to a non-existent file)
  const [newFileInitialName, setNewFileInitialName] = useState<string>('');

  // Pending anchor state for cross-document navigation
  // When navigating to another file with an anchor, we store the anchor and
  // the iframeLoadCount at the time of setting. We only scroll when the load
  // count increases (indicating the new content has loaded).
  const [pendingAnchor, setPendingAnchor] = useState<{
    anchor: string;
    loadCountAtSet: number;
  } | null>(null);

  // Editor drag-drop state for image insertion
  const [isEditorDragOver, setIsEditorDragOver] = useState(false);
  const pendingDropPositionRef = useRef<Monaco.IPosition | null>(null);

  // Scroll the preview to an anchor element
  // Note: We find the active iframe via DOM query instead of using activeIframe state
  // to avoid timing issues where the effect runs before activeIframe is updated
  const scrollToAnchor = useCallback((anchor: string) => {
    // Find the active iframe by its class rather than relying on state
    const activeIframeEl = document.querySelector('iframe.preview-active') as HTMLIFrameElement | null;
    const doc = activeIframeEl?.contentDocument;
    if (!doc) return;

    const element = doc.getElementById(anchor);
    if (element) {
      element.scrollIntoView({ behavior: 'instant', block: 'start' });
      // Scroll sync will automatically update the editor via scroll event listener
    }
    // If element doesn't exist, do nothing (no-op as specified)
  }, []);

  // Handler for .qmd link clicks and anchor clicks in the preview
  const handleQmdLinkClick = useCallback(
    (targetPath: string | null, anchor: string | null) => {
      // Case 1: Same-document anchor only (e.g., #section)
      if (!targetPath && anchor) {
        scrollToAnchor(anchor);
        return;
      }

      // Case 2: Link to a different document (with or without anchor)
      if (targetPath) {
        const file = files.find(
          (f) => f.path === targetPath || '/' + f.path === targetPath
        );

        if (file) {
          // Existing file - switch to it
          setCurrentFile(file);
          if (anchor) {
            // Store anchor and current load count to apply after new content loads
            setPendingAnchor({ anchor, loadCountAtSet: iframeLoadCount });
          }
        } else {
          // Non-existent file - open create dialog with pre-filled name
          // Strip leading slash for the dialog
          const filename = targetPath.startsWith('/') ? targetPath.slice(1) : targetPath;
          setNewFileInitialName(filename);
          setShowNewFileDialog(true);
        }
      }
    },
    [files, scrollToAnchor, iframeLoadCount]
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

    // Post-process the inactive iframe (CSS, images, link handlers)
    // This must happen BEFORE the swap to prevent layout shifts from image loading
    // on the visible iframe, which would cause scroll sync issues.
    if (inactiveIframeEl) {
      postProcessIframe(inactiveIframeEl, {
        currentFilePath: currentFile?.path ?? '',
        onQmdLinkClick: handleQmdLinkClick,
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
  }, [swapPending, iframeRef, inactiveIframeRef, currentFile, handleQmdLinkClick]);

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

  // Apply pending anchor after iframe loads new content (for cross-document navigation with anchors)
  useEffect(() => {
    // Only scroll when the load count has increased since the anchor was set
    // This ensures we wait for the new document to actually load
    if (pendingAnchor && iframeLoadCount > pendingAnchor.loadCountAtSet) {
      // Small delay to ensure content is fully rendered
      const timer = setTimeout(() => {
        scrollToAnchor(pendingAnchor.anchor);
        setPendingAnchor(null);
      }, 100);
      return () => clearTimeout(timer);
    }
  }, [pendingAnchor, iframeLoadCount, scrollToAnchor]);

  // Debounce rendering
  const renderTimeoutRef = useRef<number | null>(null);
  const lastContentRef = useRef<string>('');

  // Update document title based on current file
  useEffect(() => {
    if (currentFile) {
      // Extract just the filename from the path
      const filename = currentFile.path.split('/').pop() || currentFile.path;
      document.title = `${filename} — Quarto Hub`;
    } else {
      document.title = 'Quarto Hub';
    }
  }, [currentFile]);

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
  // We add a unique timestamp comment to ensure srcDoc always changes, forcing onLoad to fire.
  // Without this, if the rendered HTML matches what's already in the inactive iframe
  // (e.g., after undo), React won't update the DOM and onLoad won't fire, breaking the swap.
  const setInactiveHtml = useCallback((html: string) => {
    const uniqueHtml = html + `<!-- render-${Date.now()} -->`;
    if (activeIframeRef.current === 'A') {
      setIframeBHtml(uniqueHtml);
    } else {
      setIframeAHtml(uniqueHtml);
    }
  }, []);

  // Render function that uses WASM when available
  // Implements state machine transitions for error handling:
  // - On success: always transition to GOOD, swap to new content
  // - On error from START/ERROR_AT_START: show full error page
  // - On error from GOOD/ERROR_FROM_GOOD: keep last good HTML, show overlay
  const doRender = useCallback(async (qmdContent: string) => {
    lastContentRef.current = qmdContent;

    if (!isWasmReady()) {
      // For initial load before WASM is ready, load into inactive iframe and swap
      setInactiveHtml(renderFallback(qmdContent, 'Loading WASM renderer...'));
      setSwapPending(true);
      setDiagnostics([]);
      return;
    }

    try {
      // Enable source location tracking when scroll sync is enabled
      const result = await renderToHtml(qmdContent, {
        sourceLocation: scrollSyncEnabled,
      });
      const ast = await parseQmdToAst(qmdContent)
      setAst(ast)
      console.log('extractHeadings', await extractHeadings(qmdContent))
      // todo: 
      // - simplify AST to be readable
      // - make astWithSections function
      // - make css nicer
      // - make slide preview

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
        // Success: transition to GOOD state from any state
        setCurrentError(null);
        setPreviewState('GOOD');
        // Load new content into inactive iframe (will swap on load)
        setInactiveHtml(result.html);
        setSwapPending(true);
      } else {
        const errorMsg =
          typeof result.error === 'string'
            ? result.error
            : JSON.stringify(result.error, null, 2) || 'Unknown error';

        // Set current error for overlay
        setCurrentError({
          message: errorMsg,
          diagnostics: result.diagnostics,
        });

        const currentState = previewStateRef.current;
        if (currentState === 'START' || currentState === 'ERROR_AT_START') {
          // No good render yet - show full error page
          setPreviewState('ERROR_AT_START');
          setInactiveHtml(renderError(qmdContent, errorMsg));
          setSwapPending(true);
        } else {
          // Was GOOD or ERROR_FROM_GOOD - keep last good HTML, show overlay
          // DON'T swap iframes, DON'T change HTML content
          setPreviewState('ERROR_FROM_GOOD');
        }
      }
    } catch (err) {
      const errorMsg =
        err instanceof Error ? err.message : JSON.stringify(err, null, 2);

      // Set current error for overlay
      setCurrentError({
        message: errorMsg,
      });

      const currentState = previewStateRef.current;
      if (currentState === 'START' || currentState === 'ERROR_AT_START') {
        setPreviewState('ERROR_AT_START');
        setInactiveHtml(renderError(qmdContent, errorMsg));
        setSwapPending(true);
      } else {
        setPreviewState('ERROR_FROM_GOOD');
      }
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
    }, 30);
  }, [doRender]);

  // Re-render when content changes, WASM becomes ready, or scroll sync is toggled
  useEffect(() => {
    updatePreview(content);
  }, [content, updatePreview, wasmStatus, scrollSyncEnabled]);

  // Refresh intelligence (outline) when content changes
  // VFS is updated via Automerge callbacks, so we trigger refresh after content changes
  useEffect(() => {
    refreshIntelligence();
  }, [content, refreshIntelligence]);

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

  // Sync Monaco editor with external Automerge state using diff-based edits.
  //
  // This approach computes the diff between Monaco's current content and
  // Automerge's content, then applies minimal edits to preserve cursor position.
  // This is more robust than patch-based synchronization because:
  // 1. It doesn't depend on timing assumptions about when patches were computed
  // 2. It handles any divergence between Monaco and Automerge correctly
  // 3. Automerge's merged content is the authoritative source of truth
  //
  // Monaco is configured as uncontrolled (defaultValue instead of value), so
  // setContent() only updates React state for preview rendering - it won't
  // cause the wrapper to call setValue() and reset cursor position.
  //
  // Note: setState in effect is intentional - we're syncing with external state.
  useEffect(() => {
    if (!currentFile) return;

    const automergeContent = fileContents.get(currentFile.path);
    if (automergeContent === undefined) return;

    // Get Monaco's actual model content
    const model = editorRef.current?.getModel();
    const monacoContent = model?.getValue();

    // If Monaco isn't ready yet, just sync React state for preview
    if (monacoContent === undefined) {
      setContent(automergeContent);
      return;
    }

    // If Monaco content differs from Automerge, apply minimal edits
    if (monacoContent !== automergeContent) {
      const edits = diffToMonacoEdits(monacoContent, automergeContent);

      if (edits.length > 0 && editorRef.current) {
        // Mark that we're applying remote changes to prevent echo
        applyingRemoteRef.current = true;
        editorRef.current.executeEdits('remote-sync', edits);
        applyingRemoteRef.current = false;
      }
    }

    // Always update React state to keep preview in sync.
    // Since Monaco is uncontrolled, this won't affect editor content or cursor.
    setContent(automergeContent);
  }, [currentFile, fileContents]);

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

    // Register intelligence providers (DocumentSymbolProvider, FoldingRangeProvider)
    // The callback uses a ref so it always returns the current file path
    registerIntelligenceProviders(monaco, () => currentFilePathRef.current);

    // Track editor focus state for scroll sync
    editor.onDidFocusEditorText(() => {
      editorHasFocusRef.current = true;
    });
    editor.onDidBlurEditorText(() => {
      editorHasFocusRef.current = false;
    });

    // Attach drag-drop handlers to editor container
    const domNode = editor.getDomNode();
    if (domNode) {
      domNode.addEventListener('dragover', handleEditorDragOver);
      domNode.addEventListener('dragleave', handleEditorDragLeave);
      domNode.addEventListener('drop', handleEditorDrop);
    }

    // Signal that editor is ready for scroll sync
    setEditorReady(true);
  };

  // Handle symbol click from outline panel - navigate editor to symbol location
  const handleSymbolClick = useCallback((symbol: { range: { start: { line: number; character: number } } }) => {
    if (!editorRef.current) return;

    // Convert from 0-based LSP position to 1-based Monaco position
    const lineNumber = symbol.range.start.line + 1;
    const column = symbol.range.start.character + 1;

    // Move cursor and reveal the line
    editorRef.current.setPosition({ lineNumber, column });
    editorRef.current.revealLineInCenter(lineNumber);
    editorRef.current.focus();
  }, []);

  // Handle file selection from sidebar
  const handleSelectFile = useCallback((file: FileEntry) => {
    // Don't switch to binary files in the editor
    if (isBinaryExtension(file.path)) {
      // For now, just ignore binary file selection
      // Future: could show a preview panel for images
      return;
    }

    setCurrentFile(file);
    const fileContent = fileContents.get(file.path);
    setContent(fileContent ?? '');
    // Clear diagnostics when switching files
    setDiagnostics([]);
    setUnlocatedErrors([]);
    // Reset preview state machine when switching files
    setPreviewState('START');
    setCurrentError(null);
  }, [fileContents]);

  // Handle opening new file dialog
  const handleNewFile = useCallback(() => {
    setPendingUploadFiles([]);
    setShowNewFileDialog(true);
  }, []);

  // Handle closing the new file dialog
  // Note: We don't clear pendingDropPositionRef here because the upload
  // happens asynchronously after dialog close. It's cleared after insertion.
  const handleDialogClose = useCallback(() => {
    setShowNewFileDialog(false);
  }, []);

  // Handle files dropped on sidebar (open dialog with files pre-filled)
  const handleUploadFiles = useCallback((droppedFiles: File[]) => {
    setPendingUploadFiles(droppedFiles);
    setShowNewFileDialog(true);
  }, []);

  // Editor drag-drop handlers for image/file insertion
  const handleEditorDragOver = useCallback((e: DragEvent) => {
    // Handle external files OR internal file drags from sidebar
    const hasFiles = e.dataTransfer?.types.includes('Files');
    const hasInternalFile = e.dataTransfer?.types.includes('application/x-hub-file');

    if (!hasFiles && !hasInternalFile) return;

    e.preventDefault();
    e.stopPropagation();
    setIsEditorDragOver(true);
  }, []);

  const handleEditorDragLeave = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsEditorDragOver(false);
  }, []);

  const handleEditorDrop = useCallback((e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsEditorDragOver(false);

    // Check for internal file drag from sidebar first
    const internalData = e.dataTransfer?.getData('application/x-hub-file');
    if (internalData && editorRef.current) {
      try {
        const { path, type } = JSON.parse(internalData) as { path: string; type: 'image' | 'qmd' | 'other' };

        // Get editor position at drop point
        const target = editorRef.current.getTargetAtClientPoint(e.clientX, e.clientY);
        const position = target?.position ?? editorRef.current.getPosition();

        if (position && (type === 'image' || type === 'qmd')) {
          // Generate appropriate markdown
          let markdown: string;
          if (type === 'image') {
            markdown = `![](${path})`;
          } else {
            // For qmd files, use the filename as link text
            const fileName = path.split('/').pop() || path;
            markdown = `[${fileName}](${path})`;
          }

          // Insert markdown at drop position
          editorRef.current.executeEdits('file-drop', [{
            range: {
              startLineNumber: position.lineNumber,
              startColumn: position.column,
              endLineNumber: position.lineNumber,
              endColumn: position.column,
            },
            text: markdown,
            forceMoveMarkers: true,
          }]);

          // Update local content state to match
          const newContent = editorRef.current.getValue();
          setContent(newContent);
          if (currentFile) {
            onContentChange(currentFile.path, newContent);
          }
        }
        return; // Internal drag handled, don't process as external
      } catch {
        // Failed to parse internal data, fall through to external handling
      }
    }

    // Handle external file drop (from desktop)
    const files = Array.from(e.dataTransfer?.files ?? []);
    // Filter for image files only (for markdown insertion)
    const imageFiles = files.filter(f => f.type.startsWith('image/'));

    if (imageFiles.length > 0 && editorRef.current) {
      // Get editor position at drop point
      const target = editorRef.current.getTargetAtClientPoint(e.clientX, e.clientY);
      if (target?.position) {
        pendingDropPositionRef.current = target.position;
      } else {
        // Fall back to current cursor position
        pendingDropPositionRef.current = editorRef.current.getPosition();
      }
      // Open upload dialog with image files
      setPendingUploadFiles(imageFiles);
      setShowNewFileDialog(true);
    } else if (files.length > 0) {
      // Non-image files: upload without markdown insertion
      setPendingUploadFiles(files);
      setShowNewFileDialog(true);
    }
  }, [currentFile, onContentChange]);

  // Cleanup editor drag-drop listeners and Monaco providers on unmount
  useEffect(() => {
    return () => {
      const domNode = editorRef.current?.getDomNode();
      if (domNode) {
        domNode.removeEventListener('dragover', handleEditorDragOver);
        domNode.removeEventListener('dragleave', handleEditorDragLeave);
        domNode.removeEventListener('drop', handleEditorDrop);
      }
      // Clean up Monaco intelligence providers
      disposeIntelligenceProviders();
    };
  }, [handleEditorDragOver, handleEditorDragLeave, handleEditorDrop]);

  // Handle creating a new text file
  const handleCreateTextFile = useCallback(async (path: string, initialContent: string) => {
    try {
      await createFile(path, initialContent);
      // Select the newly created file
      const newFile: FileEntry = { path, docId: '' }; // docId will be set by automerge
      setCurrentFile(newFile);
      setContent(initialContent);
    } catch (err) {
      console.error('Failed to create file:', err);
    }
  }, []);

  // Handle uploading a binary file (with optional markdown insertion for images)
  const handleUploadBinaryFile = useCallback(async (file: File) => {
    try {
      const { content: binaryContent, mimeType } = await processFileForUpload(file);
      const result = await createBinaryFile(file.name, binaryContent, mimeType);

      // If this is an image and we have a pending drop position, insert markdown
      if (file.type.startsWith('image/') && pendingDropPositionRef.current && editorRef.current) {
        const position = pendingDropPositionRef.current;
        const markdown = `![](${result.path})`;

        editorRef.current.executeEdits('image-drop', [{
          range: {
            startLineNumber: position.lineNumber,
            startColumn: position.column,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          },
          text: markdown,
          forceMoveMarkers: true,
        }]);

        // Clear the pending position after insertion
        pendingDropPositionRef.current = null;

        // Update local content state to match
        const newContent = editorRef.current.getValue();
        setContent(newContent);
        if (currentFile) {
          onContentChange(currentFile.path, newContent);
        }
      }
    } catch (err) {
      console.error('Failed to upload file:', err);
      // Clear pending position on error too
      pendingDropPositionRef.current = null;
    }
  }, [currentFile, onContentChange]);

  // Handle deleting a file
  const handleDeleteFile = useCallback((file: FileEntry) => {
    try {
      deleteFile(file.path);
      // If we deleted the current file, select another one
      if (currentFile?.path === file.path) {
        const remaining = files.filter(f => f.path !== file.path);
        setCurrentFile(selectDefaultFile(remaining));
      }
    } catch (err) {
      console.error('Failed to delete file:', err);
    }
  }, [currentFile, files]);

  // Handle renaming a file
  const handleRenameFile = useCallback((file: FileEntry, newPath: string) => {
    try {
      renameFile(file.path, newPath);
      // If we renamed the current file, update the reference
      if (currentFile?.path === file.path) {
        setCurrentFile({ ...currentFile, path: newPath });
      }
    } catch (err) {
      console.error('Failed to rename file:', err);
      // Show error to user (could use a toast notification in the future)
      alert(`Failed to rename file: ${err instanceof Error ? err.message : String(err)}`);
    }
  }, [currentFile]);

  return (
    <div className="editor-container">
      <MinimalHeader
        currentFilePath={currentFile?.path ?? null}
        projectName={project.description}
        onChooseNewProject={onDisconnect}
      />

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
        <SidebarTabs>
          {(activeTab) => {
            switch (activeTab) {
              case 'files':
                return (
                  <FileSidebar
                    files={files}
                    currentFile={currentFile}
                    onSelectFile={handleSelectFile}
                    onNewFile={handleNewFile}
                    onUploadFiles={handleUploadFiles}
                    onDeleteFile={handleDeleteFile}
                    onRenameFile={handleRenameFile}
                  />
                );
              case 'outline':
                return (
                  <OutlinePanel
                    symbols={symbols}
                    onSymbolClick={handleSymbolClick}
                    loading={intelligenceLoading}
                    error={intelligenceError}
                  />
                );
              case 'project':
                return (
                  <ProjectTab
                    project={project}
                    onChooseNewProject={onDisconnect}
                  />
                );
              case 'status':
                return (
                  <StatusTab
                    wasmStatus={wasmStatus}
                    wasmError={wasmError}
                    userCount={userCount}
                    remoteUsers={remoteUsers}
                  />
                );
              case 'settings':
                return (
                  <SettingsTab
                    scrollSyncEnabled={scrollSyncEnabled}
                    onScrollSyncChange={setScrollSyncEnabled}
                  />
                );
              case 'about':
                return <AboutTab wasmStatus={wasmStatus} />;
              default:
                return null;
            }
          }}
        </SidebarTabs>
        <div className={`pane editor-pane${isEditorDragOver ? ' drag-over' : ''}`}>
          <MonacoEditor
            // Use key to force remount when switching files (resets editor state cleanly)
            key={currentFile?.path ?? ''}
            height="100%"
            language="markdown"
            theme="vs-dark"
            // Use defaultValue instead of value to make Monaco uncontrolled.
            // This prevents the wrapper from calling setValue() on re-renders,
            // which would reset cursor position. We manage content via executeEdits().
            defaultValue={content}
            onChange={handleEditorChange}
            onMount={handleEditorMount}
            options={{
              minimap: { enabled: false },
              fontSize: 14,
              lineNumbers: 'on',
              wordWrap: 'on',
              padding: { top: 16 },
              scrollBeyondLastLine: false,
              // Disable paste-as to prevent snippet expansion (e.g., URLs from browser
              // address bar being pasted with $0 appended). See quarto-dev/kyoto#3.
              pasteAs: { enabled: false },
            }}
          />
        </div>
        <div className="pane preview-pane">
          <ReactAstRenderer ast={ast} className="react-ast-renderer" />

          {/* Error overlay shown when error occurs after successful render */}
          <PreviewErrorOverlay
            error={currentError}
            visible={previewState === 'ERROR_FROM_GOOD'}
          />
        </div>
      </main>

      {/* New file dialog */}
      <NewFileDialog
        isOpen={showNewFileDialog}
        existingPaths={files.map(f => f.path)}
        onClose={() => {
          handleDialogClose();
          setNewFileInitialName(''); // Clear on close
        }}
        onCreateTextFile={handleCreateTextFile}
        onUploadBinaryFile={handleUploadBinaryFile}
        initialFiles={pendingUploadFiles}
        initialFilename={newFileInitialName}
      />
    </div>
  );
}
