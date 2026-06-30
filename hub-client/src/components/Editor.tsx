import { useState, useCallback, useRef, useEffect } from 'react';
import MonacoEditor from '@monaco-editor/react';
import type * as Monaco from 'monaco-editor';
import type { ProjectEntry, FileEntry } from '@quarto/preview-renderer/types/project';
import { isBinaryExtension, isTextExtension } from '@quarto/preview-renderer/types/project';
import type { Route } from '../utils/routing';
import { buildFullUrl, buildShareableUrl } from '../utils/routing';
import {
  createFile,
  createBinaryFile,
  deleteFile,
  renameFile,
  exportProjectAsZip,
  type EditorContentChange,
} from '@quarto/preview-runtime';
import { vfsAddFile, isWasmReady, clearCapture } from '@quarto/preview-runtime';
import { ClearCaptureControl } from './render/ClearCaptureControl';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import { useIntelligenceProviders } from '../hooks/useIntelligenceProviders';
import { registerQmdLanguage } from './quartoTheme';
import { processFileForUpload } from '../services/resourceService';
import { useProjectSearch } from '../services/search';
import { usePresence } from '../hooks/usePresence';
import { usePreference } from '../hooks/usePreference';
import { useTheme } from './ThemeContext';
import { useIntelligence } from '../hooks/useIntelligence';
import { useSlideThumbnails } from '../hooks/useSlideThumbnails';
import { useCursorToSlide } from '../hooks/useCursorToSlide';
import { useReplayMode } from '../hooks/useReplayMode';
import { useAutomergeSync } from '../hooks/useAutomergeSync';
import { diffToMonacoEdits } from '../utils/diffToMonacoEdits';
import { diagnosticsToMarkers } from '../utils/diagnosticToMonaco';
import FileSidebar from './FileSidebar';
import NewFileDialog from './NewFileDialog';
import NewAssetDialog from './NewAssetDialog';
import ShareDialog from './ShareDialog';
import MinimalHeader from './MinimalHeader';
import SidebarTabs from './SidebarTabs';
import OutlinePanel from './OutlinePanel';
import ProjectTab from './tabs/ProjectTab';
import StatusTab from './tabs/StatusTab';
import SettingsTab from './tabs/SettingsTab';
import AboutTab from './tabs/AboutTab';
import { useViewMode } from './ViewModeContext';
import MarkdownSummary from './MarkdownSummary';
import ReplayDrawer from './ReplayDrawer';
import './Editor.css';
import PreviewRouter from './render/PreviewRouter';

interface Props {
  project: ProjectEntry;
  files: FileEntry[];
  fileContents: Map<string, string>;
  onDisconnect: () => void;
  onContentOperations: (path: string, changes: EditorContentChange[]) => void;
  /** Current route from URL */
  route: Route;
  /** Callback to update URL when file changes */
  onNavigateToFile: (filePath: string, options?: { anchor?: string; replace?: boolean }) => void;
  /** Actor ID -> identity mapping from the IndexDocument */
  identities?: Record<string, import('@quarto/preview-runtime').ActorIdentity>;
  /** Path -> recorded engine capture sidecar entry (bd-sfet3264). */
  captures?: Record<string, import('@quarto/preview-runtime').CaptureRef>;
  /** Whether the project is connected to the sync server */
  isOnline: boolean;
}

// Map file extension to Monaco language ID
function getLanguageForFile(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase();
  switch (ext) {
    case 'tsx':
    case 'ts':
      return 'typescript';
    case 'jsx':
    case 'js':
      return 'javascript';
    case 'json':
      return 'json';
    case 'html':
      return 'html';
    case 'css':
      return 'css';
    case 'scss':
      return 'scss';
    case 'yaml':
    case 'yml':
      return 'yaml';
    case 'qmd':
      return 'qmd';
    case 'md':
      return 'markdown';
    default:
      return 'markdown';
  }
}

