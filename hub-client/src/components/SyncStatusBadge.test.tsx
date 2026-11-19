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
  fileDocId: 'doc-file',
  queriedDocIds: [] as string[],
}));

vi.mock('@quarto/preview-runtime', () => ({
  getConnectionInfo: () => ({
    wsReadyState: mocks.wsReadyState,
    wsUrl: null,
    peers: mocks.peers,
  }),
  getIndexHandle: () => ({ documentId: 'doc-index' }),
  getFileHandle: () => ({ documentId: mocks.fileDocId }),
}));

vi.mock('@quarto/quarto-sync-client', () => ({
  getDocSyncActivity: (docId: string) => {
    mocks.queriedDocIds.push(docId);
    return {
      lastSyncMessageAt: null,
      lastEphemeralMessageAt: null,
      lastRemoteChangeAt: mocks.lastRemoteChangeAt,
      lastLocalChangeAt: null,
      lastLocalDeliveredAt: null,
    };
  },
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
    // Verbose offline renders the label and the rest as separate
    // elements; the joined sentence is the accessible name.
    expect(screen.getByText('Offline')).toBeDefined();
    expect(screen.getByText('not synced')).toBeDefined();
    expect(screen.getByRole('button').getAttribute('aria-label')).toMatch(
      /^Offline — not synced/,
    );
  });

  it('compact offline shows just the time ago with the offline state in the accessible name', () => {
    mocks.wsReadyState = 3; // CLOSED
    mocks.lastRemoteChangeAt = null;
    render(<SyncStatusBadge scope="project" />);
    expect(screen.getByText('not synced')).toBeDefined();
    expect(screen.queryByText(/Offline/)).toBeNull();
    expect(screen.getByRole('button').getAttribute('aria-label')).toMatch(/^Offline/);
  });

  it('watches the new doc after the project owner replaces the file (GE-7)', () => {
    mocks.wsReadyState = 1;
    mocks.peers = [{ peerId: 'peer-1' }];
    mocks.fileDocId = 'doc-A';
    mocks.queriedDocIds = [];
    const { rerender } = render(<SyncStatusBadge scope="file" currentFilePath="index.qmd" />);
    expect(mocks.queriedDocIds).toContain('doc-A');

    mocks.fileDocId = 'doc-B';
    mocks.queriedDocIds = [];
    rerender(<SyncStatusBadge scope="file" currentFilePath="index.qmd" verbose />);
    expect(mocks.queriedDocIds).toContain('doc-B');
    expect(mocks.queriedDocIds).not.toContain('doc-A');
    mocks.fileDocId = 'doc-file';
  });
});
