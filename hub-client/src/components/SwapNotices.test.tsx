/**
 * GE-7 test specification 4 for the hub-client editor: the replaced-file notice is one aggregate per batch in a
 * `role="status"` region with `aria-live`, dismissible; the "this person switched versions" toast is brief.
 *
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { createSwapStatusStore } from '@quarto/preview-runtime';
import type { PeerMovedEvent } from '../services/presenceService';

import SwapNotices from './SwapNotices';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function setup() {
  const store = createSwapStatusStore({ quietMs: 10 });
  const movedListeners = new Set<(e: PeerMovedEvent) => void>();
  const onPeerMoved = (cb: (e: PeerMovedEvent) => void) => {
    movedListeners.add(cb);
    return () => {
      movedListeners.delete(cb);
    };
  };
  render(<SwapNotices store={store} onPeerMoved={onPeerMoved} />);
  return { store, moved: (e: PeerMovedEvent) => movedListeners.forEach((l) => l(e)) };
}

describe('SwapNotices', () => {
  it('keeps a polite live region mounted and shows nothing until a batch completes', () => {
    setup();
    expect(screen.getByTestId('swap-notice-region').getAttribute('aria-live')).toBe('polite');
    expect(screen.queryByTestId('swap-notice')).toBeNull();
  });

  it('shows one aggregate notice for a batch, in a status role, and dismisses it', () => {
    const { store } = setup();
    act(() => {
      store.handle({ path: 'a.qmd', phase: 'done', oldDocId: 'x', newDocId: 'y' });
      store.handle({ path: 'b.qmd', phase: 'done', oldDocId: 'x', newDocId: 'y' });
      vi.advanceTimersByTime(10);
    });
    const notice = screen.getByTestId('swap-notice');
    expect(notice.getAttribute('role')).toBe('status');
    expect(notice.textContent).toContain('2 files were replaced');

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByTestId('swap-notice')).toBeNull();
  });

  it('adds the stranded-edits line only when the count is nonzero', () => {
    const { store } = setup();
    act(() => {
      store.handle({ path: 'a.qmd', phase: 'done', oldDocId: 'x', newDocId: 'y', stranded: 0 });
      vi.advanceTimersByTime(10);
    });
    expect(screen.getByTestId('swap-notice').textContent).not.toContain('stayed with the previous version');

    act(() => {
      store.handle({ path: 'a.qmd', phase: 'done', oldDocId: 'y', newDocId: 'z', stranded: 2 });
      vi.advanceTimersByTime(10);
    });
    expect(screen.getByTestId('swap-notice').textContent).toContain(
      '2 of your recent edits stayed with the previous version.',
    );
  });

  it('shows the moved toast briefly and then removes it', () => {
    const { moved } = setup();
    act(() => moved({ peerId: 'p1', text: 'Ada switched versions' }));
    expect(screen.getByText('Ada switched versions').closest('[role="status"]')).not.toBeNull();

    act(() => {
      vi.advanceTimersByTime(4000);
    });
    expect(screen.queryByText('Ada switched versions')).toBeNull();
  });

  it('stops listening for moved events on unmount', () => {
    const store = createSwapStatusStore();
    const off = vi.fn();
    const { unmount } = render(<SwapNotices store={store} onPeerMoved={() => off} />);
    unmount();
    expect(off).toHaveBeenCalledTimes(1);
  });
});
