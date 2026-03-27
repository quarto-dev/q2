/**
 * Tests for applyEditorOperations — positional splice-based text editing.
 *
 * TDD: these tests are written BEFORE the implementation.
 * They verify that applyEditorOperations correctly translates Monaco's
 * IModelContentChange events into Automerge splice operations.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { DocumentId } from '@automerge/automerge-repo';
import type { EditorContentChange } from './types.js';

// ── Mocks ──────────────────────────────────────────────────────────────

// Track splice calls for verification
const spliceSpy = vi.fn();

vi.mock('@automerge/automerge', () => ({
  clone: vi.fn((doc: unknown) => structuredClone(doc)),
  from: vi.fn((val: unknown) => structuredClone(val)),
  save: vi.fn(() => new Uint8Array(0)),
}));

vi.mock('@automerge/automerge-repo-network-websocket', () => ({
  BrowserWebSocketClientAdapter: vi.fn(),
}));

vi.mock('@automerge/automerge-repo-storage-indexeddb', () => ({
  IndexedDBStorageAdapter: vi.fn(),
}));

vi.mock('@quarto/quarto-automerge-schema', async (importOriginal) => {
  const original = await importOriginal<typeof import('@quarto/quarto-automerge-schema')>();
  return {
    ...original,
    setIdentity: vi.fn(),
  };
});

// Mock Repo + splice
vi.mock('@automerge/automerge-repo', async (importOriginal) => {
  const original = await importOriginal<typeof import('@automerge/automerge-repo')>();
  return {
    ...original,
    Repo: vi.fn(),
    splice: (...args: unknown[]) => {
      spliceSpy(...args);
    },
    generateAutomergeUrl: original.generateAutomergeUrl,
    parseAutomergeUrl: original.parseAutomergeUrl,
  };
});

import { Repo } from '@automerge/automerge-repo';
import { createSyncClient } from './client.js';
import type { SyncClientCallbacks } from './types.js';
import type { IndexDocument, TextDocumentContent } from '@quarto/quarto-automerge-schema';

// ── Helpers ────────────────────────────────────────────────────────────

function createMockHandle<T>(initialDoc: T) {
  let current = structuredClone(initialDoc);
  const changeListeners: Array<(args: { doc: T; patches: never[] }) => void> = [];
  const handle = {
    documentId: 'mock-doc-id' as DocumentId,
    doc: () => current,
    change: (fn: (d: T) => void) => {
      const draft = structuredClone(current);
      fn(draft);
      current = draft;
      // Fire change listeners so subscriptions propagate
      for (const cb of changeListeners) {
        cb({ doc: current, patches: [] });
      }
    },
    update: (fn: (d: T) => T) => {
      current = fn(structuredClone(current));
    },
    on: vi.fn((event: string, cb: (args: { doc: T; patches: never[] }) => void) => {
      if (event === 'change') {
        changeListeners.push(cb);
      }
    }),
    off: vi.fn(),
    whenReady: () => Promise.resolve(),
  };
  return { handle, getDoc: () => current };
}

function noopCallbacks(): SyncClientCallbacks {
  return {
    onFileAdded: vi.fn(),
    onFileChanged: vi.fn(),
    onBinaryChanged: vi.fn(),
    onFileRemoved: vi.fn(),
    onFilesChange: vi.fn(),
    onIdentitiesChange: vi.fn(),
    onConnectionChange: vi.fn(),
    onError: vi.fn(),
  };
}

/**
 * Set up a connected client with a file handle for 'test.qmd'.
 * Returns the client and a getter for the file document.
 */
async function setupClientWithFile(initialText: string) {
  const indexDoc: IndexDocument = {
    files: { 'test.qmd': 'file-doc-id' as DocumentId },
    version: 1,
    identities: {},
  };
  const { handle: indexHandle } = createMockHandle(indexDoc);

  const fileDoc: TextDocumentContent = { text: initialText };
  const { handle: fileHandle, getDoc } = createMockHandle(fileDoc);

  const mockNetworkSubsystem = { on: vi.fn(), off: vi.fn() };

  vi.mocked(Repo).mockImplementation(function (this: unknown) {
    Object.assign(this as Record<string, unknown>, {
      find: vi.fn().mockImplementation((url: string) => {
        // Return fileHandle for file doc lookups, indexHandle for index
        if (url.includes('file-doc-id')) return Promise.resolve(fileHandle);
        return Promise.resolve(indexHandle);
      }),
      import: vi.fn().mockReturnValue(indexHandle),
      create: vi.fn().mockReturnValue(fileHandle),
      networkSubsystem: mockNetworkSubsystem,
    });
    return this as Repo;
  } as unknown as typeof Repo);

  const cbs = noopCallbacks();
  const client = createSyncClient(cbs);

  await client.connect('ws://localhost:9999', 'mock-doc-id', 'actor-1', 'Test', '#000');

  return { client, getDoc, cbs };
}

// ── Tests ──────────────────────────────────────────────────────────────

