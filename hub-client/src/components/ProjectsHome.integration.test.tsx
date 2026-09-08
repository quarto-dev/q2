/**
 * ProjectsHome collection-menu regressions (bd-fxdcxbpq).
 *
 * "People & invite…" in a collection's ⋯ menu opens the members/invite
 * popover. The activation click bubbles from the MenuItem to the Menu
 * root's closer, whose onClose (closeAllMenus) also resets membersFor —
 * so without keepOpen the popover was cancelled in the same React batch
 * as it was requested, and clicking the item silently did nothing.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react';
import ProjectsHome from './ProjectsHome';
import { ThemeProvider } from './ThemeContext';
import { decodeInvitePreview } from '../utils/invitePreview';
import type { CollectionSnapshot } from '../services/projectSetService';

// ProjectsHome reaches into the sync runtime for the peek refresh; the
// share flow below is the only part these tests exercise.
const connectMock = vi.fn();
vi.mock('@quarto/preview-runtime', () => ({
  getProjectChoices: vi.fn().mockResolvedValue([]),
  createProject: vi.fn(),
  importProjectFromZip: vi.fn(),
  exportProjectAsZip: vi.fn(),
  connect: (...args: unknown[]) => connectMock(...args),
  disconnect: vi.fn().mockResolvedValue(undefined),
  getFileContent: vi.fn(),
  getBinaryFileContent: vi.fn(),
  isFileBinary: vi.fn(),
}));

afterEach(cleanup);

const rootSet: CollectionSnapshot = {
  docId: 'automerge:root1',
  syncServer: 'wss://sync.example.com',
  name: 'Personal',
  entries: [],
  isRoot: true,
};

const teamDocs: CollectionSnapshot = {
  docId: 'automerge:coll1',
  syncServer: 'wss://sync.example.com',
  name: 'Team docs',
  entries: [
    {
      indexDocId: 'automerge:proj1',
      syncServer: 'wss://sync.example.com',
      description: 'Quarterly report',
      addedAt: '2026-01-15T00:00:00.000Z',
      lastAccessed: '2026-01-15T00:00:00.000Z',
    },
  ],
  isRoot: false,
};

describe('ProjectsHome collection menu', () => {
  it('"People & invite…" opens the members/invite popover', async () => {
    render(
      <ThemeProvider>
        <ProjectsHome
          onSelectProject={vi.fn()}
          projectSetStatus="connected"
          projectSetEntries={[]}
          collections={[rootSet, teamDocs]}
        />
      </ThemeProvider>,
    );

    fireEvent.click(await screen.findByRole('button', { name: 'Actions for Team docs' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: /People & invite/ }));

    expect(
      await screen.findByRole('dialog', { name: 'People on Team docs' }),
    ).toBeTruthy();
    expect(screen.getByText('INVITE BY LINK')).toBeTruthy();
  });
});

/**
 * "Share link…" must embed a `preview=` payload so the recipient's invite
 * card shows what they are joining. The summary it is built from is a
 * per-browser cache written while a project is open, so a project you
 * have never opened here — one that arrived through a collection, say —
 * had none, and its links shipped with no preview at all. The share flow
 * now fetches one on demand (bd-fxdcxbpq).
 */
