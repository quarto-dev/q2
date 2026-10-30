/**
 * Characterization tests for Editor-internal state transitions refactored in
 * Phase 3 of the useEffect audit (bd-cp9wzu6o; plan:
 * claude-notes/plans/2026-10-24-hub-client-useeffect-refactor.md).
 *
 * Heavy leaves (Monaco, the preview router, the sync/presence/intelligence
 * hooks) are mocked; the tests pin three behaviors so the derived-state
 * refactor can move where the computation happens without changing what the
 * user sees:
 *
 * 1. Default file selection: when the project's files arrive after the first
 *    render, the default file (index.qmd preferred) becomes current.
 * 2. Diagnostics: located diagnostics become Monaco markers; unlocated ones
 *    appear in the diagnostics banner, and clearing diagnostics clears both.
 * 3. "Compare with main" diff view leaves when the file or branch changes.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act, cleanup, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import type { Route } from '../utils/routing';

// ---- shared mock state ----------------------------------------------------

const h = vi.hoisted(() => ({
  /** Props Editor last handed to the PreviewRouter mock. */
  previewProps: { current: null as Record<string, unknown> | null },
  /** Backing state for the useDocBranches mock. */
  branches: {
    branches: [] as Array<{ id: string; name: string; createdAt: number }>,
    activeBranchId: null as string | null,
  },
  /** Backing store for the usePreference mock (keyed). */
  prefs: {} as Record<string, unknown>,
  /** Monaco marker write spy (the fake monaco instance forwards here). */
  setModelMarkers: vi.fn(),
}));

// ---- module mocks ----------------------------------------------------------

// jsdom lacks matchMedia, which ThemeProvider reads for 'auto' mode.
// (The shared stub in test-utils/setup.ts is only wired into the
// integration config, not the unit config.)
vi.stubGlobal('matchMedia', (query: string) => ({
  matches: false,
  media: query,
  onchange: null,
  addListener: vi.fn(),
  removeListener: vi.fn(),
  addEventListener: vi.fn(),
  removeEventListener: vi.fn(),
  dispatchEvent: vi.fn(),
}));

// Side-effect import that bundles Monaco + workers; nothing to run in jsdom.
vi.mock('../monacoSetup', () => ({}));

