/**
 * Dev-only harness for rendering components in isolation.
 *
 * Used by dev routes (#/dev/<page>) and Playwright visual regression tests
 * to render hard-to-reach UI states (migration screens, error states, etc.)
 * without needing real data.
 *
 * This component is only imported in development builds.
 */

import React, { useState } from 'react';
import ProjectSetSetup from './ProjectSetSetup';
import ProjectsHome from './ProjectsHome';
import NewFileDialog from './NewFileDialog';
import ShareDialog from './ShareDialog';
import NewAssetDialog from './NewAssetDialog';
import FileSidebar from './FileSidebar';
import OutlinePanel from './OutlinePanel';
import SidebarTabs from './SidebarTabs';
import StatusTab from './tabs/StatusTab';
import ProjectTopBar from './ProjectTopBar';
import DocumentTopBar from './DocumentTopBar';
import Toast from './Toast';
import UpdateAvailableToast from './UpdateAvailableToast';
import EphemeralSessionBanner from './EphemeralSessionBanner';
import DevTokensPage from './DevTokensPage';
import DevGalleryPage from './DevGalleryPage';
import AboutTab from './tabs/AboutTab';
import ReplayDrawer from './ReplayDrawer';
import SidebarDrawer from './SidebarDrawer';
import { useSidebarDrawer } from '../hooks/useSidebarDrawer';
import { ViewModeProvider } from './ViewModeContext';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import type { Symbol } from '@quarto/preview-renderer/types/intelligence';
import type { ProjectSetEntry } from '@quarto/quarto-automerge-schema';
import type { CollectionSnapshot } from '../services/projectSetService';
import type { SearchFiles } from '../services/search';
import type { PwaPromptStore } from '../pwaPrompt';
import type { ReplayState, ReplayControls } from '../hooks/useReplayMode';

const FAKE_LEGACY_PROJECTS: ProjectEntry[] = [
  {
    id: 'fake-1',
    indexDocId: 'automerge:fake1',
    syncServer: 'wss://sync.automerge.org',
    description: 'My Research Paper',
    createdAt: new Date(Date.now() - 86400000).toISOString(),
    lastAccessed: new Date(Date.now() - 3600000).toISOString(),
  },
  {
    id: 'fake-2',
    indexDocId: 'automerge:fake2',
    syncServer: 'wss://sync.automerge.org',
    description: 'Course Notes',
    createdAt: new Date(Date.now() - 172800000).toISOString(),
    lastAccessed: new Date(Date.now() - 7200000).toISOString(),
  },
  {
    id: 'fake-3',
    indexDocId: 'automerge:fake3',
    syncServer: 'wss://sync.automerge.org',
    description: 'Blog',
    createdAt: new Date().toISOString(),
    lastAccessed: new Date().toISOString(),
  },
];

const noop = async () => {};

/* ---- canned data for baseline routes ---- */

const FAKE_SET_ENTRIES: ProjectSetEntry[] = [
  {
    indexDocId: 'automerge:proj-alpha',
    syncServer: 'wss://sync.automerge.org',
    description: 'Research Paper',
    addedAt: '2026-08-01T10:00:00.000Z',
    lastAccessed: '2026-08-24T15:30:00.000Z',
    summary: {
      fileCount: 4,
      topFiles: ['index.qmd', 'analysis.qmd', 'references.bib'],
      contributors: [{ name: 'Ada', color: '#447099' }],
      asOf: '2026-08-24T15:30:00.000Z',
    },
  },
  {
    indexDocId: 'automerge:proj-beta',
    syncServer: 'wss://sync.automerge.org',
    description: 'Course Notes',
    addedAt: '2026-08-10T09:00:00.000Z',
    lastAccessed: '2026-08-20T11:00:00.000Z',
  },
  {
    indexDocId: 'automerge:proj-gamma',
    syncServer: 'wss://sync.automerge.org',
    description: 'Blog Redesign',
    addedAt: '2026-08-15T14:00:00.000Z',
    lastAccessed: '2026-08-18T08:00:00.000Z',
  },
];

