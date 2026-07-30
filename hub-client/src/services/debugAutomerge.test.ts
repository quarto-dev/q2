/**
 * @vitest-environment jsdom
 *
 * Tests for the Automerge-layer console debug API
 * (`window.quartoDebug.am`, bd-q93tkglb; plan:
 * claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md).
 *
 * Mocks `@quarto/preview-runtime` (sync-client accessors) and the two
 * local services (projectSetService, presenceService) so the tests
 * exercise ref resolution, snapshot sanitization, and report shapes
 * against fabricated repos/handles without booting Automerge.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import * as AutomergeModule from '@automerge/automerge';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';

const previewRuntimeMocks = vi.hoisted(() => ({
  getRepo: vi.fn<() => unknown>(),
  getDocInventory: vi.fn<() => unknown[]>(),
  getIndexHandle: vi.fn<() => unknown>(),
  getFileHandle: vi.fn<(path: string) => unknown>(),
  getSyncDiagnostics: vi.fn<() => unknown>(),
  isConnected: vi.fn<() => boolean>(),
  getFileContent: vi.fn<(path: string) => string | null>(),
  vfsListFiles: vi.fn<() => { success: boolean; files?: string[] }>(),
}));

const projectSetMocks = vi.hoisted(() => ({
  getProjectSetDebugSnapshot: vi.fn<() => unknown>(),
  getCollectionHandle: vi.fn<(docId: string) => unknown>(),
}));

const presenceMocks = vi.hoisted(() => ({
  getPresenceDebugSnapshot: vi.fn<() => unknown>(),
}));

const tapMocks = vi.hoisted(() => ({
  getTapMessages: vi.fn<(opts?: unknown) => unknown[]>(() => []),
  getTapStatus: vi.fn<() => unknown>(() => ({ installed: false })),
  installMessageTap: vi.fn(),
  uninstallMessageTap: vi.fn(),
}));

vi.mock('@quarto/preview-runtime', () => previewRuntimeMocks);
vi.mock('./projectSetService', () => projectSetMocks);
vi.mock('./presenceService', () => presenceMocks);
vi.mock('./debugMessageTap', () => tapMocks);

import { makeAutomergeDebugApi } from './debugAutomerge';
import { registerEditorTextProvider } from './editorDebugRegistry';

// ── Fabricated handles ────────────────────────────────────────────────

interface FakeHandleSpec {
  docId: string;
  state?: string;
  doc?: unknown;
  heads?: string[];
  /** Linear history, oldest first: [headsAtChange, decodedChangeMeta]. */
  history?: Array<[string[], { time?: number; actor?: string; message?: string }]>;
}

function fakeHandle(spec: FakeHandleSpec) {
  const metaByHash = new Map(
    (spec.history ?? []).map(([heads, meta]) => [heads[0], meta]),
  );
  return {
    documentId: spec.docId,
    state: spec.state ?? 'ready',
    doc: () => spec.doc,
    heads: () => spec.heads ?? ['h-current'],
    history: () => (spec.history ?? []).map(([heads]) => heads),
    metadata: (hash?: string) => (hash ? metaByHash.get(hash) : undefined),
  };
}

const sampleProject: ProjectEntry = {
  id: 'proj-1',
  indexDocId: 'idx1',
  syncServer: 'wss://hub.example/ws',
  description: 'Sample',
  createdAt: '2026-05-01T00:00:00Z',
  lastAccessed: '2026-05-01T00:00:00Z',
};

const ctx = {
  getProject: (): ProjectEntry | null => sampleProject,
  getFiles: (): { path: string; docId: string }[] => [
    { path: 'index.qmd', docId: 'f1' },
  ],
};

function makeApi(overrides: Partial<typeof ctx> = {}) {
  return makeAutomergeDebugApi({ ...ctx, ...overrides });
}

