/**
 * Characterization tests for ConnectionStatusDialog, pinned before the
 * useNow/useOnlineStatus conversion (bd-06s9sufx): the dialog refreshes
 * its relative-time cells once per second while open, and reflects the
 * browser's online/offline state.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, screen, act, cleanup } from '@testing-library/react';
import ConnectionStatusDialog from './ConnectionStatusDialog';

const mocks = vi.hoisted(() => ({
  lastRemoteChangeAt: null as number | null,
}));

vi.mock('@quarto/preview-runtime', () => ({
  getConnectionInfo: () => ({
    wsReadyState: 1,
    wsUrl: null,
    peers: [{ peerId: 'peer-1' }],
  }),
  getIndexHandle: () => ({ documentId: 'doc-index' }),
  getFileHandle: () => ({ documentId: 'doc-file' }),
}));

vi.mock('@quarto/quarto-sync-client', () => ({
  getDocSyncActivityWithPersisted: () => ({
    lastSyncMessageAt: null,
    lastEphemeralMessageAt: null,
    lastRemoteChangeAt: mocks.lastRemoteChangeAt,
    lastLocalChangeAt: null,
    lastLocalDeliveredAt: null,
  }),
  getDocRemoteChange: () => null,
  getConnectionLog: () => [],
}));

describe('ConnectionStatusDialog', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    localStorage.clear();
  });

  it('refreshes relative-time cells once per second while open', () => {
    mocks.lastRemoteChangeAt = Date.now();
    render(<ConnectionStatusDialog currentFilePath="report.qmd" onClose={() => {}} />);

    // Both columns (project + file) show the just-landed change.
    const initial = screen.getAllByText('0s ago');
    expect(initial.length).toBeGreaterThan(0);

    act(() => {
      vi.advanceTimersByTime(65_000);
    });

    expect(screen.getAllByText('1m ago').length).toBeGreaterThan(0);
    expect(screen.queryAllByText('0s ago')).toHaveLength(0);
  });

  it('shows the browser network state, refreshed on offline/online events', () => {
    const onLine = vi.spyOn(navigator, 'onLine', 'get').mockReturnValue(true);
    render(<ConnectionStatusDialog currentFilePath="report.qmd" onClose={() => {}} />);
    expect(screen.getByText('Online')).toBeDefined();

    onLine.mockReturnValue(false);
    act(() => {
      window.dispatchEvent(new Event('offline'));
    });
    expect(screen.getByText('Offline')).toBeDefined();

    onLine.mockReturnValue(true);
    act(() => {
      window.dispatchEvent(new Event('online'));
    });
    expect(screen.getByText('Online')).toBeDefined();
  });
});
