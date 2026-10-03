/**
 * Real-wasm tests for @quarto/pandoc-host (host phase H1): R0's captured native runs
 * through the pinned pandoc.wasm in command mode, plus the host behaviours.
 * Needs the wasm: `node scripts/fetch-pandoc-wasm.mjs` (a missing wasm skips locally,
 * fails when CI is set). Comparison under the extractor needs
 * `cargo build --release -p quarto-output-extract` (same rule).
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Worker, isMainThread } from 'node:worker_threads';
import { createHash } from 'node:crypto';
import { unzipSync } from 'fflate';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { execute, prepareForPost, prepareInputsForPost, type PandocRequest, type ShareTree, type WorkerRequest, type WorkerResponse } from '@quarto/pandoc-host';
import {
  CONSTANTS,
  REPO,
  WASM_PATH,
  importRecordingNames,
  loadImportRecording,
  loadRecording,
  pandocWasmAvailable,
  recordingNames,
  relocateReferenceText,
} from '../test-utils/pandocRecordings';

const CI = !!process.env.CI;
const haveWasm = pandocWasmAvailable();
const SDE = '1700000000';

const extractorPath = (): string | null => {
  const exe = process.platform === 'win32' ? '.exe' : '';
  const cands = [
    process.env.QUARTO_OUTPUT_EXTRACT,
    path.join(REPO, 'target/release/quarto-output-extract' + exe),
    path.join(REPO, 'target/debug/quarto-output-extract' + exe),
  ];
  return cands.find((c): c is string => !!c && existsSync(c)) ?? null;
};
const extractor = extractorPath();

const enc = new TextEncoder();
const dec = new TextDecoder();

describe('environment', () => {
  it('runs in a forked process with the exnref flag (vitest.wasm.config.ts)', () => {
    expect(isMainThread).toBe(true); // not a worker thread
    expect(process.execArgv).toContain('--experimental-wasm-exnref');
  });
  it('has pandoc.wasm available (required in CI)', () => {
    if (!haveWasm && CI) throw new Error(`pandoc.wasm missing at ${WASM_PATH}: run node scripts/fetch-pandoc-wasm.mjs --require`);
    if (!haveWasm) console.warn(`SKIPPING real-wasm pandoc tests: ${WASM_PATH} missing (node scripts/fetch-pandoc-wasm.mjs)`);
  });
});

describe.skipIf(!haveWasm)('pandoc.wasm in command mode', () => {
  let module: WebAssembly.Module;
  beforeAll(async () => {
    module = await WebAssembly.compile(readFileSync(WASM_PATH));
  });

  // ---- captured native runs ---------------------------------------------------
  describe('R0 recordings reproduce the native output', () => {
    if (!extractor && CI) {
      it('has the quarto-output-extract binary (required in CI)', () => {
        throw new Error('quarto-output-extract missing: cargo build --release -p quarto-output-extract');
      });
    }
    const scratch = mkdtempSync(path.join(os.tmpdir(), 'pandoc-host-'));
    afterAll(() => rmSync(scratch, { recursive: true, force: true }));

    // True when the native reference embeds the path it ran at (docx image descriptions do),
    // so a byte comparison against a run at another path cannot hold.
    const embedsReplayPath = (file: string, bytes: Uint8Array): boolean => {
      const needle = 'q2-pandoc-replay';
      if (file.endsWith('.typ')) return dec.decode(bytes).includes(needle);
      return Object.values(unzipSync(bytes)).some((e) => dec.decode(e).includes(needle));
    };

    for (const name of recordingNames()) {
      it(name, async () => {
        const rec = loadRecording(name);
        const result = await execute(rec.request, rec.shareTree, { module });
        if (!result.ok) throw new Error(`${name}: ${result.kind} status=${result.status}\n${result.stderr}\n${JSON.stringify(result.diagnostics)}`);
        const ref = readFileSync(rec.referencePath);
        const strict = rec.format === 'docx' || rec.format === 'typst';
        const identical = Buffer.from(result.output).equals(ref);
        if (strict && !embedsReplayPath(rec.referencePath, ref)) {
          // docx and typst are byte-equal under SOURCE_DATE_EPOCH (R0) unless the reference embeds its path.
          expect(identical, `${name}: not byte-identical to the native output`).toBe(true);
          return;
        }
        if (identical) return;
        if (!extractor) {
          console.warn(`${name}: not byte-identical and no quarto-output-extract binary; comparison skipped`);
          return;
        }
        // Equal under the extractor, with the native run's replay paths mapped into the request layout.
        const ext = path.extname(rec.outputName);
        const ours = path.join(scratch, `${name}${ext}`);
        writeFileSync(ours, result.output);
        const extract = (f: string) => execFileSync(extractor, ['extract', f], { maxBuffer: 1 << 28 }).toString('utf8');
        expect(extract(ours)).toBe(relocateReferenceText(extract(rec.referencePath), name));
      });
    }
  });

  // ---- import recordings: pandoc as a reader (document import, plan P1 T7) -------------------
  describe('import recordings: wasm reads what native pandoc read', () => {
    const sha = (b: Uint8Array) => createHash('sha256').update(b).digest('hex');
    // Both runs saw the canonical /__q2_share__/import paths; compare stderr modulo line endings and trailing space.
    const norm = (t: string) => t.replace(/\r\n/g, '\n').trimEnd();
    // `images-docx` first: it is the one that proves `--extract-media` writes anything under wasm
    // (upstream pandoc#11584 says the wasm build cannot write the typst writer's extracted images).
    const names = importRecordingNames().sort((a, b) => Number(b === 'images-docx') - Number(a === 'images-docx'));

    for (const name of names) {
      it(name, async () => {
        const rec = loadImportRecording(name);
        const r = await execute(rec.request, rec.shareTree, { module, inputs: { [rec.sourcePath]: rec.source.slice() } });
        if (rec.pandocJson === null) {
          // A failing fixture (`corrupt-docx`): the exit status and stderr are what P3 classifies.
          expect(r.ok, `${name} should fail`).toBe(false);
          if (r.ok) return;
          expect(r.kind).toBe('pandoc-exit');
          expect(r.status).toBe(rec.status);
          expect(norm(r.stderr)).toBe(norm(rec.stderr));
          return;
        }
        if (!r.ok) throw new Error(`${name}: ${r.kind} status=${r.status}\n${r.stderr}\n${JSON.stringify(r.diagnostics)}`);
        expect(JSON.parse(dec.decode(r.output))).toEqual(rec.pandocJson);
        expect(r.collected.map((f) => [f.path, sha(f.bytes)])).toEqual(rec.media.map((m) => [m.pandocPath, m.sha256]));
        expect(norm(r.stderr)).toBe(norm(rec.stderr));
        expect(r.diagnostics).toEqual([]);
      });
    }

    it('a worker thread takes the source as a transferred input and returns the collected media', async () => {
      const rec = loadImportRecording('images-docx');
      const worker = new Worker(new URL('../test-utils/pandocHostThread.mjs', import.meta.url));
      try {
        const msgs: WorkerResponse[] = [];
        const done = new Promise<void>((resolve) =>
          worker.on('message', (m: WorkerResponse) => {
            msgs.push(m);
            if (m.type === 'result') resolve();
          }),
        );
        worker.postMessage({ type: 'init', module } satisfies WorkerRequest);
        const p = prepareForPost(rec.request);
        const inputs = prepareInputsForPost({ [rec.sourcePath]: rec.source.slice() });
        worker.postMessage({ type: 'run', id: 3, request: p.value, shareTree: rec.shareTree, inputs: inputs.value } satisfies WorkerRequest, [...p.transfer, ...inputs.transfer]);
        await done;
        const last = msgs[msgs.length - 1];
        expect(last.type === 'result' && last.result.ok).toBe(true);
        if (last.type !== 'result' || !last.result.ok) return;
        expect(last.result.collected.map((f) => f.path)).toEqual(rec.media.map((m) => m.pandocPath));
        expect(last.result.collected.map((f) => sha(f.bytes))).toEqual(rec.media.map((m) => m.sha256));
      } finally {
        await worker.terminate();
      }
    });

    it('--extract-media writes under wasm (the main risk to P1): images-docx collects its PNG and JPEG', async () => {
      const rec = loadImportRecording('images-docx');
      const r = await execute(rec.request, rec.shareTree, { module, inputs: { [rec.sourcePath]: rec.source.slice() } });
      expect(r.ok).toBe(true);
      if (!r.ok) return;
      expect(r.collected.length).toBeGreaterThan(0);
      expect(r.collected.map((f) => f.path.split('.').pop()).sort()).toEqual(['jpg', 'png']);
    });
  });

  // ---- small synthetic jobs ---------------------------------------------------
  const SHARE = '/__q2_share__';
  const TREE = `${SHARE}/pandoc-share`;
  const job = (over: Partial<PandocRequest> & { md?: string; version?: string; filter?: string; extraArgs?: string[]; out?: string }) => {
    const out = over.out ?? `${SHARE}/out.txt`;
    const files = [{ path: `${SHARE}/in.md`, bytes: enc.encode(over.md ?? 'Hello\n') }];
    const request: PandocRequest = {
      schema_version: 1,
      kind: 'pandoc',
      job_id: '0'.repeat(16),
      writer: 'plain',
      argv: ['pandoc', '-f', 'markdown', '-t', 'plain', '-L', `${TREE}/probe.lua`, ...(over.extraArgs ?? []), '-o', out, `${SHARE}/in.md`],
      env: { SOURCE_DATE_EPOCH: SDE, ...(over.env ?? {}) },
      files,
      dirs: ['/tmp'],
      resource_refs: [],
      share_root: CONSTANTS.share_root,
      share_tree_path: TREE,
      doc_dir: '/proj',
      project_root: '/proj',
      output_path: out,
      stage_name: 'pandoc-write',
      json_path: `${SHARE}/in.md`,
      post: 'none',
      expected_pandoc_wasm_sha256: CONSTANTS.wasm_sha256,
      share_tree_version: over.version ?? 'v1',
      typst_available_fonts: null,
      ...Object.fromEntries(Object.entries(over).filter(([k]) => !['md', 'version', 'filter', 'extraArgs', 'out', 'env'].includes(k))),
    } as PandocRequest;
    const shareTree: ShareTree = {
      share_tree_version: over.version ?? 'v1',
      files: [{ path: 'probe.lua', bytes: enc.encode(over.filter ?? 'function Pandoc(d) return d end') }],
    };
    return { request, shareTree };
  };
  // Replaces the document with one paragraph: the filter's report.
  const report = (expr: string) => `function Pandoc(d) return pandoc.Pandoc({pandoc.Para({pandoc.Str(tostring(${expr}))})}) end`;
  const text = (r: Awaited<ReturnType<typeof execute>>) => {
    if (!r.ok) throw new Error(`${r.kind}: ${r.stderr} ${JSON.stringify(r.diagnostics)}`);
    return dec.decode(r.output).trim();
  };

  it('a successful run is exit 0 with the output present', async () => {
    const { request, shareTree } = job({});
    const r = await execute(request, shareTree, { module });
    expect(text(r)).toBe('Hello');
    expect(r.ok && r.status).toBe(0);
  });

  it('a filter sees the request env (QUARTO_FILTER_PARAMS)', async () => {
    const { request, shareTree } = job({ env: { QUARTO_FILTER_PARAMS: 'eyJhIjoxfQ==' }, filter: report("os.getenv('QUARTO_FILTER_PARAMS')") });
    expect(text(await execute(request, shareTree, { module }))).toBe('eyJhIjoxfQ==');
  });

  it('SOURCE_DATE_EPOCH reaches the filter, and LANG is not invented', async () => {
    const { request, shareTree } = job({ filter: report("tostring(os.getenv('SOURCE_DATE_EPOCH')) .. '/' .. tostring(os.getenv('LANG'))") });
    expect(text(await execute(request, shareTree, { module }))).toBe(`${SDE}/nil`);
  });

  it('/tmp is mounted only because dirs names it', async () => {
    const probe = report("io.open('/tmp/q2-probe', 'w') ~= nil");
    const withTmp = job({ filter: probe });
    expect(text(await execute(withTmp.request, withTmp.shareTree, { module }))).toBe('true');
    const noTmp = job({ filter: probe, dirs: [] });
    expect(text(await execute(noTmp.request, noTmp.shareTree, { module }))).toBe('false');
  });

  it('the guest filesystem holds only the request: no host passthrough', async () => {
    const { request, shareTree } = job({ filter: report("io.open('/etc/passwd', 'r') ~= nil or io.open('" + WASM_PATH.replaceAll('\\', '/') + "', 'r') ~= nil") });
    expect(text(await execute(request, shareTree, { module }))).toBe('false');
  });

  it('parent directories of files are created implicitly', async () => {
    const { request, shareTree } = job({ filter: report("io.open('/proj/deep/er/f.txt', 'r'):read('a')") });
    request.resource_refs.push({ path: '/proj/deep/er/f.txt', bytes: enc.encode('nested') });
    request.dirs = ['/tmp'];
    expect(text(await execute(request, shareTree, { module }))).toBe('nested');
  });

  it('a changed share_tree_version remounts the share tree', async () => {
    const v1 = job({ version: 'v1', filter: report("'one'") });
    const v2 = job({ version: 'v2', filter: report("'two'") });
    expect(text(await execute(v1.request, v1.shareTree, { module }))).toBe('one');
    expect(text(await execute(v2.request, v2.shareTree, { module }))).toBe('two');
    // A request and tree that disagree on the version never run.
    const bad = await execute(v1.request, v2.shareTree, { module });
    expect(bad.ok).toBe(false);
    expect(!bad.ok && bad.kind).toBe('invalid-request');
  });

  it('keeps an unterminated last line of stderr', async () => {
    const { request, shareTree } = job({ filter: "io.stderr:write('no-newline-tail')\nfunction Pandoc(d) return d end" });
    const r = await execute(request, shareTree, { module });
    expect(r.ok).toBe(true);
    expect(r.stderr.endsWith('no-newline-tail')).toBe(true);
  });

  it('writes nothing to the console during a run ({debug: false})', async () => {
    const spies = (['log', 'warn', 'error', 'info', 'debug'] as const).map((m) => vi.spyOn(console, m));
    try {
      const { request, shareTree } = job({});
      await execute(request, shareTree, { module });
      for (const s of spies) expect(s).not.toHaveBeenCalled();
    } finally {
      spies.forEach((s) => s.mockRestore());
    }
  });

  it('a non-ASCII document name and a long non-ASCII argument work', async () => {
    const out = `${SHARE}/数据 résumé.txt`;
    const { request, shareTree } = job({ out, extraArgs: ['--metadata', 'title=' + '数据'.repeat(300) + '-résumé'], md: 'ünï\n' });
    expect(text(await execute(request, shareTree, { module }))).toBe('ünï');
  });

  it('exits non-zero with the stderr intact when pandoc fails', async () => {
    const { request, shareTree } = job({ filter: "error('boom from filter')" });
    const r = await execute(request, shareTree, { module });
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.kind).toBe('pandoc-exit');
      expect(r.status).toBe(83);
      expect(r.stderr).toContain('boom from filter');
    }
  });

  it('reports exit 0 with no output file as no-output', async () => {
    const { request, shareTree } = job({});
    request.output_path = `${SHARE}/never-written.txt`;
    const r = await execute(request, shareTree, { module });
    expect(!r.ok && r.kind).toBe('no-output');
  });

  it('has the readers document import needs (epic I1)', async () => {
    // --list-input-formats prints to stdout and writes no output file, so the run
    // ends as `no-output`; the list is read from the failure's stdout.
    const { request, shareTree } = job({});
    request.argv = ['pandoc', '--list-input-formats'];
    const r = await execute(request, shareTree, { module });
    expect(r.ok).toBe(false);
    if (r.ok) return;
    expect(r.kind).toBe('no-output');
    const formats = r.stdout.split('\n').map((l) => l.trim()).filter(Boolean);
    for (const f of ['docx', 'odt', 'rtf', 'epub', 'pptx']) expect(formats, `reader ${f}`).toContain(f);
  });

  it('rejects an invalid request without running it', async () => {
    const { request, shareTree } = job({});
    request.resource_refs.push({ path: `${TREE}/probe.lua`, bytes: enc.encode('shadow') });
    const r = await execute(request, shareTree, { module });
    expect(!r.ok && r.kind).toBe('invalid-request');
    expect(!r.ok && r.stats).toBeUndefined();
  });

  // ---- fault injection --------------------------------------------------------
  describe('typed fault injection', () => {
    it('oom: +RTS -M5m exits 251', async () => {
      const rec = loadRecording('callouts-docx');
      const r = await execute(rec.request, rec.shareTree, { module, fault: { kind: 'oom', limit: '5m' } });
      expect(r.ok).toBe(false);
      if (!r.ok) {
        expect(r.kind).toBe('oom');
        expect(r.status).toBe(251);
        expect(r.diagnostics.map((d) => 'code' in d && d.code)).toContain('pandoc-oom');
      }
    });
    it('crash: a throwing import is a crash with no stderr', async () => {
      const { request, shareTree } = job({});
      const r = await execute(request, shareTree, { module, fault: { kind: 'crash', message: 'injected' } });
      expect(r.ok).toBe(false);
      if (!r.ok) {
        expect(r.kind).toBe('crash');
        expect(r.status).toBeNull();
        expect(r.stderr).toBe('');
        expect(JSON.stringify(r.diagnostics)).toContain('injected');
      }
    });
    it('hang: a never-returning import is stopped by terminating the worker; the Module survives', async () => {
      const worker = new Worker(new URL('../test-utils/pandocHostThread.mjs', import.meta.url));
      const next = (pred: (m: WorkerResponse) => boolean) =>
        new Promise<WorkerResponse>((resolve) => {
          const on = (m: WorkerResponse) => {
            if (pred(m)) {
              worker.off('message', on);
              resolve(m);
            }
          };
          worker.on('message', on);
        });
      const send = (m: WorkerRequest, transfer: ArrayBuffer[] = []) => worker.postMessage(m, transfer);
      try {
        const ready = next((m) => m.type === 'ready');
        send({ type: 'init', module });
        await ready;
        const { request, shareTree } = job({});
        const p = prepareForPost(request);
        const result = Promise.race([
          next((m) => m.type === 'result').then(() => 'finished'),
          new Promise((r) => setTimeout(() => r('timeout'), 1500)),
        ]);
        send({ type: 'run', id: 1, request: p.value, shareTree, fault: { kind: 'hang' } }, p.transfer);
        expect(await result).toBe('timeout');
      } finally {
        await worker.terminate();
      }
      // The compiled module is untouched: a fresh run still works.
      const { request, shareTree } = job({});
      expect(text(await execute(request, shareTree, { module }))).toBe('Hello');
    }, 30_000);
  });

  // ---- worker protocol ----------------------------------------------------------
  it('a worker thread runs a request through the message protocol', async () => {
    const worker = new Worker(new URL('../test-utils/pandocHostThread.mjs', import.meta.url));
    try {
      const msgs: WorkerResponse[] = [];
      const done = new Promise<void>((resolve) =>
        worker.on('message', (m: WorkerResponse) => {
          msgs.push(m);
          if (m.type === 'result') resolve();
        }),
      );
      worker.postMessage({ type: 'init', module } satisfies WorkerRequest);
      const { request, shareTree } = job({});
      const p = prepareForPost(request);
      worker.postMessage({ type: 'run', id: 7, request: p.value, shareTree } satisfies WorkerRequest, p.transfer);
      await done;
      expect(msgs.map((m) => m.type)).toEqual(['ready', 'progress', 'progress', 'result']);
      const last = msgs[3];
      expect(last.type === 'result' && last.id === 7 && last.result.ok).toBe(true);
    } finally {
      await worker.terminate();
    }
  });
});