beforeEach(() => {
  vi.clearAllMocks();

  const indexHandle = fakeHandle({ docId: 'idx1', doc: { files: { 'index.qmd': 'f1' } } });
  const fileHandle = fakeHandle({
    docId: 'f1',
    doc: { text: 'hello body' },
    heads: ['h2'],
    history: [
      [['h1'], { time: 1000, actor: 'actor-a', message: 'first' }],
      [['h2'], { time: 2000, actor: 'actor-b' }],
    ],
  });

  previewRuntimeMocks.getRepo.mockReturnValue({
    peerId: 'peer-self',
    peers: ['peer-hub'],
    handles: { idx1: indexHandle, f1: fileHandle },
  });
  previewRuntimeMocks.getIndexHandle.mockReturnValue(indexHandle);
  previewRuntimeMocks.getFileHandle.mockImplementation((path: string) =>
    path === 'index.qmd' ? fileHandle : null,
  );
  previewRuntimeMocks.getDocInventory.mockReturnValue([
    {
      docId: 'idx1',
      role: 'index',
      path: null,
      handleState: 'ready',
      heads: ['h-current'],
      unavailableMarker: false,
    },
    {
      docId: 'f1',
      role: 'file',
      path: 'index.qmd',
      handleState: 'ready',
      heads: ['h2'],
      unavailableMarker: false,
    },
  ]);
  previewRuntimeMocks.getSyncDiagnostics.mockReturnValue({
    connectedPeers: 1,
    unavailableRetryTicks: 0,
    retryTimerActive: false,
    stranded: [],
  });
  previewRuntimeMocks.isConnected.mockReturnValue(true);

  projectSetMocks.getProjectSetDebugSnapshot.mockReturnValue({
    servers: [
      {
        url: 'wss://hub.example/ws',
        peerId: 'peer-ps',
        connectedPeers: ['peer-hub'],
        refCount: 1,
      },
    ],
    collections: [
      {
        docId: 'coll1',
        syncServer: 'wss://hub.example/ws',
        name: 'Personal',
        isRoot: true,
        entryCount: 3,
        handleState: 'ready',
        heads: ['hc'],
      },
    ],
  });
  projectSetMocks.getCollectionHandle.mockReturnValue(null);
  presenceMocks.getPresenceDebugSnapshot.mockReturnValue({
    peerId: 'presence-peer',
    identity: { userId: 'u', userName: 'U', userColor: '#000000' },
    currentFilePath: 'index.qmd',
    localCursor: 3,
    localSelection: null,
    remotePresences: [],
  });
});

describe('am.repos', () => {
  it('reports the sync-client repo and each project-set server', () => {
    expect(makeApi().repos()).toEqual([
      {
        name: 'sync-client',
        syncServer: 'wss://hub.example/ws',
        peerId: 'peer-self',
        connectedPeers: ['peer-hub'],
        cachedHandles: 2,
      },
      {
        name: 'project-set',
        syncServer: 'wss://hub.example/ws',
        peerId: 'peer-ps',
        connectedPeers: ['peer-hub'],
        cachedHandles: null,
      },
    ]);
  });

  it('omits the sync-client entry when no project is connected', () => {
    previewRuntimeMocks.getRepo.mockReturnValue(null);
    const repos = makeApi().repos();
    expect(repos.map((r) => r.name)).toEqual(['project-set']);
  });
});

describe('am.docs', () => {
  it('merges the sync-client inventory with project-set collection docs', () => {
    expect(makeApi().docs()).toEqual([
      {
        docId: 'idx1',
        role: 'index',
        path: null,
        handleState: 'ready',
        heads: ['h-current'],
        unavailableMarker: false,
      },
      {
        docId: 'f1',
        role: 'file',
        path: 'index.qmd',
        handleState: 'ready',
        heads: ['h2'],
        unavailableMarker: false,
      },
      {
        docId: 'coll1',
        role: 'project-set',
        path: null,
        handleState: 'ready',
        heads: ['hc'],
        unavailableMarker: false,
      },
    ]);
  });
});

