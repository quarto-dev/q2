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
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import {
  getFileContent,
  setImmediateFileChangeCallback,
  type EditorContentChange,
} from '@quarto/preview-runtime';
import { diffToMonacoEdits, diffToEditorChanges } from '../utils/diffToMonacoEdits';

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

  // Latest remote content deferred while the tab is hidden — flushed as one
  // edit on visibilitychange→visible (or window focus). See plan
  // claude-notes/plans/2026-04-24-hub-client-visibility-gating.md. Keyed on
  // the active file only because hub-client opens at most one editor at a
  // time; if that ever changes this would need to become a Map by path.
  const pendingRemoteContentRef = useRef<string | null>(null);

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

    // Idempotent: reads-and-clears the stash ref so double-firing (both
    // visibilitychange and focus) is a no-op on the second call.
    const flushPendingRemote = () => {
      const pending = pendingRemoteContentRef.current;
      if (pending === null) return;
      pendingRemoteContentRef.current = null;

      const editor = editorRef.current;
      const model = editor?.getModel();
      if (!editor || !model) return;

      const monacoContent = model.getValue();
      if (monacoContent === pending) return;

      const edits = diffToMonacoEdits(monacoContent, pending);
      if (edits.length > 0) {
        applyingRemoteRef.current = true;
        editor.executeEdits('remote-sync', edits);
        applyingRemoteRef.current = false;
      }
    };

    const handleImmediateSync = (path: string, newContent: string) => {
      if (path !== currentFile.path) return;
      if (replayActiveRef.current) return;

      const editor = editorRef.current;
      const model = editor?.getModel();
      if (!editor || !model) return;

      const monacoContent = model.getValue();
      if (monacoContent === newContent) return; // Local change — already in sync

      // Visibility gate: while the tab is hidden, stash the latest content
      // and defer the Monaco edit until visibility returns. React state
      // still advances so Preview (and any other content consumer) stays
      // current; only the Monaco edit is coalesced.
      if (document.visibilityState === 'hidden') {
        pendingRemoteContentRef.current = newContent;
        setContent(newContent);
        return;
      }

      const edits = diffToMonacoEdits(monacoContent, newContent);
      if (edits.length > 0) {
        applyingRemoteRef.current = true;
        editor.executeEdits('remote-sync', edits);
        applyingRemoteRef.current = false;
      }
      setContent(newContent);
    };

    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') {
        flushPendingRemote();
      }
    };

    setImmediateFileChangeCallback(handleImmediateSync);
    document.addEventListener('visibilitychange', onVisibilityChange);
    // `focus` is a belt-and-braces for documented cases where
    // `visibilitychange` doesn't fire on a tab becoming active
    // (Chrome/Edge DevTools "Emulate a focused page"; Firefox macOS
    // Cmd-H → Cmd-Tab restore; headless Chrome on tab switch;
    // Brave/Chrome on HTTP with DevTools open). The flush is idempotent,
    // so double-firing is harmless.
    window.addEventListener('focus', flushPendingRemote);

    return () => {
      setImmediateFileChangeCallback(null);
      document.removeEventListener('visibilitychange', onVisibilityChange);
      window.removeEventListener('focus', flushPendingRemote);
      // Discard any pending content on unmount or file switch; the
      // reconciliation effect below reads live Automerge content on every
      // file switch, so both the new file and any later revisit of the
      // old file catch up from source rather than from a stale diff.
      pendingRemoteContentRef.current = null;
    };
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
  // Stable callback: uses refs so the identity never changes, preventing
  // @monaco-editor/react from re-subscribing its listener every render.
  const handleEditorChange = useCallback((value: string | undefined, event: Monaco.editor.IModelContentChangedEvent) => {
    if (replayActiveRef.current) return;
    if (applyingRemoteRef.current) return;

    if (value !== undefined && currentFileRef.current) {
      setContent(value);
      onContentOperations(currentFileRef.current.path, event.changes);
    }
  }, [onContentOperations]);

  // ── AST rewrite ────────────────────────────────────────────────────────
  //
  // Always write directly to Automerge, then optionally sync Monaco.
  //
  // Routing the write through Monaco's executeEdits → onChange used to work
  // when Monaco was visible, but fails silently when Monaco is hidden
  // (display:none in Preview view) because the hidden editor does not
  // reliably fire onChange.  Writing to Automerge directly is always safe:
  // the setImmediateFileChangeCallback in the real-time path will update
  // Monaco (with applyingRemoteRef guarding against echo) if it is ready.
  // If Monaco is not yet mounted, setContent keeps the preview in sync.
  const handleContentRewrite = useCallback((newContent: string) => {
    if (!currentFile) return;

    const oldContent = getFileContent(currentFile.path) ?? '';
    const changes = diffToEditorChanges(oldContent, newContent);
    if (changes.length > 0) {
      onContentOperations(currentFile.path, changes);
      // onContentOperations fires setImmediateFileChangeCallback synchronously;
      // handleImmediateSync updates Monaco (if mounted) and calls setContent.
    }
    setContent(newContent);

    // Sync Monaco model if it's mounted and not already up-to-date.
    // Treat this as a remote edit (applyingRemoteRef) so handleEditorChange
    // does not echo the change back to Automerge.
    const editor = editorRef.current;
    const model = editor?.getModel();
    if (editor && model && model.getValue() !== newContent) {
      const edits = diffToMonacoEdits(model.getValue(), newContent);
      if (edits.length > 0) {
        applyingRemoteRef.current = true;
        editor.executeEdits('ast-rewrite', edits);
        applyingRemoteRef.current = false;
      }
    }
  }, [currentFile, onContentOperations]);

  return {
    content,
    setContent,
    applyingRemoteRef,
    handleEditorChange,
    handleContentRewrite,
    onEditorMount,
  };
}
