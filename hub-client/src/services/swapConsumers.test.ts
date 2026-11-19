/**
 * GE-7 test specification 3 and 5: what the app does when a swap of a file completes.
 *
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  rebindCurrentFile: vi.fn(),
  handleDocSwapped: vi.fn<(path: string, oldDocId: string) => number>(() => 0),
  appendToNextNotice: vi.fn(),
}));

vi.mock('./presenceService', () => ({ rebindCurrentFile: mocks.rebindCurrentFile }));
vi.mock('./branchService', () => ({ handleDocSwapped: mocks.handleDocSwapped }));
vi.mock('@quarto/preview-runtime', () => ({
  swapStatus: { appendToNextNotice: mocks.appendToNextNotice },
}));

import { installSwapConsumers } from './swapConsumers';
import { emitFileReplaced } from './swapEvents';

let uninstall: () => void;

beforeEach(() => {
  vi.clearAllMocks();
  uninstall = installSwapConsumers();
});

afterEach(() => uninstall());

const done = (over: Record<string, unknown> = {}) => ({
  path: 'index.qmd',
  phase: 'done' as const,
  oldDocId: 'old',
  newDocId: 'new',
  ...over,
});

describe('installSwapConsumers', () => {
  it('rebinds presence and resets branches when a swap completes', () => {
    emitFileReplaced(done());
    expect(mocks.rebindCurrentFile).toHaveBeenCalledTimes(1);
    expect(mocks.handleDocSwapped).toHaveBeenCalledWith('index.qmd', 'old');
  });

  it('adds the "branches stay" line to the notice only when the path had branches', () => {
    mocks.handleDocSwapped.mockReturnValueOnce(0);
    emitFileReplaced(done());
    expect(mocks.appendToNextNotice).not.toHaveBeenCalled();

    mocks.handleDocSwapped.mockReturnValueOnce(3);
    emitFileReplaced(done());
    expect(mocks.appendToNextNotice).toHaveBeenCalledWith('3 branches of this file stay with the previous version');
  });

  it('does nothing for pending, unavailable, or a done that replaced nothing', () => {
    emitFileReplaced(done({ phase: 'pending' }));
    emitFileReplaced(done({ phase: 'unavailable' }));
    emitFileReplaced(done({ newDocId: 'old' }));
    expect(mocks.rebindCurrentFile).not.toHaveBeenCalled();
    expect(mocks.handleDocSwapped).not.toHaveBeenCalled();
  });

  it('stops after uninstall', () => {
    uninstall();
    emitFileReplaced(done());
    expect(mocks.rebindCurrentFile).not.toHaveBeenCalled();
    uninstall = installSwapConsumers();
  });
});
