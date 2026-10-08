import { describe, expect, it } from 'vitest';
import type { PandocRequest, ShareTree } from '@quarto/pandoc-host';
import { createNativeImportRunner, parseFrame } from './nativeImportRunner';
import type { RunStage } from './pandocRunner';

const BASE = 'http://127.0.0.1:8080/app/';
const SOURCE_PATH = '/__q2_share__/import/source.docx';
const request = { output_path: '/__q2_share__/import/out.json', host_inputs: [{ path: SOURCE_PATH, sha256: 'x', size: 3 }] } as unknown as PandocRequest;
const tree = { share_tree_version: 'v', files: [] } as unknown as ShareTree;
const inputs = () => ({ [SOURCE_PATH]: new Uint8Array([1, 2, 3]) });

interface HeaderOver {
  status?: number | null;
  stderr?: string;
  stdout?: string;
  timedOut?: boolean;
  aborted?: boolean;
  diagnostics?: unknown[];
}

/** `[u32 BE header_len][JSON header][output][media…]`, as the route frames it. */
function frame(output: string, media: [string, number[]][] = [], over: HeaderOver = {}): Uint8Array {
  const outBytes = new TextEncoder().encode(output);
  const header = new TextEncoder().encode(
    JSON.stringify({
      status: 0,
      stderr: '',
      stdout: '',
      timedOut: false,
      aborted: false,
      diagnostics: [],
      ...over,
      outputLen: outBytes.length,
      collected: media.map(([path, bytes]) => ({ path, len: bytes.length })),
    }),
  );
  const parts = [new Uint8Array(4), header, outBytes, ...media.map(([, b]) => Uint8Array.from(b))];
  new DataView(parts[0].buffer).setUint32(0, header.length, false);
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

function fakeFetch(res: Response | Error | ((init: RequestInit) => Promise<Response>)) {
  const calls: { url: string; init: RequestInit }[] = [];
  const fn = (async (url: string, init: RequestInit) => {
    calls.push({ url, init });
    if (typeof res === 'function') return res(init);
    if (res instanceof Error) throw res;
    return res;
  }) as unknown as typeof fetch;
  return { fn, calls };
}

const run = (fetchFn: typeof fetch, options: Parameters<ReturnType<typeof createNativeImportRunner>['run']>[2] = { inputs: inputs() }) =>
  createNativeImportRunner({ fetchFn, baseUrl: BASE }).run(request, tree, options);

describe('parseFrame', () => {
  it('parses frames and rejects truncated bodies', () => {
    const body = frame('{"a":1}', [
      ['/__q2_share__/import/media/a.png', [1, 2, 3]],
      ['/__q2_share__/import/media/b.png', [4]],
    ]);
    const f = parseFrame(body);
    expect(new TextDecoder().decode(f.output)).toBe('{"a":1}');
    expect(f.collected.map((c) => [c.path, [...c.bytes]])).toEqual([
      ['/__q2_share__/import/media/a.png', [1, 2, 3]],
      ['/__q2_share__/import/media/b.png', [4]],
    ]);
    expect(() => parseFrame(body.subarray(0, 2))).toThrow(/truncated/);
    expect(() => parseFrame(body.subarray(0, 10))).toThrow(/truncated/);
    expect(() => parseFrame(body.subarray(0, body.length - 1))).toThrow(/truncated/);
    expect(() => parseFrame(Uint8Array.from([...body, 0]))).toThrow(/trailing/);
  });

  it('reads a frame that is a view into a larger buffer', () => {
    const body = frame('{}');
    const padded = new Uint8Array(body.length + 8);
    padded.set(body, 5);
    expect(new TextDecoder().decode(parseFrame(padded.subarray(5, 5 + body.length)).output)).toBe('{}');
  });
});

describe('the native import runner', () => {
  it('posts the source bytes with its basename and fills a RunSuccess', async () => {
    const warning = { kind: 'warning', code: 'collect-limit', title: 'x', origin: 'host' };
    const { fn, calls } = fakeFetch(new Response(frame('{"blocks":[]}', [['/__q2_share__/import/media/a.png', [9]]], { stderr: 'w', diagnostics: [warning] })));
    const stages: RunStage[] = [];
    const out = await run(fn, { inputs: inputs(), onStage: (s) => stages.push(s) });
    expect(calls[0].url).toBe('http://127.0.0.1:8080/app/api/preview/import');
    expect(calls[0].init.method).toBe('POST');
    expect((calls[0].init.headers as Record<string, string>).file_name).toBe('source.docx');
    expect([...(calls[0].init.body as Uint8Array)]).toEqual([1, 2, 3]);
    expect(stages).toEqual(['running']);
    expect(out.ok).toBe(true);
    if (!out.ok) return;
    expect(new TextDecoder().decode(out.output)).toBe('{"blocks":[]}');
    expect(out.outputPath).toBe('/__q2_share__/import/out.json');
    expect(out.collected.map((c) => c.path)).toEqual(['/__q2_share__/import/media/a.png']);
    expect(out.stderr).toBe('w');
    expect(out.diagnostics).toEqual([warning]);
    expect(out.stats).toEqual({ mountMs: 0, instanceMs: 0, runMs: 0, memoryBytes: 0, mountedBytes: 0 });
    expect(out.notices).toEqual([]);
  });

  it.each([
    ['heap exhaustion is oom', { status: 251, stderr: 'pandoc: Heap exhausted' }, 'oom'],
    ['251 without the message is a plain exit', { status: 251, stderr: 'other' }, 'pandoc-exit'],
    ['another exit is pandoc-exit', { status: 63, stderr: "couldn't unpack docx container" }, 'pandoc-exit'],
    ['a signal (no status) is a crash', { status: null }, 'crash'],
    ['a timeout is timeout', { status: null, timedOut: true }, 'timeout'],
    ['an abort is aborted', { status: null, aborted: true }, 'aborted'],
  ] as const)('%s', async (_name, over, kind) => {
    const { fn } = fakeFetch(new Response(frame('', [], over as HeaderOver)));
    const out = await run(fn);
    expect(out.ok).toBe(false);
    if (out.ok) return;
    expect(out.kind).toBe(kind);
    expect(out.status).toBe((over as HeaderOver).status ?? null);
    expect(out.stderr).toBe((over as HeaderOver).stderr ?? '');
  });

  it('exit 0 with no output is no-output', async () => {
    const { fn } = fakeFetch(new Response(frame('')));
    const out = await run(fn);
    expect(out.ok === false && out.kind).toBe('no-output');
  });

  it('a truncated body is an invalid-request, not a throw', async () => {
    const { fn } = fakeFetch(new Response(frame('{}').subarray(0, 6)));
    const out = await run(fn);
    expect(out.ok === false && out.kind).toBe('invalid-request');
  });

  it.each([
    [422, 'Q-20-1'],
    [400, 'Q-24-1'],
    [413, 'Q-24-2'],
  ])('a %i with diagnostics is a refusal that carries them as Rust\'s own', async (status, code) => {
    const { fn } = fakeFetch(new Response(JSON.stringify({ diagnostics: [{ kind: 'error', code, title: 't' }] }), { status }));
    const out = await run(fn);
    expect(out.ok).toBe(false);
    if (out.ok) return;
    expect(out.kind).toBe('refused');
    expect(out.diagnostics).toEqual([{ kind: 'error', code, title: 't', origin: 'rust' }]);
  });

  it('a non-JSON error is an invalid-request with the server text', async () => {
    const { fn } = fakeFetch(new Response('missing file_name header', { status: 400 }));
    const out = await run(fn);
    expect(out.ok === false && out.kind).toBe('invalid-request');
    expect(out.ok === false && out.stderr).toBe('missing file_name header');
  });

  it('an unreachable server is a crash with a message', async () => {
    const { fn } = fakeFetch(new TypeError('Failed to fetch'));
    const out = await run(fn);
    expect(out.ok === false && out.kind).toBe('crash');
    expect(out.ok === false && out.stderr).toMatch(/Could not reach the preview server/);
  });

  it('a request without source bytes never reaches the server', async () => {
    const { fn, calls } = fakeFetch(new Response(frame('{}')));
    const out = await run(fn, {});
    expect(out.ok === false && out.kind).toBe('invalid-request');
    expect(calls).toHaveLength(0);
  });

  it('abort: a signal that is already aborted never posts, and one aborted mid-flight is aborted', async () => {
    const pre = new AbortController();
    pre.abort();
    const never = fakeFetch(new Response(frame('{}')));
    expect((await run(never.fn, { inputs: inputs(), signal: pre.signal })).ok === false).toBe(true);
    expect(never.calls).toHaveLength(0);

    const ctl = new AbortController();
    const { fn } = fakeFetch(
      (init) =>
        new Promise((_, reject) => {
          init.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
        }),
    );
    const pending = run(fn, { inputs: inputs(), signal: ctl.signal });
    ctl.abort();
    const out = await pending;
    expect(out.ok === false && out.kind).toBe('aborted');
  });
});