const FAKE_COLLECTIONS: CollectionSnapshot[] = [
  {
    docId: 'automerge:root-set',
    syncServer: 'wss://sync.automerge.org',
    name: undefined,
    entries: FAKE_SET_ENTRIES,
    isRoot: true,
  },
];

const FAKE_FILES: FileEntry[] = [
  { path: 'index.qmd', docId: 'automerge:file-1' },
  { path: 'analysis.qmd', docId: 'automerge:file-2' },
  { path: 'data/survey.csv', docId: 'automerge:file-3' },
  { path: 'figures/plot.png', docId: 'automerge:file-4' },
  { path: '_quarto.yml', docId: 'automerge:file-5' },
  { path: 'references.bib', docId: 'automerge:file-6' },
];

const fakeRange = (line: number) => ({
  start: { line, character: 0 },
  end: { line, character: 10 },
});

const FAKE_SYMBOLS: Symbol[] = [
  {
    name: 'Introduction',
    kind: 'string',
    range: fakeRange(1),
    selectionRange: fakeRange(1),
    children: [
      {
        name: 'Background',
        kind: 'string',
        range: fakeRange(5),
        selectionRange: fakeRange(5),
        children: [],
      },
    ],
  },
  {
    name: 'setup',
    detail: '5 lines',
    kind: 'function',
    range: fakeRange(12),
    selectionRange: fakeRange(12),
    children: [],
  },
  {
    name: 'Results',
    kind: 'string',
    range: fakeRange(20),
    selectionRange: fakeRange(20),
    children: [
      {
        name: 'plot-temperature',
        detail: '12 lines',
        kind: 'module',
        range: fakeRange(24),
        selectionRange: fakeRange(24),
        children: [],
      },
    ],
  },
];

/** Always-pending prompt store so the update toast renders in the harness. */
const pendingPrompt: PwaPromptStore = {
  show() {},
  subscribe: () => () => {},
  isPending: () => true,
};

/** Search fixture for the sidebar route: substring match over FAKE_FILES. */
const fakeSearchFiles: SearchFiles = async (query) => {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  return FAKE_FILES.filter((f) => f.path.toLowerCase().includes(q)).map(
    (f) => ({ path: f.path, score: 1, terms: [q] }),
  );
};

/**
 * Stateful sidebar sections fixture, shared by the sidebar and
 * editor-shell routes. Interaction specs need observable behavior, so
 * this tracks the selected file (aria-selected / active-row state) and
 * records the last action in an offscreen testid element for assertions.
 */
function StatefulSidebarSections({ files = FAKE_FILES }: { files?: FileEntry[] }) {
  const [currentFile, setCurrentFile] = useState<FileEntry | null>(files[0] ?? null);
  const [lastAction, setLastAction] = useState('none');
  return (
    <>
      <SidebarTabs>
        {(sectionId) =>
          sectionId === 'files' ? (
            <FileSidebar
              files={files}
              currentFile={currentFile}
              onSelectFile={(f) => {
                setCurrentFile(f);
                setLastAction(`select:${f.path}`);
              }}
              onNewFile={() => setLastAction('new-file')}
              onUploadFiles={() => setLastAction('upload')}
              onDeleteFile={(f) => setLastAction(`delete:${f.path}`)}
              onRenameFile={(f, p) => setLastAction(`rename:${f.path}->${p}`)}
              onOpenInNewTab={(f) => setLastAction(`new-tab:${f.path}`)}
              onCopyLink={(f) => setLastAction(`copy:${f.path}`)}
              currentFormat="q2-preview"
              searchFiles={fakeSearchFiles}
            />
          ) : sectionId === 'outline' ? (
            <OutlinePanel
              symbols={FAKE_SYMBOLS}
              onSymbolClick={(s) => setLastAction(`symbol:${s.name}`)}
            />
          ) : (
            <div style={{ padding: 12, fontSize: 13, color: 'var(--text-secondary)' }}>
              {sectionId} section
            </div>
          )
        }
      </SidebarTabs>
      {/* Offscreen action recorder for Playwright assertions. */}
      <div
        data-testid="sidebar-last-action"
        style={{ position: 'fixed', left: -10000, top: 0 }}
      >
        {lastAction}
      </div>
    </>
  );
}

