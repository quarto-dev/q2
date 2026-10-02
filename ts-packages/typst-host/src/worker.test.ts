import { Worker } from 'node:worker_threads';
import { describe, expect, it } from 'vitest';
import type { WorkerRequest, WorkerResponse } from './protocol.ts';
import { defaultFonts, text, typstModule } from './fixtures.test-util.ts';

// The handler running in a real worker thread, as the browser shell runs it: the compiled
// Module and the fonts are posted in, the PDF buffer comes back transferred.
describe('createHandler in a worker thread', () => {
  it('inits from a posted Module, compiles, and returns the PDF', async () => {
    const worker = new Worker(new URL('./worker.test-thread.mjs', import.meta.url));
    const messages: WorkerResponse[] = [];
    const waitFor = (pred: (m: WorkerResponse) => boolean) =>
      new Promise<WorkerResponse>((resolve, reject) => {
        const hit = messages.find(pred);
        if (hit) return resolve(hit);
        const on = (m: WorkerResponse) => {
          messages.push(m);
          if (pred(m)) {
            worker.off('message', on);
            resolve(m);
          }
        };
        worker.on('message', on);
        worker.once('error', reject);
      });
    try {
      const init: WorkerRequest = { type: 'init', init: { module: await typstModule(), fonts: defaultFonts() } };
      worker.postMessage(init);
      const ready = await waitFor((m) => m.type === 'ready');
      expect(ready.type === 'ready' && ready.fontFamilies).toContain('Libertinus Serif');
      const run: WorkerRequest = { type: 'run', id: 7, input: { main: '/main.typ', files: [{ path: '/main.typ', bytes: text('Hello\n#pagebreak()\nWorld') }] } };
      worker.postMessage(run);
      const done = await waitFor((m) => m.type === 'result');
      if (done.type !== 'result' || !done.result.ok) throw new Error(JSON.stringify(done));
      expect(done.id).toBe(7);
      expect(done.result.pages).toBe(2);
      expect(messages.some((m) => m.type === 'progress' && m.stage === 'compiling')).toBe(true);
    } finally {
      await worker.terminate();
    }
  }, 60_000);

  it('answers a run that arrives before init with a crash result, not a hang', async () => {
    const worker = new Worker(new URL('./worker.test-thread.mjs', import.meta.url));
    try {
      const reply = new Promise<WorkerResponse>((resolve) => worker.once('message', resolve));
      worker.postMessage({ type: 'run', id: 1, input: { main: '/m.typ', files: [] } } satisfies WorkerRequest);
      expect(await reply).toMatchObject({ type: 'result', id: 1, result: { ok: false, kind: 'crash' } });
    } finally {
      await worker.terminate();
    }
  });
});
