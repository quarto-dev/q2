/**
 * The editor-only git gate (GE-9, G20). In editor-only mode a hub owns the
 * directory and git may rewrite files under VS Code. While git works, and for
 * a quiet window after, the extension must not send edits (a buffer reload is
 * not a typed edit) and must not write the old doc into a buffer. When the
 * gate reopens it compares each bound buffer with its doc: all equal means a
 * plain commit and editing resumes; otherwise a checkout rewrote the files, so
 * it waits for the hub's flip, rebinds, and warns if none comes.
 *
 * Built on the GE-5 controller (`createController`): its `check()` is the
 * synchronous gate, its `tick()` decides once the repository has been quiet.
 * All time comes from the injected clock.
 */
import {
  EDITOR_ONLY_NO_FLIP,
  EDITOR_ONLY_NO_FLIP_ACTIONS,
  GIT_OP_EDITS_DROPPED,
  NUMBERS,
  createController,
  type Controller,
  type ControllerProbe,
} from '@quarto/quarto-git-epochs';
import type { Notifier } from './host';

export interface GateClock {
  now(): number;
  setTimeout: typeof setTimeout;
  clearTimeout: typeof clearTimeout;
}

/** A bound buffer as the gate sees it. */
export interface GateBuffer {
  rel: string;
  /** The buffer's text now. */
  text: string | null;
  /** The text of the doc the buffer is bound to. */
  docText: string | undefined;
  dirty: boolean;
}

export interface EditorGateOptions {
  probe: ControllerProbe;
  clock: GateClock;
  notifier: Notifier;
  log(line: string): void;
  /** The bound buffers right now. */
  inspect(): GateBuffer[];
  /** After the gate reopens: bring every bound buffer to its doc's text. */
  reconcile(): void;
  /** How long to wait for the hub's flip after the gate reopens (G32: 10 000). */
  flipWaitMs?: number;
}

export class EditorGate {
  private readonly controller: Controller;
  private readonly flipWaitMs: number;
  /** The controller said closed and we noticed (the start of an episode). */
  private closedSeen = false;
  /** The gate reopened but a buffer still differs from its doc: waiting for the flip. */
  private waiting = false;
  private episode: { dirtyAtClose: boolean } | null = null;
  /** Paths whose doc swapped during this episode: their buffers take the new doc's text on resume. */
  private readonly swapped = new Set<string>();
  private tickTimer: unknown = null;
  private watchTimer: unknown = null;
  private waitTimer: unknown = null;
  private disposed = false;

  constructor(private readonly o: EditorGateOptions) {
    this.flipWaitMs = o.flipWaitMs ?? 10_000;
    this.controller = createController(o.probe, { log: o.log }, o.clock);
  }

  async start(): Promise<void> {
    await this.controller.init();
    this.scheduleTick(NUMBERS.controllerTickPendingMs);
    this.scheduleWatch();
  }

  dispose(): void {
    this.disposed = true;
    for (const t of [this.tickTimer, this.watchTimer, this.waitTimer]) {
      if (t !== null) this.o.clock.clearTimeout(t as ReturnType<typeof setTimeout>);
    }
  }

  /** Synchronous. True while local edits must be dropped and the doc must not be written into buffers. */
  isClosed(): boolean {
    if (this.disposed) return false;
    if (this.waiting) return true;
    return this.observe();
  }

  /** The doc for `rel` was swapped (the hub flipped it). That ends the wait for it. */
  noteSwapped(rel: string): void {
    if (!this.closedSeen && !this.waiting) return;
    this.swapped.add(rel);
    if (this.waiting) this.maybeResume();
  }

  // --- internals -------------------------------------------------------------

  /** Reads the gate; notes the start of an episode. */
  private observe(): boolean {
    const state = this.controller.check();
    if (state.mode !== 'closed') return false;
    if (!this.closedSeen) {
      this.closedSeen = true;
      this.episode = { dirtyAtClose: this.o.inspect().some((b) => b.dirty) };
      this.swapped.clear();
      this.scheduleTick(NUMBERS.controllerTickPendingMs);
    }
    return true;
  }

  private scheduleTick(ms: number): void {
    if (this.disposed) return;
    if (this.tickTimer !== null) this.o.clock.clearTimeout(this.tickTimer as ReturnType<typeof setTimeout>);
    this.tickTimer = this.o.clock.setTimeout(() => void this.runTick(), ms);
  }

  private async runTick(): Promise<void> {
    this.tickTimer = null;
    if (this.disposed) return;
    let next = NUMBERS.controllerTickPendingMs;
    try {
      const r = await this.controller.tick();
      next = r.nextTickMs;
      this.observe();
      if (r.actions.length > 0) this.onDecision();
    } catch (err) {
      this.o.log(`editor gate tick failed: ${err instanceof Error ? err.message : String(err)}`);
    }
    this.scheduleTick(next);
  }

  /** A cheap poll (marker stats only) so the start of an episode is seen before the next keystroke. */
  private scheduleWatch(): void {
    if (this.disposed) return;
    this.watchTimer = this.o.clock.setTimeout(() => {
      if (!this.disposed) {
        if (!this.waiting) this.observe();
        this.scheduleWatch();
      }
    }, NUMBERS.controllerTickPendingMs);
  }

  /** The controller decided: the repository has been quiet. */
  private onDecision(): void {
    if (!this.closedSeen) {
      this.closedSeen = true;
      this.episode = { dirtyAtClose: false };
    }
    this.waiting = true;
    this.maybeResume() || this.armWait();
  }

  private settled(): boolean {
    // A buffer is settled when it equals its doc, or its doc was just swapped (it takes the new text).
    return this.o.inspect().every((b) => b.text === b.docText || this.swapped.has(b.rel));
  }

  private maybeResume(): boolean {
    if (!this.settled()) return false;
    if (this.waitTimer !== null) this.o.clock.clearTimeout(this.waitTimer as ReturnType<typeof setTimeout>);
    this.waitTimer = null;
    this.waiting = false;
    this.closedSeen = false;
    const episode = this.episode;
    this.episode = null;
    this.swapped.clear();
    this.o.reconcile();
    if (episode?.dirtyAtClose) void this.o.notifier.warn(GIT_OP_EDITS_DROPPED);
    return true;
  }

  private armWait(): void {
    if (this.waitTimer !== null) this.o.clock.clearTimeout(this.waitTimer as ReturnType<typeof setTimeout>);
    this.waitTimer = this.o.clock.setTimeout(() => void this.onWaitTimeout(), this.flipWaitMs);
  }

  private async onWaitTimeout(): Promise<void> {
    this.waitTimer = null;
    if (this.disposed || !this.waiting) return;
    // Stay suspended: the buffers still hold what git wrote and the doc is the old version.
    const choice = await this.o.notifier.warn(EDITOR_ONLY_NO_FLIP, ...EDITOR_ONLY_NO_FLIP_ACTIONS);
    if (choice === EDITOR_ONLY_NO_FLIP_ACTIONS[0] && this.waiting && !this.disposed) this.armWait();
  }
}