describe('am.snapshot', () => {
  it('resolves a project path and returns the doc with metadata', () => {
    const snap = makeApi().snapshot('index.qmd');
    expect(snap).toEqual({
      docId: 'f1',
      path: 'index.qmd',
      handleState: 'ready',
      heads: ['h2'],
      truncated: false,
      doc: { text: 'hello body' },
    });
  });

  it("resolves the literal ref 'index' to the index doc", () => {
    const snap = makeApi().snapshot('index');
    expect(snap.docId).toBe('idx1');
    expect(snap.doc).toEqual({ files: { 'index.qmd': 'f1' } });
  });

  it('resolves a bare docId through the repo handle cache', () => {
    const snap = makeApi().snapshot('f1');
    expect(snap.docId).toBe('f1');
    expect(snap.path).toBe('index.qmd');
  });

  it('strips the automerge: prefix from docId refs', () => {
    expect(makeApi().snapshot('automerge:f1').docId).toBe('f1');
  });

  it('truncates long strings by default and marks the snapshot truncated', () => {
    const long = 'x'.repeat(1000);
    previewRuntimeMocks.getFileHandle.mockReturnValue(
      fakeHandle({ docId: 'f1', doc: { text: long } }),
    );
    const snap = makeApi().snapshot('index.qmd');
    expect(snap.truncated).toBe(true);
    const text = (snap.doc as { text: string }).text;
    expect(text.length).toBeLessThan(1000);
    expect(text).toMatch(/\[\+500 chars\]$/);
    expect(text.startsWith('xxx')).toBe(true);
  });

  it('honors maxStringLength and full options', () => {
    const long = 'y'.repeat(1000);
    previewRuntimeMocks.getFileHandle.mockReturnValue(
      fakeHandle({ docId: 'f1', doc: { text: long } }),
    );
    const tight = makeApi().snapshot('index.qmd', { maxStringLength: 10 });
    expect((tight.doc as { text: string }).text).toBe('y'.repeat(10) + '… [+990 chars]');

    const full = makeApi().snapshot('index.qmd', { full: true });
    expect(full.truncated).toBe(false);
    expect((full.doc as { text: string }).text).toBe(long);
  });

  it('summarizes byte arrays instead of dumping them', () => {
    previewRuntimeMocks.getFileHandle.mockReturnValue(
      fakeHandle({
        docId: 'f1',
        doc: { content: new Uint8Array([1, 2, 3, 4, 5]), mimeType: 'image/png' },
      }),
    );
    const snap = makeApi().snapshot('index.qmd');
    expect(snap.doc).toEqual({
      content: { $type: 'bytes', length: 5 },
      mimeType: 'image/png',
    });
    // Byte summarization is structural, not data loss worth flagging.
    expect(snap.truncated).toBe(false);
  });

  it('caps nesting depth and marks the snapshot truncated', () => {
    // depth 4 nesting with maxDepth 2 → inner object replaced by marker.
    previewRuntimeMocks.getFileHandle.mockReturnValue(
      fakeHandle({ docId: 'f1', doc: { a: { b: { c: { d: 1 } } } } }),
    );
    const snap = makeApi().snapshot('index.qmd', { maxDepth: 2 });
    expect(snap.truncated).toBe(true);
    expect(snap.doc).toEqual({ a: { b: { $type: 'max-depth' } } });
  });

  it('returns null doc for a not-ready handle', () => {
    previewRuntimeMocks.getFileHandle.mockReturnValue(
      fakeHandle({ docId: 'f1', state: 'requesting', doc: undefined }),
    );
    const snap = makeApi().snapshot('index.qmd');
    expect(snap.handleState).toBe('requesting');
    expect(snap.doc).toBeNull();
    expect(snap.heads).toBeNull();
  });

  it('throws on an unknown ref', () => {
    expect(() => makeApi().snapshot('nope.qmd')).toThrow(/unknown doc ref/);
  });
});

describe('am.history', () => {
  it('returns newest-first change summaries with actor/time/message', () => {
    const hist = makeApi().history('index.qmd');
    expect(hist).toEqual({
      docId: 'f1',
      path: 'index.qmd',
      changeCount: 2,
      changes: [
        { index: 1, hash: 'h2', actor: 'actor-b', timestamp: 2000, message: null },
        { index: 0, hash: 'h1', actor: 'actor-a', timestamp: 1000, message: 'first' },
      ],
    });
  });

  it('caps the list via opts.limit but reports the full count', () => {
    const hist = makeApi().history('index.qmd', { limit: 1 });
    expect(hist.changeCount).toBe(2);
    expect(hist.changes).toHaveLength(1);
    expect(hist.changes[0].hash).toBe('h2');
  });

  it('throws on an unknown ref', () => {
    expect(() => makeApi().history('nope.qmd')).toThrow(/unknown doc ref/);
  });
});

