/**
 * usePresence Hook
 *
 * React hook for integrating presence features with the Monaco editor.
 * Handles cursor tracking, remote cursor rendering, and presence state management.
 *
 * Remote cursor positions arrive as Automerge cursor strings (see
 * `presenceService.ts`) and are resolved to numeric Monaco offsets against
 * the current local doc on every render. Automerge cursors are anchored
 * to specific ops in the text sequence, so they track their logical
 * position through concurrent and local edits without needing hand-rolled
 * operational transformation on the receiver.
 */

import { useEffect, useLayoutEffect, useRef, useCallback, useState } from 'react';
import type * as Monaco from 'monaco-editor';
import { next as A } from '@automerge/automerge';
import {
  initPresence,
  cleanupPresence,
  setCurrentFile,
  updatePresence,
  onPresenceChange,
  refreshIdentity,
  getLocalPeerId,
  type PresenceState,
} from '../services/presenceService';
import { getFileHandle } from '../services/automergeSync';

/**
 * Options for the usePresence hook.
 */
interface UsePresenceOptions {
  /** Whether presence features are enabled. Default: true */
  enabled?: boolean;
}

/**
 * Return value from usePresence hook.
 */
interface UsePresenceResult {
  /** List of other users' presence states */
  remoteUsers: PresenceState[];
  /** Number of other users currently viewing this file */
  userCount: number;
  /** Refresh identity after user changes their name/color */
  refreshIdentity: () => Promise<void>;
  /** Call this when the Monaco editor mounts */
  onEditorMount: (editor: Monaco.editor.IStandaloneCodeEditor) => void;
}

/**
 * Generate CSS for a specific user's cursor decorations.
 * Creates/updates a per-user style element with their color and name label.
 *
 * A single decoration element uses ::after for the teardrop dot and
 * ::before for the name label. The label stays visible as long as
 * the cursor is present.
 */
function ensureUserCursorStyle(peerId: string, color: string, userName: string): void {
  const styleId = `presence-user-${peerId}`;
  const escapedName = userName.replace(/\\/g, '\\\\').replace(/'/g, "\\'");

  let style = document.getElementById(styleId) as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement('style');
    style.id = styleId;
    document.head.appendChild(style);
  }

  const newContent = `
    .presence-cursor-${peerId} {
      position: relative;
      background-color: ${color};
      width: 2px !important;
      margin-left: -1px;
    }
    .presence-cursor-${peerId}::after {
      content: '';
      position: absolute;
      top: 0;
      left: -3px;
      width: 8px;
      height: 8px;
      background-color: ${color};
      border-radius: 50% 50% 50% 0;
      transform: rotate(-45deg);
    }
    .presence-cursor-${peerId}::before {
      content: '${escapedName}';
      position: absolute;
      top: -5px;
      left: 5px;
      color: ${color};
      font-size: 10px;
      font-weight: 600;
      line-height: 1;
      white-space: nowrap;
      pointer-events: none;
      z-index: 10;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
      text-shadow: 0 0 3px var(--vscode-editor-background, #fff), 0 0 3px var(--vscode-editor-background, #fff);

    }
    .presence-selection-${peerId} {
      background-color: ${color}22;
    }
  `;
  if (style.textContent !== newContent) {
    style.textContent = newContent;
  }
}

/**
 * Remove a user's cursor styles when they leave.
 */
function removeUserCursorStyle(peerId: string): void {
  document.getElementById(`presence-user-${peerId}`)?.remove();
}

/**
 * Sanitize a peerId for use as a CSS class suffix.
 * Replaces non-alphanumeric characters with hyphens.
 */
function sanitizePeerId(peerId: string): string {
  return peerId.replace(/[^a-zA-Z0-9]/g, '-');
}

/**
 * Hook for managing presence in the Monaco editor.
 *
 * @param currentFilePath - Path of the currently edited file (null if none)
 * @param options - Configuration options
 */
