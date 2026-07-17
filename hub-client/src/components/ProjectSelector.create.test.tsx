/**
 * Tests for the "Create New Project" form's Sync Server URL field.
 *
 * The field defaults to DEFAULT_SYNC_SERVER (matching the Connect form) and
 * is editable — restored after a brief removal during the local-first epic
 * (bd-u4p8xhdc); see claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md.
 * Clearing the field still creates a local-only project (empty syncServer).
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

function renderSelector(onProjectCreated = vi.fn()) {
  render(
    <ProjectSelector
      onSelectProject={vi.fn()}
      onProjectCreated={onProjectCreated}
      projectSetEntries={[]}
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

  it('shows a Sync Server URL field defaulting to DEFAULT_SYNC_SERVER', async () => {
    renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /create new project/i }));

    const input = (await screen.findByLabelText(/sync server url/i)) as HTMLInputElement;
    expect(input.value).toBe('wss://sync.automerge.org');
  });

  it('is editable and the edited value flows to onProjectCreated', async () => {
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

  it('clearing the field still creates a local-only project (empty syncServer)', async () => {
    const { onProjectCreated } = renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /create new project/i }));

    await screen.findByLabelText(/project type/i);
    fireEvent.change(screen.getByLabelText(/sync server url/i), { target: { value: '' } });
    fireEvent.change(screen.getByLabelText('Project Title'), {
      target: { value: 'Local Project' },
    });
    fireEvent.click(screen.getByRole('button', { name: /^create project$/i }));

    await waitFor(() => expect(onProjectCreated).toHaveBeenCalledTimes(1));
    const [, , , syncServer] = onProjectCreated.mock.calls[0];
    expect(syncServer).toBe('');
  });
});
