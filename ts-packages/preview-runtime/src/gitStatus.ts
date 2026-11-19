/**
 * The "paused for git" indicator (GE-7, G33).
 *
 * A preview that owns the project's pointer (`q2 preview --git track`) reports through `GET /api/preview/git-status`
 * whether it is holding edits back while a git operation runs. Web clients show a banner while it is paused; there is
 * no read-only mode. The poller is framework-agnostic: a UI reads it with `useSyncExternalStore(poller.subscribe,
 * poller.getSnapshot)`.
 *
 * Polling rules: ask once at start; keep asking every 2 s only while the answer says `tracking` is true. A 404 (an
 * older preview, or one that does not track) or `tracking: false` ends polling for good, because those answers do not
 * change during a session. A network error or another HTTP status is treated as "not known yet": nothing is shown, and
 * the poller tries again on the next tick.
 */

export interface GitStatusSnapshot {
  /** True while the server says it tracks git for this project. */
  readonly tracking: boolean;
  /** True while a git operation holds edits back. Always false when `tracking` is false. */
  readonly paused: boolean;
}

export interface GitStatusPoller {
  start(): void;
  stop(): void;
  getSnapshot(): GitStatusSnapshot;
  subscribe(listener: () => void): () => void;
}

export interface GitStatusPollerOptions {
  /** The route's URL, resolved against the page by the caller. */
  url: string;
  fetch?: typeof fetch;
  intervalMs?: number;
  clock?: { setTimeout: typeof setTimeout; clearTimeout: typeof clearTimeout };
}

const DEFAULT_INTERVAL_MS = 2000;

export function createGitStatusPoller(opts: GitStatusPollerOptions): GitStatusPoller {
  const intervalMs = opts.intervalMs ?? DEFAULT_INTERVAL_MS;
  const clock = opts.clock ?? { setTimeout, clearTimeout };
  const doFetch = opts.fetch ?? ((...args: Parameters<typeof fetch>) => fetch(...args));

  let snapshot: GitStatusSnapshot = { tracking: false, paused: false };
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running = false;
  const listeners = new Set<() => void>();

  function set(next: GitStatusSnapshot): void {
    if (next.tracking === snapshot.tracking && next.paused === snapshot.paused) return;
    snapshot = next;
    for (const l of [...listeners]) l();
  }

  function schedule(): void {
    if (!running) return;
    timer = clock.setTimeout(() => void poll(), intervalMs);
  }

  async function poll(): Promise<void> {
    timer = undefined;
    if (!running) return;
    try {
      const res = await doFetch(opts.url, { headers: { accept: 'application/json' } });
      if (!running) return;
      if (res.status === 404) {
        running = false;
        set({ tracking: false, paused: false });
        return;
      }
      if (!res.ok) {
        set({ tracking: snapshot.tracking, paused: false });
        schedule();
        return;
      }
      const body = (await res.json()) as { tracking?: unknown; paused?: unknown };
      if (!running) return;
      if (body.tracking !== true) {
        running = false;
        set({ tracking: false, paused: false });
        return;
      }
      set({ tracking: true, paused: body.paused === true });
    } catch {
      if (!running) return;
      set({ tracking: snapshot.tracking, paused: false });
    }
    schedule();
  }

  return {
    start() {
      if (running) return;
      running = true;
      void poll();
    },

    stop() {
      running = false;
      if (timer !== undefined) {
        clock.clearTimeout(timer);
        timer = undefined;
      }
    },

    getSnapshot() {
      return snapshot;
    },

    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
