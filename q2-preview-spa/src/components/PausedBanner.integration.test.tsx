/**
 * GE-7 test specification 6 (the SPA's banner component): shown while the poller says paused, hidden otherwise, and
 * the poller runs only while the banner is mounted.
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen } from '@testing-library/react';
import { PAUSED_BANNER } from '@quarto/quarto-git-epochs';
import type { GitStatusPoller, GitStatusSnapshot } from '@quarto/preview-runtime';

import { PausedBanner } from './PausedBanner';

afterEach(cleanup);

function fakePoller(initial: GitStatusSnapshot) {
  let snapshot = initial;
  const listeners = new Set<() => void>();
  const poller: GitStatusPoller = {
    start: vi.fn(),
    stop: vi.fn(),
    getSnapshot: () => snapshot,
    subscribe: (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
  };
  return {
    poller,
    set(next: GitStatusSnapshot) {
      snapshot = next;
      listeners.forEach((l) => l());
    },
  };
}

describe('PausedBanner', () => {
  it('shows the banner text in a status region while paused and hides it when not', () => {
    const f = fakePoller({ tracking: true, paused: true });
    render(<PausedBanner poller={f.poller} />);
    const banner = screen.getByTestId('paused-banner');
    expect(banner.textContent).toBe(PAUSED_BANNER);
    expect(banner.getAttribute('role')).toBe('status');

    act(() => f.set({ tracking: true, paused: false }));
    expect(screen.queryByTestId('paused-banner')).toBeNull();
  });

  it('renders nothing when the server does not track git', () => {
    const f = fakePoller({ tracking: false, paused: false });
    render(<PausedBanner poller={f.poller} />);
    expect(screen.queryByTestId('paused-banner')).toBeNull();
  });

  it('starts polling on mount and stops on unmount', () => {
    const f = fakePoller({ tracking: false, paused: false });
    const { unmount } = render(<PausedBanner poller={f.poller} />);
    expect(f.poller.start).toHaveBeenCalledTimes(1);
    unmount();
    expect(f.poller.stop).toHaveBeenCalledTimes(1);
  });
});
