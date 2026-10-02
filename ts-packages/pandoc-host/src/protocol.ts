import { execute } from './execute.ts';
import { prepareForPost } from './transfer.ts';
import type { ExecuteResult, Fault, PandocRequest, ShareTree } from './types.ts';
import type { Limits } from './limits.ts';

/** Messages to the worker. The shell in hub-client forwards `onmessage` data to `createHandler`. */
export type WorkerRequest =
  | { type: 'init'; module: WebAssembly.Module; limits?: Limits }
  | { type: 'run'; id: number; request: PandocRequest; shareTree: ShareTree; fault?: Fault };

/** Messages from the worker. */
export type WorkerResponse =
  | { type: 'ready' }
  | { type: 'progress'; id: number; stage: 'mounting' | 'running' }
  | { type: 'result'; id: number; result: ExecuteResult };

/**
 * The worker-side state machine, independent of `self`/`parentPort`: `post` is the
 * environment's postMessage. The output buffer is transferred, not copied.
 */
export function createHandler(post: (msg: WorkerResponse, transfer?: Transferable[]) => void) {
  let module: WebAssembly.Module | undefined;
  let limits: Limits | undefined;
  return async (msg: WorkerRequest): Promise<void> => {
    if (msg.type === 'init') {
      module = msg.module;
      limits = msg.limits;
      post({ type: 'ready' });
      return;
    }
    if (!module) {
      post({
        type: 'result',
        id: msg.id,
        result: { ok: false, kind: 'crash', status: null, stderr: '', stdout: '', diagnostics: [{ origin: 'host', kind: 'error', code: 'pandoc-crash', message: 'worker received a run before init' }] },
      });
      return;
    }
    const result = await execute(msg.request, msg.shareTree, {
      module,
      fault: msg.fault,
      limits,
      onProgress: (stage) => post({ type: 'progress', id: msg.id, stage }),
    });
    const transfer: Transferable[] = [];
    if (result.ok && result.output.buffer instanceof ArrayBuffer && result.output.byteLength === result.output.buffer.byteLength)
      transfer.push(result.output.buffer);
    post({ type: 'result', id: msg.id, result }, transfer);
  };
}

export { prepareForPost };
