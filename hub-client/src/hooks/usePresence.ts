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

  // Track model version to re-render decorations when content changes
  // This is needed because decorations are created based on character offsets,
  // and Monaco auto-shifts decorations when text is inserted. By tracking
  // model changes, we can recalculate decoration positions after content syncs.
  const [modelVersion, setModelVersion] = useState(0);

  // Track previous cursor state to optimize single-char edits and reduce flicker.
  // When presence arrives before document sync, we pre-compensate the cursor position
  // so that Monaco's auto-shift lands it in the correct place. This avoids the need
  // for a second render pass to correct the position.
  const prevCursorsRef = useRef<Map<string, number>>(new Map());
  const cursorModelLengthRef = useRef<Map<string, number>>(new Map());

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

  // Track model content changes to trigger decoration recalculation
  // This ensures decorations are repositioned after document syncs
  useEffect(() => {
    if (!editor) return;

    const model = editor.getModel();
    if (!model) return;

    const disposable = model.onDidChangeContent(() => {
      setModelVersion(v => v + 1);
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

      // Add cursor decoration
      if (user.cursor !== null) {
        try {
          const prevCursor = prevCursorsRef.current.get(user.peerId);
          const cursorModelLength = cursorModelLengthRef.current.get(user.peerId) ?? docLength;

          let cursorToRender = user.cursor;

          // Pre-compensation for single-char (and small multi-char) inserts:
          // When presence arrives before document sync, the cursor position is based
          // on the NEW document state, but our model still has the OLD state.
          // If we render at user.cursor, Monaco will shift the decoration when the
          // document syncs, causing a flicker.
          //
          // Instead, we render at (prevCursor + modelDelta). This way, when Monaco
          // shifts the decoration due to the insert, it lands at the correct position.
          //
          // Example: prevCursor=50, user.cursor=51 (typed 1 char), model unchanged yet
          // - cursorDelta=1, modelDelta=0
          // - We render at 50. When char inserts at 50, decoration shifts to 51. Correct!
          //
          // IMPORTANT: Only apply this for small cursor movements (1-2 chars).
          // Larger movements are likely navigation (clicking, arrow keys), not typing.
          // We don't want to pre-compensate for navigation since no document change is coming.
          const MAX_TYPING_DELTA = 2;

          if (prevCursor !== undefined) {
            const cursorDelta = user.cursor - prevCursor;
            const modelDelta = docLength - cursorModelLength;

            // Only pre-compensate for small forward movements (likely typing)
            if (cursorDelta > 0 && cursorDelta <= MAX_TYPING_DELTA && modelDelta < cursorDelta) {
              cursorToRender = prevCursor + modelDelta;
            }
          }

          // Update tracking state when cursor changes
          if (user.cursor !== prevCursor) {
            prevCursorsRef.current.set(user.peerId, user.cursor);
            cursorModelLengthRef.current.set(user.peerId, docLength);
          }

          // Skip if out of bounds after adjustment
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
      }

      // Add selection decoration (simpler handling - no pre-compensation needed
      // since typing usually clears selection, and selection changes are less
      // frequent than cursor movements)
      if (user.selection && user.selection.start !== user.selection.end) {
        try {
          // Skip if selection extends beyond document length
          if (user.selection.end > docLength) {
            continue;
          }

          const startPos = model.getPositionAt(user.selection.start);
          const endPos = model.getPositionAt(user.selection.end);
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
  // Note: modelVersion is included to recalculate decorations after content changes.
  // This prevents the off-by-one cursor issue caused by Monaco auto-shifting decorations
  // when the decoration effect runs before the content-sync effect.
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
