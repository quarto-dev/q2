/**
 * GE-7 test specification 6 for the hub-client editor: the "paused for git" banner (G33) shows while the poller says
 * paused, hides otherwise, and the poller runs only while the banner is mounted.
 *
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen } from '@testing-library/react';
import { PAUSED_BANNER } from '@quarto/quarto-git-epochs';
import type { GitStatusPoller, GitStatusSnapshot } from '@quarto/preview-runtime';

import PausedForGitBanner from './PausedForGitBanner';

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

describe('PausedForGitBanner', () => {
  it('shows the shared banner text in a status role while paused, and hides it when not', () => {
    const f = fakePoller({ tracking: true, paused: true });
    render(<PausedForGitBanner poller={f.poller} />);
    const banner = screen.getByRole('status');
    expect(banner.textContent).toBe(PAUSED_BANNER);

    act(() => f.set({ tracking: true, paused: false }));
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('renders nothing when the server does not track git (including a 404)', () => {
    const f = fakePoller({ tracking: false, paused: false });
    render(<PausedForGitBanner poller={f.poller} />);
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('starts polling on mount and stops on unmount', () => {
    const f = fakePoller({ tracking: false, paused: false });
    const { unmount } = render(<PausedForGitBanner poller={f.poller} />);
    expect(f.poller.start).toHaveBeenCalledTimes(1);
    unmount();
    expect(f.poller.stop).toHaveBeenCalledTimes(1);
  });
});
