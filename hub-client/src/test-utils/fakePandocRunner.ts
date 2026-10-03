import type { ExecuteResult, WorkerRequest, WorkerResponse } from '@quarto/pandoc-host';
import type { LoadResult, PandocLoader } from '../pandoc/pandocLoader';
import type { WorkerLike } from '../pandoc/pandocRunner';

// Test doubles shared by the fresh and warm runner suites.

/** A scriptable Worker double: answers init with `ready`, and records posts and terminations. */
export class FakeWorker implements WorkerLike {
  onmessage: WorkerLike['onmessage'] = null;
  onerror: WorkerLike['onerror'] = null;
  onmessageerror: WorkerLike['onmessageerror'] = null;
  terminated = 0;
  posts: WorkerRequest[] = [];
  autoReady = true;
  postMessage(message: unknown, transfer?: Transferable[]) {
    // A transfer moves the sender's buffers into the receiver's copy, as a real worker does.
    const msg = (transfer?.length ? structuredClone(message, { transfer }) : message) as WorkerRequest;
    this.posts.push(msg);
    if (msg.type === 'init' && this.autoReady) queueMicrotask(() => this.emit({ type: 'ready' }));
  }
  private answered = new Set<number>();
  /** The `run` posts so far, in order (a warm worker is sent several). */
  runs() {
    return this.posts.filter((m): m is Extract<WorkerRequest, { type: 'run' }> => m.type === 'run');
  }
  /** Answer the first `run` message not yet answered with `result`. */
  respond(result: ExecuteResult) {
    const run = this.runs().find((m) => !this.answered.has(m.id));
    if (!run) throw new Error('no unanswered run message');
    this.answered.add(run.id);
    this.emit({ type: 'result', id: run.id, result });
  }
  terminate() {
    this.terminated++;
  }
  /** Deliver a message as the worker would (even after terminate, for the late-message test). */
  emit(m: WorkerResponse) {
    this.onmessage?.({ data: m });
  }
}

export const OK_RESULT = (): ExecuteResult => ({
  ok: true,
  status: 0,
  output: new Uint8Array([1, 2, 3]),
  outputPath: '/__q2_share__/out.txt',
  stderr: '',
  stdout: '',
  diagnostics: [],
  stats: { instanceMs: 1, runMs: 1, memoryBytes: 1, mountedBytes: 1 },
});

export function fakeLoader(load?: PandocLoader['load']) {
  const holds = { active: 0, total: 0 };
  const dropListeners = new Set<() => void>();
  let rendered = 0;
  const loader = {
    onDrop: (listener: () => void) => {
      dropListeners.add(listener);
      return () => dropListeners.delete(listener);
    },
    dropResident: () => {
      rendered = 0;
      dropListeners.forEach((l) => l());
    },
    countRender: () => {
      rendered++;
    },
    get renders() {
      return rendered;
    },
    load:
      load ??
      (async () => ({ module: {} as WebAssembly.Module, source: 'resident', notices: [] }) satisfies LoadResult),
    hold: () => {
      holds.active++;
      holds.total++;
      let done = false;
      return () => {
        if (!done) {
          done = true;
          holds.active--;
        }
      };
    },
  } as unknown as PandocLoader;
  return { loader, holds };
}

