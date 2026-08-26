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
import MinimalHeader from './MinimalHeader';
import Toast from './Toast';
import UpdateAvailableToast from './UpdateAvailableToast';
import EphemeralSessionBanner from './EphemeralSessionBanner';
import DevTokensPage from './DevTokensPage';
import DevGalleryPage from './DevGalleryPage';
import { ViewModeProvider } from './ViewModeContext';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import type { Symbol } from '@quarto/preview-renderer/types/intelligence';
import type { ProjectSetEntry } from '@quarto/quarto-automerge-schema';
import type { CollectionSnapshot } from '../services/projectSetService';
import type { SearchFiles } from '../services/search';
import type { PwaPromptStore } from '../pwaPrompt';

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
 * Stateful sidebar fixture. Keyboard-interaction specs need observable
 * behavior, so unlike the other baseline routes this one tracks the
 * selected file (aria-selected / active-row state) and records the last
 * action in an offscreen testid element for assertions.
 */
function SidebarHarness() {
  const [currentFile, setCurrentFile] = useState<FileEntry | null>(FAKE_FILES[0]);
  const [lastAction, setLastAction] = useState('none');
  return (
    <EditorChrome>
      <div style={{ width: 280, height: '100%', borderRight: '1px solid var(--sidebar-border)' }}>
        <SidebarTabs>
          {(sectionId) =>
            sectionId === 'files' ? (
              <FileSidebar
                files={FAKE_FILES}
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
      </div>
      {/* Offscreen action recorder for Playwright assertions. */}
      <div
        data-testid="sidebar-last-action"
        style={{ position: 'fixed', left: -10000, top: 0 }}
      >
        {lastAction}
      </div>
    </EditorChrome>
  );
}

/** Editor chrome must render inside .editor-container for the dark-ramp
 *  token overrides (:root.dark .editor-container) to apply, and under a
 *  ViewModeProvider for ViewToggleControl (MinimalHeader). */
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

  'projects-home': () => (
    <ProjectsHome
      onSelectProject={() => {}}
      isConnecting={false}
      error={null}
      projectSetStatus="connected"
      projectSetEntries={FAKE_SET_ENTRIES}
      collections={FAKE_COLLECTIONS}
      onRemoveProjectFromSet={() => {}}
      onTouchProject={() => {}}
      onAddProjectToSet={() => {}}
      onRenameProject={() => {}}
      onUpdateProjectSummary={() => {}}
      onCreateCollection={async () => 'automerge:new-collection'}
      onUnsubscribeCollection={noop}
      onRenameCollection={() => {}}
      onAddProjectToCollection={() => {}}
      onRemoveProjectFromCollection={() => {}}
      onMoveProjectBetweenCollections={() => {}}
    />
  ),
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
  header: () => (
    <EditorChrome>
      <MinimalHeader
        currentFilePath="index.qmd"
        projectName="Research Paper"
        onChooseNewProject={() => {}}
        onShare={() => {}}
        onToggleFullscreenPreview={() => {}}
        isFullscreenPreview={false}
        isOnline={true}
      />
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