describe('applyEditorOperations', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // 1.2: Single insert
  it('applies a single insert at a position', async () => {
    const { client } = await setupClientWithFile('Hello world');

    const changes: EditorContentChange[] = [
      { rangeOffset: 5, rangeLength: 0, text: ',' },
    ];

    client.applyEditorOperations('test.qmd', changes);

    expect(spliceSpy).toHaveBeenCalledTimes(1);
    expect(spliceSpy).toHaveBeenCalledWith(
      expect.anything(),       // doc
      ['text'],                // path
      5,                       // offset
      0,                       // deleteCount
      ',',                     // insertText
    );
  });

  // 1.2: Single delete
  it('applies a single delete', async () => {
    const { client } = await setupClientWithFile('Hello world');

    const changes: EditorContentChange[] = [
      { rangeOffset: 5, rangeLength: 1, text: '' },
    ];

    client.applyEditorOperations('test.qmd', changes);

    expect(spliceSpy).toHaveBeenCalledTimes(1);
    expect(spliceSpy).toHaveBeenCalledWith(
      expect.anything(),
      ['text'],
      5,
      1,
      '',
    );
  });

  // 1.2: Replace
  it('applies a replace operation', async () => {
    const { client } = await setupClientWithFile('Hello world');

    const changes: EditorContentChange[] = [
      { rangeOffset: 6, rangeLength: 5, text: 'there' },
    ];

    client.applyEditorOperations('test.qmd', changes);

    expect(spliceSpy).toHaveBeenCalledTimes(1);
    expect(spliceSpy).toHaveBeenCalledWith(
      expect.anything(),
      ['text'],
      6,
      5,
      'there',
    );
  });

  // 1.2: Multi-change batch (e.g., find-replace)
  it('applies all changes from a batch in a single transaction', async () => {
    const { client } = await setupClientWithFile('aaa bbb ccc');

    // Replace all 3-letter words (changes ordered end-to-beginning by Monaco)
    const changes: EditorContentChange[] = [
      { rangeOffset: 8, rangeLength: 3, text: 'ZZZ' },
      { rangeOffset: 4, rangeLength: 3, text: 'YYY' },
      { rangeOffset: 0, rangeLength: 3, text: 'XXX' },
    ];

    client.applyEditorOperations('test.qmd', changes);

    // All splices should be in one transaction (spliceSpy called 3 times)
    expect(spliceSpy).toHaveBeenCalledTimes(3);

    // Verify end-to-beginning order is preserved
    expect(spliceSpy).toHaveBeenNthCalledWith(1, expect.anything(), ['text'], 8, 3, 'ZZZ');
    expect(spliceSpy).toHaveBeenNthCalledWith(2, expect.anything(), ['text'], 4, 3, 'YYY');
    expect(spliceSpy).toHaveBeenNthCalledWith(3, expect.anything(), ['text'], 0, 3, 'XXX');
  });

  // 1.2: Multi-cursor (N changes at arbitrary positions)
  it('handles multi-cursor edits at arbitrary positions', async () => {
    const { client } = await setupClientWithFile('line1\nline2\nline3');

    // Multi-cursor inserts "X" at start of each line (end-to-beginning)
    const changes: EditorContentChange[] = [
      { rangeOffset: 12, rangeLength: 0, text: 'X' },
      { rangeOffset: 6, rangeLength: 0, text: 'X' },
      { rangeOffset: 0, rangeLength: 0, text: 'X' },
    ];

    client.applyEditorOperations('test.qmd', changes);

    expect(spliceSpy).toHaveBeenCalledTimes(3);
    expect(spliceSpy).toHaveBeenNthCalledWith(1, expect.anything(), ['text'], 12, 0, 'X');
    expect(spliceSpy).toHaveBeenNthCalledWith(2, expect.anything(), ['text'], 6, 0, 'X');
    expect(spliceSpy).toHaveBeenNthCalledWith(3, expect.anything(), ['text'], 0, 0, 'X');
  });

  // 1.4: Non-BMP characters (emoji) — UTF-16 offsets
  it('handles non-BMP characters with correct UTF-16 offsets', async () => {
    // '🎉' is U+1F389, which is 2 UTF-16 code units (surrogate pair)
    const { client } = await setupClientWithFile('🎉 hello');

    // Insert " world" after "hello" (offset: 🎉=2 + space=1 + hello=5 = 8)
    const changes: EditorContentChange[] = [
      { rangeOffset: 8, rangeLength: 0, text: ' world' },
    ];

    client.applyEditorOperations('test.qmd', changes);

    expect(spliceSpy).toHaveBeenCalledWith(
      expect.anything(),
      ['text'],
      8,
      0,
      ' world',
    );
  });

  // 1.4: Delete after emoji
  it('handles deletion after emoji with correct UTF-16 offsets', async () => {
    const { client } = await setupClientWithFile('🎉🎊 test');

    // Delete " test" (offset: 🎉=2 + 🎊=2 = 4, length: 5)
    const changes: EditorContentChange[] = [
      { rangeOffset: 4, rangeLength: 5, text: '' },
    ];

    client.applyEditorOperations('test.qmd', changes);

    expect(spliceSpy).toHaveBeenCalledWith(
      expect.anything(),
      ['text'],
      4,
      5,
      '',
    );
  });

  // 1.5: Empty changes array is a no-op
  it('does nothing for an empty changes array', async () => {
    const { client } = await setupClientWithFile('Hello world');

    client.applyEditorOperations('test.qmd', []);

    // splice should never be called — no Automerge transaction created
    expect(spliceSpy).not.toHaveBeenCalled();
  });

  // Edge case: no handle for path
  it('warns and returns when no handle exists for path', async () => {
    const { client } = await setupClientWithFile('Hello');
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    client.applyEditorOperations('nonexistent.qmd', [
      { rangeOffset: 0, rangeLength: 0, text: 'X' },
    ]);

    expect(spliceSpy).not.toHaveBeenCalled();
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('nonexistent.qmd'));

    warnSpy.mockRestore();
  });
});