/**
 * Stateful sidebar fixture at a fixed harness width, mirroring how the
 * sidebar sits in the editor shell.
 */
function SidebarHarness({ files = FAKE_FILES }: { files?: FileEntry[] }) {
  return (
    <EditorChrome>
      <div style={{ width: 280, height: '100%', borderRight: '1px solid var(--sidebar-border)' }}>
        <StatefulSidebarSections files={files} />
      </div>
    </EditorChrome>
  );
}

/**
 * Composed editor-shell fixture (Phase 4): the real top bars,
 * SidebarTabs, and `.editor-main view-mode-*` pane layout from Editor.tsx
 * with placeholder pane content standing in for Monaco and the preview
 * iframe (both need services the no-server harness doesn't have). This
 * lets viewport-matrix specs exercise the shell's flex interplay at
 * narrow widths. The view mode is pinned per route (the segmented
 * view-toggle control was removed; the divider drag owns the split now).
 */
function EditorShellHarness({ viewMode }: { viewMode: 'markup' | 'both' | 'preview' }) {
  // Phase 5: the same drawer wiring as Editor.tsx, plus an offscreen
  // recorder so specs can assert overflow-menu actions fire.
  const drawer = useSidebarDrawer();
  const [lastAction, setLastAction] = useState('none');
  const placeholder: React.CSSProperties = {
    height: '100%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: 'var(--editor-text-muted)',
    fontFamily: 'var(--font-mono)',
    fontSize: 12,
  };
  return (
    <EditorChrome>
      <div className="editor-columns">
        <SidebarDrawer drawer={drawer}>
          <div className="project-column">
            <ProjectTopBar
              projectName="Research Paper"
              onChooseNewProject={() => {}}
              onShare={() => setLastAction('share')}
            />
            <StatefulSidebarSections />
            <div className="project-bottom-bar" />
          </div>
        </SidebarDrawer>
        <div className="document-column">
          <DocumentTopBar
            currentFilePath="index.qmd"
            onToggleFullscreenPreview={() => setLastAction('fullscreen-preview')}
            isFullscreenPreview={false}
            sidebarOpen={drawer.sidebarVisible}
            onToggleSidebar={drawer.toggle}
            sidebarToggleRef={drawer.toggleRef}
          />
          <main className={`editor-main view-mode-${viewMode}`}>
            <div className="pane editor-pane">
              <div style={placeholder}>Editor pane</div>
            </div>
            <div className="pane-divider" />
            <div className="pane preview-pane">
              <div style={placeholder}>Preview pane</div>
            </div>
          </main>
        </div>
      </div>
      {/* Offscreen action recorder for Playwright assertions. */}
      <div
        data-testid="header-last-action"
        style={{ position: 'fixed', left: -10000, top: 0 }}
      >
        {lastAction}
      </div>
    </EditorChrome>
  );
}

/** Shared canned props for the ProjectsHome routes. */
const fakeProjectsHomeProps = {
  onSelectProject: () => {},
  isConnecting: false,
  error: null,
  projectSetStatus: 'connected' as const,
  projectSetEntries: FAKE_SET_ENTRIES,
  collections: FAKE_COLLECTIONS,
  onRemoveProjectFromSet: () => {},
  onTouchProject: () => {},
  onAddProjectToSet: () => {},
  onRenameProject: () => {},
  onUpdateProjectSummary: () => {},
  onCreateCollection: async () => 'automerge:new-collection',
  onUnsubscribeCollection: noop,
  onRenameCollection: () => {},
  onAddProjectToCollection: () => {},
  onRemoveProjectFromCollection: () => {},
  onMoveProjectBetweenCollections: () => {},
};