describe('ProjectsHome share link', () => {
  const summarized = {
    indexDocId: 'automerge:proj-sum',
    syncServer: 'wss://sync.example.com',
    description: 'Has a summary',
    addedAt: '2026-01-15T00:00:00.000Z',
    lastAccessed: '2026-01-15T00:00:00.000Z',
    summary: {
      fileCount: 7,
      topFiles: ['index.qmd', 'notes.qmd'],
      contributors: [{ name: 'Carlos Scheidegger', color: '#E91E63' }],
      asOf: '2026-01-15T00:00:00.000Z',
    },
  };

  const unsummarized = {
    indexDocId: 'automerge:proj-bare',
    syncServer: 'wss://sync.example.com',
    description: 'No summary yet',
    addedAt: '2026-01-15T00:00:00.000Z',
    lastAccessed: '2026-01-15T00:00:00.000Z',
  };

  let written: string[];

  beforeEach(() => {
    written = [];
    connectMock.mockReset();
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn(async (t: string) => { written.push(t); }) },
    });
  });

  const renderHome = (entries: typeof summarized[]) =>
    render(
      <ThemeProvider>
        <ProjectsHome
          onSelectProject={vi.fn()}
          projectSetStatus="connected"
          projectSetEntries={entries}
          collections={[{ ...rootSet, entries }]}
          onUpdateProjectSummary={vi.fn()}
        />
      </ThemeProvider>,
    );

  async function shareFrom(name: string) {
    fireEvent.click(await screen.findByRole('button', { name: `Actions for ${name}` }));
    fireEvent.click(await screen.findByRole('menuitem', { name: /Share link/ }));
  }

  it('uses the cached summary without touching the network', async () => {
    renderHome([summarized]);
    await shareFrom('Has a summary');

    await waitFor(() => expect(written).toHaveLength(1));
    expect(connectMock).not.toHaveBeenCalled();
    const preview = decodeInvitePreview(
      new URL(written[0]).hash.match(/preview=([^&]*)/)![1],
    );
    expect(preview?.kind).toBe('project');
    if (preview?.kind === 'project') expect(preview.fileCount).toBe(7);
  });

  it('fetches a summary on demand when the project has none', async () => {
    connectMock.mockResolvedValue([
      { path: 'index.qmd', docId: 'automerge:f1' },
      { path: 'data.csv', docId: 'automerge:f2' },
      { path: '_quarto.yml', docId: 'automerge:f3' },
    ]);
    renderHome([unsummarized]);
    await shareFrom('No summary yet');

    await waitFor(() => expect(written).toHaveLength(1));
    expect(connectMock).toHaveBeenCalledTimes(1);
    const match = new URL(written[0]).hash.match(/preview=([^&]*)/);
    expect(match, 'share link carried no preview payload').not.toBeNull();
    const preview = decodeInvitePreview(match![1]);
    expect(preview?.kind).toBe('project');
    if (preview?.kind === 'project') expect(preview.fileCount).toBe(3);
  });

  /** Copy the invite link for the non-root collection named `name`. */
  async function inviteFrom(name: string) {
    fireEvent.click(await screen.findByRole('button', { name: `Actions for ${name}` }));
    fireEvent.click(await screen.findByRole('menuitem', { name: /People & invite/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Copy link' }));
  }

  const renderWithCollection = (entries: typeof summarized[]) =>
    render(
      <ThemeProvider>
        <ProjectsHome
          onSelectProject={vi.fn()}
          projectSetStatus="connected"
          projectSetEntries={entries}
          collections={[
            { ...rootSet, entries },
            { ...teamDocs, entries },
          ]}
          onUpdateProjectSummary={vi.fn()}
        />
      </ThemeProvider>,
    );

  it('a collection invite always carries a preview payload', async () => {
    // A link with no preview= at all can only come from a build predating
    // this feature — worth pinning, because that is indistinguishable
    // from a bug when you are looking at the card.
    renderWithCollection([summarized]);
    await inviteFrom('Team docs');

    await waitFor(() => expect(written).toHaveLength(1));
    expect(written[0]).toContain('#/join-collection/');
    const match = new URL(written[0]).hash.match(/preview=([^&]*)/);
    expect(match, 'collection invite carried no preview payload').not.toBeNull();
    const preview = decodeInvitePreview(match![1]);
    expect(preview?.kind).toBe('collection');
    if (preview?.kind === 'collection') {
      expect(preview.projects[0].fileCount).toBe(7);
    }
  });

  it('a collection row for a project with no cached summary carries a zero count', async () => {
    renderWithCollection([unsummarized]);
    await inviteFrom('Team docs');

    await waitFor(() => expect(written).toHaveLength(1));
    const preview = decodeInvitePreview(
      new URL(written[0]).hash.match(/preview=([^&]*)/)![1],
    );
    expect(preview?.kind).toBe('collection');
    if (preview?.kind === 'collection') {
      // 0 means "no cached summary", never "this project is empty" —
      // every Quarto Hub project is scaffolded with at least two files.
      // InviteLanding must therefore not render it as "0 files".
      expect(preview.projects[0].fileCount).toBe(0);
      expect(preview.projects[0].name).toBe('No summary yet');
    }
  });
});
