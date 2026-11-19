/**
 * The clone peer's gate (GE-10, G1, G3): the GE-5 controller plus the timers that drive it. `isClosed()` is the
 * synchronous check every sync path calls; when the repository has been quiet the decision callback runs, and the gate
 * stays closed until it finishes. All time comes from the injected clock.
 */
import { NUMBERS, createController, type Controller, type ControllerProbe, type DecisionAction, type GateState } from '@quarto/quarto-git-epochs';
import type { GateClock } from '../editor-gate';

export interface CloneGateOptions {
  probe: ControllerProbe;
  clock: GateClock;
  log(line: string): void;
  /** Paths whose disk content differs from their last synced content; read once when the gate goes pending. */
  diskDiffersFromSynced(): string[];
  /** True for a path the extension itself just wrote: its file event does not restart the quiet window. */
  isOwnWrite(path: string): boolean;
  /** Runs once per decision, while the gate stays closed. */
  onDecision(action: DecisionAction): Promise<void>;
}

export class CloneGate {
  private readonly controller: Controller;
  private tickTimer: unknown = null;
  private watchTimer: unknown = null;
  private deciding = false;
  private disposed = false;
  /** False until the controller has its baseline: before that the gate has nothing to compare with and must stay open. */
  private ready = false;
  private lastState: GateState = { mode: 'open', pending: false, markers: [], held: [] };

  constructor(private readonly o: CloneGateOptions) {
    this.controller = createController(
      o.probe,
      { log: o.log, diskDiffersFromSynced: o.diskDiffersFromSynced, isOwnWrite: o.isOwnWrite },
      o.clock,
    );
  }

  async start(): Promise<void> {
    await this.controller.init();
    this.ready = true;
    this.scheduleTick(NUMBERS.controllerTickPendingMs);
    this.scheduleWatch();
  }

  dispose(): void {
    this.disposed = true;
    for (const t of [this.tickTimer, this.watchTimer]) if (t !== null) this.o.clock.clearTimeout(t as ReturnType<typeof setTimeout>);
  }

  /** Synchronous. True while git works, for the quiet window after, and while a decision is being applied. */
  isClosed(): boolean {
    if (this.disposed || !this.ready) return false;
    if (this.deciding) return true;
    this.lastState = this.controller.check();
    return this.lastState.mode === 'closed';
  }

  /** The gate is `off` (git unavailable or unborn): ordinary sync applies. */
  isOff(): boolean {
    return this.lastState.mode === 'off';
  }

  /** The markers holding the gate, or why it is closed (diagnostics for `cloneState`). */
  describe(): string {
    return `${this.deciding ? 'deciding ' : ''}${this.lastState.mode}${this.lastState.pending ? ' pending' : ''} [${this.lastState.markers.join(',')}]`;
  }

  heldPaths(): string[] {
    return this.lastState.held;
  }

  noteActivity(rel: string): void {
    this.controller.noteActivity(rel);
  }

  private scheduleTick(ms: number): void {
    if (this.disposed) return;
    if (this.tickTimer !== null) this.o.clock.clearTimeout(this.tickTimer as ReturnType<typeof setTimeout>);
    this.tickTimer = this.o.clock.setTimeout(() => void this.runTick(), ms);
  }

  private async runTick(): Promise<void> {
    this.tickTimer = null;
    if (this.disposed) return;
    let next: number = NUMBERS.controllerTickPendingMs;
    try {
      const r = await this.controller.tick();
      next = r.nextTickMs;
      this.lastState = this.controller.check();
      for (const action of r.actions) {
        this.deciding = true;
        try {
          await this.o.onDecision(action);
        } finally {
          this.deciding = false;
        }
      }
    } catch (err) {
      this.o.log(`clone gate tick failed: ${err instanceof Error ? err.message : String(err)}`);
    }
    this.scheduleTick(next);
  }

  /** A cheap poll (marker stats only) so the start of an episode is seen before the next keystroke. */
  private scheduleWatch(): void {
    if (this.disposed) return;
    this.watchTimer = this.o.clock.setTimeout(() => {
      if (!this.disposed) {
        if (!this.deciding) this.lastState = this.controller.check();
        this.scheduleWatch();
      }
    }, NUMBERS.controllerTickPendingMs);
  }
}