/**
 * ProjectsHome with a pinned connection error and a working retry, wired
 * to the offscreen action recorder so specs can assert the retry fires.
 */
function ProjectsHomeErrorHarness() {
  const [lastAction, setLastAction] = useState('none');
  return (
    <>
      <ProjectsHome
        {...fakeProjectsHomeProps}
        error="Could not reach the sync server."
        onRetry={() => setLastAction('retry')}
      />
      <div
        data-testid="async-last-action"
        style={{ position: 'fixed', left: -10000, top: 0 }}
      >
        {lastAction}
      </div>
    </>
  );
}

/** StatusTab with a pinned WASM boot error and a recorded retry. */
function StatusTabErrorHarness() {
  const [lastAction, setLastAction] = useState('none');
  return (
    <EditorChrome>
      <div style={{ width: 280, height: '100%', overflowY: 'auto', borderRight: '1px solid var(--sidebar-border)', background: 'var(--sidebar-bg)' }}>
        <StatusTab
          wasmStatus="error"
          wasmError="WebAssembly compilation failed: unexpected section id"
          userCount={0}
          remoteUsers={[]}
          isOnline={true}
          onRetry={() => setLastAction('reload')}
        />
      </div>
      <div
        data-testid="async-last-action"
        style={{ position: 'fixed', left: -10000, top: 0 }}
      >
        {lastAction}
      </div>
    </EditorChrome>
  );
}

/** Editor chrome must render inside .editor-container for the dark-ramp
 *  token overrides (:root.dark .editor-container) to apply, and under a
 *  ViewModeProvider for Editor-shell fixtures. */
/** Module-level so ReplayHarness render stays pure (see timestamp below). */
const REPLAY_FIXTURE_TIMESTAMP = (Date.now() - 42 * 60_000) / 1000;

/**
 * Replay drawer fixture (Phase 5): the real expanded drawer with a remote
 * actor, plus a static replica of the "me" actor chip. The real
 * `--me` state requires the live sync actor id (getActorId()), which the
 * no-server harness doesn't have — the replica uses the real
 * `replay-drawer__actor` classes so token changes render authentically.
 * Keep the replica's markup in sync with the actor chip in
 * ReplayDrawer.tsx.
 */
function ReplayHarness() {
  const state: ReplayState = {
    isActive: true,
    historyLength: 100,
    currentIndex: 41,
    isPlaying: false,
    playbackSpeed: 1,
    currentContent: '',
    // Seconds, not ms — ReplayDrawer formats with `new Date(ts * 1000)`.
    // Module-level so render stays pure (react-hooks/purity); visual
    // specs pin the clock (FIXED_NOW) before page load, so the module
    // evaluation is deterministic there. The offset gives a "42 minutes
    // ago" label.
    timestamp: REPLAY_FIXTURE_TIMESTAMP,
    actor: 'b3f7c2a1-remote',
    chunkActors: Array.from({ length: 24 }, (_, i) =>
      i % 3 === 2
        ? [
            { actor: 'b3f7c2a1-remote', fraction: 0.6 },
            { actor: 'e4d8f0a2-other', fraction: 0.4 },
          ]
        : [{ actor: 'b3f7c2a1-remote', fraction: 1 }],
    ),
  };
  const controls: ReplayControls = {
    enter: () => {},
    exit: () => {},
    apply: () => {},
    seekTo: () => {},
    seekToStart: () => {},
    seekToEnd: () => {},
    play: () => {},
    pause: () => {},
    stepForward: () => {},
    stepBackward: () => {},
    cycleSpeed: () => {},
    getTimestampAtIndex: () => null,
  };
  return (
    <EditorChrome>
      <div style={{ display: 'flex', flexDirection: 'column', justifyContent: 'flex-end', height: '100%' }}>
        {/* Tight crop target for the visual spec — the full-height flex
            wrapper above is just for bottom-docking. */}
        <div data-testid="replay-fixture">
          {/* Static replica of the "me" actor chip (see comment above). */}
          <div style={{ padding: '8px 12px', display: 'flex', gap: 8, alignItems: 'center' }}>
            <span style={{ fontSize: 12, color: 'var(--editor-text-dim)' }}>
              &ldquo;me&rdquo; chip replica:
            </span>
            <span className="replay-drawer__actor replay-drawer__actor--me">
              <span
                className="replay-drawer__actor-dot"
                style={{ backgroundColor: '#447099' }}
              />
              Ada Lovelace
            </span>
            <span className="replay-drawer__actor">
              <span
                className="replay-drawer__actor-dot"
                style={{ backgroundColor: '#E91E63' }}
              />
              Grace Hopper
            </span>
          </div>
          <ReplayDrawer
            state={state}
            controls={controls}
            identities={{ 'b3f7c2a1-remote': { name: 'Grace Hopper', color: '#E91E63' } }}
          />
        </div>
      </div>
    </EditorChrome>
  );
}

