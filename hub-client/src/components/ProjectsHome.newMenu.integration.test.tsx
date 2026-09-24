/**
 * @vitest-environment jsdom
 *
 * The "＋ New" menu renders the project-choice registry as a tree
 * (bd-q33ylfxf): each choice's `path` becomes a submenu, so the skeleton
 * templates and the populated examples sit in separate groups and two
 * choices may share a display name without colliding.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react';
import ProjectsHome from './ProjectsHome';
import { ThemeProvider } from './ThemeContext';
import type { CollectionSnapshot } from '../services/projectSetService';

const choices = [
  { id: 'default', name: 'Default', description: 'A minimal Quarto project', path: ['Templates'] },
  { id: 'website', name: 'Website', description: 'A Quarto website with navigation', path: ['Templates'] },
  { id: 'presentation', name: 'Presentation', description: 'A reveal.js deck', path: ['Templates'] },
  { id: 'example-website', name: 'Website', description: 'A few pages and a tour', path: ['Examples'], seed: true },
  { id: 'example-article', name: 'Article', description: 'A short article', path: ['Examples'], seed: true },
];

vi.mock('@quarto/preview-runtime', () => ({
  getProjectChoices: vi.fn(async () => choices),
  createProject: vi.fn(),
  importProjectFromZip: vi.fn(),
  exportProjectAsZip: vi.fn(),
  connect: vi.fn(),
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

function renderHome() {
  return render(
    <ThemeProvider>
      <ProjectsHome
        onSelectProject={vi.fn()}
        projectSetStatus="connected"
        projectSetEntries={[]}
        collections={[rootSet]}
      />
    </ThemeProvider>,
  );
}

describe('ProjectsHome New menu tree', () => {
  it('shows one submenu per path group instead of a flat list', async () => {
    renderHome();
    fireEvent.click(await screen.findByRole('button', { name: '＋ New ▾' }));
    const menu = await screen.findByRole('menu', { name: 'New project' });

    expect(within(menu).getByRole('menuitem', { name: /Templates/ })).toBeTruthy();
    expect(within(menu).getByRole('menuitem', { name: /Examples/ })).toBeTruthy();
    // Leaves are not visible until their group opens.
    expect(within(menu).queryByRole('menuitem', { name: /Default/ })).toBeNull();
    expect(within(menu).queryByRole('menuitem', { name: /Article/ })).toBeNull();
  });

  it('opens a group to reveal its choices in registry order', async () => {
    renderHome();
    fireEvent.click(await screen.findByRole('button', { name: '＋ New ▾' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: /Templates/ }));

    const submenu = await screen.findByRole('menu', { name: 'Templates' });
    const labels = within(submenu)
      .getAllByRole('menuitem')
      .map((el) => el.querySelector('.qh-menu-item-label')?.textContent);
    expect(labels).toEqual(['Default', 'Website', 'Presentation']);
  });

  it('keeps the two "Website" choices apart, one per group', async () => {
    renderHome();
    fireEvent.click(await screen.findByRole('button', { name: '＋ New ▾' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: /Examples/ }));

    const examples = await screen.findByRole('menu', { name: 'Examples' });
    const websiteInExamples = within(examples).getByRole('menuitem', { name: /Website/ });
    expect(websiteInExamples.textContent).toContain('A few pages and a tour');
    expect(within(examples).queryByRole('menuitem', { name: /Default/ })).toBeNull();
  });
});
