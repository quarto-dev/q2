/**
 * useAutomergeSync Hook
 *
 * Bidirectional sync between Automerge documents and Monaco editor.
 *
 * Two sync paths push Automerge content into Monaco, each for a different scenario:
 *
 * 1. **Real-time remote edits** — A synchronous callback fires within the same
 *    macrotask as the WebSocket message handler. This updates Monaco *before*
 *    the user's next keystroke can read stale positions, preventing the
 *    position-correctness bug described in PR #102.
 *
 * 2. **Reconciliation on mount / file switch** — A React effect that runs when
 *    `fileContents` changes or the active file switches. Handles cases where no
 *    Automerge change event fires: initial mount, file switching, and Monaco not
 *    yet ready (sets React state for preview).
 *
 * In the opposite direction, `handleEditorChange` forwards local Monaco edits
 * to Automerge as positional splice operations.
 *
 * Echo prevention: `applyingRemoteRef` gates the onChange handler while edits
 * are being applied to Monaco, preventing infinite feedback loops.
 */

import { useState, useRef, useEffect, useCallback } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '../types/project';
import {
  getFileContent,
  setImmediateFileChangeCallback,
  type EditorContentChange,
} from '../services/automergeSync';
import { diffToMonacoEdits } from '../utils/diffToMonacoEdits';

interface UseAutomergeSyncOptions {
  /** Current file being edited (null if none selected) */
  currentFile: FileEntry | null;
  /** Map of file paths to content — triggers reconciliation on change */
  fileContents: Map<string, string>;
  /** Callback to propagate local Monaco changes to Automerge splices */
  onContentOperations: (path: string, changes: EditorContentChange[]) => void;
  /** Ref from useReplayMode — gates sync when replay is active */
  replayActiveRef: React.RefObject<boolean>;
  /** Whether replay mode is active (for effect dependency arrays) */
  replayIsActive: boolean;
}

interface UseAutomergeSyncResult {
  /** Current content string for preview rendering */
  content: string;
  /** Direct setter — used by file-switching and replay code */
  setContent: React.Dispatch<React.SetStateAction<string>>;
  /** True while applying remote edits to Monaco (prevents echo).
   *  Also used by replay code when applying replay edits. */
  applyingRemoteRef: React.MutableRefObject<boolean>;
  /** Monaco onChange handler */
  handleEditorChange: (value: string | undefined, event: Monaco.editor.IModelContentChangedEvent) => void;
  /** Apply an AST rewrite through Monaco's executeEdits (preserves undo) */
  handleContentRewrite: (newContent: string) => void;
  /** Call when Monaco editor mounts */
  onEditorMount: (editor: Monaco.editor.IStandaloneCodeEditor) => void;
}

export function useAutomergeSync({
  currentFile,
  fileContents,
  onContentOperations,
  replayActiveRef,
  replayIsActive,
}: UseAutomergeSyncOptions): UseAutomergeSyncResult {
  const [content, setContent] = useState<string>('');
  const applyingRemoteRef = useRef(false);
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);

  // Keep a ref to currentFile so the stable handleEditorChange callback
  // always reads the latest value without needing it as a dependency.
  const currentFileRef = useRef(currentFile);
  currentFileRef.current = currentFile;

  const onEditorMount = useCallback((editor: Monaco.editor.IStandaloneCodeEditor) => {
    editorRef.current = editor;
  }, []);

  // ── Real-time remote edits ────────────────────────────────────────────
  //
  // Synchronous callback that fires within the same macrotask as the
  // WebSocket message handler, BEFORE React state updates. Updates Monaco
  // immediately so the user's next keystroke reads correct positions.
  // Only fires on actual Automerge change events (not on mount/switch).
  useEffect(() => {
    if (!currentFile) return;

    const handleImmediateSync = (path: string, newContent: string) => {
      if (path !== currentFile.path) return;
      if (replayActiveRef.current) return;

      const editor = editorRef.current;
      const model = editor?.getModel();
      if (!editor || !model) return;

      const monacoContent = model.getValue();
      if (monacoContent === newContent) return; // Local change — already in sync

      const edits = diffToMonacoEdits(monacoContent, newContent);
      if (edits.length > 0) {
        applyingRemoteRef.current = true;
        editor.executeEdits('remote-sync', edits);
        applyingRemoteRef.current = false;
      }
      setContent(newContent);
    };

    setImmediateFileChangeCallback(handleImmediateSync);
    return () => setImmediateFileChangeCallback(null);
  }, [currentFile, replayIsActive]);

  // ── Reconciliation on mount / file switch ─────────────────────────────
  //
  // React effect that fires on initial mount, file switch, or when
  // fileContents changes. Reads live Automerge content (not the stale
  // closure value) and reconciles Monaco. For ongoing remote edits this
  // is usually a no-op since the real-time callback above already handled it.
  useEffect(() => {
    if (!currentFile) return;
    if (replayIsActive) return;

    const automergeContent = getFileContent(currentFile.path);
    if (automergeContent === null) return;

    const model = editorRef.current?.getModel();
    const monacoContent = model?.getValue();

    // If Monaco isn't ready yet, just sync React state for preview
    if (monacoContent === undefined) {
      setContent(automergeContent);
      return;
    }

    if (monacoContent !== automergeContent) {
      const edits = diffToMonacoEdits(monacoContent, automergeContent);
      if (edits.length > 0 && editorRef.current) {
        applyingRemoteRef.current = true;
        editorRef.current.executeEdits('remote-sync', edits);
        applyingRemoteRef.current = false;
      }
    }

    setContent(automergeContent);
  }, [currentFile, fileContents, replayIsActive]);

  // ── Monaco → Automerge ───────────────────────────────────────────────
  //
  // Stable callback: uses refs for mutable state so the identity never
  // changes.  This prevents @monaco-editor/react from disposing and
  // re-subscribing its internal onDidChangeModelContent listener on
  // every React render, which can race with keystrokes and cause the
  // first character after a selection to be silently dropped.
  const handleEditorChange = useCallback((value: string | undefined, event: Monaco.editor.IModelContentChangedEvent) => {
    if (replayActiveRef.current) return;
    if (applyingRemoteRef.current) return;

    if (value !== undefined && currentFileRef.current) {
      setContent(value);
      onContentOperations(currentFileRef.current.path, event.changes);
    }
  }, [onContentOperations]);

  // ── AST rewrite (through Monaco → onChange → splice path) ────────────
  const handleContentRewrite = useCallback((newContent: string) => {
    if (!editorRef.current || !currentFile) return;
    const model = editorRef.current.getModel();
    if (!model) return;

    const oldContent = model.getValue();
    const edits = diffToMonacoEdits(oldContent, newContent);
    if (edits.length > 0) {
      editorRef.current.executeEdits('ast-rewrite', edits);
    }
    // onChange fires synchronously → handleEditorChange → setContent + onContentOperations
  }, [currentFile]);

  return {
    content,
    setContent,
    applyingRemoteRef,
    handleEditorChange,
    handleContentRewrite,
    onEditorMount,
  };
}
