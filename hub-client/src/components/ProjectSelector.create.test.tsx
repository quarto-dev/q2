/**
 * Tests for the "Create New Project" form's Sync Server URL field.
 *
 * The field is editable, but its *default* value must track whether the app
 * is currently connected to a hub: empty (local) when disconnected, the
 * connected hub's server when connected. Defaulting unconditionally to
 * DEFAULT_SYNC_SERVER (a prior version of this field) silently turned local
 * creation into a hub creation attempt with no session — createNewProject's
 * resolveActorId callback swallows the resulting 401 (client.ts:1568-1571)
 * instead of aborting, so the project was created anyway, wired to a real
 * WS adapter, and then immediately torn down by App.tsx's auth-loss-teardown
 * effect (a "flash" of the editor before bouncing back to the selector).
 * See claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react';

const { createProjectMock } = vi.hoisted(() => ({ createProjectMock: vi.fn() }));

vi.mock('@quarto/preview-runtime', () => ({
  getProjectChoices: vi.fn().mockResolvedValue([
    { id: 'website', name: 'Website', description: 'A basic Quarto website' },
  ]),
  createProject: createProjectMock,
  importProjectFromZip: vi.fn(),
}));

vi.mock('../services/projectStorage', () => ({
  listProjects: vi.fn().mockResolvedValue([]),
}));

vi.mock('../services/userSettings', () => ({
  getUserIdentity: vi.fn().mockResolvedValue(null),
  updateUserName: vi.fn(),
  updateUserColor: vi.fn(),
  resetUserIdentity: vi.fn(),
}));

vi.mock('./ThemeContext', () => ({
  useTheme: () => ({ colorScheme: 'auto', cycleColorScheme: vi.fn() }),
}));

import ProjectSelector from './ProjectSelector';

function renderSelector(
  onProjectCreated = vi.fn(),
  props: Partial<React.ComponentProps<typeof ProjectSelector>> = {},
) {
  render(
    <ProjectSelector
      onSelectProject={vi.fn()}
      onProjectCreated={onProjectCreated}
      projectSetEntries={[]}
      {...props}
    />,
  );
  return { onProjectCreated };
}

afterEach(cleanup);

describe('ProjectSelector — Create New Project sync server field', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    createProjectMock.mockResolvedValue({
      success: true,
      files: [{ path: 'index.qmd', content_type: 'text', content: '# Hi' }],
    });
  });

  it('defaults to empty (local) when not connected to a hub', async () => {
    renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /create new project/i }));

    const input = (await screen.findByLabelText(/sync server url/i)) as HTMLInputElement;
    expect(input.value).toBe('');
  });

  it('defaults to the connected hub server when a project set is connected', async () => {
    renderSelector(vi.fn(), { projectSetSyncServer: 'wss://hub.example.com/ws' });
    fireEvent.click(await screen.findByRole('button', { name: /create new project/i }));

    const input = (await screen.findByLabelText(/sync server url/i)) as HTMLInputElement;
    expect(input.value).toBe('wss://hub.example.com/ws');
  });

  it('leaving the field empty (not connected to a hub) creates a local-only project', async () => {
    const { onProjectCreated } = renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /create new project/i }));

    await screen.findByLabelText(/project type/i);
    fireEvent.change(screen.getByLabelText('Project Title'), {
      target: { value: 'Local Project' },
    });
    fireEvent.click(screen.getByRole('button', { name: /^create project$/i }));

    await waitFor(() => expect(onProjectCreated).toHaveBeenCalledTimes(1));
    const [, , , syncServer] = onProjectCreated.mock.calls[0];
    expect(syncServer).toBe('');
  });

  it('is editable — a typed value overrides the default and flows to onProjectCreated', async () => {
    const { onProjectCreated } = renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /create new project/i }));

    await screen.findByLabelText(/project type/i);
    fireEvent.change(screen.getByLabelText(/sync server url/i), {
      target: { value: 'wss://my-hub.example.com/ws' },
    });
    fireEvent.change(screen.getByLabelText('Project Title'), {
      target: { value: 'My Project' },
    });
    fireEvent.click(screen.getByRole('button', { name: /^create project$/i }));

    await waitFor(() => expect(onProjectCreated).toHaveBeenCalledTimes(1));
    const [, , , syncServer] = onProjectCreated.mock.calls[0];
    expect(syncServer).toBe('wss://my-hub.example.com/ws');
  });
});