vi.mock('@monaco-editor/react', async () => {
  const React = await import('react');
  const fakeModel = {};
  const fakeEditor = {
    getModel: () => fakeModel,
    addAction: () => undefined,
    getLayoutInfo: () => ({ width: 800 }),
    onDidLayoutChange: () => undefined,
    onDidFocusEditorText: () => undefined,
    onDidBlurEditorText: () => undefined,
    onKeyDown: () => undefined,
    onDidChangeCursorPosition: () => undefined,
    getDomNode: () => null,
    updateOptions: () => undefined,
  };
  const fakeMonaco = {
    KeyMod: { CtrlCmd: 1 },
    KeyCode: { KeyB: 1 },
    editor: {
      setModelMarkers: (...args: unknown[]) => h.setModelMarkers(...args),
    },
  };
  function MonacoEditorStub(props: {
    onMount?: (e: unknown, m: unknown) => void;
    defaultValue?: string;
    language?: string;
  }) {
    React.useEffect(() => {
      props.onMount?.(fakeEditor, fakeMonaco);
      // Mount once per component instance, mirroring @monaco-editor/react
      // (Editor remounts Monaco per file via `key`).
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);
    return React.createElement(
      'div',
      { 'data-testid': 'monaco-editor', 'data-language': props.language },
      props.defaultValue,
    );
  }
  return {
    __esModule: true,
    default: MonacoEditorStub,
    DiffEditor: () => React.createElement('div', { 'data-testid': 'diff-editor' }),
  };
});

vi.mock('@quarto/preview-runtime', () => ({
  createFile: vi.fn(),
  createBinaryFile: vi.fn(),
  deleteFile: vi.fn(),
  renameFile: vi.fn(),
  createFolder: vi.fn(),
  deleteFolder: vi.fn(),
  exportProjectAsZip: vi.fn(),
  vfsAddFile: vi.fn(),
  isWasmReady: () => false,
  clearCapture: vi.fn(),
  getFileContent: () => 'main-branch-content',
  // SyncStatusBadge (rendered in the document top bar) reads these.
  getFileHandle: () => null,
  getIndexHandle: () => null,
  getConnectionInfo: () => ({ connected: false }),
  getAuthorId: () => 'test-actor',
}));

// The Automerge sync hook reports content per current file so tests can tell
// which file Editor believes is selected from the Monaco stub's defaultValue.
vi.mock('../hooks/useAutomergeSync', () => ({
  useAutomergeSync: (args: { currentFile: { path: string } | null }) => ({
    content: `content-for-${args.currentFile?.path ?? 'none'}`,
    setContent: vi.fn(),
    applyingRemoteRef: { current: false },
    handleEditorChange: vi.fn(),
    handleContentRewrite: vi.fn(),
    onEditorMount: vi.fn(),
  }),
}));

vi.mock('../hooks/usePresence', () => ({
  usePresence: () => ({ remoteUsers: [], userCount: 0, onEditorMount: vi.fn() }),
}));

vi.mock('../hooks/useIntelligence', () => ({
  useIntelligence: () => ({ symbols: [], loading: false, error: null, refresh: vi.fn() }),
}));

vi.mock('../hooks/useCursorToSlide', () => ({
  useCursorToSlide: () => () => null,
}));

vi.mock('../hooks/useReplayMode', () => ({
  useReplayMode: () => ({
    state: {
      isActive: false,
      historyLength: 0,
      currentIndex: 0,
      isPlaying: false,
      playbackSpeed: 1,
      currentContent: '',
      timestamp: null,
      actor: null,
      chunkActors: [],
    },
    controls: {
      enter: vi.fn(),
      exit: vi.fn(),
      apply: vi.fn(),
      seekTo: vi.fn(),
      seekToStart: vi.fn(),
      seekToEnd: vi.fn(),
      play: vi.fn(),
      pause: vi.fn(),
      stepForward: vi.fn(),
      stepBackward: vi.fn(),
      cycleSpeed: vi.fn(),
      getTimestampAtIndex: vi.fn(),
    },
    isActiveRef: { current: false },
  }),
}));

vi.mock('../hooks/useSidebarDrawer', () => ({
  useSidebarDrawer: () => ({
    sidebarVisible: true,
    toggle: vi.fn(),
    toggleRef: { current: null },
  }),
}));

vi.mock('../hooks/useDocBranches', () => ({
  useDocBranches: () => ({
    branches: h.branches.branches,
    activeBranchId: h.branches.activeBranchId,
  }),
}));

vi.mock('../hooks/usePreference', () => ({
  usePreference: (key: string) => [h.prefs[key] ?? false, vi.fn()],
}));

vi.mock('../hooks/useIntelligenceProviders', () => ({
  useIntelligenceProviders: () => vi.fn(),
}));

vi.mock('../hooks/useWindowFileDrop', () => ({ useWindowFileDrop: vi.fn() }));

vi.mock('../hooks/useSlideThumbnails', () => ({ useSlideThumbnails: () => [] }));

vi.mock('../services/search', () => ({ useProjectSearch: () => vi.fn() }));

vi.mock('../pandoc/importService', () => ({ importAvailable: () => false }));
vi.mock('../pandoc/useImportFormats', () => ({ useImportFormats: () => [] }));
vi.mock('../pandoc/useDownloadAs', () => ({ useDownloadAs: () => ({ available: false }) }));

vi.mock('../services/branchService', () => ({
  setActiveBranch: vi.fn(),
  createBranch: vi.fn(),
  mergeBranchToMain: vi.fn(),
  deleteBranch: vi.fn(),
}));

// Capture PreviewRouter props so tests can drive Editor the way the preview
// does (diagnostics, wasm status) without the WASM render pipeline.
vi.mock('./render/PreviewRouter', async () => {
  const React = await import('react');
  return {
    __esModule: true,
    default: (props: Record<string, unknown>) => {
      h.previewProps.current = props;
      return React.createElement('div', { 'data-testid': 'preview-router' });
    },
  };
});

// ---- render helper ----------------------------------------------------------

import Editor from './Editor';
import { ThemeProvider } from './ThemeContext';
import { ViewModeProvider } from './ViewModeContext';

const indexQmd: FileEntry = { path: 'index.qmd', docId: 'doc-index' };
const otherQmd: FileEntry = { path: 'other.qmd', docId: 'doc-other' };

function editorProps(overrides: Record<string, unknown> = {}) {
  return {
    project: {
      id: 'p1',
      indexDocId: 'idx1',
      syncServer: 'wss://sync.example.com',
      description: 'Test project',
      createdAt: '2026-01-01T00:00:00.000Z',
      lastAccessed: '2026-01-01T00:00:00.000Z',
    },
    files: [indexQmd] as FileEntry[],
    fileContents: new Map<string, string>(),
    onDisconnect: vi.fn(),
    onContentOperations: vi.fn(),
    route: { type: 'project', projectId: 'p1' } as Route,
    onNavigateToFile: vi.fn(),
    isOnline: true,
    ...overrides,
  };
}

function Wrapper({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider>
      <ViewModeProvider>{children}</ViewModeProvider>
    </ThemeProvider>
  );
}

function renderEditor(overrides: Record<string, unknown> = {}) {
  const props = editorProps(overrides);
  const utils = render(
    <Wrapper>
      <Editor {...props} />
    </Wrapper>,
  );
  return {
    ...utils,
    props,
    rerenderWith(next: Record<string, unknown>) {
      Object.assign(props, next);
      utils.rerender(
        <Wrapper>
          <Editor {...props} />
        </Wrapper>,
      );
    },
  };
}

function makeDiagnostic(overrides: Partial<Diagnostic>): Diagnostic {
  return {
    kind: 'error',
    title: 'Diagnostic title',
    details: [],
    hints: [],
    ...overrides,
  } as Diagnostic;
}

beforeEach(() => {
  h.previewProps.current = null;
  h.branches.branches = [];
  h.branches.activeBranchId = null;
  h.prefs = {};
  h.setModelMarkers.mockClear();
});

afterEach(cleanup);

// ---- 1. default file selection ---------------------------------------------

describe('Editor default file selection', () => {
  it('selects the default file when the project files arrive after mount', () => {
    const { rerenderWith } = renderEditor({ files: [] });

    // Nothing to select yet: no file is current.
    expect(screen.getByTestId('monaco-editor').textContent).toBe('content-for-none');

    rerenderWith({ files: [otherQmd, indexQmd] });

    // index.qmd is the preferred default.
    expect(screen.getByTestId('monaco-editor').textContent).toBe('content-for-index.qmd');
  });

  it('keeps the current file when files change and one is already selected', () => {
    const { rerenderWith } = renderEditor({ files: [otherQmd] });
    expect(screen.getByTestId('monaco-editor').textContent).toBe('content-for-other.qmd');

    rerenderWith({ files: [otherQmd, indexQmd] });
    expect(screen.getByTestId('monaco-editor').textContent).toBe('content-for-other.qmd');
  });
});

// ---- share links -------------------------------------------------------------

describe('Editor file sidebar "Copy share link"', () => {
  let writeText: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
  });

  async function copyLinkFor(path: string): Promise<string> {
    fireEvent.contextMenu(screen.getByRole('treeitem', { name: new RegExp(path) }));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Copy share link' }));
    await waitFor(() => expect(writeText).toHaveBeenCalled());
    return writeText.mock.calls[0][0] as string;
  }

  it('copies a #/share/ URL for the clicked file, not the browser-local #/p/ URL', async () => {
    renderEditor({ files: [indexQmd, otherQmd], userName: 'Ada Lovelace' });

    const copied = await copyLinkFor('other.qmd');

    expect(copied).toContain('#/share/idx1?');
    expect(copied).not.toContain('#/p/');
    const params = new URLSearchParams(copied.split('?')[1]);
    expect(params.get('server')).toBe('wss://sync.example.com');
    expect(params.get('file')).toBe('other.qmd');
    expect(params.get('name')).toBe('Test project');
    expect(params.get('from')).toBe('Ada Lovelace');
  });

  it('matches the "Share this project" URL when the clicked file is the current one', async () => {
    renderEditor({ files: [indexQmd, otherQmd], userName: 'Ada Lovelace' });

    const copied = await copyLinkFor('index.qmd');

    fireEvent.click(screen.getByRole('button', { name: 'Share this project' }));
    expect(screen.getByRole<HTMLInputElement>('textbox').value).toBe(copied);
  });
});

