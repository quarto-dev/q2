/**
 * Perf counters — the hub-client / preview-runtime counterpart of the Rust
 * `QUARTO_PERF_STATS` gauges (see
 * claude-notes/instructions/performance-profiling.md).
 *
 * TWO COPIES OF THIS FILE EXIST and must be kept in sync:
 *   - hub-client/src/utils/perfCounters.ts   (this one)
 *   - ts-packages/preview-runtime/src/perfCounters.ts
 * The duplication is deliberate (bd-k8ptcrd9): hub-client app code cannot
 * import the package copy through the package boundary without breaking
 * the repo's many PARTIAL `vi.mock('@quarto/preview-runtime', ...)` test
 * mocks (any export they don't stub goes undefined). Both copies delegate
 * all state to a `globalThis` sink, so there is exactly ONE gauge table
 * per JS realm (hub app window, preview iframe, Node driver) no matter
 * how many bundle copies exist — `perfDump` from either copy prints
 * everything, and gauges recorded through one copy aggregate with the
 * other. When editing one copy, edit the other identically.
 *
 * Enablement is read once at startup, from the first match:
 *
 * 1. `process.env.QUARTO_PERF_STATS === '1'` (Node drivers; same variable
 *    the Rust gauges use),
 * 2. `localStorage['quartoPerfStats'] === '1'`, or
 * 3. a `perf=1` URL query parameter.
 *
 * When disabled, every entry point is a single predictable branch on a
 * cached boolean — effectively free. When enabled, counters and timings
 * accumulate in memory and are dumped to the console (a) every 5 seconds
 * and (b) on demand via `window.quartoPerfDump()`. Output lines are
 * machine-readable: `perf.<gauge> runs=N total_ms=X avg_ms=Y max_ms=Z
 * [key=value ...]`, so a session log can be grepped with `^perf.` and
 * parsed column-wise.
 *
 * Gauges are realm-global by name; counters are never reset
 * automatically (dump is cumulative).
 */

interface Gauge {
  /** Number of recorded events (segments measured or marks counted). */
  runs: number;
  /** Accumulated milliseconds across all recorded segments. */
  totalMs: number;
  /** Largest single segment. */
  maxMs: number;
  /** Free-form cumulative counters (e.g. files=, changed=). */
  extras: Map<string, number>;
}

/** Shared state across every copy of this module loaded in one realm. */
interface PerfSink {
  gauges: Map<string, Gauge>;
  /** Test override: true/false forces recording on/off for all copies. */
  override: boolean | null;
  /** Guards the 5s browser interval so two copies don't double-dump. */
  intervalInstalled: boolean;
}

const SINK_KEY = '__quartoPerfSink';

function sink(): PerfSink {
  const g = globalThis as { __quartoPerfSink?: PerfSink };
  let s = g[SINK_KEY];
  if (!s) {
    s = { gauges: new Map(), override: null, intervalInstalled: false };
    g[SINK_KEY] = s;
  }
  return s;
}

// Cache the sink once: the disabled fast path reads this object's fields
// directly (one property read) instead of re-resolving globalThis per call.
const S = sink();

// `process` is not declared by every consumer's tsconfig; reach it
// through globalThis so Node drivers and browsers both compile and run.
const nodeProcess = (
  globalThis as { process?: { env?: Record<string, string | undefined> } }
).process;

/**
 * True when the flag came from the Node env var rather than a browser
 * source. Node drivers use this to suppress the browser conveniences
 * (interval dump, window handle) that would pollute driver output and
 * keep the process alive.
 */
const enabledByNodeEnv = nodeProcess?.env?.QUARTO_PERF_STATS === '1';

