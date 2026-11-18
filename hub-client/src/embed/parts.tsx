/**
 * The editor's parts, each bound to the surrounding `ProjectProvider`.
 *
 * PROTOTYPE. Each is a thin wrapper that feeds a hub-client component from
 * `useProject()`: it takes no required props, fills whatever box the host
 * puts it in, and passes any prop the host does give straight through, so
 * a default can be overridden one prop at a time. The underlying
 * components are exported too (`./index`) for a host that wants to do the
 * wiring itself.
 *
 *   ProjectFileToolbar — the New file / New folder / Add asset buttons
 *   ProjectFileTree   — the file tree and every file action it offers
 *   ProjectDocument   — the Monaco editor on the open file
 *   ProjectPreview    — the rendered preview of the open file
 *   ProjectOutline    — the open file's outline; a click moves the editor
 *   ProjectSyncStatus — the sync indicator and its dialog
 */

import { useCallback, useEffect, useRef } from 'react';
import {
  applyEditorOperations,
  diffToEditorChanges,
  getFileContent,
} from '@quarto/preview-runtime';
import { isBinaryExtension } from '@quarto/preview-renderer/types/project';
import DocumentEditor, { type DocumentEditorProps } from '../components/DocumentEditor';
import FileSidebarToolbar, { type FileSidebarToolbarProps } from '../components/FileSidebarToolbar';
import FileTree, { type FileTreeProps } from '../components/FileTree';
// The toolbar's styles; `FileTree` loads them too, but the toolbar may be used without it.
import '../components/FileSidebar.css';
import OutlinePanel from '../components/OutlinePanel';
import SyncStatusBadge from '../components/SyncStatusBadge';
import PreviewRouter from '../components/render/PreviewRouter';
import { useTheme } from '../components/ThemeContext';
import { useIntelligence } from '../hooks/useIntelligence';
import { useProject } from './ProjectProvider';
import { useFileActions } from './useFileActions';

const noop = () => {};

/**
 * The New file / New folder / Add asset buttons. Independent of
 * `ProjectFileTree`: place it anywhere, or leave it out.
 */
export function ProjectFileToolbar(props: Partial<FileSidebarToolbarProps>) {
  const { currentFile } = useProject();
  const { actions, dialogs } = useFileActions();
  return (
    <>
      <FileSidebarToolbar
        currentFile={currentFile}
        onNewFile={actions.onNewFile}
        onNewFolder={actions.onNewFolder}
        onUploadFiles={actions.onUploadFiles}
        {...props}
      />
      {dialogs}
    </>
  );
}

/**
 * The file tree. Selecting opens the file; creating, uploading, dropping,
 * moving, renaming and deleting act on the project (`useFileActions`).
 *
 * "Open in new tab" and "Copy link" appear in a file's menu only when the
 * host passes `onOpenInNewTab` / `onCopyLink`: what a link to a file looks
 * like is the host's to say.
 */
export function ProjectFileTree(props: Partial<FileTreeProps>) {
  const { files, folders, currentFile, openFile } = useProject();
  const { actions, dialogs } = useFileActions();
  // `onNewFile` is the toolbar's; the tree has no use for it.
  const { onNewFile: _onNewFile, ...treeActions } = actions;
  return (
    <>
      <FileTree
        files={files}
        folders={folders}
        currentFile={currentFile}
        onSelectFile={(file) => openFile(file.path)}
        {...treeActions}
        {...props}
      />
      {dialogs}
    </>
  );
}

/** The Monaco editor on the open file, with sync, cursors and the qmd language. */
export function ProjectDocument(props: Partial<DocumentEditorProps>) {
  const { currentFile, fileContents, internals } = useProject();
  const { effectiveTheme } = useTheme();
  const { setEditor } = internals;
  // Binary files have no text to edit.
  const file = currentFile !== null && !isBinaryExtension(currentFile.path) ? currentFile : null;

  // The Monaco instance is gone once this part (or its file) is.
  useEffect(() => () => setEditor(null), [setEditor, file?.path]);

  return (
    <DocumentEditor
      ref={internals.editorHandleRef}
      file={file}
      fileContents={fileContents}
      onContentOperations={applyEditorOperations}
      theme={effectiveTheme}
      onContentChange={internals.setContent}
      onPresenceChange={internals.setRemoteUsers}
      onMount={(editor) => setEditor(editor)}
      {...props}
    />
  );
}

export type CommentsMode = 'expand' | 'show' | 'hide';

export interface ProjectPreviewProps {
  /** How editorial comments show in the preview. Default: `show`. */
  commentsMode?: CommentsMode;
  /** Color the text by who wrote it. Default: false. */
  authors?: boolean;
}

/**
 * The rendered preview of the open file. Scroll-syncs with `ProjectDocument`
 * when one is mounted, and works without it.
 *
 * Whether blocks can be edited in the preview is not a prop: the preview
 * reads the persisted `previewEditing` preference itself, so a host
 * switches it with `usePreference('previewEditing')`.
 */
export function ProjectPreview({ commentsMode = 'show', authors = false }: ProjectPreviewProps) {
  const { project, currentFile, files, fileContents, identities, captures, content, editor, openFile, internals } =
    useProject();
  const editorHasFocusRef = useRef(false);
  const path = currentFile?.path ?? null;

  // In-preview edits arrive as the whole rewritten file.
  const rewrite = useCallback(
    (newContent: string) => {
      const handle = internals.editorHandleRef.current;
      if (handle !== null && handle.editor !== null) {
        // Through the editor, so its undo stack stays right.
        handle.rewriteContent(newContent);
      } else if (path !== null) {
        const changes = diffToEditorChanges(getFileContent(path) ?? '', newContent);
        if (changes.length > 0) applyEditorOperations(path, changes);
      }
    },
    [internals.editorHandleRef, path],
  );

  if (currentFile === null) return null;
  return (
    <PreviewRouter
      projectKey={project?.id}
      // With no editor mounted nothing mirrors the text into `content`.
      content={editor !== null ? content : fileContents.get(currentFile.path) ?? ''}
      currentFile={currentFile}
      files={files}
      fileContents={fileContents}
      scrollSyncEnabled={editor !== null}
      editorRef={internals.editorRef}
      editorReady={editor !== null}
      editorHasFocusRef={editorHasFocusRef}
      onFileChange={(file) => openFile(file.path)}
      onOpenNewFileDialog={noop}
      onDiagnosticsChange={noop}
      onContentRewrite={rewrite}
      identities={identities}
      captures={captures}
      attributionOn={authors}
      commentsMode={commentsMode}
    />
  );
}

/** The open file's outline. Clicking an entry moves the editor there, if one is mounted. */
export function ProjectOutline() {
  const { currentFile, content, editor } = useProject();
  const { symbols, loading, error, refresh } = useIntelligence({
    path: currentFile?.path ?? null,
    enableSymbols: true,
  });

  useEffect(() => {
    refresh();
  }, [content, refresh]);

  return (
    <OutlinePanel
      symbols={symbols}
      loading={loading}
      error={error}
      onSymbolClick={(symbol) => {
        if (editor === null) return;
        const lineNumber = symbol.range.start.line + 1;
        editor.setPosition({ lineNumber, column: symbol.range.start.character + 1 });
        editor.revealLineInCenter(lineNumber);
        editor.focus();
      }}
    />
  );
}

/** The sync indicator; clicking it opens the connection status dialog. */
export function ProjectSyncStatus({ scope = 'project' }: { scope?: 'project' | 'document' }) {
  const { currentFile } = useProject();
  return <SyncStatusBadge scope={scope} currentFilePath={currentFile?.path ?? null} />;
}
