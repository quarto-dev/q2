/**
 * The GE-7 additions to the `@quarto/preview-runtime` module mock: the swap status store, the doc-id lookup behind
 * `baseDocId`, and the git-status poller. Tests mock the runtime with a strict full-object factory, so every new export
 * `PreviewApp` imports has to appear in each of those factories; they spread `swapRuntimeMock()` for that.
 *
 * `swapController` lets a test drive the store: set the pending paths or the notice, and the component re-renders.
 */
import { vi } from 'vitest';

interface Snapshot {
  pending: readonly string[];
  notice: string | null;
  noticeSeq: number;
}

let snapshot: Snapshot = { pending: [], notice: null, noticeSeq: 0 };
const listeners = new Set<() => void>();

export const swapController = {
  set(next: Partial<Snapshot>): void {
    snapshot = { ...snapshot, ...next };
    listeners.forEach((l) => l());
  },
  reset(): void {
    snapshot = { pending: [], notice: null, noticeSeq: 0 };
  },
  /** The doc id `getFileHandle` reports for every path. */
  docId: 'doc-current',
};

const IDLE_GIT_STATUS = { tracking: false, paused: false };

export function swapRuntimeMock() {
  return {
    swapStatus: {
      subscribe: (l: () => void) => {
        listeners.add(l);
        return () => listeners.delete(l);
      },
      getSnapshot: () => snapshot,
      handle: vi.fn(),
      setOpenFile: vi.fn(),
      appendToNextNotice: vi.fn(),
      dismissNotice: vi.fn(() => swapController.set({ notice: null })),
      reset: vi.fn(),
    },
    getFileHandle: vi.fn(() => ({ documentId: swapController.docId })),
    createGitStatusPoller: vi.fn(() => ({
      start: vi.fn(),
      stop: vi.fn(),
      // Stable identity: useSyncExternalStore treats a new object per call as a change and loops.
      getSnapshot: () => IDLE_GIT_STATUS,
      subscribe: () => () => {},
    })),
  };
}
