/**
 * Tests for the SyncStatusBadge connected/disconnected states.
 * (Fix-forward of the old DocumentTopBar connection-indicator test —
 * the online/offline surface moved into this badge.)
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach, vi } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import SyncStatusBadge from './SyncStatusBadge';

const mocks = vi.hoisted(() => ({
  wsReadyState: 1 as number | null,
  peers: [{ peerId: 'peer-1' }] as Array<{ peerId: string }>,
  lastRemoteChangeAt: null as number | null,
}));

vi.mock('@quarto/preview-runtime', () => ({
  getConnectionInfo: () => ({
    wsReadyState: mocks.wsReadyState,
    wsUrl: null,
    peers: mocks.peers,
  }),
  getIndexHandle: () => ({ documentId: 'doc-index' }),
  getFileHandle: () => ({ documentId: 'doc-file' }),
}));

vi.mock('@quarto/quarto-sync-client', () => ({
  getDocSyncActivity: () => ({
    lastSyncMessageAt: null,
    lastEphemeralMessageAt: null,
    lastRemoteChangeAt: mocks.lastRemoteChangeAt,
    lastLocalChangeAt: null,
    lastLocalDeliveredAt: null,
  }),
}));

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe('SyncStatusBadge', () => {
  it('shows Synced just now when connected with recent activity', () => {
    mocks.wsReadyState = 1;
    mocks.peers = [{ peerId: 'peer-1' }];
    mocks.lastRemoteChangeAt = Date.now();
    render(<SyncStatusBadge scope="project" verbose />);
    const dot = document.querySelector('.sync-status-dot')!;
    expect(dot.className).toContain('green');
    expect(screen.getByText(/Synced just now/)).toBeDefined();
  });

  it('compact mode shows only the time, with the full status as accessible name', () => {
    mocks.wsReadyState = 1;
    mocks.peers = [{ peerId: 'peer-1' }];
    mocks.lastRemoteChangeAt = Date.now();
    render(<SyncStatusBadge scope="project" />);
    expect(screen.getByText('just now')).toBeDefined();
    expect(screen.queryByText(/Synced/)).toBeNull();
    expect(screen.getByRole('button').getAttribute('aria-label')).toMatch(/^Synced just now/);
  });

  it('shows Offline when the websocket is down', () => {
    mocks.wsReadyState = 3; // CLOSED
    mocks.lastRemoteChangeAt = null;
    render(<SyncStatusBadge scope="project" verbose />);
    const dot = document.querySelector('.sync-status-dot')!;
    expect(dot.className).toContain('yellow');
    expect(screen.getByText(/Offline — not synced yet/)).toBeDefined();
  });

  it('compact offline shows just the time ago with the offline state in the accessible name', () => {
    mocks.wsReadyState = 3; // CLOSED
    mocks.lastRemoteChangeAt = null;
    render(<SyncStatusBadge scope="project" />);
    expect(screen.getByText('not synced yet')).toBeDefined();
    expect(screen.queryByText(/Offline/)).toBeNull();
    expect(screen.getByRole('button').getAttribute('aria-label')).toMatch(/^Offline/);
  });
});
