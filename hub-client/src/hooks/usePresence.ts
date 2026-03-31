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
 * Shift a character offset to account for a single edit.
 * Offsets after the edit move by delta; offsets inside the replaced range
 * clamp to the end of the replacement text; offsets before are unchanged.
 */
function transformOffset(
  offset: number,
  editStart: number,
  oldEnd: number,
  newEnd: number,
  delta: number,
): number {
  if (offset >= oldEnd) return offset + delta;
  if (offset > editStart) return newEnd;
  return offset;
}

/**
 * Per-peer OT state for remote cursor/selection tracking.
 */
interface PeerCursorState {
  /** OT-adjusted cursor offset */
  cursor: number;
  /** Last raw cursor value from the presence network */
  lastPresenceCursor: number;
  /** OT-adjusted selection range (absent if peer has no selection) */
  selection?: { start: number; end: number };
  /** Last raw selection from the presence network */
  lastPresenceSelection?: { start: number; end: number };
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

  // Per-peer OT state: adjusted offsets and last raw presence values.
  // Between presence updates, content edits shift `cursor`/`selection` so
  // decorations stay at the correct logical position.  `lastPresence*`
  // fields detect genuinely new presence updates vs. OT drift.
  const peerStateRef = useRef<Map<string, PeerCursorState>>(new Map());

  // Peers whose cursor already reflects an edit whose content change hasn't
  // arrived yet.  The OT handler skips these once to avoid double-shifting.
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

        for (const [peerId, state] of peerStateRef.current) {
          if (skip.has(peerId)) continue;
          state.cursor = transformOffset(state.cursor, editStart, oldEnd, newEnd, delta);
          if (state.selection) {
            state.selection = {
              start: transformOffset(state.selection.start, editStart, oldEnd, newEnd, delta),
              end: transformOffset(state.selection.end, editStart, oldEnd, newEnd, delta),
            };
          }
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
          let state = peerStateRef.current.get(user.peerId);
          const isNewPresence = !state || state.lastPresenceCursor !== user.cursor;

          let cursorToRender: number;
          if (isNewPresence) {
            // If OT hasn't shifted the cursor since the previous presence
            // update, the corresponding content change hasn't arrived yet.
            // Only anticipate for small forward movements (typing); large
            // jumps or backward moves are navigation and won't produce a
            // matching content change.
            const cursorDelta = user.cursor - (state?.lastPresenceCursor ?? user.cursor);
            if (state && state.cursor === state.lastPresenceCursor &&
                cursorDelta > 0 && cursorDelta <= 2) {
              anticipatingEditRef.current.add(user.peerId);
            }

            // New presence update — adopt the authoritative offset
            cursorToRender = user.cursor;
            if (state) {
              state.cursor = user.cursor;
              state.lastPresenceCursor = user.cursor;
            } else {
              state = { cursor: user.cursor, lastPresenceCursor: user.cursor };
              peerStateRef.current.set(user.peerId, state);
            }
          } else {
            // No new update — use the OT-shifted value
            cursorToRender = state.cursor;
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
        peerStateRef.current.delete(user.peerId);
      }

      // --- Selection decoration (OT-adjusted) ---
      if (user.selection && user.selection.start !== user.selection.end) {
        try {
          const state = peerStateRef.current.get(user.peerId);
          if (!state) continue;

          const lastSel = state.lastPresenceSelection;
          const isNewSel = !lastSel ||
            lastSel.start !== user.selection.start ||
            lastSel.end !== user.selection.end;

          let selToRender: { start: number; end: number };
          if (isNewSel) {
            selToRender = user.selection;
            state.selection = { ...user.selection };
            state.lastPresenceSelection = { ...user.selection };
          } else {
            selToRender = state.selection ?? user.selection;
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
        const state = peerStateRef.current.get(user.peerId);
        if (state) {
          delete state.selection;
          delete state.lastPresenceSelection;
        }
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
