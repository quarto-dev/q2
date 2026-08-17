/**
 * @vitest-environment jsdom
 *
 * Tests for the inspector mount/unmount service (bd-lb1cxprv).
 * Uses the real lazy-loaded panel and a real storage-less Repo; only
 * the preview-runtime accessor is mocked.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { waitFor } from '@testing-library/dom';
import { Repo } from '@automerge/automerge-repo';

const previewRuntimeMocks = vi.hoisted(() => ({
  getRepo: vi.fn<() => unknown>(() => null),
}));

vi.mock('@quarto/preview-runtime', () => previewRuntimeMocks);

// The panel pulls these transitively; give them benign fakes so the
// real chunk can load without the full service graph.
vi.mock('./debugMessageTap', () => ({
  clearTapMessages: vi.fn(),
}));

import { openInspector, closeInspector, isInspectorOpen } from './debugInspector';
import type { QuartoDebugAutomergeApi } from './debugAutomerge';

const fakeAm = {
  docs: () => [],
  doctor: () => [],
  syncStatus: () => ({}),
  presence: () => ({}),
  messages: () => ({ tap: {}, messages: [] }),
} as unknown as QuartoDebugAutomergeApi;

describe('debugInspector', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    closeInspector();
  });

  it('throws when no project is connected', async () => {
    previewRuntimeMocks.getRepo.mockReturnValue(null);
    await expect(openInspector(fakeAm)).rejects.toThrow(/no project connected/);
    expect(isInspectorOpen()).toBe(false);
  });

  it('mounts the panel into a second root and unmounts on close', async () => {
    previewRuntimeMocks.getRepo.mockReturnValue(new Repo({}));

    await openInspector(fakeAm);
    expect(isInspectorOpen()).toBe(true);
    await waitFor(() => {
      expect(document.querySelector('.quarto-debug-inspector')).toBeTruthy();
    });

    closeInspector();
    expect(isInspectorOpen()).toBe(false);
    expect(document.querySelector('.quarto-debug-inspector')).toBeNull();
    expect(
      document.getElementById('quarto-debug-inspector-container'),
    ).toBeNull();
  });

  it('double-open is a no-op (one container)', async () => {
    previewRuntimeMocks.getRepo.mockReturnValue(new Repo({}));
    await openInspector(fakeAm);
    await openInspector(fakeAm);
    expect(
      document.querySelectorAll('#quarto-debug-inspector-container'),
    ).toHaveLength(1);
  });

  it('close is idempotent', () => {
    expect(() => {
      closeInspector();
      closeInspector();
    }).not.toThrow();
  });
});
