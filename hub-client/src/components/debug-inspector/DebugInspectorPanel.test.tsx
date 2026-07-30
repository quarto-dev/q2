/**
 * @vitest-environment jsdom
 *
 * Tests for the in-context live inspector panel (bd-lb1cxprv; plan:
 * claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md).
 *
 * Mounts the panel against a real storage-less Repo (the same way
 * openInspector mounts it against the live sync-client Repo) and a
 * fabricated `am` API, then exercises tab switching and close paths.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, cleanup, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Repo } from '@automerge/automerge-repo';

import { DebugInspectorPanel } from './DebugInspectorPanel';
import type { QuartoDebugAutomergeApi } from '../../services/debugAutomerge';

function makeWorld() {
  const repo = new Repo({});
  const indexHandle = repo.create({ files: { 'index.qmd': 'f1' } });
  const am = {
    docs: vi.fn(() => [
      {
        docId: indexHandle.documentId as string,
        role: 'index' as const,
        path: null,
        handleState: 'ready',
        heads: ['h'],
        unavailableMarker: false,
      },
    ]),
    doctor: vi.fn(() => [
      { kind: 'stranded-file', path: 'lost.qmd', detail: 'never loaded' },
    ]),
    syncStatus: vi.fn(() => ({ connected: true })),
    presence: vi.fn(() => ({ peerId: 'p-1' })),
    messages: vi.fn(() => ({
      tap: { installed: true },
      messages: [
        {
          at: 1000,
          direction: 'outgoing' as const,
          type: 'sync',
          senderId: 'peer-a',
          targetId: 'peer-b',
          documentId: 'doc-1',
          byteLength: 42,
        },
      ],
    })),
  } as unknown as QuartoDebugAutomergeApi;
  return { repo, indexHandle, am };
}

describe('DebugInspectorPanel', () => {
  let onClose: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    onClose = vi.fn();
  });

  afterEach(() => {
    cleanup();
  });

  function mount() {
    const world = makeWorld();
    render(
      <DebugInspectorPanel repo={world.repo} am={world.am} onClose={onClose} />,
    );
    return world;
  }

  it('renders the header and seeds the index doc into the viewer', async () => {
    mount();
    expect(
      screen.getByRole('heading', { name: /Live Inspector/ }),
    ).toBeTruthy();
    // The seeded index doc renders as JSON in the DocumentViewer, and
    // its files map gets the per-file subscribe UI.
    await waitFor(() => {
      expect(screen.getByText(/"index\.qmd": "f1"/)).toBeTruthy();
    });
    expect(screen.getByText('Files in this index')).toBeTruthy();
  });

  it('switches to the Doctor tab and renders the report', async () => {
    const { am } = mount();
    await userEvent.click(screen.getByRole('tab', { name: 'Doctor' }));
    expect(am.doctor).toHaveBeenCalled();
    expect(screen.getByText(/stranded-file/)).toBeTruthy();
    expect(screen.getByText(/never loaded/)).toBeTruthy();
  });

  it('switches to the Messages tab and shows tap traffic', async () => {
    const { am } = mount();
    await userEvent.click(screen.getByRole('tab', { name: 'Messages' }));
    expect(am.messages).toHaveBeenCalled();
    // 'sync' appears both as a message row and in the type filter.
    await waitFor(() => {
      expect(screen.getAllByText('sync').length).toBeGreaterThan(0);
    });
    expect(screen.getByText('42B')).toBeTruthy();
  });

  it('closes via the close button', async () => {
    mount();
    await userEvent.click(
      screen.getByRole('button', { name: 'Close inspector' }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('closes on Escape', async () => {
    mount();
    await userEvent.keyboard('{Escape}');
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