// ---- 2. diagnostics ----------------------------------------------------------

describe('Editor diagnostics', () => {
  it('writes located diagnostics as Monaco markers and shows unlocated in the banner', async () => {
    renderEditor();
    await waitFor(() => expect(h.previewProps.current).toBeTruthy());

    const unlocated = makeDiagnostic({ title: 'Render failed globally', problem: 'boom' });
    const located = makeDiagnostic({
      title: 'Located problem',
      kind: 'warning',
      start_line: 3,
      start_column: 5,
    });

    act(() => {
      (h.previewProps.current as { onDiagnosticsChange: (d: Diagnostic[]) => void })
        .onDiagnosticsChange([unlocated, located]);
    });

    // The unlocated diagnostic appears in the banner...
    expect(screen.getByText('Render failed globally')).toBeDefined();
    // ...and only the located one becomes a Monaco marker.
    const markersCalls = h.setModelMarkers.mock.calls;
    expect(markersCalls.length).toBeGreaterThan(0);
    const markers = markersCalls[markersCalls.length - 1][2] as Array<{
      startLineNumber: number;
      startColumn: number;
      message: string;
    }>;
    expect(markers).toHaveLength(1);
    expect(markers[0].startLineNumber).toBe(3);
    expect(markers[0].startColumn).toBe(5);
    expect(markers[0].message).toContain('Located problem');

    // Clearing diagnostics clears both the banner and the markers.
    act(() => {
      (h.previewProps.current as { onDiagnosticsChange: (d: Diagnostic[]) => void })
        .onDiagnosticsChange([]);
    });
    expect(screen.queryByText('Render failed globally')).toBeNull();
    const cleared = h.setModelMarkers.mock.calls.at(-1)![2] as unknown[];
    expect(cleared).toHaveLength(0);
  });
});

