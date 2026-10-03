import { execute } from './execute.ts';
import { prepareForPost } from './transfer.ts';
import { WarmSession } from './warmSession.ts';
import type { ExecuteResult, Fault, PandocRequest, ShareTree } from './types.ts';
import type { Limits } from './limits.ts';

/** Messages to the worker. The shell in hub-client forwards `onmessage` data to `createHandler`. */
export type WorkerRequest =
  | { type: 'init'; module: WebAssembly.Module; limits?: Limits; warm?: boolean }
  | { type: 'run'; id: number; request: PandocRequest; shareTree: ShareTree; fault?: Fault };

/** Messages from the worker. */
export type WorkerResponse =
  | { type: 'ready' }
  | { type: 'progress'; id: number; stage: 'mounting' | 'running' }
  | { type: 'result'; id: number; result: ExecuteResult };

/**
 * The worker-side state machine, independent of `self`/`parentPort`: `post` is the
 * environment's postMessage. The output buffer is transferred, not copied.
 *
 * In warm mode (`init.warm`) the worker owns a `WarmSession`, whose first instance is created during
 * `init`, so `ready` means the instance exists and no render pays for instantiation. Results (failures
 * too) then carry `stats`, including `retire`. If that first instance cannot be created, `ready` is
 * still posted and the next `run` tries again, so the failure is reported against a run.
 */
export function createHandler(post: (msg: WorkerResponse, transfer?: Transferable[]) => void) {
  let module: WebAssembly.Module | undefined;
  let limits: Limits | undefined;
  let warm = false;
  let session: WarmSession | undefined;
  const crash = (id: number, message: string): WorkerResponse => ({
    type: 'result',
    id,
    result: { ok: false, kind: 'crash', status: null, stderr: '', stdout: '', diagnostics: [{ origin: 'host', kind: 'error', code: 'pandoc-crash', message }] },
  });
  return async (msg: WorkerRequest): Promise<void> => {
    if (msg.type === 'init') {
      module = msg.module;
      limits = msg.limits;
      warm = msg.warm === true;
      session = undefined;
      if (warm) session = await WarmSession.create(msg.module, { limits }).catch(() => undefined);
      post({ type: 'ready' });
      return;
    }
    if (!module) {
      post(crash(msg.id, 'worker received a run before init'));
      return;
    }
    const onProgress = (stage: 'mounting' | 'running') => post({ type: 'progress', id: msg.id, stage });
    let result;
    if (warm) {
      try {
        session ??= await WarmSession.create(module, { limits });
      } catch (e) {
        post(crash(msg.id, `The warm pandoc instance could not be created (${String(e)}).`));
        return;
      }
      result = await session.run(msg.request, msg.shareTree, { fault: msg.fault, onProgress });
    } else {
      result = await execute(msg.request, msg.shareTree, { module, fault: msg.fault, limits, onProgress });
    }
    const transfer: Transferable[] = [];
    if (result.ok && result.output.buffer instanceof ArrayBuffer && result.output.byteLength === result.output.buffer.byteLength)
      transfer.push(result.output.buffer);
    post({ type: 'result', id: msg.id, result }, transfer);
  };
}

export { prepareForPost };
