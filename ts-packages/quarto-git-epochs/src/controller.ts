/**
 * The gate and quiescence controller (G1, G3, G32). `check()` is the synchronous entry every sync path calls
 * first; `tick()` is the async driver that resolves HEAD and emits a decision once the repository has been
 * quiet. All time comes from the injected clock; the only I/O is through the probe.
 * See the contract, "Module map" and "Numbers".
 */
import { NUMBERS } from './vocabulary.js';
import {
  type CheapState,
  type ControllerProbe,
  type ReflogMark,
  GitUnavailableError,
} from './probe-types.js';

export interface Clock {
  now(): number;
}

export interface ControllerOptions {
  /** Paths whose disk content differs from their last synced content. Read once, when the gate goes pending. */
  diskDiffersFromSynced?: () => string[];
  /** True for a path the peer itself just wrote; such events do not restart the quiet window. */
  isOwnWrite?: (path: string) => boolean;
  /** One-line diagnostics (stale lock, git off). */
  log?: (message: string) => void;
  /** Tick interval while nothing is pending. Default 5000. */
  idleTickMs?: number;
}

export type GateReason = 'git-missing' | 'unborn';

export interface GateState {
  /** `open`: sync as usual. `closed`: a decision is pending; sync nothing. `off`: git-aware logic is inactive; sync as usual. */
  mode: 'open' | 'closed' | 'off';
  pending: boolean;
  /** Names of the markers holding the gate. */
  markers: string[];
  /** Files held since the gate went pending (disk differed from the last synced content). */
  held: string[];
  reason?: GateReason;
}

export interface DecisionAction {
  type: 'decision';
  head: string | null;
  /** Reflog subjects since the previous decision, or null when there is no reflog (the content rule alone applies). */
  reflog: string[] | null;
  held: string[];
  /** True when the 30 s cap forced the decision. */
  capped: boolean;
}

export interface TickResult {
  actions: DecisionAction[];
  nextTickMs: number;
}

const sigOf = (c: CheapState) => (c.reflog ? `${c.reflog.size}:${c.reflog.mtimeMs}` : 'none');

export function createController(probe: ControllerProbe, options: ControllerOptions, clock: Clock) {
  const idleTickMs = options.idleTickMs ?? 5000;
  const log = options.log ?? (() => {});

  let off: GateReason | null = null;
  let unborn = false;
  let lastHead: string | null = null;
  let seenHead: string | null = null;
  let lastMark: ReflogMark | null = null;
  let seenSig = 'none';
  let pending = false;
  let pendingSince = 0;
  let quietSince = 0;
  let held: string[] = [];
  let blockingKey = '';
  const staleLogged = new Set<string>();
  let newlyStale = false;

  function goOff(e: GitUnavailableError) {
    if (off) return;
    off = 'git-missing';
    log(`git epochs off: ${e.message}`);
  }

  function enterPending() {
    if (pending) return;
    pending = true;
    pendingSince = clock.now();
    quietSince = pendingSince;
    held = options.diskDiffersFromSynced?.() ?? [];
  }

  /** Reads the cheap state and updates pending, the quiet window, and the stale-lock bookkeeping. */
  function observe(): { blocking: string[]; sig: string } {
    const now = clock.now();
    const cs = probe.cheapState();
    const blocking: string[] = [];
    for (const m of cs.markers) {
      if (m.kind === 'lock' && now - m.mtimeMs > NUMBERS.staleLockMs) {
        if (!staleLogged.has(m.name)) {
          staleLogged.add(m.name);
          newlyStale = true;
          log(`ignoring stale lock ${m.name} (older than ${NUMBERS.staleLockMs / 1000} s); resuming`);
        }
        continue;
      }
      blocking.push(m.name);
    }
    for (const n of [...staleLogged]) if (!cs.markers.some((m) => m.name === n)) staleLogged.delete(n);
    const sig = sigOf(cs);
    if (blocking.length > 0 || sig !== seenSig) enterPending();
    if (pending) {
      const key = blocking.join('\n');
      if (key === '' && blockingKey !== '') pendingSince = now; // the cap counts from the resume, not from the pause
      if (key !== blockingKey || sig !== seenSig || newlyStale) quietSince = now;
      blockingKey = key;
    } else blockingKey = blocking.join('\n');
    seenSig = sig;
    return { blocking, sig };
  }

  function state(blocking: string[]): GateState {
    if (off) return { mode: 'off', pending: false, markers: [], held: [], reason: off };
    if (unborn) return { mode: 'off', pending: false, markers: [], held: [], reason: 'unborn' };
    return { mode: pending ? 'closed' : 'open', pending, markers: blocking, held: pending ? [...held] : [] };
  }

  async function baseline() {
    const head = await probe.headSha();
    lastHead = head;
    seenHead = head;
    unborn = head === null;
    lastMark = (await probe.reflogSince(null))?.tail ?? null;
    seenSig = sigOf(probe.cheapState());
  }

  return {
    /** Records the baseline: HEAD, the reflog tail, and the reflog signature. */
    async init(): Promise<void> {
      try {
        await baseline();
      } catch (e) {
        if (e instanceof GitUnavailableError) goOff(e);
        else throw e;
      }
    },

    /** Synchronous: marker stats and the reflog file only, no subprocess. Call before every sync. */
    check(): GateState {
      if (off || unborn) return state([]);
      const { blocking } = observe();
      return state(blocking);
    },

    /** A file system event. The peer's own writes are ignored; any other event restarts the quiet window. */
    noteActivity(path: string): void {
      if (pending && !options.isOwnWrite?.(path)) quietSince = clock.now();
    },

    async tick(): Promise<TickResult> {
      const none = (ms: number): TickResult => ({ actions: [], nextTickMs: ms });
      if (off) return none(idleTickMs);
      let head: string | null;
      try {
        head = await probe.headSha();
      } catch (e) {
        if (e instanceof GitUnavailableError) {
          goOff(e);
          return none(idleTickMs);
        }
        // HEAD cannot be resolved right now: no change, keep re-checking.
        if (!off && !unborn) observe();
        return none(NUMBERS.controllerTickPendingMs);
      }
      if (unborn) {
        if (head === null) return none(idleTickMs);
        await baseline(); // first commit: adopt it without a decision
        return none(idleTickMs);
      }
      newlyStale = false;
      const { blocking, sig } = observe();
      if (head !== lastHead) {
        enterPending();
        if (head !== seenHead) quietSince = clock.now(); // a new HEAD is activity; the same HEAD seen again is not
      }
      seenHead = head;
      if (!pending) return none(idleTickMs);
      const now = clock.now();
      const quiet = blocking.length === 0 && now - quietSince >= NUMBERS.quietWindowMs;
      const capped = blocking.length === 0 && now - pendingSince >= NUMBERS.decisionCapMs;
      if (newlyStale || !(quiet || capped)) return none(NUMBERS.controllerTickPendingMs);
      const slice = await probe.reflogSince(lastMark);
      const action: DecisionAction = {
        type: 'decision',
        head,
        reflog: slice ? slice.subjects : null,
        held: [...held],
        capped: capped && !quiet,
      };
      lastHead = head;
      if (slice?.tail) lastMark = slice.tail;
      seenSig = sig;
      pending = false;
      held = [];
      return { actions: [action], nextTickMs: idleTickMs };
    },
  };
}

export type Controller = ReturnType<typeof createController>;
