/**
 * Execution channel (bd-sfet3264, Phase 2).
 *
 * The remote-execution-provider feature needs two ephemeral, project-scoped
 * signals between the editor and a connected `q2` executor:
 *
 *  - a **capability beacon** the executor re-broadcasts periodically
 *    ("I'm online and can run engines X, Y"), and
 *  - an **execute request** the editor sends ("please run this document now").
 *
 * Both ride Automerge's ephemeral messaging on the **index** DocHandle
 * (project-scoped), so a single channel reaches every peer regardless of the
 * active file. Ephemeral messages are best-effort and not persisted — exactly
 * right for liveness + "run now" nudges; durable status stays in the
 * persisted `CaptureRef.state`/`staleness` sidecar (D2).
 *
 * This module is split into:
 *  - a cross-language **wire format** (the Rust executor mirrors it in
 *    Phase 4) + pure helpers (builders, parse, live-executor bookkeeping),
 *    all timer/handle-free and unit-tested in isolation; and
 *  - a small stateful **service** (`createExecutionChannel`) that wires the
 *    index handle's broadcast/subscribe to those helpers and prunes stale
 *    executors on a timer.
 *
 * Phase 2 builds only the editor side + a stub responder for tests. The real
 * executor (which produces beacons and consumes requests) is Phase 4; claim /
 * heartbeat / `--force` takeover (D5) also land then.
 */

import type { DocHandle, DocHandleEphemeralMessagePayload } from '@automerge/automerge-repo';

/** How often the executor re-broadcasts its capability beacon. */
export const BEACON_INTERVAL_MS = 3000;
/**
 * Liveness window: an editor marks an executor offline if no beacon arrives
 * within this long. Locked at 1.5x the interval (D2) — tolerates a late
 * beacon and CRDT latency without flicker, but a genuinely disconnected
 * executor disappears within ~1.5 intervals.
 */
export const BEACON_TIMEOUT_MS = BEACON_INTERVAL_MS * 1.5;

/** Executor capability announcement (re-broadcast every `BEACON_INTERVAL_MS`). */
export interface ExecBeaconMessage {
  kind: 'exec/beacon';
  actorId: string;
  engines: string[];
  /**
   * Monotonic per-executor counter. Unused in Phase 2 (no claims); reserved
   * for the Phase 4 `--force` takeover protocol (D5).
   */
  generation: number;
}

/** Editor → executor "run this document now" request. */
export interface ExecRequestMessage {
  kind: 'exec/request';
  path: string;
  requestId: string;
  requesterActorId: string;
}

export type ExecMessage = ExecBeaconMessage | ExecRequestMessage;

/** An executor the editor currently believes is online. */
export interface LiveExecutor {
  actorId: string;
  engines: string[];
  generation: number;
  /** Epoch-ms of the most recent beacon from this executor. */
  lastSeen: number;
}

// ── Builders ────────────────────────────────────────────────────────────

export function makeBeacon(
  actorId: string,
  engines: string[],
  generation: number,
): ExecBeaconMessage {
  return { kind: 'exec/beacon', actorId, engines, generation };
}

export function makeExecuteRequest(
  path: string,
  requestId: string,
  requesterActorId: string,
): ExecRequestMessage {
  return { kind: 'exec/request', path, requestId, requesterActorId };
}

// ── Parsing / validation ────────────────────────────────────────────────

function isStringArray(v: unknown): v is string[] {
  return Array.isArray(v) && v.every((x) => typeof x === 'string');
}

/**
 * Validate and discriminate an untrusted ephemeral payload into a typed
 * `ExecMessage`, or `null` if it isn't a well-formed execution message. The
 * index handle may carry other ephemeral traffic in future, so anything that
 * isn't an `exec/*` message we recognise is ignored.
 */
export function parseExecMessage(raw: unknown): ExecMessage | null {
  if (!raw || typeof raw !== 'object') return null;
  const m = raw as Record<string, unknown>;
  switch (m.kind) {
    case 'exec/beacon':
      if (
        typeof m.actorId === 'string' &&
        isStringArray(m.engines) &&
        typeof m.generation === 'number'
      ) {
        return { kind: 'exec/beacon', actorId: m.actorId, engines: m.engines, generation: m.generation };
      }
      return null;
    case 'exec/request':
      if (
        typeof m.path === 'string' &&
        typeof m.requestId === 'string' &&
        typeof m.requesterActorId === 'string'
      ) {
        return {
          kind: 'exec/request',
          path: m.path,
          requestId: m.requestId,
          requesterActorId: m.requesterActorId,
        };
      }
      return null;
    default:
      return null;
  }
}

// ── Live-executor bookkeeping (pure) ────────────────────────────────────

/**
 * Upsert the executor named by `beacon`, stamping `lastSeen = nowMs`. Returns
 * a new map (does not mutate the input).
 */
export function applyBeacon(
  executors: ReadonlyMap<string, LiveExecutor>,
  beacon: ExecBeaconMessage,
  nowMs: number,
): Map<string, LiveExecutor> {
  const next = new Map(executors);
  next.set(beacon.actorId, {
    actorId: beacon.actorId,
    engines: beacon.engines,
    generation: beacon.generation,
    lastSeen: nowMs,
  });
  return next;
}