function readEnabledFlag(): boolean {
  // Node driver path (tsx scripts under scripts/perf or throwaway
  // /tmp drivers) — same variable as the Rust gauges.
  if (enabledByNodeEnv) {
    return true;
  }
  try {
    if (globalThis.localStorage?.getItem('quartoPerfStats') === '1') {
      return true;
    }
  } catch {
    // localStorage can throw (privacy modes); fall through to URL param.
  }
  try {
    if (typeof window !== 'undefined') {
      return new URLSearchParams(window.location.search).get('perf') === '1';
    }
  } catch {
    // Malformed location in exotic embeds; treat as disabled.
  }
  return false;
}

let enabled = readEnabledFlag();

/** Whether perf recording is on. One property read on the cached sink. */
export function perfEnabled(): boolean {
  return S.override ?? enabled;
}

/**
 * Start a timing sample. Returns 0 when disabled, so callers can pass
 * the result straight to {@link perfSegment} without branching.
 */
export function perfNow(): number {
  return S.override ?? enabled ? performance.now() : 0;
}

function gauge(name: string): Gauge {
  let g = S.gauges.get(name);
  if (!g) {
    g = { runs: 0, totalMs: 0, maxMs: 0, extras: new Map() };
    S.gauges.set(name, g);
  }
  return g;
}

/**
 * Record one timed segment for `name`: `endMs - startMs` milliseconds.
 * No-op when disabled. Omit `endMs` to measure up to the call.
 */
export function perfSegment(name: string, startMs: number, endMs?: number): void {
  if (!perfEnabled()) return;
  const end = endMs ?? performance.now();
  const ms = end - startMs;
  const g = gauge(name);
  g.runs += 1;
  g.totalMs += ms;
  if (ms > g.maxMs) g.maxMs = ms;
}

/** Increment the run counter of `name` without a duration (e.g. early-outs). */
export function perfMark(name: string): void {
  if (!perfEnabled()) return;
  gauge(name).runs += 1;
}

/**
 * Add `n` to a named cumulative extra on the gauge (e.g.
 * `perfExtra('effect.project-search', 'changed', changed)`).
 */
export function perfExtra(name: string, key: string, n = 1): void {
  if (!perfEnabled()) return;
  const g = gauge(name);
  g.extras.set(key, (g.extras.get(key) ?? 0) + n);
}

/** Format one gauge line (also the unit-tested shape). */
function formatGauge(name: string, g: Gauge): string {
  const parts = [`perf.${name}`, `runs=${g.runs}`];
  if (g.totalMs > 0) {
    parts.push(`total_ms=${g.totalMs.toFixed(3)}`);
    parts.push(`avg_ms=${(g.totalMs / g.runs).toFixed(3)}`);
    parts.push(`max_ms=${g.maxMs.toFixed(3)}`);
  }
  for (const [k, v] of [...g.extras.entries()].sort()) {
    parts.push(`${k}=${v}`);
  }
  return parts.join(' ');
}

/** Log all recorded gauges, sorted by name. Cumulative; does not reset. */
export function perfDump(): void {
  for (const [name, g] of [...S.gauges.entries()].sort()) {
    console.log(formatGauge(name, g));
  }
}

/** Clear all gauges. Exposed for drivers and tests. */
export function perfReset(): void {
  S.gauges.clear();
}

/**
 * Test-only override: force recording on (`true`), off (`false`), or
 * back to the startup flag (`null`) — for EVERY copy of this module in
 * the realm (the override lives on the shared sink).
 */
export function setPerfOverrideForTests(value: boolean | null): void {
  S.override = value;
  if (value === null) {
    enabled = readEnabledFlag();
  }
}

// Browser conveniences: periodic dump + manual dump handle, installed at
// most once per realm (guarded via the sink). Skipped in Node drivers
// (they call perfDump themselves at checkpoints).
{
  if (perfEnabled() && !enabledByNodeEnv && !S.intervalInstalled && typeof window !== 'undefined') {
    S.intervalInstalled = true;
    window.setInterval(perfDump, 5000);
    (window as unknown as { quartoPerfDump: () => void }).quartoPerfDump = perfDump;
  }
}
