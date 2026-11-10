import { describe, expect, it } from 'vitest';
import { Rpc } from './rpc';

/** Two connected RPC peers over a real MessageChannel. */
function pair(
  aHandlers: Record<string, (...args: never[]) => unknown>,
  bHandlers: Record<string, (...args: never[]) => unknown>,
  bSerial = false,
) {
  const channel = new MessageChannel();
  const a = new Rpc(channel.port1, aHandlers);
  const b = new Rpc(channel.port2, bHandlers, { serial: bSerial });
  return { a, b, close: () => channel.port1.close() };
}

function deferred<T = void>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe('Rpc', () => {
  it('matches responses to requests by id, whatever order they settle in', async () => {
    const slow = deferred<string>();
    const { a, close } = pair({}, {
      slow: () => slow.promise,
      echo: (x: string) => x,
    });
    const first = a.call<string>('slow', []);
    const second = a.call<string>('echo', ['b']);
    await expect(second).resolves.toBe('b');
    slow.resolve('a');
    await expect(first).resolves.toBe('a');
    close();
  });

  it('propagates a thrown error with its name and message', async () => {
    const { a, close } = pair({}, {
      fail: () => {
        const e = new Error('superseded');
        e.name = 'RenderSupersededError';
        throw e;
      },
    });
    const err = await a.call('fail', []).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(Error);
    expect((err as Error).name).toBe('RenderSupersededError');
    expect((err as Error).message).toBe('superseded');
    const unknown = await a.call('nope', []).catch((e: unknown) => e);
    expect((unknown as Error).message).toMatch(/no handler for "nope"/);
    close();
  });

  it('calls in both directions, including back to the caller mid-request', async () => {
    const channel = new MessageChannel();
    const main = new Rpc(channel.port1, { rasterize: (n: number) => n * 2 });
    // The worker side is serial, yet its own outbound call must still
    // get its response while the request that made it is in progress.
    const worker: Rpc = new Rpc(
      channel.port2,
      { render: async () => worker.call<number>('rasterize', [21]) },
      { serial: true },
    );
    await expect(main.call('render', [])).resolves.toBe(42);
    channel.port1.close();
  });

  it('in serial mode runs one request at a time, in arrival order', async () => {
    const log: string[] = [];
    const gate = deferred();
    const { a, close } = pair(
      {},
      {
        render: async (n: number) => {
          log.push(`render ${n} start`);
          if (n === 1) await gate.promise;
          log.push(`render ${n} end`);
        },
        write: (p: string) => {
          log.push(`write ${p}`);
        },
      },
      true,
    );
    const r1 = a.call('render', [1]);
    const w = a.call('write', ['x']);
    const r2 = a.call('render', [2]);
    await new Promise((r) => setTimeout(r, 10));
    expect(log).toEqual(['render 1 start']);
    gate.resolve();
    await Promise.all([r1, w, r2]);
    expect(log).toEqual(['render 1 start', 'render 1 end', 'write x', 'render 2 start', 'render 2 end']);
    close();
  });
});
