/**
 * GE-7 test specification 4 (the SPA's notice component): one aggregate notice per batch, in a `role="status"` region
 * with `aria-live`, with a dismiss action. The wording itself is tested in the git-epochs package.
 */
import { afterEach, describe, expect, it } from 'vitest';
import { act, cleanup, render, screen } from '@testing-library/react';
import { createSwapStatusStore } from '@quarto/preview-runtime';

import { SwapNotice } from './SwapNotice';

afterEach(cleanup);

function manualClock() {
  const timers: Array<() => void> = [];
  return {
    clock: {
      setTimeout: ((fn: () => void) => {
        timers.push(fn);
        return timers.length;
      }) as unknown as typeof setTimeout,
      clearTimeout: (() => {}) as unknown as typeof clearTimeout,
    },
    fire: () => timers.splice(0).forEach((fn) => fn()),
  };
}

describe('SwapNotice', () => {
  it('keeps a polite live region mounted and shows nothing until a batch completes', () => {
    const c = manualClock();
    const store = createSwapStatusStore({ clock: c.clock });
    render(<SwapNotice store={store} />);
    const region = screen.getByRole('status');
    expect(region.getAttribute('aria-live')).toBe('polite');
    expect(screen.queryByTestId('swap-notice')).toBeNull();
  });

  it('announces the aggregate notice once per batch and clears it on dismiss', () => {
    const c = manualClock();
    const store = createSwapStatusStore({ clock: c.clock });
    render(<SwapNotice store={store} />);

    act(() => {
      store.handle({ path: 'a.qmd', phase: 'done', oldDocId: 'x', newDocId: 'y' });
      store.handle({ path: 'b.qmd', phase: 'done', oldDocId: 'x', newDocId: 'y' });
      c.fire();
    });
    const notice = screen.getByTestId('swap-notice');
    expect(notice.textContent).toContain('2 files were replaced');
    expect(screen.getByRole('status').contains(notice)).toBe(true);

    act(() => screen.getByRole('button', { name: /dismiss/i }).click());
    expect(screen.queryByTestId('swap-notice')).toBeNull();
  });
});
