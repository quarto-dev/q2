/**
 * Tests for usePresence hook.
 *
 * These tests exercise the hook against a mocked Monaco editor, a mocked
 * presenceService, and a real `@automerge/automerge` doc (via a stub
 * file-handle). They assert that cursor/selection decorations resolve to
 * the right Monaco offsets under various race scenarios that PR #94
 * (cursor tracking across concurrent edits) and PR #110 (cross-line flash
 * at EOL) introduced — plus the new race scenarios that Automerge cursor
 * resolution enables (presence-before-content-change, concurrent remote
 * edits).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { next as A } from '@automerge/automerge';
import type { Doc } from '@automerge/automerge';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

// `presenceService` is mocked so tests can drive the remote-presence stream
// directly via `emitPresences` instead of going through the ephemeral-message
// round trip.
let presenceCallback: ((presences: PresenceState[]) => void) | null = null;

vi.mock('../services/presenceService', () => ({
  initPresence: vi.fn().mockResolvedValue(undefined),
  cleanupPresence: vi.fn(),
  setCurrentFile: vi.fn(),
  updatePresence: vi.fn(),
  refreshIdentity: vi.fn().mockResolvedValue(undefined),
  getLocalPeerId: vi.fn(() => 'local-peer'),
  onPresenceChange: vi.fn((cb: (presences: PresenceState[]) => void) => {
    presenceCallback = cb;
    cb([]);
    return () => {
      presenceCallback = null;
    };
  }),
}));

// `automergeSync.getFileHandle` is mocked to return a stub handle wrapping
// the `localDoc` held below. Tests mutate `localDoc` through the mock
// editor's helpers, which also notify the hook via onDidChangeContent.
let localDoc: Doc<{ text: string }> = A.from({ text: '' });
let handleThrows = false;

vi.mock('@quarto/preview-runtime', () => ({
  getFileHandle: vi.fn(() => ({
    doc: () => {
      if (handleThrows) throw new Error('handle unavailable');
      return localDoc;
    },
  })),
}));

import { usePresence } from './usePresence';
import type { PresenceState } from '../services/presenceService';

// ---------------------------------------------------------------------------
// Mock Monaco editor
// ---------------------------------------------------------------------------

interface MonacoChange {
  rangeOffset: number;
  rangeLength: number;
  text: string;
  range: unknown;
}

interface DecorationRecord {
  range: {
    startLineNumber: number;
    startColumn: number;
    endLineNumber: number;
    endColumn: number;
  };
  options: { className?: string };
}

/**
 * Minimal Monaco editor mock. Supports offset<->position conversion on text
 * with embedded newlines, records decorations by id, and allows tests to
 * drive `onDidChangeContent` via `_applyEdit` / `_syncDoc`. Both helpers
 * keep `localDoc` and the mock's displayed text in lockstep so that cursors
 * created against `localDoc` resolve to the same position Monaco renders.
 */
