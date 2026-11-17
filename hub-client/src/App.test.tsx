/**
 * App-level regression tests (bd-lsf2v5n0 — Phase 1 of the hub-client
 * useEffect refactor). Heavy leaves (Editor, home screens, the sync
 * runtime, IndexedDB stores) are mocked; the tests drive App's own state
 * machine through the seams a real session would use.
 *
 * Covered:
 * - identity seeding re-runs when auth resolves/changes after the initial
 *   load (App.tsx's identity effect previously depended only on
 *   `authLoading`, so a later login never re-seeded the screen name);
 * - the sync-handler registration effect wires `onConnectionChange`
 *   through to `isOnline` (the error-banner semantics of that handler
 *   live in services/connectionHandlers.test.ts);
 * - characterization: the project summary denormalization effect keeps
 *   its current behavior across the exhaustive-deps triage.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest';
import { render, act, waitFor, cleanup } from '@testing-library/react';
import type { AuthState } from './services/authService';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';

// ---- shared mock state ----------------------------------------------------

const mocks = vi.hoisted(() => {
  const identity = {
    userId: 'user-1',
    userName: 'Anonymous Badger',
    userColor: '#447099',
    createdAt: '2026-10-01T00:00:00.000Z',
    updatedAt: '2026-10-01T00:00:00.000Z',
  };
  const collectionSetsActions = {
    createProjectSet: vi.fn(async () => {}),
    linkProjectSet: vi.fn(async () => {}),
    migrateProjects: vi.fn(async () => {}),
    mergeIntoProjectSet: vi.fn(async () => {}),
    retry: vi.fn(async () => {}),
    addProject: vi.fn(),
    removeProject: vi.fn(),
    updateProjectDescription: vi.fn(),
    updateProjectSummary: vi.fn(),
    touchProject: vi.fn(),
    importProjects: vi.fn(),
    getProjectSetDocId: () => 'root-doc-id',
    getSyncServer: () => 'ws://sync.example',
    createCollection: vi.fn(async () => 'collection-1'),
    subscribeCollection: vi.fn(async () => {}),
    unsubscribeCollection: vi.fn(),
    renameCollection: vi.fn(),
    addProjectToCollection: vi.fn(),
    removeProjectFromCollection: vi.fn(),
    moveProjectBetweenCollections: vi.fn(),
  };
  return {
    auth: { auth: null as AuthState | null, loading: true },
    identity,
    syncHandlers: { current: null as Record<string, (arg: never) => void> | null },
    editorProps: { current: null as { isOnline?: boolean } | null },
    homeProps: {
      current: null as {
        error?: string | null;
        onSelectProject: (p: ProjectEntry) => Promise<void>;
      } | null,
    },
    connect: vi.fn(),
    getFileContent: vi.fn(),
    getUserIdentity: vi.fn(async () => ({ ...identity })),
    updateUserName: vi.fn(async (name: string) => {
      identity.userName = name;
      identity.updatedAt = '2026-10-02T00:00:00.000Z';
      return { ...identity };
    }),
    resolveAuthorId: vi.fn(async () => 'author-1'),
    projectStorageGet: vi.fn(),
    collectionSetsActions,
  };
});

// ---- module mocks ----------------------------------------------------------

vi.mock('@quarto/preview-runtime', () => ({
  connect: (...args: unknown[]) => mocks.connect(...args),
  disconnect: vi.fn(async () => {}),
  setSyncHandlers: (handlers: Record<string, (arg: never) => void>) => {
    mocks.syncHandlers.current = handlers;
  },
  getFileContent: (path: string) => mocks.getFileContent(path),
  applyEditorOperations: vi.fn(),
  createNewProject: vi.fn(),
  getProjectChoices: vi.fn(async () => []),
  createProject: vi.fn(),
}));

vi.mock('./services/projectStorage', () => ({
  addProject: vi.fn(),
  listProjects: vi.fn(async () => []),
  getProject: (id: string) => mocks.projectStorageGet(id),
}));

vi.mock('./services/seedExamples', () => ({
  seedExampleProjects: vi.fn(async () => null),
}));

vi.mock('./services/debugApi', () => ({ installDebugApi: vi.fn(() => undefined) }));

vi.mock('./services/userSettings', () => ({
  getUserIdentity: () => mocks.getUserIdentity(),
  updateUserName: (name: string) => mocks.updateUserName(name),
  authorIdFromUserId: (userId: string) => `author-${userId}`,
}));

vi.mock('./services/authService', () => ({
  resolveAuthorId: (...args: unknown[]) => mocks.resolveAuthorId(...args),
}));

vi.mock('./hooks/useCollectionSets', () => ({
  useCollectionSets: () => [
    {
      status: 'connected',
      collections: [],
      projects: [],
      unreachable: [],
      error: null,
      legacyProjects: [],
    },
    mocks.collectionSetsActions,
  ],
}));

vi.mock('./hooks/useAuth', () => ({
  useAuth: () => ({
    auth: mocks.auth.auth,
    loading: mocks.auth.loading,
    logout: vi.fn(),
    sessionExpired: false,
    expireSession: vi.fn(),
    applyAuth: vi.fn(),
  }),
}));

vi.mock('./hooks/useAuthProbe', () => ({ useAuthProbe: vi.fn() }));
vi.mock('./hooks/useSessionKeepAlive', () => ({ useSessionKeepAlive: vi.fn() }));
vi.mock('./hooks/useExecutionChannel', () => ({
  useExecutionChannel: () => ({ executors: [], requestExecution: vi.fn() }),
}));
vi.mock('./hooks/usePreviewSession', () => ({ usePreviewSession: () => null }));

vi.mock('./auth/AuthProvider', () => ({
  useAuthProvider: () => ({ SignInButton: () => null }),
}));

vi.mock('./components/Editor', () => ({
  default: (props: { isOnline?: boolean }) => {
    mocks.editorProps.current = props;
    return <div data-testid="editor-stub" />;
  },
}));

vi.mock('./components/ProjectsHome', () => ({
  default: (props: {
    error?: string | null;
    onSelectProject: (p: ProjectEntry) => Promise<void>;
  }) => {
    mocks.homeProps.current = props;
    return <div data-testid="projects-home-stub">{props.error ?? ''}</div>;
  },
}));

vi.mock('./components/ProjectSelector', () => ({
  default: () => <div data-testid="project-selector-stub" />,
}));

// The demo gallery statically imports the real Monaco editor, which
// jsdom cannot load; App only renders it on the demo route.
vi.mock('./components/demo/DemoGallery', () => ({
  default: () => <div data-testid="demo-gallery-stub" />,
}));

vi.mock('./components/auth/LoginScreen', () => ({
  LoginScreen: () => <div data-testid="login-screen-stub" />,
}));

// ---- fixtures ---------------------------------------------------------------

const ADA: AuthState = {
  email: 'ada@example.com',
  name: 'Ada Lovelace',
  picture: null,
  expiresAt: Date.now() + 3_600_000,
  credential: 'session',
};

const ENTRY: ProjectEntry = {
  id: 'p1',
  indexDocId: 'doc-1',
  syncServer: 'ws://sync.example',
  description: 'Project One',
  createdAt: '2026-10-01T00:00:00.000Z',
  lastAccessed: '2026-10-01T00:00:00.000Z',
};

// App.tsx computes AUTH_ENABLED at module scope from the env, so the env
// must be stubbed before the module is imported.
let App: typeof import('./App').default;

beforeAll(async () => {
  vi.stubEnv('VITE_GOOGLE_CLIENT_ID', 'test-client-id');
  App = (await import('./App')).default;
});

beforeEach(() => {
  vi.clearAllMocks();
  window.location.hash = '';
  mocks.auth.auth = null;
  mocks.auth.loading = true;
  mocks.identity.userName = 'Anonymous Badger';
  mocks.identity.updatedAt = mocks.identity.createdAt;
  mocks.syncHandlers.current = null;
  mocks.editorProps.current = null;
  mocks.homeProps.current = null;
});

afterEach(cleanup);

// ---- tests ------------------------------------------------------------------

describe('identity seeding (auth)', () => {
  it('re-seeds the screen name when auth resolves after the initial anonymous load', async () => {
    const { rerender } = render(<App />);
    // Auth still resolving: the identity effect waits, no IndexedDB read yet.
    expect(mocks.getUserIdentity).not.toHaveBeenCalled();

    // Auth resolves to "signed out": seed the anonymous identity.
    mocks.auth.loading = false;
    rerender(<App />);
    await waitFor(() => expect(mocks.getUserIdentity).toHaveBeenCalledTimes(1));
    expect(mocks.updateUserName).not.toHaveBeenCalled();

    // The user signs in: the untouched anonymous identity upgrades to the
    // OIDC display name. Pre-fix the effect depended only on `authLoading`
    // and never re-ran here.
    mocks.auth.auth = ADA;
    rerender(<App />);
    await waitFor(() => expect(mocks.updateUserName).toHaveBeenCalledWith('Ada Lovelace'));
  });

  it('does not rewrite IndexedDB when the same session re-resolves', async () => {
    mocks.auth.loading = false;
    mocks.auth.auth = ADA;
    const { rerender } = render(<App />);
    await waitFor(() => expect(mocks.updateUserName).toHaveBeenCalledWith('Ada Lovelace'));
    expect(mocks.updateUserName).toHaveBeenCalledTimes(1);

    // Same session re-resolves as a fresh object (keep-alive slide): the
    // identity is no longer untouched, so no second write.
    mocks.auth.auth = { ...ADA };
    rerender(<App />);
    await waitFor(() => expect(mocks.getUserIdentity).toHaveBeenCalledTimes(2));
    expect(mocks.updateUserName).toHaveBeenCalledTimes(1);
  });
});

describe('sync handler registration', () => {
  it('drives isOnline from onConnectionChange and denormalizes the project summary', async () => {
    mocks.auth.loading = false;
    mocks.auth.auth = ADA;
    mocks.connect.mockResolvedValue([{ path: 'a.qmd', docId: 'file-doc-1' }]);
    mocks.getFileContent.mockReturnValue('# Hello');
    mocks.projectStorageGet.mockResolvedValue(ENTRY);

    render(<App />);
    await waitFor(() => expect(mocks.homeProps.current).toBeTruthy());

    // Open a project through the home screen's callback.
    await act(async () => {
      await mocks.homeProps.current!.onSelectProject(ENTRY);
    });
    await waitFor(() => expect(mocks.editorProps.current).toBeTruthy());
    expect(mocks.syncHandlers.current).toBeTruthy();

    // The registered onConnectionChange handler drives isOnline both ways.
    const handlers = mocks.syncHandlers.current!;
    act(() => handlers.onConnectionChange(false));
    expect(mocks.editorProps.current!.isOnline).toBe(false);
    act(() => handlers.onConnectionChange(true));
    expect(mocks.editorProps.current!.isOnline).toBe(true);

    // Characterization (deps-triaged effect): the open project denormalizes
    // a peek summary onto its project-set entry.
    await waitFor(() =>
      expect(mocks.collectionSetsActions.updateProjectSummary).toHaveBeenCalledWith(
        ENTRY.indexDocId,
        expect.objectContaining({ fileCount: 1, topFiles: ['a.qmd'] }),
      ),
    );
  });
});