// Stable options object — defined at module level so @monaco-editor/react
// never sees a new reference and skips its internal updateOptions() effect.
const editorOptions = {
  minimap: { enabled: false },
  fontSize: 14,
  lineNumbers: 'on' as const,
  wordWrap: 'on' as const,
  padding: { top: 16 },
  scrollBeyondLastLine: false,
  // Disable paste-as to prevent snippet expansion (e.g., URLs from browser
  // address bar being pasted with $0 appended). See quarto-dev/kyoto#3.
  pasteAs: { enabled: false },
  fixedOverflowWidgets: true,
  // Prefer showing hover below the line — prevents diagnostic popups near
  // the top of the editor from overlapping the navbar.
  hover: { above: false },
  quickSuggestions: false,
  suggestOnTriggerCharacters: false,
  wordBasedSuggestions: 'off' as const,
  acceptSuggestionOnEnter: 'off' as const,
  acceptSuggestionOnCommitCharacter: false,
  suggest: { showWords: false, showSnippets: false },
  inlineSuggest: { enabled: false },
  // Enable the semantic-tokens layer (the quarto-{light,dark} themes also set
  // `semanticHighlighting: true`; setting both is the reliable combination).
  'semanticHighlighting.enabled': true as const,
};

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

export default function Editor({ project, files, fileContents, onDisconnect, onContentOperations, route, onNavigateToFile, identities, captures, isOnline }: Props) {
  // View mode for pane sizing
  const { viewMode } = useViewMode();
  const { effectiveTheme } = useTheme();

  // Full-text search index over the open project (Phase 1: client-side).
  const searchFiles = useProjectSearch(files, fileContents);

  // Select initial file based on URL route or default
  const getInitialFile = useCallback((): FileEntry | null => {
    if (route.type === 'file' && route.filePath) {
      const fileFromRoute = files.find(f => f.path === route.filePath);
      if (fileFromRoute) {
        return fileFromRoute;
      }
      // File not found - fall back to default
    }
    return selectDefaultFile(files);
  }, [files, route]);

  const [currentFile, setCurrentFile] = useState<FileEntry | null>(getInitialFile);

  // Ensure URL includes the current file path on initial render
  // This handles the case where the URL is just #/p/<id> and we select a default file
  const initialUrlSyncRef = useRef(false);
  useEffect(() => {
    if (initialUrlSyncRef.current) return;
    initialUrlSyncRef.current = true;

    // If route doesn't include a file but we have a currentFile, update the URL
    if (route.type === 'project' && currentFile) {
      onNavigateToFile(currentFile.path, { replace: true });
    }
  }, [route, currentFile, onNavigateToFile]);

  // Monaco editor instance ref
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);

  // Track current file path in a ref for Monaco providers (they need stable callbacks)
  const currentFilePathRef = useRef<string | null>(currentFile?.path ?? null);

  // Stable path getter for the Monaco intelligence providers (wired up by
  // useIntelligenceProviders below, once wasmStatus/editorReady exist).
  const getCurrentFilePath = useCallback(() => currentFilePathRef.current, []);

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

  // Replay mode for document history.
  // isActiveRef is updated synchronously in enter()/exit() — before React
  // re-renders — so it can guard handleEditorChange against stale closures.
  const { state: replayState, controls: replayControls, isActiveRef: replayActiveRef } = useReplayMode(currentFile?.path ?? null);

  // Bidirectional Automerge ↔ Monaco sync
  const {
    content,
    setContent,
    applyingRemoteRef,
    handleEditorChange,
    handleContentRewrite,
    onEditorMount: onSyncEditorMount,
  } = useAutomergeSync({
    currentFile,
    fileContents,
    onContentOperations,
    replayActiveRef,
    replayIsActive: replayState.isActive,
  });

  // Keep current file path ref in sync for Monaco providers
  useEffect(() => {
    currentFilePathRef.current = currentFile?.path ?? null;
  }, [currentFile]);

  // Diagnostics state for Monaco markers
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [unlocatedErrors, setUnlocatedErrors] = useState<Diagnostic[]>([]);

  // WASM status (from Preview component)
  const [wasmStatus, setWasmStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [wasmError, setWasmError] = useState<string | null>(null);

  // Scroll sync state (persisted in localStorage)
  const [scrollSyncEnabled, setScrollSyncEnabled] = usePreference('scrollSyncEnabled');

  // Attribution overlay — session-only useState (not persisted).
  // Owned here, surfaced via the toggle in the replay bar, and
  // threaded into ReactPreview where `useAttribution` consumes it
  // as the `enabled` flag. Treated as an inspection mode rather
  // than a setting: resets on reload so a previously-curious view
  // doesn't bleed into the next session.
  const [attributionOn, setAttributionOn] = useState(false);
  // `useAttribution` (inside ReactPreview) reports whether it's
  // mid-build via `onAttributionGeneratingChange`; the flag drives
  // the rotating-gradient border on the Attribution pill so a slow
  // run-list build on a large document is visible to the user.
  const [attributionGenerating, setAttributionGenerating] = useState(false);
  // Track if editor has focus (to prevent scroll feedback loop)
  const editorHasFocusRef = useRef(false);
  // Track when editor is mounted (for scroll sync initialization)
  const [editorReady, setEditorReady] = useState(false);

  // Monaco intelligence providers (symbols, folding, semantic tokens). Here
  // rather than above because it needs wasmStatus/editorReady.
  const registerIntelligenceProvidersOnMount = useIntelligenceProviders(
    getCurrentFilePath,
    wasmStatus,
    editorReady
  );

  // Monaco instance ref for setting markers
  const monacoRef = useRef<typeof Monaco | null>(null);

  // Content version for triggering thumbnail regeneration
  const [contentVersion, setContentVersion] = useState(0);

  // AST JSON for slide thumbnails
  const [astJson, setAstJson] = useState<string | null>(null);

  // Current document format (e.g., 'q2-slides', 'q2-debug', or null for default)
  const [currentFormat, setCurrentFormat] = useState<string | null>(null);

  // Handle format changes from PreviewRouter
  const handleFormatChange = useCallback((format: string | null) => {
    setCurrentFormat(format);
  }, []);

  // Generate thumbnails for slides (only when format is q2-slides)
  const thumbnails = useSlideThumbnails({
    astJson,
    currentFilePath: currentFile?.path ?? '',
    symbols,
    previewReady: wasmStatus === 'ready' && editorReady,
    contentVersion,
    enabled: currentFormat === 'q2-slides',
  });

  // New file dialog state (text-only after Phase C of generic-file-uploader plan)
  const [showNewFileDialog, setShowNewFileDialog] = useState(false);
  // Initial filename for new file dialog (e.g., from clicking a link to a non-existent file)
  const [newFileInitialName, setNewFileInitialName] = useState<string>('');

  // Asset upload dialog state
  const [showNewAssetDialog, setShowNewAssetDialog] = useState(false);
  const [assetInitialFiles, setAssetInitialFiles] = useState<File[]>([]);
  const [assetDestination, setAssetDestination] = useState<string>('');

  // Share dialog state
  const [showShareDialog, setShowShareDialog] = useState(false);

  // Preview scroll functions for external control (from MarkdownSummary / replay)
  const previewScrollToLineRef = useRef<((line: number) => void) | null>(null);
  const previewSetScrollRatioRef = useRef<((ratio: number) => void) | null>(null);
  // q2-preview only: deferred scroll-to-line used by replay so scrubbing
  // shares the render-deferred scroll mechanism of normal q2-preview editing.
  const previewReplayScrollRef = useRef<((line: number) => void) | null>(null);

  // Editor drag-drop state for image insertion
  const [isEditorDragOver, setIsEditorDragOver] = useState(false);
  const pendingDropPositionRef = useRef<Monaco.IPosition | null>(null);

  // Fullscreen preview mode
  const [isFullscreenPreview, setIsFullscreenPreview] = useState(false);

  // Current slide index (for cursor-driven slide navigation)
  const [currentSlideIndex, setCurrentSlideIndex] = useState<number>(0);

  // Map cursor position to slide index
  const getSlideForLine = useCursorToSlide(astJson, symbols);

  // Keep getSlideForLine in a ref so the cursor listener always has the latest version
  const getSlideForLineRef = useRef(getSlideForLine);
  useEffect(() => {
    getSlideForLineRef.current = getSlideForLine;
  }, [getSlideForLine]);

  // Handle manual slide changes (from arrow keys or buttons in preview)
  const handleSlideChange = useCallback((slideIndex: number) => {
    setCurrentSlideIndex(slideIndex);
  }, []);

  // Toggle fullscreen preview mode
  const handleToggleFullscreenPreview = useCallback(() => {
    setIsFullscreenPreview(prev => !prev);
  }, []);

  // Callback for when preview wants to change file (via link click - adds history)
  const handlePreviewFileChange = useCallback((file: FileEntry, anchor?: string) => {
    setCurrentFile(file);
    const fileContent = fileContents.get(file.path);
    setContent(fileContent ?? '');
    // Clear diagnostics when switching files
    setDiagnostics([]);
    setUnlocatedErrors([]);
    // Update URL with history entry (link navigation)
    onNavigateToFile(file.path, { anchor, replace: false });
  }, [fileContents, onNavigateToFile]);

  // Callback for when preview wants to open new file dialog
  const handlePreviewOpenNewFileDialog = useCallback((initialFilename: string) => {
    setNewFileInitialName(initialFilename);
    setShowNewFileDialog(true);
  }, []);

  // Callback for when preview updates diagnostics
  const handleDiagnosticsChange = useCallback((newDiagnostics: Diagnostic[]) => {
    setDiagnostics(newDiagnostics);
  }, []);

  // Callback for when preview WASM status changes
  const handleWasmStatusChange = useCallback((status: 'loading' | 'ready' | 'error', error: string | null) => {
    setWasmStatus(status);
    setWasmError(error);
  }, []);

  // Callback for when preview AST changes
  const handleAstChange = useCallback((newAstJson: string | null) => {
    setAstJson(newAstJson);
    // Increment content version to trigger thumbnail regeneration
    setContentVersion(prev => prev + 1);
  }, []);

  // Callbacks for preview to register scroll functions
  const handleRegisterScrollToLine = useCallback((fn: (line: number) => void) => {
    previewScrollToLineRef.current = fn;
  }, []);
  const handleRegisterSetScrollRatio = useCallback((fn: (ratio: number) => void) => {
    previewSetScrollRatioRef.current = fn;
  }, []);
  const handleRegisterReplayScroll = useCallback((fn: (line: number) => void) => {
    previewReplayScrollRef.current = fn;
  }, []);

  // Update document title based on current file and project
  useEffect(() => {
    if (currentFile) {
      // Extract just the filename from the path
      const filename = currentFile.path.split('/').pop() || currentFile.path;
      document.title = `${filename} — ${project.description} — Quarto Hub`;
    } else {
      document.title = `${project.description} — Quarto Hub`;
    }
  }, [currentFile, project.description]);

  // Toggle Monaco read-only mode during replay
  useEffect(() => {
    if (!editorRef.current) return;
    const readOnly = replayState.isActive;
    editorRef.current.updateOptions({ readOnly, domReadOnly: readOnly });
  }, [replayState.isActive]);

  // Content for preview and MarkdownSummary: show replay content when active,
  // otherwise the normal Automerge-synced content. We keep `content` state
  // untouched during replay so that when replay exits, the Automerge sync
  // effect's setContent(automergeContent) always produces a state change.
  const displayContent = replayState.isActive ? replayState.currentContent : content;

  // When replay content changes, update Monaco and VFS for display.
  // Writing to VFS ensures the preview renderer sees historical content.
  // We also move the cursor to the first changed line so that both the
  // editor and preview scroll to the relevant location automatically.
  const prevReplayContentRef = useRef<string>('');
  const prevReplayIndexRef = useRef<number>(0);
  useEffect(() => {
    if (!replayState.isActive) return;

    const editor = editorRef.current;
    const model = editor?.getModel();
    if (model && editor) {
      const prevContent = prevReplayContentRef.current;
      const newContent = replayState.currentContent;

      if (prevContent !== newContent) {
        const indexDelta = Math.abs(replayState.currentIndex - prevReplayIndexRef.current);
        applyingRemoteRef.current = true;
        if (indexDelta === 1 && prevContent !== '') {
          // Single-step change (playback or manual step) — use applyEdits
          // for minimal incremental changes. This preserves tokenization
          // in unchanged regions and avoids the full re-layout that
          // setValue() triggers.
          const edits = diffToMonacoEdits(prevContent, newContent);
          if (edits.length > 0) {
            model.applyEdits(edits);
          }
        } else {
          // Initial entry or large jump (scrubbing) — use setValue
          model.setValue(newContent);
        }
        applyingRemoteRef.current = false;

        // Find the first line that differs and scroll both panes there
        let changedLine = 1;
        const minLen = Math.min(prevContent.length, newContent.length);
        let ci = 0;
        for (; ci < minLen; ci++) {
          if (prevContent[ci] !== newContent[ci]) break;
          if (prevContent[ci] === '\n') changedLine++;
        }
        editor.revealLineInCenter(changedLine);
        // q2-preview: scroll to the changed line through the same
        // render-deferred mechanism as normal editing (no-op if the HTML
        // preview is active, which uses the ratio path below instead).
        previewReplayScrollRef.current?.(changedLine);
        requestAnimationFrame(() => {
          const maxScroll = editor.getScrollHeight() - editor.getLayoutInfo().height;
          if (maxScroll > 0) {
            previewSetScrollRatioRef.current?.(editor.getScrollTop() / maxScroll);
          }
        });
      }
    }
    prevReplayContentRef.current = replayState.currentContent;
    prevReplayIndexRef.current = replayState.currentIndex;

    // Update VFS so the WASM renderer sees the historical content
    if (currentFile && isWasmReady()) {
      vfsAddFile(currentFile.path, replayState.currentContent);
    }
  }, [replayState.isActive, replayState.currentContent, replayState.currentIndex, currentFile]);

  // Restore VFS when replay exits — the Automerge sync effect restores Monaco
  // and React state, but doesn't re-write VFS unless fileContents changed.
  const prevReplayActiveRef = useRef(false);
  useEffect(() => {
    const wasActive = prevReplayActiveRef.current;
    prevReplayActiveRef.current = replayState.isActive;

    if (wasActive && !replayState.isActive) {
      prevReplayContentRef.current = '';
      prevReplayIndexRef.current = 0;
      if (currentFile && isWasmReady()) {
        const liveContent = fileContents.get(currentFile.path);
        if (liveContent !== undefined) {
          vfsAddFile(currentFile.path, liveContent);
        }
      }
    }
  }, [replayState.isActive, currentFile, fileContents]);

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

  // Update currentFile when files list changes (e.g., on initial load)
  // Note: setState in effect is intentional - syncing with external file list
  useEffect(() => {
    if (!currentFile && files.length > 0) {

      setCurrentFile(selectDefaultFile(files));
    }
  }, [files, currentFile]);

  // Handle browser back/forward navigation for file changes
  // Note: setState in effect is intentional - syncing with external route state
  const prevRouteFilePathRef = useRef<string | undefined>(
    route.type === 'file' ? route.filePath : undefined
  );
  useEffect(() => {
    const prevFilePath = prevRouteFilePathRef.current;
    const newFilePath = route.type === 'file' ? route.filePath : undefined;
    prevRouteFilePathRef.current = newFilePath;

    // Only handle if file path actually changed (browser navigation)
    if (newFilePath && newFilePath !== prevFilePath) {
      const targetFile = files.find(f => f.path === newFilePath);
      if (targetFile && targetFile.path !== currentFile?.path) {
        // Don't switch to binary files
        if (!isBinaryExtension(targetFile.path)) {
          setCurrentFile(targetFile);
          const fileContent = fileContents.get(targetFile.path);
          setContent(fileContent ?? '');
          setDiagnostics([]);
          setUnlocatedErrors([]);
        }
      }
    }
  }, [route, files, fileContents, currentFile]);

  // Configure Monaco before mount (for TypeScript diagnostics)
  const handleBeforeMount = (monaco: typeof Monaco) => {
    // Register the dedicated `qmd` language: Monarch base (Hybrid-A instant
    // paint + nextEmbedded routing of code/frontmatter) + quarto-{light,dark}
    // themes. The semantic-tokens provider is the authoritative colour source.
    registerQmdLanguage(monaco);

    // Disable TypeScript diagnostics to avoid noisy errors in TSX/TS files
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const languages = (monaco.languages as any);
    try {
      if (languages.typescript?.typescriptDefaults) {
        // Enable JSX support
        languages.typescript.typescriptDefaults.setCompilerOptions({
          jsx: languages.typescript.JsxEmit.React,
          jsxFactory: 'React.createElement',
          reactNamespace: 'React',
          allowNonTsExtensions: true,
          allowJs: true,
        });
        languages.typescript.typescriptDefaults.setDiagnosticsOptions({
          noSemanticValidation: true,
          noSyntaxValidation: true, // Disable all validation
        });
      }
      if (languages.typescript?.javascriptDefaults) {
        languages.typescript.javascriptDefaults.setCompilerOptions({
          jsx: languages.typescript.JsxEmit.React,
          jsxFactory: 'React.createElement',
          reactNamespace: 'React',
          allowNonTsExtensions: true,
        });
        languages.typescript.javascriptDefaults.setDiagnosticsOptions({
          noSemanticValidation: true,
          noSyntaxValidation: true, // Disable all validation
        });
      }
    } catch (e) {
      // Ignore if TypeScript language service is not available
      console.warn('Could not configure TypeScript diagnostics:', e);
    }
  };

  // Capture Monaco editor instance on mount
  const handleEditorMount = (editor: Monaco.editor.IStandaloneCodeEditor, monaco: typeof Monaco) => {
    editorRef.current = editor;
    monacoRef.current = monaco;
    onSyncEditorMount(editor);
    onPresenceEditorMount(editor);

    // Register intelligence providers (symbols, folding, semantic tokens).
    // The getter uses a ref so it always returns the current file path.
    registerIntelligenceProvidersOnMount(monaco);

    // Track editor focus state for scroll sync
    editor.onDidFocusEditorText(() => {
      editorHasFocusRef.current = true;
    });
    editor.onDidBlurEditorText(() => {
      editorHasFocusRef.current = false;
    });

    // Workaround: normalize backward (RTL) selections to forward (LTR) on
    // any printable keyDown. On some platforms the browser's input pipeline
    // silently drops the first character typed into a backward selection
    // because the hidden textarea's selection state confuses the OS input
    // method. Flipping to LTR keeps the same highlighted range but places
    // the cursor at the end, which the input method handles reliably.
    editor.onKeyDown((e) => {
      const sel = editor.getSelection();
      if (!sel || sel.isEmpty() || sel.getDirection() === 0) return; // LTR or no selection
      // Only intervene for printable characters (single char from e.browserEvent.key)
      const key = e.browserEvent.key;
      if (!key || key.length !== 1) return;
      // Flip to LTR (same range, cursor moves to end)
      editor.setSelection({
        selectionStartLineNumber: sel.startLineNumber,
        selectionStartColumn: sel.startColumn,
        positionLineNumber: sel.endLineNumber,
        positionColumn: sel.endColumn,
      });
    });

    // Track cursor position changes for slide navigation
    editor.onDidChangeCursorPosition((e) => {
      // Get the cursor line (0-based in Monaco)
      const line = e.position.lineNumber - 1; // Convert to 0-based for our mapping
      const slideIndex = getSlideForLineRef.current(line);
      // Only update if we have a valid slide mapping (null means AST is invalid)
      if (slideIndex !== null) {
        setCurrentSlideIndex(slideIndex);
      }
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

  // Handle file selection from sidebar (uses replaceState - no history entry)
  const handleSelectFile = useCallback((file: FileEntry) => {
    // Block file switching during replay mode
    if (replayState.isActive) return;
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
    // Update URL without adding history entry (sidebar navigation)
    onNavigateToFile(file.path, { replace: true });
  }, [fileContents, onNavigateToFile, replayState.isActive]);

  // Handle opening a file in a new browser tab
  const handleOpenInNewTab = useCallback((file: FileEntry) => {
    const url = buildFullUrl({
      type: 'file',
      projectId: project.id,
      filePath: file.path,
    });
    window.open(url, '_blank');
  }, [project.id]);

  // Handle copying a file link to clipboard
  const handleCopyLink = useCallback(async (file: FileEntry) => {
    const url = buildFullUrl({
      type: 'file',
      projectId: project.id,
      filePath: file.path,
    });
    try {
      await navigator.clipboard.writeText(url);
      // Could show a toast notification here in the future
    } catch (err) {
      console.error('Failed to copy link:', err);
    }
  }, [project.id]);

  // Handle opening share dialog
  const handleShare = useCallback(() => {
    setShowShareDialog(true);
  }, []);

  // Build shareable URL for the current file
  const shareableUrl = currentFile
    ? buildShareableUrl(
        project.indexDocId,
        project.syncServer,
        project.description,
        currentFile.path
      )
    : undefined;

  // Handle opening new file dialog (text files only after Phase C).
  // Pre-fill the filename with the current file's directory path
  const handleNewFile = useCallback(() => {
    if (currentFile) {
      const lastSlash = currentFile.path.lastIndexOf('/');
      if (lastSlash >= 0) {
        setNewFileInitialName(currentFile.path.substring(0, lastSlash + 1));
      }
    }
    setShowNewFileDialog(true);
  }, [currentFile]);

  // Handle closing the new file dialog
  const handleDialogClose = useCallback(() => {
    setShowNewFileDialog(false);
  }, []);

  // Open the asset dialog with the given files and destination. Invoked by
  // FileSidebar (its Upload button and its drop handler).
  const handleUploadFiles = useCallback(
    (droppedFiles: File[], destination: string) => {
      setAssetInitialFiles(droppedFiles);
      setAssetDestination(destination);
      setShowNewAssetDialog(true);
    },
    []
  );

  const handleAssetDialogClose = useCallback(() => {
    setShowNewAssetDialog(false);
    setAssetInitialFiles([]);
    // Note: we don't clear pendingDropPositionRef here — editor-drop image
    // uploads happen inside handleUploadAsset and clear it after insertion.
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
          // Monaco's onChange fires synchronously from executeEdits,
          // updating both React state and CRDT via the splice path.
        }
        return; // Internal drag handled, don't process as external
      } catch {
        // Failed to parse internal data, fall through to external handling
      }
    }

    // Handle external file drop (from desktop). All editor drops route to
    // the asset dialog. For image drops we stash the drop position so that
    // after the upload completes, handleUploadAsset can insert a markdown
    // image reference at the drop point.
    const files = Array.from(e.dataTransfer?.files ?? []);
    if (files.length === 0) return;

    const hasImages = files.some(f => f.type.startsWith('image/'));
    if (hasImages && editorRef.current) {
      const target = editorRef.current.getTargetAtClientPoint(e.clientX, e.clientY);
      pendingDropPositionRef.current = target?.position ?? editorRef.current.getPosition();
    }

    setAssetInitialFiles(files);
    setAssetDestination('');
    setShowNewAssetDialog(true);
  }, [currentFile]);

  // Cleanup editor drag-drop listeners on unmount. Note: intelligence-provider
  // disposal lives in useIntelligenceProviders (mount-only) — it must NOT be
  // coupled to `handleEditorDrop`, whose identity changes with `currentFile`.
  useEffect(() => {
    return () => {
      const domNode = editorRef.current?.getDomNode();
      if (domNode) {
        domNode.removeEventListener('dragover', handleEditorDragOver);
        domNode.removeEventListener('dragleave', handleEditorDragLeave);
        domNode.removeEventListener('drop', handleEditorDrop);
      }
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

  // Handle uploading an asset (text files go through text creation,
  // everything else becomes a binary file). `targetPath` is the already-
  // validated final path (e.g. "_quarto/grammars/toml/toml.wasm").
  const handleUploadAsset = useCallback(async (file: File, targetPath: string) => {
    try {
      // Text files must be created as text documents so the editor can display them
      if (isTextExtension(targetPath)) {
        const textContent = await file.text();
        handleCreateTextFile(targetPath, textContent);
        return;
      }

      const { content: binaryContent, mimeType } = await processFileForUpload(file);
      const result = await createBinaryFile(targetPath, binaryContent, mimeType);

      // If this is an image and we have a pending drop position (editor drop),
      // insert a markdown image reference at the drop point.
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

        pendingDropPositionRef.current = null;
      }
    } catch (err) {
      console.error('Failed to upload file:', err);
      pendingDropPositionRef.current = null;
    }
  }, [handleCreateTextFile]);

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
      {!isFullscreenPreview && (
        <div className="header-wrapper">
          <MinimalHeader
            currentFilePath={currentFile?.path ?? null}
            projectName={project.description}
            onChooseNewProject={onDisconnect}
            onShare={handleShare}
            onToggleFullscreenPreview={handleToggleFullscreenPreview}
            isFullscreenPreview={isFullscreenPreview}
            isOnline={isOnline}
          />
          {replayState.isActive && (
            <div className="replay-mode-banner">REPLAY MODE</div>
          )}
        </div>
      )}

      {!isFullscreenPreview && unlocatedErrors.length > 0 && (
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

      <main className={`editor-main view-mode-${viewMode}`}>
        {!isFullscreenPreview && (
          <SidebarTabs disabled={replayState.isActive}>
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
                      onOpenInNewTab={handleOpenInNewTab}
                      onCopyLink={handleCopyLink}
                      searchFiles={searchFiles}
                      fileContents={fileContents}
                    />
                  );
                case 'outline':
                  return (
                    <OutlinePanel
                      symbols={symbols}
                      onSymbolClick={handleSymbolClick}
                      loading={intelligenceLoading}
                      error={intelligenceError}
                      thumbnails={thumbnails}
                    />
                  );
                case 'project':
                  return (
                    <ProjectTab
                      project={project}
                      onChooseNewProject={onDisconnect}
                      onExportZip={exportProjectAsZip}
                    />
                  );
                case 'status':
                  return (
                    <StatusTab
                      wasmStatus={wasmStatus}
                      wasmError={wasmError}
                      userCount={userCount}
                      remoteUsers={remoteUsers}
                      isOnline={isOnline}
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
        )}
        {!isFullscreenPreview && (
          <div className={`pane editor-pane${isEditorDragOver ? ' drag-over' : ''}`}>
            {/* Show MarkdownSummary overlay in preview mode */}
            {viewMode === 'preview' && (
              <div className="markdown-summary-overlay">
                <MarkdownSummary
                  content={displayContent}
                  onLineClick={(lineNumber) => {
                    if (previewScrollToLineRef.current) {
                      previewScrollToLineRef.current(lineNumber);
                    }
                  }}
                />
              </div>
            )}
            {/* Always render Monaco but hide in preview mode */}
            <div style={{ display: viewMode === 'preview' ? 'none' : 'block', height: '100%' }}>
              <MonacoEditor
                // Use key to force remount when switching files (resets editor state cleanly)
                key={currentFile?.path ?? ''}
                height="100%"
                language={getLanguageForFile(currentFile?.path ?? '')}
                theme={effectiveTheme === 'dark' ? 'quarto-dark' : 'quarto-light'}
                // Use defaultValue instead of value to make Monaco uncontrolled.
                // This prevents the wrapper from calling setValue() on re-renders,
                // which would reset cursor position. We manage content via executeEdits().
                defaultValue={content}
                beforeMount={handleBeforeMount}
                onChange={handleEditorChange}
                onMount={handleEditorMount}
                options={editorOptions}
              />
            </div>
          </div>
        )}

        {/* Pane divider */}
        {!isFullscreenPreview && (
          <div className="pane-divider" />
        )}

        <div className={`pane preview-pane${isFullscreenPreview ? ' fullscreen' : ''}`}>
          {isFullscreenPreview && (
            <button
              className="fullscreen-close-btn"
              onClick={handleToggleFullscreenPreview}
              aria-label="Exit fullscreen preview"
            >
              ✕
            </button>
          )}
          <ClearCaptureControl
            path={currentFile?.path ?? null}
            hasCapture={!!(currentFile && captures?.[currentFile.path])}
            onClear={(p) => clearCapture(p)}
          />
          <PreviewRouter
            content={displayContent}
            currentFile={currentFile}
            files={files}
            fileContents={fileContents}
            scrollSyncEnabled={scrollSyncEnabled}
            editorRef={editorRef}
            editorReady={editorReady}
            editorHasFocusRef={editorHasFocusRef}
            onFileChange={handlePreviewFileChange}
            onOpenNewFileDialog={handlePreviewOpenNewFileDialog}
            onDiagnosticsChange={handleDiagnosticsChange}
            onWasmStatusChange={handleWasmStatusChange}
            onRegisterScrollToLine={handleRegisterScrollToLine}
            onRegisterSetScrollRatio={handleRegisterSetScrollRatio}
            onRegisterReplayScroll={handleRegisterReplayScroll}
            onAstChange={handleAstChange}
            currentSlideIndex={currentSlideIndex}
            onSlideChange={handleSlideChange}
            onFormatChange={handleFormatChange}
            onContentRewrite={handleContentRewrite}
            identities={identities}
            captures={captures}
            attributionOn={attributionOn}
            onAttributionGeneratingChange={setAttributionGenerating}
          />
        </div>
      </main>

      {/* Replay mode drawer */}
      {!isFullscreenPreview && (
        <ReplayDrawer
          state={replayState}
          controls={replayControls}
          disabled={!!currentFile && isBinaryExtension(currentFile.path)}
          identities={identities}
          attributionOn={attributionOn}
          onAttributionChange={setAttributionOn}
          attributionGenerating={attributionGenerating}
          attributionDisabled={
            currentFormat !== 'q2-debug' && currentFormat !== 'q2-preview'
          }
        />
      )}

      {/* New text-file dialog */}
      <NewFileDialog
        isOpen={showNewFileDialog}
        existingPaths={files.map(f => f.path)}
        onClose={() => {
          handleDialogClose();
          setNewFileInitialName(''); // Clear on close
        }}
        onCreateTextFile={handleCreateTextFile}
        initialFilename={newFileInitialName}
      />

      {/* Asset upload dialog */}
      <NewAssetDialog
        isOpen={showNewAssetDialog}
        existingPaths={files.map(f => f.path)}
        defaultDestination={assetDestination}
        initialFiles={assetInitialFiles}
        onClose={handleAssetDialogClose}
        onUploadAsset={handleUploadAsset}
      />

      {/* Share project dialog */}
      <ShareDialog
        isOpen={showShareDialog}
        shareableUrl={shareableUrl}
        onClose={() => setShowShareDialog(false)}
      />
    </div>
  );
}
