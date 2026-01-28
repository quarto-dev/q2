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
import { registerIntelligenceProviders, disposeIntelligenceProviders } from '../services/monacoProviders';
import { processFileForUpload } from '../services/resourceService';
import { usePresence } from '../hooks/usePresence';
import { usePreference } from '../hooks/usePreference';
import { useIntelligence } from '../hooks/useIntelligence';
import { diffToMonacoEdits } from '../utils/diffToMonacoEdits';
import { diagnosticsToMarkers } from '../utils/diagnosticToMonaco';
import Preview from './Preview';
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

interface Props {
  project: ProjectEntry;
  files: FileEntry[];
  fileContents: Map<string, string>;
  onDisconnect: () => void;
  onContentChange: (path: string, content: string) => void;
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
  // Track if editor has focus (to prevent scroll feedback loop)
  const editorHasFocusRef = useRef(false);
  // Track when editor is mounted (for scroll sync initialization)
  const [editorReady, setEditorReady] = useState(false);

  // Monaco instance ref for setting markers
  const monacoRef = useRef<typeof Monaco | null>(null);

  // New file dialog state
  const [showNewFileDialog, setShowNewFileDialog] = useState(false);
  const [pendingUploadFiles, setPendingUploadFiles] = useState<File[]>([]);
  // Initial filename for new file dialog (e.g., from clicking a link to a non-existent file)
  const [newFileInitialName, setNewFileInitialName] = useState<string>('');

  // Editor drag-drop state for image insertion
  const [isEditorDragOver, setIsEditorDragOver] = useState(false);
  const pendingDropPositionRef = useRef<Monaco.IPosition | null>(null);

  // Callback for when preview wants to change file
  const handlePreviewFileChange = useCallback((file: FileEntry) => {
    setCurrentFile(file);
    const fileContent = fileContents.get(file.path);
    setContent(fileContent ?? '');
    // Clear diagnostics when switching files
    setDiagnostics([]);
    setUnlocatedErrors([]);
  }, [fileContents]);

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
        <Preview
          content={content}
          currentFile={currentFile}
          files={files}
          scrollSyncEnabled={scrollSyncEnabled}
          editorRef={editorRef}
          editorReady={editorReady}
          editorHasFocusRef={editorHasFocusRef}
          onFileChange={handlePreviewFileChange}
          onOpenNewFileDialog={handlePreviewOpenNewFileDialog}
          onDiagnosticsChange={handleDiagnosticsChange}
          onWasmStatusChange={handleWasmStatusChange}
        />
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