describe('am.syncStatus', () => {
  it('combines connection flag, sync diagnostics, and project-set state', () => {
    const status = makeApi().syncStatus();
    expect(status.connected).toBe(true);
    expect(status.diagnostics).toEqual({
      connectedPeers: 1,
      unavailableRetryTicks: 0,
      retryTimerActive: false,
      stranded: [],
    });
    expect(status.projectSet).toEqual(
      projectSetMocks.getProjectSetDebugSnapshot.mock.results[0].value,
    );
  });

  it('reports null diagnostics when no sync client exists', () => {
    previewRuntimeMocks.isConnected.mockReturnValue(false);
    previewRuntimeMocks.getSyncDiagnostics.mockImplementation(() => {
      throw new Error('Sync client not connected');
    });
    const status = makeApi().syncStatus();
    expect(status.connected).toBe(false);
    expect(status.diagnostics).toBeNull();
  });
});

describe('am.presence', () => {
  it('passes through the presence snapshot', () => {
    expect(makeApi().presence()).toEqual({
      peerId: 'presence-peer',
      identity: { userId: 'u', userName: 'U', userColor: '#000000' },
      currentFilePath: 'index.qmd',
      localCursor: 3,
      localSelection: null,
      remotePresences: [],
    });
  });
});

describe('am.messages', () => {
  it('combines tap status with filtered messages', () => {
    const canned = [{ at: 5, direction: 'outgoing', type: 'sync' }];
    tapMocks.getTapMessages.mockReturnValue(canned);
    tapMocks.getTapStatus.mockReturnValue({
      installed: true,
      capture: 'summary',
      limit: 500,
      recorded: 1,
      dropped: 0,
      attached: true,
    });

    const result = makeApi().messages({ type: 'sync', limit: 10 });

    expect(tapMocks.getTapMessages).toHaveBeenCalledWith({
      type: 'sync',
      limit: 10,
    });
    expect(result.messages).toEqual(canned);
    expect(result.tap.installed).toBe(true);
  });
});

