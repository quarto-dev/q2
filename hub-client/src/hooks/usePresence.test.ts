/**
 * Tests for usePresence hook.
 *
 * These tests exercise the hook against a mocked Monaco editor and a mocked
 * presenceService, asserting the cursor/selection decorations the hook
 * applies under various race scenarios that PR #94 (OT cursor tracking) and
 * PR #110 (EOL same-line guard) introduced. The assertions are written at the
 * level of observable decoration positions so they survive the Automerge-
 * cursor refactor planned in issue #113.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

// The mocked presenceService lets tests control the remote-presence stream.
// We capture the subscriber passed to onPresenceChange and fire it
// imperatively via `emitPresences` below.
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

import { usePresence } from './usePresence';
import type { PresenceState } from '../services/presenceService';

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
 * drive `onDidChangeContent` via `_applyEdit`.
 */
function createMockEditor(initialText: string) {
  let text = initialText;
  let nextId = 1;
  const contentCallbacks: Array<(e: { changes: MonacoChange[] }) => void> = [];
  const decorations = new Map<string, DecorationRecord>();

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

  const editor = {
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
    _applyEdit(rangeOffset: number, rangeLength: number, newText: string): void {
      text = text.slice(0, rangeOffset) + newText + text.slice(rangeOffset + rangeLength);
      const change: MonacoChange = { rangeOffset, rangeLength, text: newText, range: null };
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

  return editor;
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

/**
 * Resolve the first cursor decoration's start position back to a character
 * offset against the editor's *current* model text. Returns null if no
 * cursor decoration is present.
 */
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

describe('usePresence — remote cursor tracking across content edits', () => {
  beforeEach(() => {
    presenceCallback = null;
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

    emitPresences([makePresence({ peerId: 'remote-1', cursor: 14 })]);
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(14);

    // Local deletion of 2 chars at the start of paragraph 1.
    act(() => {
      editor._applyEdit(0, 2, '');
    });

    // Remote cursor must still reference the same logical character: 14 - 2 = 12.
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(12);
  });

  it('local insertion before a remote cursor shifts it forward by the inserted length', () => {
    const editor = createMockEditor('hello world');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    emitPresences([makePresence({ peerId: 'remote-1', cursor: 6 })]); // 'w'

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

    emitPresences([makePresence({ peerId: 'remote-1', cursor: 5 })]); // 'f'

    // Replace offsets [2, 7) ("cdefg") with "X". Cursor was inside the
    // deleted range and should clamp to editStart + newText.length = 3.
    act(() => {
      editor._applyEdit(2, 5, 'X');
    });

    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(3);
  });

  it('remote cursor arrives before its content change: no double-shift', () => {
    // Peer will insert "X" at offset 5 (EOF of "hello"), moving their cursor
    // from 5 to 6. The presence message carries the post-insert offset and
    // can arrive before the content change syncs into our doc.
    const editor = createMockEditor('hello');

    const { result } = renderHook(() => usePresence('test.qmd'));
    act(() => {
      result.current.onEditorMount(editor as never);
    });

    emitPresences([makePresence({ peerId: 'remote-1', cursor: 5 })]);
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(5);

    // Presence update with the anticipated post-edit offset (6) arrives first.
    emitPresences([makePresence({ peerId: 'remote-1', cursor: 6 })]);

    // Then the corresponding content change lands.
    act(() => {
      editor._applyEdit(5, 0, 'X');
    });

    // After both events, the decoration must be at offset 6 — not 7, which
    // would be the tell-tale double-shift if OT advanced an already-advanced
    // offset.
    expect(cursorOffset(editor._cursorDecorationsFor('remote-1'), editor)).toBe(6);
  });

  it('remote cursor at end-of-line, presence before content change: decoration never appears on the following line (PR #110)', () => {
    // Doc "aa\nbb": offset 2 is end of line 1, offset 3 is start of line 2.
    // Peer types "X" at offset 2. The post-edit cursor offset (3) maps to
    // line 2 col 1 in the *pre-edit* doc — a cross-line flash if rendered
    // naively.
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

    emitPresences([makePresence({ peerId: 'remote-1', cursor: 2 })]);
    assertNotOnLine2('initial');

    emitPresences([makePresence({ peerId: 'remote-1', cursor: 3 })]);
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
