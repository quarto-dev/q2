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
 *    `fileContents` changes, the active file switches, or a Monaco editor
 *    instance mounts. Handles cases where no Automerge change event fires:
 *    initial mount, file switching, and Monaco not yet ready (sets React state
 *    for preview). Editor mount must be a trigger (bd-eakukmlr): onMount lands
 *    a commit after the editor is created, so content arriving in that window
 *    would otherwise never reach the (uncontrolled) editor — blank until reload.
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
  isWasmReady,
  setImmediateFileChangeCallback,
  vfsAddFile,
  type EditorContentChange,
} from '@quarto/preview-runtime';
import { diffToMonacoEdits } from '../utils/diffToMonacoEdits';
import { diffToEditorChanges } from '@quarto/preview-runtime';
import { registerEditorTextProvider } from '../services/editorDebugRegistry';
import { getBranchText, applyBranchEdits } from '../services/branchService';
import { perfNow, perfSegment, perfMark } from '../utils/perfCounters';

/**
 * A document the editor can bind to instead of the preview runtime's
 * synced project: something that holds the text, reports changes made
 * elsewhere, and takes the editor's edits as splices. The canvas demo
 * binds the editor to an Automerge doc handle this way.
 */
export interface DocumentSource {
  /** The current text, or null while it is not available. */
  getText(): string | null;
  /** Called with the full new text whenever the document changes elsewhere; returns the unsubscribe. */
  subscribe(listener: (text: string) => void): () => void;
  /** Apply the editor's edits to the document. */
  applyChanges(changes: EditorContentChange[]): void;
}

interface UseAutomergeSyncOptions {
  /** Current file being edited (null if none selected) */
  currentFile: FileEntry | null;
  /** Map of file paths to content — triggers reconciliation on change */
  fileContents: Map<string, string>;
  /** Callback to propagate local Monaco changes to Automerge splices (the runtime path). */
  onContentOperations?: (path: string, changes: EditorContentChange[]) => void;
  /**
   * The document to bind to, instead of the runtime's project file at
   * `currentFile.path`. Text, remote changes and local edits then all go
   * through it; the VFS still mirrors what the editor shows.
   */
  source?: DocumentSource;
  /** Ref from useReplayMode — gates sync when replay is active */
  replayActiveRef: React.RefObject<boolean>;
  /** Whether replay mode is active (for effect dependency arrays) */
  replayIsActive: boolean;
  /** Active local branch id for the current file, or null when on main.
   *  When non-null, Monaco is bound to the branch doc (branchService)
   *  instead of the synced main doc. */
  activeBranchId?: string | null;
  /**
   * Called synchronously with every `content` update (the hook's own and
   * `setContent` calls), in the same batch as the state change. Lets a
   * parent that mirrors `content` stay one commit ahead of an effect.
   */
  onContentChange?: (content: string) => void;
}