/**
 * Drop executors whose most recent beacon is older than `timeoutMs`. Returns
 * a new map (does not mutate the input). An executor exactly at the boundary
 * is kept (`nowMs - lastSeen <= timeoutMs`).
 */
export function pruneExecutors(
  executors: ReadonlyMap<string, LiveExecutor>,
  nowMs: number,
  timeoutMs: number = BEACON_TIMEOUT_MS,
): Map<string, LiveExecutor> {
  const next = new Map<string, LiveExecutor>();
  for (const [id, ex] of executors) {
    if (nowMs - ex.lastSeen <= timeoutMs) next.set(id, ex);
  }
  return next;
}

// ── Stateful service ────────────────────────────────────────────────────

/** A DocHandle restricted to the ephemeral surface this channel needs. */
export interface EphemeralHandle {
  broadcast(message: unknown): void;
  on(event: 'ephemeral-message', handler: (payload: DocHandleEphemeralMessagePayload<unknown>) => void): void;
  off(event: 'ephemeral-message', handler: (payload: DocHandleEphemeralMessagePayload<unknown>) => void): void;
}

export interface ExecutionChannelOptions {
  /** Returns the index DocHandle to broadcast/subscribe on (null until connected). */
  getIndexHandle: () => EphemeralHandle | DocHandle<unknown> | null;
  /** Called whenever the set of live executors changes (added / refreshed / pruned). */
  onExecutorsChange: (executors: LiveExecutor[]) => void;
  /** This client's actor id (so an editor that is also an executor can ignore self). Optional. */
  selfActorId?: string;
  /** Injectable clock (ms). Defaults to `Date.now`. */
  now?: () => number;
  /** Injectable id generator for request ids. Defaults to a random-ish token. */
  generateRequestId?: () => string;
  /** Prune cadence (ms). Defaults to the beacon interval. */
  pruneIntervalMs?: number;
}

export interface ExecutionChannel {
  /** Begin listening for beacons + start the prune timer. */
  start(): void;
  /** Stop listening and clear the prune timer. */
  stop(): void;
  /** Broadcast an execute request for `path`. Returns the request id, or null if not connected. */
  requestExecution(path: string): string | null;
  /** Current live executors (snapshot). */
  getExecutors(): LiveExecutor[];
}

export function createExecutionChannel(opts: ExecutionChannelOptions): ExecutionChannel {
  const now = opts.now ?? (() => Date.now());
  const pruneIntervalMs = opts.pruneIntervalMs ?? BEACON_INTERVAL_MS;
  const genId =
    opts.generateRequestId ??
    (() => `req-${now().toString(36)}-${Math.floor(Math.random() * 1e9).toString(36)}`);

  let executors: Map<string, LiveExecutor> = new Map();
  let handle: EphemeralHandle | null = null;
  let pruneTimer: ReturnType<typeof setInterval> | null = null;
  let messageHandler:
    | ((payload: DocHandleEphemeralMessagePayload<unknown>) => void)
    | null = null;

  function emit(): void {
    opts.onExecutorsChange(Array.from(executors.values()));
  }

  function handleMessage(payload: DocHandleEphemeralMessagePayload<unknown>): void {
    const msg = parseExecMessage(payload.message);
    if (!msg) return;
    if (msg.kind === 'exec/beacon') {
      // An editor that is also an executor ignores its own beacon.
      if (opts.selfActorId && msg.actorId === opts.selfActorId) return;
      const before = executors.get(msg.actorId);
      executors = applyBeacon(executors, msg, now());
      // Fire on a newly-seen executor or on any refresh — the editor wants the
      // freshest engine list / liveness. (A pure lastSeen bump still matters
      // because it pushes back the prune deadline.)
      if (!before || before.generation !== msg.generation || before.engines.join() !== msg.engines.join()) {
        emit();
      }
    }
    // exec/request is consumed by the executor (Phase 4), not the editor.
  }

  function prune(): void {
    const before = executors.size;
    executors = pruneExecutors(executors, now());
    if (executors.size !== before) emit();
  }

  function start(): void {
    if (handle) return; // already started
    const h = opts.getIndexHandle();
    if (!h) return;
    handle = h as EphemeralHandle;
    messageHandler = handleMessage;
    handle.on('ephemeral-message', messageHandler);
    pruneTimer = setInterval(prune, pruneIntervalMs);
  }

  function stop(): void {
    if (handle && messageHandler) handle.off('ephemeral-message', messageHandler);
    if (pruneTimer !== null) clearInterval(pruneTimer);
    handle = null;
    messageHandler = null;
    pruneTimer = null;
    executors = new Map();
  }

  function requestExecution(path: string): string | null {
    const h = handle ?? (opts.getIndexHandle() as EphemeralHandle | null);
    if (!h) return null;
    const requestId = genId();
    h.broadcast(makeExecuteRequest(path, requestId, opts.selfActorId ?? ''));
    return requestId;
  }

  function getExecutors(): LiveExecutor[] {
    return Array.from(executors.values());
  }

  return { start, stop, requestExecution, getExecutors };
}
