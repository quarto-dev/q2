/**
 * Stale-tab guard for ephemeral `q2 preview` editor sessions (bd-ug0doz5q).
 *
 * A preview server is per-run: when it dies its port never comes back
 * for this page. Left alone, the tab's WebSocket adapters retry the dead
 * port forever, and in Firefox those retries occupy the per-host
 * handshake queue, so the NEXT preview server's handshakes stall behind
 * them (the "second link wedges" bug). This watchdog polls the server's
 * plain-HTTP `/health` (which that queue cannot block); after
 * `strikes` consecutive failures it calls `teardown` so the adapters
 * stop, then keeps polling HTTP only and reloads the page if the server
 * returns.
 */

export interface WatchdogOptions {
  /** Single `/health` probe; must not throw. `true` means healthy. */
  checkHealth: () => Promise<boolean>;
  /** Stop all sync transports (terminal — adapters must not retry). */
  teardown: () => void | Promise<void>;
  /** Called when the server answers again after teardown. */
  onServerBack: () => void;
  /** Poll spacing. Default 2000 ms. */
  intervalMs?: number;
  /** Consecutive failed probes that mean "gone". Default 3. */
  strikes?: number;
  /** Poll spacing once gone. Default 3000 ms. */
  gonePollMs?: number;
}

/** Start the watchdog; returns a stop function. */
export function startEphemeralServerWatchdog(options: WatchdogOptions): () => void {
  const { checkHealth, teardown, onServerBack } = options;
  const intervalMs = options.intervalMs ?? 2000;
  const strikes = options.strikes ?? 3;
  const gonePollMs = options.gonePollMs ?? 3000;
  let stopped = false;
  let failures = 0;
  let gone = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const tick = async (): Promise<void> => {
    if (stopped) return;
    const ok = await checkHealth();
    if (stopped) return;
    if (gone) {
      if (ok) {
        onServerBack();
        return;
      }
    } else if (ok) {
      failures = 0;
    } else if (++failures >= strikes) {
      gone = true;
      await teardown();
    }
    if (!stopped) timer = setTimeout(tick, gone ? gonePollMs : intervalMs);
  };

  timer = setTimeout(tick, intervalMs);
  return () => {
    stopped = true;
    if (timer !== undefined) clearTimeout(timer);
  };
}

/** `/health` probe against the page's own origin. Never throws. */
export async function probeHealth(): Promise<boolean> {
  try {
    const res = await fetch('/health', { cache: 'no-store' });
    return res.ok;
  } catch {
    return false;
  }
}