interface UseAutomergeSyncResult {
  /** Current content string for preview rendering */
  content: string;
  /** Direct setter — used by file-switching and replay code */
  setContent: (content: string) => void;
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
  activeBranchId = null,
  onContentChange,
  source,
}: UseAutomergeSyncOptions): UseAutomergeSyncResult {
  // The latest source, for the stable callbacks below.
  const sourceRef = useRef(source);
  useEffect(() => {
    sourceRef.current = source;
  }, [source]);
  const [content, setContentState] = useState<string>('');
  const onContentChangeRef = useRef(onContentChange);
  useEffect(() => {
    onContentChangeRef.current = onContentChange;
  });
  const setContent = useCallback((next: string) => {
    setContentState(next);
    onContentChangeRef.current?.(next);
  }, []);
  const applyingRemoteRef = useRef(false);
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);

  // Bumped on every onEditorMount so reconciliation re-runs once the editor
  // is reachable. A counter, not a boolean: key remounts must reconcile too.
  const [editorMountGen, setEditorMountGen] = useState(0);

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

  // Same for the active local branch (branch docs bypass the sync client).
  const activeBranchIdRef = useRef(activeBranchId);
  activeBranchIdRef.current = activeBranchId;

  const onEditorMount = useCallback((editor: Monaco.editor.IStandaloneCodeEditor) => {
    editorRef.current = editor;
    setEditorMountGen((gen) => gen + 1);
  }, []);

  // Expose the live Monaco text to quartoDebug.am.doctor() through the
  // debug registry (bd-6ogrov5r). Reads go through refs, so one
  // registration per hook lifetime suffices.
  useEffect(() => {
    return registerEditorTextProvider({
      getPath: () => currentFileRef.current?.path ?? null,
      getText: () => editorRef.current?.getModel()?.getValue() ?? null,
    });
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
      // On a local branch, Monaco shows the branch doc; ignore main-doc
      // updates (they'll reconcile on switch-back or via CRDT merge).
      if (activeBranchIdRef.current !== null) return;

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

    // Remote changes come from the bound source when there is one, else
    // from the runtime's synchronous file-change callback.
    const path = currentFile.path;
    const unsubscribe = source !== undefined
      ? source.subscribe((text) => handleImmediateSync(path, text))
      : (setImmediateFileChangeCallback(handleImmediateSync), () => setImmediateFileChangeCallback(null));
    document.addEventListener('visibilitychange', onVisibilityChange);
    // `focus` is a belt-and-braces for documented cases where
    // `visibilitychange` doesn't fire on a tab becoming active
    // (Chrome/Edge DevTools "Emulate a focused page"; Firefox macOS
    // Cmd-H → Cmd-Tab restore; headless Chrome on tab switch;
    // Brave/Chrome on HTTP with DevTools open). The flush is idempotent,
    // so double-firing is harmless.
    window.addEventListener('focus', flushPendingRemote);

    return () => {
      unsubscribe();
      document.removeEventListener('visibilitychange', onVisibilityChange);
      window.removeEventListener('focus', flushPendingRemote);
      // Discard any pending content on unmount or file switch; the
      // reconciliation effect below reads live Automerge content on every
      // file switch, so both the new file and any later revisit of the
      // old file catch up from source rather than from a stale diff.
      pendingRemoteContentRef.current = null;
    };
  }, [currentFile, replayIsActive, setContent, source]);

  // ── Reconciliation on mount / file switch / editor mount ─────────────
  //
  // React effect that fires on initial mount, file switch, when
  // fileContents changes, or when a Monaco editor instance mounts
  // (editorMountGen). Reads live Automerge content (not the stale
  // closure value) and reconciles Monaco. For ongoing remote edits this
  // is usually a no-op since the real-time callback above already handled it.
  useEffect(() => {
    const fxT0 = perfNow();
    if (!currentFile) {
      perfMark('effect.automerge-reconcile.early-out');
      return;
    }
    if (replayIsActive) {
      perfMark('effect.automerge-reconcile.early-out');
      return;
    }

    // On a local branch, reconcile from the branch doc instead of main.
    const readT0 = perfNow();
    const automergeContent = activeBranchId !== null
      ? getBranchText(currentFile.path, activeBranchId)
      : source !== undefined ? source.getText() : getFileContent(currentFile.path);
    if (automergeContent === null) {
      perfMark('effect.automerge-reconcile.early-out');
      return;
    }
    const readT1 = perfNow();

    // Keep the VFS in step with whatever the editor displays, so the
    // preview renders the same state (covers branch switches in both
    // directions — the sync client only maintains the VFS for main edits).
    // A bound source may be editing before the WASM module is up.
    if (isWasmReady()) vfsAddFile(currentFile.path, automergeContent);
    const vfsT1 = perfNow();

    const model = editorRef.current?.getModel();
    const monacoContent = model?.getValue();

    // If Monaco isn't ready yet, just sync React state for preview
    if (monacoContent === undefined) {
      setContent(automergeContent);
      perfSegment('effect.automerge-reconcile.read', readT0, readT1);
      perfSegment('effect.automerge-reconcile.vfs', readT1, vfsT1);
      perfSegment('effect.automerge-reconcile', fxT0);
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
    const monacoT1 = perfNow();

    setContent(automergeContent);
    perfSegment('effect.automerge-reconcile.read', readT0, readT1);
    perfSegment('effect.automerge-reconcile.vfs', readT1, vfsT1);
    perfSegment('effect.automerge-reconcile.monaco', vfsT1, monacoT1);
    perfSegment('effect.automerge-reconcile', fxT0);
  }, [currentFile, fileContents, replayIsActive, editorMountGen, activeBranchId, setContent, source]);

  // ── Monaco → Automerge ───────────────────────────────────────────────
  //
  // Stable callback: uses refs so the identity never changes, preventing
  // @monaco-editor/react from re-subscribing its listener every render.
  const handleEditorChange = useCallback((value: string | undefined, event: Monaco.editor.IModelContentChangedEvent) => {
    if (replayActiveRef.current) return;
    if (applyingRemoteRef.current) return;

    if (value !== undefined && currentFileRef.current) {
      setContent(value);
      const branchId = activeBranchIdRef.current;
      if (branchId !== null) {
        // Local branch: edits go to the branch doc, never to the synced doc.
        // Write the VFS too — the preview's WASM render reads the file from
        // the VFS (the content prop is only its trigger); on main the sync
        // client does this in onFileChanged.
        applyBranchEdits(currentFileRef.current.path, branchId, event.changes);
        vfsAddFile(currentFileRef.current.path, value);
      } else if (sourceRef.current !== undefined) {
        sourceRef.current.applyChanges(event.changes);
        // The bound source does not maintain the VFS; mirror the editor.
        if (isWasmReady()) vfsAddFile(currentFileRef.current.path, value);
      } else {
        onContentOperations?.(currentFileRef.current.path, event.changes);
      }
    }
  }, [onContentOperations, setContent]);

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
    // AST rewrites target the synced main doc; ignore while on a branch so
    // a preview interaction can't silently write to main. (Prototype scope.)
    if (activeBranchIdRef.current !== null) return;

    const oldContent = (source !== undefined ? source.getText() : getFileContent(currentFile.path)) ?? '';
    const changes = diffToEditorChanges(oldContent, newContent);
    if (changes.length > 0) {
      if (source !== undefined) source.applyChanges(changes);
      else onContentOperations?.(currentFile.path, changes);
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
  }, [currentFile, onContentOperations, setContent, source]);

  return {
    content,
    setContent,
    applyingRemoteRef,
    handleEditorChange,
    handleContentRewrite,
    onEditorMount,
  };
}