export function usePresence(
  currentFilePath: string | null,
  options: UsePresenceOptions = {}
): UsePresenceResult {
  const { enabled = true } = options;

  // State for remote users
  const [remoteUsers, setRemoteUsers] = useState<PresenceState[]>([]);

  // State to track when editor is mounted
  const [editor, setEditor] = useState<Monaco.editor.IStandaloneCodeEditor | null>(null);

  // Track decoration IDs for cleanup
  const decorationIdsRef = useRef<string[]>([]);

  // Track model version to re-resolve Automerge cursors against the
  // updated doc after every content change. The stored cursor strings
  // don't change, but their resolved offsets do once local or remote
  // edits land in the Automerge doc — we need a re-render to pick up
  // the new offsets. Remote edits reach this path via
  // `immediateFileChangeCallback` in `automergeSync.ts`, which applies
  // diffs to Monaco and triggers `onDidChangeContent`.
  const [modelVersion, setModelVersion] = useState(0);

  // Track which peerIds have active styles, for cleanup
  const activePeerStylesRef = useRef<Set<string>>(new Set());

  // Callback for when editor mounts
  const onEditorMount = useCallback((mountedEditor: Monaco.editor.IStandaloneCodeEditor) => {
    setEditor(mountedEditor);
  }, []);

  // Initialize presence service
  useEffect(() => {
    if (!enabled) return;

    initPresence();

    return () => {
      cleanupPresence();
    };
  }, [enabled]);

  // Update current file in presence service
  useEffect(() => {
    if (!enabled) return;
    setCurrentFile(currentFilePath);
  }, [currentFilePath, enabled]);

  // Subscribe to presence changes
  useEffect(() => {
    if (!enabled) return;

    const unsubscribe = onPresenceChange((presences) => {
      setRemoteUsers(presences);
    });

    return unsubscribe;
  }, [enabled]);

  // Bump modelVersion on every content change so the render effect
  // re-resolves Automerge cursors against the updated doc state. This
  // replaces the hand-rolled OT loop that previously lived here.
  useEffect(() => {
    if (!editor) return;

    const model = editor.getModel();
    if (!model) return;

    const disposable = model.onDidChangeContent(() => {
      setModelVersion((v) => v + 1);
    });

    return () => disposable.dispose();
  }, [editor]);

  // Render remote cursors and selections using Monaco decorations.
  // useLayoutEffect ensures decorations update before the browser paints,
  // preventing visible shifting of auto-adjusted Monaco decorations.
  useLayoutEffect(() => {
    if (!editor || !enabled) return;

    const model = editor.getModel();
    if (!model) return;

    // Automerge cursor strings are resolved against the current local
    // Automerge doc. If we don't have a handle yet (file not synced) or
    // the handle can't produce a doc (deleted/unavailable), we can't
    // place decorations — bail out and wait for the next render, which
    // will fire when remoteUsers or modelVersion changes.
    const handle = currentFilePath ? getFileHandle(currentFilePath) : null;
    let doc: A.Doc<{ text: string }> | null = null;
    try {
      doc = handle ? (handle.doc() as A.Doc<{ text: string }>) : null;
    } catch {
      doc = null;
    }
    if (!doc) return;

    const localPeerId = getLocalPeerId();
    const docLength = model.getValueLength();
    const currentPeerIds = new Set<string>();
    const newDecorations: Monaco.editor.IModelDeltaDecoration[] = [];

    for (const user of remoteUsers) {
      if (user.peerId === localPeerId) continue;

      const safePeerId = sanitizePeerId(user.peerId);
      currentPeerIds.add(safePeerId);
      ensureUserCursorStyle(safePeerId, user.userColor, user.userName);
      activePeerStylesRef.current.add(safePeerId);

      // --- Cursor decoration ---
      if (user.cursor !== null) {
        let cursorOffset: number;
        try {
          cursorOffset = A.getCursorPosition(doc, ['text'], user.cursor);
        } catch {
          // Cursor references an op we haven't synced yet — skip this
          // render; the next onDidChangeContent after the op arrives
          // will re-run and resolve successfully.
          continue;
        }

        if (cursorOffset < 0 || cursorOffset > docLength) continue;

        const position = model.getPositionAt(cursorOffset);
        newDecorations.push({
          range: {
            startLineNumber: position.lineNumber,
            startColumn: position.column,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          },
          options: {
            className: `presence-cursor-${safePeerId}`,
            hoverMessage: { value: user.userName },
            stickiness: 1, // NeverGrowsWhenTypingAtEdges
          },
        });
      }

      // --- Selection decoration ---
      if (user.selection && user.selection.start !== user.selection.end) {
        let startOffset: number;
        let endOffset: number;
        try {
          startOffset = A.getCursorPosition(doc, ['text'], user.selection.start);
          endOffset = A.getCursorPosition(doc, ['text'], user.selection.end);
        } catch {
          continue;
        }
        if (startOffset === endOffset) continue;
        if (endOffset > docLength || startOffset < 0) continue;

        const startPos = model.getPositionAt(startOffset);
        const endPos = model.getPositionAt(endOffset);
        newDecorations.push({
          range: {
            startLineNumber: startPos.lineNumber,
            startColumn: startPos.column,
            endLineNumber: endPos.lineNumber,
            endColumn: endPos.column,
          },
          options: {
            className: `presence-selection-${safePeerId}`,
            hoverMessage: { value: `${user.userName}'s selection` },
            stickiness: 1,
          },
        });
      }
    }

    // Clean up styles for users who have left
    for (const peerId of activePeerStylesRef.current) {
      if (!currentPeerIds.has(peerId)) {
        removeUserCursorStyle(peerId);
        activePeerStylesRef.current.delete(peerId);
      }
    }

    // Apply decorations (deltaDecorations replaces old with new)
    decorationIdsRef.current = editor.deltaDecorations(
      decorationIdsRef.current,
      newDecorations
    );

    // Cleanup on unmount
    return () => {
      if (editor && decorationIdsRef.current.length > 0) {
        editor.deltaDecorations(decorationIdsRef.current, []);
        decorationIdsRef.current = [];
      }
      for (const peerId of activePeerStylesRef.current) {
        removeUserCursorStyle(peerId);
      }
      activePeerStylesRef.current.clear();
    };
  }, [editor, enabled, remoteUsers, modelVersion, currentFilePath]);

  // Track local cursor/selection changes
  useEffect(() => {
    if (!editor || !enabled) return;

    const model = editor.getModel();
    if (!model) return;

    const handleCursorChange = () => {
      const selection = editor.getSelection();
      if (!selection) {
        updatePresence(null, null);
        return;
      }

      const cursorOffset = model.getOffsetAt(selection.getPosition());

      let selectionRange: { start: number; end: number } | null = null;
      if (!selection.isEmpty()) {
        const startOffset = model.getOffsetAt(selection.getStartPosition());
        const endOffset = model.getOffsetAt(selection.getEndPosition());
        selectionRange = { start: startOffset, end: endOffset };
      }

      updatePresence(cursorOffset, selectionRange);
    };

    const disposable = editor.onDidChangeCursorSelection(handleCursorChange);

    // Send initial position
    handleCursorChange();

    return () => {
      disposable.dispose();
    };
  }, [editor, enabled, currentFilePath]);

  const handleRefreshIdentity = useCallback(async () => {
    await refreshIdentity();
  }, []);

  return {
    remoteUsers,
    userCount: remoteUsers.length,
    refreshIdentity: handleRefreshIdentity,
    onEditorMount,
  };
}
