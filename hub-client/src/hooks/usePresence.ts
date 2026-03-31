/**
 * usePresence Hook
 *
 * React hook for integrating presence features with the Monaco editor.
 * Handles cursor tracking, remote cursor rendering, and presence state management.
 *
 * Uses Monaco's native decoration APIs instead of external libraries for compatibility
 * with @monaco-editor/react's CDN-loaded Monaco instance.
 */

import { useEffect, useRef, useCallback, useState } from 'react';
import type * as Monaco from 'monaco-editor';
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
 * Generate CSS for cursor decorations.
 * Injected once per color into the document head.
 */
function ensureCursorStyle(color: string, odorId: string): void {
  const styleId = `presence-cursor-${odorId}`;
  if (document.getElementById(styleId)) return;

  const style = document.createElement('style');
  style.id = styleId;
  style.textContent = `
    .presence-cursor-${odorId} {
      background-color: ${color};
      width: 2px !important;
      margin-left: -1px;
    }
    .presence-cursor-${odorId}::after {
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
    .presence-selection-${odorId} {
      background-color: ${color}33;
    }
  `;
  document.head.appendChild(style);
}

/**
 * Convert a color to a safe CSS class identifier.
 */
function colorToId(color: string): string {
  return color.replace('#', '').toLowerCase();
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

  // Track if we've initialized
  const initializedRef = useRef(false);

  // Track model version to re-render decorations when content changes.
  // Decorations are recreated from character offsets on each render, so we
  // must re-render after every content change to keep them in sync.
  const [modelVersion, setModelVersion] = useState(0);

  // Operational-transform adjusted cursor offsets for each remote peer.
  // Between presence updates, content edits shift these offsets so that
  // decorations stay at the correct logical position even when the document
  // length changes (e.g. a deletion in paragraph 1 shouldn't move a cursor
  // in paragraph 2).
  const adjustedCursorsRef = useRef<Map<string, number>>(new Map());
  const adjustedSelectionsRef = useRef<Map<string, { start: number; end: number }>>(new Map());

  // Last raw presence values received from the network.  Used to detect when
  // a genuinely new presence update has arrived (as opposed to OT drift).
  const lastPresenceCursorRef = useRef<Map<string, number>>(new Map());
  const lastPresenceSelRef = useRef<Map<string, { start: number; end: number }>>(new Map());

  // Peers whose cursor already reflects an edit whose content change hasn't
  // arrived yet.  The OT handler must skip these once so it doesn't
  // double-shift the offset.
  const anticipatingEditRef = useRef<Set<string>>(new Set());

  // Callback for when editor mounts
  const onEditorMount = useCallback((mountedEditor: Monaco.editor.IStandaloneCodeEditor) => {
    setEditor(mountedEditor);
  }, []);

  // Initialize presence service
  useEffect(() => {
    if (!enabled) return;

    initPresence().then(() => {
      initializedRef.current = true;
    });

    return () => {
      cleanupPresence();
      initializedRef.current = false;
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

  // Transform remote cursor/selection offsets on every content change (OT).
  // Each edit shifts offsets that fall after it, and clamps offsets inside
  // the deleted range to the end of the replacement text.  This keeps
  // decorations at their correct logical positions between presence updates.
  useEffect(() => {
    if (!editor) return;

    const model = editor.getModel();
    if (!model) return;

    const disposable = model.onDidChangeContent((e) => {
      // Peers whose presence arrived before this content change already have
      // post-edit offsets — skip OT for them for this entire event.
      const skip = new Set(anticipatingEditRef.current);
      anticipatingEditRef.current.clear();

      for (const change of e.changes) {
        const editStart = change.rangeOffset;
        const oldEnd = editStart + change.rangeLength;
        const newEnd = editStart + change.text.length;
        const delta = change.text.length - change.rangeLength;

        // --- cursors ---
        for (const [peerId, offset] of adjustedCursorsRef.current) {
          if (skip.has(peerId)) continue;
          if (offset >= oldEnd) {
            adjustedCursorsRef.current.set(peerId, offset + delta);
          } else if (offset > editStart) {
            adjustedCursorsRef.current.set(peerId, newEnd);
          }
        }

        // --- selections ---
        for (const [peerId, sel] of adjustedSelectionsRef.current) {
          if (skip.has(peerId)) continue;
          const s = sel.start >= oldEnd ? sel.start + delta
            : sel.start > editStart ? newEnd
            : sel.start;
          const end = sel.end >= oldEnd ? sel.end + delta
            : sel.end > editStart ? newEnd
            : sel.end;
          adjustedSelectionsRef.current.set(peerId, { start: s, end });
        }
      }

      setModelVersion((v) => v + 1);
    });

    return () => disposable.dispose();
  }, [editor]);

  // Render remote cursors and selections using Monaco decorations
  useEffect(() => {
    if (!editor || !enabled) return;

    const model = editor.getModel();
    if (!model) return;

    const localPeerId = getLocalPeerId();

    // Build new decorations
    const newDecorations: Monaco.editor.IModelDeltaDecoration[] = [];

    const docLength = model.getValueLength();

    for (const user of remoteUsers) {
      // Skip our own presence
      if (user.peerId === localPeerId) continue;

      const colorId = colorToId(user.userColor);
      ensureCursorStyle(user.userColor, colorId);

      // --- Cursor decoration (OT-adjusted) ---
      if (user.cursor !== null) {
        try {
          const lastRaw = lastPresenceCursorRef.current.get(user.peerId);
          const isNewPresence = lastRaw === undefined || lastRaw !== user.cursor;

          let cursorToRender: number;
          if (isNewPresence) {
            // If OT hasn't shifted the cursor since the previous presence
            // update, the corresponding content change hasn't arrived yet.
            // Only anticipate for small forward movements (typing); large
            // jumps or backward moves are navigation and won't produce a
            // matching content change.
            const prevAdjusted = adjustedCursorsRef.current.get(user.peerId);
            const cursorDelta = user.cursor - (lastRaw ?? user.cursor);
            if (lastRaw !== undefined && prevAdjusted === lastRaw &&
                cursorDelta > 0 && cursorDelta <= 2) {
              anticipatingEditRef.current.add(user.peerId);
            }

            // New presence update — adopt the authoritative offset
            cursorToRender = user.cursor;
            adjustedCursorsRef.current.set(user.peerId, user.cursor);
            lastPresenceCursorRef.current.set(user.peerId, user.cursor);
          } else {
            // No new update — use the OT-shifted value
            cursorToRender = adjustedCursorsRef.current.get(user.peerId) ?? user.cursor;
          }

          if (cursorToRender < 0 || cursorToRender > docLength) {
            continue;
          }

          const position = model.getPositionAt(cursorToRender);
          newDecorations.push({
            range: {
              startLineNumber: position.lineNumber,
              startColumn: position.column,
              endLineNumber: position.lineNumber,
              endColumn: position.column,
            },
            options: {
              className: `presence-cursor-${colorId}`,
              hoverMessage: { value: user.userName },
              stickiness: 1, // NeverGrowsWhenTypingAtEdges
            },
          });
        } catch {
          // Ignore invalid positions
        }
      } else {
        adjustedCursorsRef.current.delete(user.peerId);
        lastPresenceCursorRef.current.delete(user.peerId);
      }

      // --- Selection decoration (OT-adjusted) ---
      if (user.selection && user.selection.start !== user.selection.end) {
        try {
          const lastSel = lastPresenceSelRef.current.get(user.peerId);
          const isNewSel =
            lastSel === undefined ||
            lastSel.start !== user.selection.start ||
            lastSel.end !== user.selection.end;

          let selToRender: { start: number; end: number };
          if (isNewSel) {
            selToRender = user.selection;
            adjustedSelectionsRef.current.set(user.peerId, { ...user.selection });
            lastPresenceSelRef.current.set(user.peerId, { ...user.selection });
          } else {
            selToRender = adjustedSelectionsRef.current.get(user.peerId) ?? user.selection;
          }

          if (selToRender.end > docLength) {
            continue;
          }

          const startPos = model.getPositionAt(selToRender.start);
          const endPos = model.getPositionAt(selToRender.end);
          newDecorations.push({
            range: {
              startLineNumber: startPos.lineNumber,
              startColumn: startPos.column,
              endLineNumber: endPos.lineNumber,
              endColumn: endPos.column,
            },
            options: {
              className: `presence-selection-${colorId}`,
              hoverMessage: { value: `${user.userName}'s selection` },
              stickiness: 1,
            },
          });
        } catch {
          // Ignore invalid positions
        }
      } else {
        adjustedSelectionsRef.current.delete(user.peerId);
        lastPresenceSelRef.current.delete(user.peerId);
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
    };
  // modelVersion triggers a re-render after every content change so that
  // OT-adjusted offsets are used to reposition decorations.
  }, [editor, enabled, remoteUsers, modelVersion]);

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

      // Convert Monaco position to offset
      const cursorOffset = model.getOffsetAt(selection.getPosition());

      // Check if there's a selection (not just cursor)
      let selectionRange: { start: number; end: number } | null = null;
      if (!selection.isEmpty()) {
        const startOffset = model.getOffsetAt(selection.getStartPosition());
        const endOffset = model.getOffsetAt(selection.getEndPosition());
        selectionRange = { start: startOffset, end: endOffset };
      }

      updatePresence(cursorOffset, selectionRange);
    };

    // Subscribe to cursor/selection changes
    const disposable = editor.onDidChangeCursorSelection(handleCursorChange);

    // Send initial position
    handleCursorChange();

    return () => {
      disposable.dispose();
    };
  }, [editor, enabled, currentFilePath]);

  // Memoized refresh function
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