function EditorChrome({ children }: { children: React.ReactNode }) {
  return (
    <ViewModeProvider>
      <div className="editor-container" style={{ height: '100vh', background: 'var(--editor-bg)' }}>
        {children}
      </div>
    </ViewModeProvider>
  );
}

interface Props {
  page: string;
}

const DEV_PAGES: Record<string, () => React.ReactNode> = {
  'setup-migration': () => (
    <ProjectSetSetup
      hasMigration={true}
      legacyProjects={FAKE_LEGACY_PROJECTS}
      error={null}
      isConnecting={false}
      onCreateProjectSet={noop}
      onLinkProjectSet={noop}
      onMigrateProjects={noop}
      onMergeIntoProjectSet={noop}
    />
  ),
  'setup-migration-error': () => (
    <ProjectSetSetup
      hasMigration={true}
      legacyProjects={FAKE_LEGACY_PROJECTS}
      error="Connection failed: could not reach sync server"
      isConnecting={false}
      onCreateProjectSet={noop}
      onLinkProjectSet={noop}
      onMigrateProjects={noop}
      onMergeIntoProjectSet={noop}
    />
  ),
  'setup-fresh': () => (
    <ProjectSetSetup
      hasMigration={false}
      legacyProjects={[]}
      error={null}
      isConnecting={false}
      onCreateProjectSet={noop}
      onLinkProjectSet={noop}
      onMigrateProjects={noop}
      onMergeIntoProjectSet={noop}
    />
  ),

  /* ---- Phase 0 baseline routes (characterization testing) ---- */

  tokens: () => <DevTokensPage />,

  gallery: () => <DevGalleryPage />,

  'projects-home': () => <ProjectsHome {...fakeProjectsHomeProps} />,

  /* ---- Phase 3 async-state routes (loading / error+retry / empty) ---- */

  'projects-home-loading': () => (
    <ProjectsHome
      {...fakeProjectsHomeProps}
      projectSetStatus="connecting"
      projectSetEntries={undefined}
      collections={undefined}
    />
  ),
  'projects-home-error': () => <ProjectsHomeErrorHarness />,
  'projects-home-empty': () => (
    <ProjectsHome
      {...fakeProjectsHomeProps}
      projectSetEntries={[]}
      collections={[{ ...FAKE_COLLECTIONS[0], entries: [] }]}
    />
  ),
  'sidebar-empty': () => <SidebarHarness files={[]} />,
  'status-tab-loading': () => (
    <EditorChrome>
      <div style={{ width: 280, height: '100%', overflowY: 'auto', borderRight: '1px solid var(--sidebar-border)', background: 'var(--sidebar-bg)' }}>
        <StatusTab wasmStatus="loading" wasmError={null} userCount={0} remoteUsers={[]} isOnline={true} />
      </div>
    </EditorChrome>
  ),
  'status-tab-error': () => <StatusTabErrorHarness />,
  'dialog-new-file': () => (
    <EditorChrome>
      <NewFileDialog
        isOpen={true}
        existingPaths={FAKE_FILES.map((f) => f.path)}
        onClose={() => {}}
        onCreateTextFile={() => {}}
      />
    </EditorChrome>
  ),
  'dialog-share': () => (
    <EditorChrome>
      <ShareDialog
        isOpen={true}
        shareableUrl="https://hub.example.com/#/share/abc123/index.qmd?name=Research%20Paper"
        onClose={() => {}}
      />
    </EditorChrome>
  ),
  'dialog-new-asset': () => (
    <EditorChrome>
      <NewAssetDialog
        isOpen={true}
        existingPaths={FAKE_FILES.map((f) => f.path)}
        defaultDestination="figures"
        onClose={() => {}}
        onUploadAsset={() => {}}
      />
    </EditorChrome>
  ),
  sidebar: () => <SidebarHarness />,
  /* ---- Phase 5: replay drawer (actor-chip token review) ---- */
  replay: () => <ReplayHarness />,
  /* ---- Phase 4: composed editor shell for the viewport matrix ---- */
  'editor-shell': () => <EditorShellHarness viewMode="both" />,
  'editor-shell-markup': () => <EditorShellHarness viewMode="markup" />,
  'editor-shell-preview': () => <EditorShellHarness viewMode="preview" />,
  // Fullscreen preview (Editor.tsx: header/editor-pane/divider unmount,
  // the preview pane gains .fullscreen) — regression cover for the
  // ≤700px split-collapse rule hiding the fullscreen pane.
  'editor-shell-fullscreen': () => (
    <EditorChrome>
      <main className="editor-main view-mode-both">
        <div className="pane preview-pane fullscreen">
          <button className="fullscreen-close-btn" aria-label="Exit fullscreen preview">
            ✕
          </button>
          <div
            style={{
              height: '100%',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              color: 'var(--editor-text-muted)',
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
            }}
          >
            Preview pane (fullscreen)
          </div>
        </div>
      </main>
    </EditorChrome>
  ),
  // The About tab standalone (sidebar width) — covers the shortcuts
  // reference, which the sidebar route's collapsed ABOUT section hides.
  'about-tab': () => (
    <EditorChrome>
      <div style={{ width: 280, height: '100%', overflowY: 'auto', borderRight: '1px solid var(--sidebar-border)', background: 'var(--sidebar-bg)' }}>
        <AboutTab wasmStatus="loading" />
      </div>
    </EditorChrome>
  ),
  header: () => (
    <EditorChrome>
      <div className="top-bars">
        <ProjectTopBar
          projectName="Research Paper"
          onChooseNewProject={() => {}}
          onShare={() => {}}
        />
        <DocumentTopBar
          currentFilePath="index.qmd"
          onToggleFullscreenPreview={() => {}}
          isFullscreenPreview={false}
          // The sidebar toggle is permanent chrome (Phase 5 review); the
          // header route has no sidebar, so a static open state + dummy ref.
          sidebarOpen={true}
          onToggleSidebar={() => {}}
          sidebarToggleRef={{ current: null }}
        />
      </div>
      {/* The toggle's aria-controls points at the drawer's id; in the
          real app the drawer always exists, so the route stubs it
          (axe's aria-valid-attr-value flags a dangling reference). */}
      <div id="sidebar-drawer" hidden />
    </EditorChrome>
  ),
  notifications: () => (
    <EditorChrome>
      <EphemeralSessionBanner />
      <Toast message="Auto-saved" visible={true} onHide={() => {}} duration={1_000_000} />
      <UpdateAvailableToast prompt={pendingPrompt} />
    </EditorChrome>
  ),
};

export default function DevHarness({ page }: Props) {
  const renderPage = DEV_PAGES[page];

  if (!renderPage) {
    const available = Object.keys(DEV_PAGES).join(', ');
    return (
      <div style={{ padding: 40, color: 'var(--text-primary)', fontFamily: 'monospace' }}>
        <h2>Unknown dev page: {page}</h2>
        <p>Available pages: {available}</p>
      </div>
    );
  }

  return renderPage();
}
