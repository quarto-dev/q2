/**
 * Header hub-connection control (A4, bd-u4p8xhdc).
 *
 * The account-level control shows "Connect to a hub" when disconnected
 * (triggering sign-in) and the signed-in identity + Sign out when connected.
 * It is deliberately separate from the per-project "Connect to Project"
 * (join-by-doc-id) action. The Create/Import forms' Sync Server URL field is
 * covered in ProjectSelector.create.test.tsx / ProjectSelector.import.test.tsx.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';

vi.mock('@quarto/preview-runtime', () => ({
  getProjectChoices: vi.fn().mockResolvedValue([]),
  createProject: vi.fn(),
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

const baseProps = {
  onSelectProject: vi.fn(),
  onProjectCreated: vi.fn(),
  projectSetStatus: 'connected' as const,
  projectSetEntries: [],
};

describe('ProjectSelector hub-connection header control', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });
  afterEach(() => {
    cleanup();
  });

  it('shows "Connect to a hub" when disconnected and calls onConnectToHub', () => {
    const onConnectToHub = vi.fn();
    render(
      <ProjectSelector
        {...baseProps}
        isHubConnected={false}
        onConnectToHub={onConnectToHub}
      />,
    );
    const btn = screen.getByRole('button', { name: /connect to a hub/i });
    fireEvent.click(btn);
    expect(onConnectToHub).toHaveBeenCalledTimes(1);
    // No Sign out affordance while disconnected.
    expect(screen.queryByText(/sign out/i)).toBeNull();
  });

  it('shows the signed-in identity + Sign out when connected', () => {
    const onSignOut = vi.fn();
    render(
      <ProjectSelector
        {...baseProps}
        isHubConnected={true}
        authEmail="user@example.com"
        onSignOut={onSignOut}
      />,
    );
    const btn = screen.getByRole('button', { name: /signed in as user@example\.com · sign out/i });
    fireEvent.click(btn);
    expect(onSignOut).toHaveBeenCalledTimes(1);
    // The disconnected affordance is gone.
    expect(screen.queryByRole('button', { name: /^connect to a hub$/i })).toBeNull();
  });
});