describe('am.doctor', () => {
  let unregister: (() => void) | null = null;

  beforeEach(() => {
    // Healthy defaults: VFS mirrors the loaded file, automerge text
    // matches what the (registered) editor shows.
    previewRuntimeMocks.vfsListFiles.mockReturnValue({
      success: true,
      files: ['/project/index.qmd'],
    });
    previewRuntimeMocks.getFileContent.mockImplementation((path: string) =>
      path === 'index.qmd' ? 'hello body' : null,
    );
  });

  afterEach(() => {
    unregister?.();
    unregister = null;
  });

  function registerEditor(path: string | null, text: string | null) {
    unregister = registerEditorTextProvider({
      getPath: () => path,
      getText: () => text,
    });
  }

  it('returns no discrepancies for a healthy world', () => {
    registerEditor('index.qmd', 'hello body');
    expect(makeApi().doctor()).toEqual([]);
  });

  it('is healthy with no editor registered (monaco check skipped)', () => {
    expect(makeApi().doctor()).toEqual([]);
  });

  it('reports Monaco/Automerge divergence with lengths and first offset', () => {
    registerEditor('index.qmd', 'hello BODY');
    const found = makeApi().doctor();
    expect(found).toHaveLength(1);
    expect(found[0].kind).toBe('monaco-vs-automerge');
    expect(found[0].path).toBe('index.qmd');
    expect(found[0].detail).toContain('10 chars');
    expect(found[0].detail).toMatch(/offset 6/);
  });

  it('reports a file entry with no sync-client handle', () => {
    const found = makeApi({
      getFiles: () => [
        { path: 'index.qmd', docId: 'f1' },
        { path: 'ghost.qmd', docId: 'fx' },
      ],
    }).doctor();
    expect(found).toEqual([
      {
        kind: 'file-entry-without-handle',
        path: 'ghost.qmd',
        detail: expect.stringContaining('no sync-client doc'),
      },
    ]);
  });

  it('reports a handle with no file entry', () => {
    previewRuntimeMocks.getDocInventory.mockReturnValue([
      ...previewRuntimeMocks.getDocInventory(),
      {
        docId: 'f9',
        role: 'file',
        path: 'orphan.qmd',
        handleState: 'ready',
        heads: ['h9'],
        unavailableMarker: false,
      },
    ]);
    previewRuntimeMocks.vfsListFiles.mockReturnValue({
      success: true,
      files: ['/project/index.qmd', '/project/orphan.qmd'],
    });
    const found = makeApi().doctor();
    expect(found).toEqual([
      {
        kind: 'handle-without-file-entry',
        path: 'orphan.qmd',
        detail: expect.stringContaining('f9'),
      },
    ]);
  });

  it('reports a loaded file missing from the VFS', () => {
    previewRuntimeMocks.vfsListFiles.mockReturnValue({ success: true, files: [] });
    const found = makeApi().doctor();
    expect(found).toEqual([
      {
        kind: 'vfs-missing-file',
        path: 'index.qmd',
        detail: expect.stringContaining('/project/index.qmd'),
      },
    ]);
  });

  it('reports non-ready handles and stranded files distinctly', () => {
    previewRuntimeMocks.getDocInventory.mockReturnValue([
      {
        docId: 'idx1',
        role: 'index',
        path: null,
        handleState: 'ready',
        heads: ['h'],
        unavailableMarker: false,
      },
      {
        docId: 'f1',
        role: 'file',
        path: 'index.qmd',
        handleState: 'requesting',
        heads: null,
        unavailableMarker: false,
      },
      {
        docId: 'f2',
        role: 'file',
        path: 'lost.qmd',
        handleState: 'unavailable',
        heads: null,
        unavailableMarker: true,
      },
    ]);
    const found = makeApi({
      getFiles: () => [
        { path: 'index.qmd', docId: 'f1' },
        { path: 'lost.qmd', docId: 'f2' },
      ],
    }).doctor();
    const kinds = found.map((d) => [d.kind, d.path]);
    expect(kinds).toContainEqual(['handle-not-ready', 'index.qmd']);
    expect(kinds).toContainEqual(['stranded-file', 'lost.qmd']);
    // A stranded file is one problem, not three: no double-reporting
    // as not-ready or missing-from-VFS.
    expect(found.filter((d) => d.path === 'lost.qmd')).toHaveLength(1);
    // A not-ready file has no VFS expectation yet either.
    expect(kinds).not.toContainEqual(['vfs-missing-file', 'index.qmd']);
  });

  it('is probe-safe when nothing is connected', () => {
    previewRuntimeMocks.getDocInventory.mockReturnValue([]);
    previewRuntimeMocks.vfsListFiles.mockImplementation(() => {
      throw new Error('wasm not ready');
    });
    previewRuntimeMocks.getFileContent.mockImplementation(() => {
      throw new Error('no client');
    });
    registerEditor('index.qmd', 'anything');
    expect(makeApi({ getFiles: () => [] }).doctor()).toEqual([]);
  });
});

describe('am.unsafe', () => {
  it('handle(ref) returns the live DocHandle (identity, not a copy)', () => {
    const live = previewRuntimeMocks.getFileHandle('index.qmd');
    expect(makeApi().unsafe.handle('index.qmd')).toBe(live);
  });

  it('handle(ref) resolves project-set collection docs too', () => {
    const collHandle = fakeHandle({ docId: 'coll1', doc: {} });
    projectSetMocks.getCollectionHandle.mockImplementation((docId: string) =>
      docId === 'coll1' ? collHandle : null,
    );
    expect(makeApi().unsafe.handle('coll1')).toBe(collHandle);
  });

  it('handle(ref) throws on unknown refs', () => {
    expect(() => makeApi().unsafe.handle('nope')).toThrow(/unknown doc ref/);
  });

  it('exposes the automerge module for console use', () => {
    expect(makeApi().unsafe.Automerge).toBe(AutomergeModule);
  });
});