function createMockEditor(initialText: string) {
  let text = initialText;
  let nextId = 1;
  const contentCallbacks: Array<(e: { changes: MonacoChange[] }) => void> = [];
  const decorations = new Map<string, DecorationRecord>();

  localDoc = A.from({ text: initialText });

  const lineStartOffsets = (t: string): number[] => {
    const offsets = [0];
    for (let i = 0; i < t.length; i++) {
      if (t[i] === '\n') offsets.push(i + 1);
    }
    return offsets;
  };

  const model = {
    getValue: () => text,
    getValueLength: () => text.length,
    getPositionAt(offset: number) {
      const starts = lineStartOffsets(text);
      const clamped = Math.max(0, Math.min(offset, text.length));
      let line = 0;
      for (let i = 0; i < starts.length; i++) {
        if (clamped >= starts[i]) line = i;
      }
      return { lineNumber: line + 1, column: clamped - starts[line] + 1 };
    },
    getOffsetAt(pos: { lineNumber: number; column: number }): number {
      const starts = lineStartOffsets(text);
      const line = Math.max(0, Math.min(pos.lineNumber - 1, starts.length - 1));
      return starts[line] + pos.column - 1;
    },
    onDidChangeContent(cb: (e: { changes: MonacoChange[] }) => void) {
      contentCallbacks.push(cb);
      return {
        dispose: () => {
          const i = contentCallbacks.indexOf(cb);
          if (i >= 0) contentCallbacks.splice(i, 1);
        },
      };
    },
  };

  return {
    getModel: () => model,
    getSelection: () => null,
    deltaDecorations(oldIds: string[], newDecos: DecorationRecord[]): string[] {
      for (const id of oldIds) decorations.delete(id);
      const newIds: string[] = [];
      for (const d of newDecos) {
        const id = `dec${nextId++}`;
        decorations.set(id, d);
        newIds.push(id);
      }
      return newIds;
    },
    onDidChangeCursorSelection() {
      return { dispose: () => {} };
    },
    /**
     * Apply an edit to `localDoc` and the mock's displayed text, then fire
     * onDidChangeContent so the hook re-resolves cursors. Models both local
     * typing and remote ops that have synced into our doc — either way the
     * hook sees the same thing: doc advanced + content change fired.
     */
    _applyEdit(rangeOffset: number, rangeLength: number, newText: string): void {
      text = text.slice(0, rangeOffset) + newText + text.slice(rangeOffset + rangeLength);
      localDoc = A.change(localDoc, (d) => A.splice(d, ['text'], rangeOffset, rangeLength, newText));
      const change: MonacoChange = { rangeOffset, rangeLength, text: newText, range: null };
      for (const cb of [...contentCallbacks]) cb({ changes: [change] });
    },
    /**
     * Replace `localDoc` with a doc the test constructed externally (e.g.
     * the result of `A.merge`), update the mock's displayed text, and fire a
     * whole-buffer content change so the hook re-resolves cursors. Used when
     * the test needs to install a doc state that isn't easily expressed as a
     * single splice against the current state.
     */
    _syncDoc(newDoc: Doc<{ text: string }>): void {
      const oldLen = text.length;
      const newText = newDoc.text;
      text = newText;
      localDoc = newDoc;
      const change: MonacoChange = {
        rangeOffset: 0,
        rangeLength: oldLen,
        text: newText,
        range: null,
      };
      for (const cb of [...contentCallbacks]) cb({ changes: [change] });
    },
    _cursorDecorationsFor(peerId: string): DecorationRecord[] {
      const safe = peerId.replace(/[^a-zA-Z0-9]/g, '-');
      return Array.from(decorations.values()).filter(
        (d) => d.options.className === `presence-cursor-${safe}`,
      );
    },
    _selectionDecorationsFor(peerId: string): DecorationRecord[] {
      const safe = peerId.replace(/[^a-zA-Z0-9]/g, '-');
      return Array.from(decorations.values()).filter(
        (d) => d.options.className === `presence-selection-${safe}`,
      );
    },
  };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function cursorAt(offset: number): string {
  return A.getCursor(localDoc, ['text'], offset);
}

function makePresence(overrides: Partial<PresenceState>): PresenceState {
  return {
    peerId: 'remote-1',
    userId: 'user-1',
    userName: 'Alice',
    userColor: '#ff0000',
    filePath: 'test.qmd',
    cursor: null,
    selection: null,
    lastSeen: Date.now(),
    ...overrides,
  };
}

function emitPresences(presences: PresenceState[]): void {
  act(() => {
    presenceCallback?.(presences);
  });
}

function cursorOffset(
  decos: DecorationRecord[],
  editor: ReturnType<typeof createMockEditor>,
): number | null {
  if (decos.length === 0) return null;
  const d = decos[0];
  return editor.getModel().getOffsetAt({
    lineNumber: d.range.startLineNumber,
    column: d.range.startColumn,
  });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('usePresence — remote cursor tracking across content edits', () => {
  beforeEach(() => {
    presenceCallback = null;
    handleThrows = false;
    localDoc = A.from({ text: '' });
  });

  afterEach(() => {
    document.head.querySelectorAll('style[id^="presence-user-"]').forEach((s) => s.remove());
  });

  it('deletion in an earlier paragraph does not shift a remote cursor in a later paragraph (PR #94)', () => {
    // "abcdef\n\nghi\n\njkl" — 'k' (later paragraph) is at offset 14.
    const editor = createMockEditor('abcdef\n\nghi\n\njkl');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    emitPresences([makePresence({ peerId: 'remote-1', cursor: cursorAt(14) })]);
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(14);

    act(() => {
      editor._applyEdit(0, 2, '');
    });

    // Remote cursor still references the same logical character: 14 - 2 = 12.
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(12);
  });

  it('local insertion before a remote cursor shifts it forward by the inserted length', () => {
    const editor = createMockEditor('hello world');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    emitPresences([makePresence({ peerId: 'remote-1', cursor: cursorAt(6) })]); // 'w'

    act(() => {
      editor._applyEdit(0, 0, 'XYZ');
    });

    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(9);
  });

  it('local deletion across a remote cursor clamps it to the end of the replacement text', () => {
    const editor = createMockEditor('abcdefgh');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    emitPresences([makePresence({ peerId: 'remote-1', cursor: cursorAt(5) })]); // 'f'

    // Replace offsets [2, 7) ("cdefg") with "X". The char the cursor was
    // anchored to is deleted; Automerge collapses the cursor to
    // editStart + newText.length = 3.
    act(() => {
      editor._applyEdit(2, 5, 'X');
    });

    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(3);
  });

  it('remote cursor arrives before its content change: final position is correct, no double-shift', () => {
    // Peer's cursor moves from 6 (in "hello world") to 7 (in "hello Xworld"
    // after they insert "X" at offset 6). Simulate the presence update with
    // the post-edit cursor arriving before the content change.
    const editor = createMockEditor('hello world');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    emitPresences([makePresence({ peerId: 'remote-1', cursor: cursorAt(6) })]);
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(6);

    // Sender forks, applies the edit, builds the post-edit cursor. Cloning
    // leaves `localDoc` untouched so the hook can still resolve against it.
    const senderPostEdit = A.change(A.clone(localDoc), (d) =>
      A.splice(d, ['text'], 6, 0, 'X'),
    );
    const anticipatedCursor = A.getCursor(senderPostEdit, ['text'], 7);
    emitPresences([makePresence({ peerId: 'remote-1', cursor: anticipatedCursor })]);

    // The op syncs into our doc.
    act(() => {
      editor._applyEdit(6, 0, 'X');
    });

    // Final state: post-edit doc "hello Xworld", cursor at offset 7. The
    // same cursor string must never produce a "double-shift" value (8).
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(7);
  });

  it('remote cursor at end-of-line, presence before content change: decoration never appears on the following line (PR #110)', () => {
    const editor = createMockEditor('aa\nbb');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    const assertNotOnLine2 = (label: string) => {
      const decos = editor._cursorDecorationsFor('remote-1');
      for (const d of decos) {
        expect(
          d.range.startLineNumber,
          `${label}: cursor decoration must not be on line 2`,
        ).not.toBe(2);
      }
    };

    emitPresences([makePresence({ peerId: 'remote-1', cursor: cursorAt(2) })]);
    assertNotOnLine2('initial');

    // Peer types "X" at offset 2. Their post-edit cursor is at offset 3 in
    // their post-edit doc. Send that cursor before the content syncs.
    const senderPostEdit = A.change(A.clone(localDoc), (d) =>
      A.splice(d, ['text'], 2, 0, 'X'),
    );
    const anticipatedCursor = A.getCursor(senderPostEdit, ['text'], 3);
    emitPresences([makePresence({ peerId: 'remote-1', cursor: anticipatedCursor })]);
    assertNotOnLine2('after presence, before content change');

    act(() => {
      editor._applyEdit(2, 0, 'X');
    });
    assertNotOnLine2('after content change');

    // Final state: "aaX\nbb" with cursor at offset 3 → line 1, col 4.
    const decos = editor._cursorDecorationsFor('remote-1');
    expect(decos).toHaveLength(1);
    expect(decos[0].range.startLineNumber).toBe(1);
    expect(decos[0].range.startColumn).toBe(4);
  });
});

describe('usePresence — Automerge-cursor race scenarios', () => {
  beforeEach(() => {
    presenceCallback = null;
    handleThrows = false;
    localDoc = A.from({ text: '' });
  });

  afterEach(() => {
    document.head.querySelectorAll('style[id^="presence-user-"]').forEach((s) => s.remove());
  });

  it('cursor referencing an op not yet synced: decoration skipped, resolves once op arrives', () => {
    // Peer inserts "XY" at offset 2 and places their cursor at offset 3 in
    // their post-edit doc — which anchors (default 'after') to the 'Y' they
    // just inserted. That cursor cannot resolve on our pre-edit doc because
    // the 'Y' op hasn't synced yet.
    const editor = createMockEditor('hello');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    const senderPostEdit = A.change(A.clone(localDoc), (d) =>
      A.splice(d, ['text'], 2, 0, 'XY'),
    );
    const unsyncedCursor = A.getCursor(senderPostEdit, ['text'], 3);

    emitPresences([makePresence({ peerId: 'remote-1', cursor: unsyncedCursor })]);

    // The hook caught the RangeError and dropped the decoration for this
    // render rather than placing it at a stale offset.
    expect(editor._cursorDecorationsFor('remote-1')).toHaveLength(0);

    // Now the sender's ops sync into our doc. In production this happens
    // via Automerge sync and preserves op-ids; `_syncDoc` here installs the
    // sender's post-edit doc directly so the same cursor string resolves.
    act(() => {
      editor._syncDoc(senderPostEdit);
    });

    // Post-sync state: "heXYllo", cursor at offset 3 (anchored to 'Y').
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(3);
  });

  it('two concurrent remote edits: third peer cursor anchored in base resolves to merged offset', () => {
    // Base: "hello". Two peers insert concurrently at offset 5 with
    // different actor ids. Third peer anchored to EOF of the base tracks
    // through the merge.
    const editor = createMockEditor('hello');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    const peerCCursor = A.getCursor(localDoc, ['text'], 5); // EOF ('e' sentinel)

    const peerAFork = A.change(A.clone(localDoc), (d) =>
      A.splice(d, ['text'], 5, 0, 'X'),
    );
    const peerBFork = A.change(A.clone(localDoc), (d) =>
      A.splice(d, ['text'], 5, 0, 'Y'),
    );
    const merged = A.merge(peerAFork, peerBFork);
    expect(merged.text.length).toBe(7);
    const expectedOffset = A.getCursorPosition(merged, ['text'], peerCCursor);

    act(() => {
      editor._syncDoc(merged);
    });

    emitPresences([makePresence({ peerId: 'peer-c', cursor: peerCCursor })]);

    expect(cursorOffset(editor._cursorDecorationsFor('peer-c'), editor)).toBe(expectedOffset);
  });

  it('drops the decoration (no crash) when the file handle is unavailable', () => {
    const editor = createMockEditor('hello');
    handleThrows = true;

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    // Build a cursor before flipping the flag so at least it's syntactically
    // valid; the flag turns the handle stub into one whose `doc()` throws.
    emitPresences([makePresence({ peerId: 'remote-1', cursor: 'e' })]);
    expect(editor._cursorDecorationsFor('remote-1')).toHaveLength(0);
  });
});
