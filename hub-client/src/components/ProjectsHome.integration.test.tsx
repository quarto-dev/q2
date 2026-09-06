/**
 * ProjectsHome collection-menu regressions (bd-fxdcxbpq).
 *
 * "People & invite…" in a collection's ⋯ menu opens the members/invite
 * popover. The activation click bubbles from the MenuItem to the Menu
 * root's closer, whose onClose (closeAllMenus) also resets membersFor —
 * so without keepOpen the popover was cancelled in the same React batch
 * as it was requested, and clicking the item silently did nothing.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import ProjectsHome from './ProjectsHome';
import { ThemeProvider } from './ThemeContext';
import type { CollectionSnapshot } from '../services/projectSetService';

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