// ---- 3. compare-with-main reset ----------------------------------------------

describe('Editor compare-with-main', () => {
  function setupBranches() {
    h.prefs.documentBranches = true;
    h.branches.branches = [{ id: 'b1', name: 'b1', createdAt: 0 }];
    h.branches.activeBranchId = 'b1';
  }

  async function enterCompareMode() {
    fireEvent.click(await screen.findByRole('button', { name: /compare with main/i }));
    expect(screen.getByTestId('diff-editor')).toBeDefined();
    expect(screen.queryByTestId('monaco-editor')).toBeNull();
  }

  it('leaves compare mode when switching files', async () => {
    setupBranches();
    const { rerenderWith } = renderEditor({ files: [indexQmd, otherQmd] });
    await enterCompareMode();

    // Browser back/forward navigation switches the file through the route.
    rerenderWith({ route: { type: 'file', projectId: 'p1', filePath: 'other.qmd' } as Route });

    await waitFor(() => expect(screen.queryByTestId('diff-editor')).toBeNull());
    expect(screen.getByTestId('monaco-editor').textContent).toBe('content-for-other.qmd');
  });

  it('leaves compare mode when the active branch changes', async () => {
    setupBranches();
    const { rerenderWith } = renderEditor();
    await enterCompareMode();

    // Merging or deleting the branch resets the active branch to main.
    h.branches.activeBranchId = null;
    rerenderWith({}); // force a render with the new branch state

    await waitFor(() => expect(screen.queryByTestId('diff-editor')).toBeNull());
    expect(screen.getByTestId('monaco-editor')).toBeDefined();
  });
});
