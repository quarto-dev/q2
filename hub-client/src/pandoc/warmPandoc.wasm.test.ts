/**
 * The warm pandoc executor (pandoc-host H10a) against the real thing: Rust `render_pandoc_request` in the
 * built wasm builds the request, `WarmPandoc` runs it on a persistent pandoc.wasm instance, and the result is
 * compared with the fresh `execute()` path. Needs the built wasm pkg and pandoc.wasm (skips locally when
 * either is missing, fails under CI).
 */
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, describe, expect, it } from 'vitest';
import { WarmPandoc, WarmSession, execute, type ExecuteResult, type PandocRequest, type ShareTree } from '@quarto/pandoc-host';
import { WASM_PATH, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { DOC_DIR, matrixDocs, recordedDocs } from '../test-utils/recordingFixtures';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_binary_file: (path: string, content: Uint8Array) => string;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  render_pandoc_request: (path: string, format: string, source_date_epoch?: number, capture?: Uint8Array, typst_available_fonts?: string[]) => Promise<{ error?: string; diagnostics: unknown[]; request?: unknown }>;
  get_pandoc_share_tree: () => ShareTree;
  classify_pandoc_completion: (stage: string, success: boolean, status: string, stderr: string, jsonPath: string) => string;
}

const here = path.dirname(fileURLToPath(import.meta.url));
const PKG_WASM = path.join(here, '../../../crates/wasm-quarto-hub-client/pkg/wasm_quarto_hub_client_bg.wasm');
const CI = !!process.env.CI;
const SDE = 1_700_000_000;
const ready = pandocWasmAvailable() && existsSync(PKG_WASM);

describe('environment', () => {
  it('has pandoc.wasm and the built hub wasm (required in CI)', () => {
    if (!ready && CI) throw new Error(`warm pandoc tests need pandoc.wasm (${WASM_PATH}) and ${PKG_WASM}`);
    if (!ready) console.warn('SKIPPING the warm pandoc tests: pandoc.wasm or the built hub wasm is missing');
  });
});

describe.skipIf(!ready)('WarmPandoc', () => {
  let module: WebAssembly.Module;
  let shareTree: ShareTree;
  let wasm: WasmModule;

  beforeAll(async () => {
    module = await WebAssembly.compile(readFileSync(WASM_PATH));
    wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
    await wasm.default(readFileSync(PKG_WASM));
    shareTree = wasm.get_pandoc_share_tree();
  });

  /** The request the Rust builder makes for a document seeded at `DOC_DIR`. */
  async function build(files: { path: string; bytes: Uint8Array }[], qmd: string, format = 'typst', fonts?: string[]): Promise<PandocRequest> {
    wasm.vfs_clear();
    for (const f of files) wasm.vfs_add_binary_file(`${DOC_DIR}/${f.path}`, f.bytes);
    const envelope = await wasm.render_pandoc_request(`${DOC_DIR}/${qmd}`, format, SDE, undefined, fonts);
    expect(envelope.error, JSON.stringify(envelope.diagnostics)).toBeUndefined();
    expect(envelope.request, JSON.stringify(envelope.diagnostics)).toBeDefined();
    return envelope.request as PandocRequest;
  }

  it('renders the callouts recording byte-equal to a fresh run', async () => {
    const doc = recordedDocs().find((d) => d.name === 'callouts-typst')!;
    const request = await build(doc.files, doc.qmd);
    const fresh = await execute(request, shareTree, { module });
    if (!fresh.ok) throw new Error(`fresh run failed: ${fresh.kind}\n${fresh.stderr}`);
    const warm = await WarmPandoc.create(module);
    const result = await warm.run(request, shareTree);
    if (!result.ok) throw new Error(`warm run failed: ${result.kind}\n${result.stderr}`);
    expect(Buffer.from(result.output).equals(Buffer.from(fresh.output))).toBe(true);
    expect(result.stats).toMatchObject({ warm: true });
    expect(result.stats?.fallback).toBeUndefined();
    expect(warm.memoryBytes()).toBeGreaterThan(0);
  });

  it("does not let the WASI fd table grow with the number of renders (WebKit stopped finding files after about 47 of them)", async () => {
    const doc = recordedDocs().find((d) => d.name === 'callouts-typst')!;
    const request = await build(doc.files, doc.qmd);
    const warm = await WarmPandoc.create(module);
    const lengths: number[] = [];
    for (let i = 0; i < 5; i++) {
      const r = await warm.run(structuredClone(request), shareTree);
      expect(r.ok).toBe(true);
      lengths.push(warm.fdTableLength());
    }
    // The standard streams and the preopened `/`: nothing the guest opened survives a render.
    expect(lengths).toEqual([4, 4, 4, 4, 4]);
  });

  it("rebuilds pandoc's own warnings from /warnings, as _start prints them on stderr", async () => {
    // The typst writer never reads an image, so a missing image warns nowhere (H10a plan 4b assumed it would);
    // an unconvertible TeX command is the warning pandoc's typst writer does raise.
    const files = [{ path: 'd.qmd', bytes: new TextEncoder().encode('---\ntitle: T\n---\n\n$$\\unknowncmd{x}$$\n\n![alt](missing.png)\n') }];
    const request = await build(files, 'd.qmd');
    const fresh = await execute(request, shareTree, { module });
    const warm = await (await WarmPandoc.create(module)).run(request, shareTree);
    expect(warm.stderr).toContain('[WARNING] Could not convert TeX math');
    expect(warm.stderr).not.toContain('[INFO]');
    expect(warm.stderr).toBe(fresh.stderr);
    expect(warm.ok && fresh.ok).toBe(true);
  });

  // ---- 4c: classification through the real Rust classifier, and the instance-level preamble tests ----

  const qmd = (body: string) => [{ path: 'd.qmd', bytes: new TextEncoder().encode(`---\ntitle: T\n---\n\n${body}\n`) }];
  /** `pandoc:` (fresh) and `convert:` (warm) name the program in error text. */
  const modProgram = (text: string) => text.replace(/^(pandoc|convert):/gm, 'PROGRAM:');
  const classify = (request: PandocRequest, success: boolean, status: string, stderr: string) =>
    JSON.parse(wasm.classify_pandoc_completion(request.stage_name, success, status, stderr, request.json_path)) as { success: boolean; diagnostics: { code?: string; title?: string }[] };
  const codes = (c: { diagnostics: { code?: string }[] }) => c.diagnostics.map((d) => d.code);

  const warnFixtures: [string, string, string][] = [
    ['an unconvertible TeX command (pandoc [WARNING])', '$$\\unknowncmd{x}$$', 'Q-11-1'],
    ['a callout with a foreign crossref category (shim)', '::: {#thm-x .callout-note}\nA callout wearing a theorem-shaped identifier.\n:::', 'Q-20-6'],
    ['a theorem with a foreign reference type (shim)', '::: {#fig-x .theorem}\nSome theorem-shaped content.\n:::', 'Q-20-7'],
  ];
  for (const [label, body, code] of warnFixtures) {
    it(`warnings from ${label} classify identically to a fresh run's`, async () => {
      const request = await build(qmd(body), 'd.qmd');
      const fresh = await execute(request, shareTree, { module });
      const warm = await (await WarmPandoc.create(module)).run(request, shareTree);
      if (!fresh.ok || !warm.ok) throw new Error(`failed: fresh ${fresh.ok ? 'ok' : fresh.kind} warm ${warm.ok ? 'ok' : warm.kind}\n${fresh.stderr}\n${warm.stderr}`);
      expect(modProgram(warm.stderr)).toBe(modProgram(fresh.stderr));
      const w = classify(request, true, 'exit status: 0', warm.stderr);
      const f = classify(request, true, 'exit status: 0', fresh.stderr);
      expect(w).toEqual(f);
      expect(codes(w), warm.stderr).toContain(code);
    });
  }

  it('a failing run is pandoc-exit with the synthetic status 1 and classifies to Q-20-3, as a fresh failure does', async () => {
    const request = await build(qmd('Hello.'), 'd.qmd');
    // pandoc cannot parse its JSON input.
    const broken: PandocRequest = { ...request, files: request.files.map((f) => (f.path === request.json_path ? { ...f, bytes: new TextEncoder().encode('{not json') } : f)) };
    const fresh = await execute(broken, shareTree, { module });
    const warm = await (await WarmPandoc.create(module)).run(broken, shareTree);
    expect(fresh.ok).toBe(false);
    expect(warm.ok).toBe(false);
    if (fresh.ok || warm.ok) return;
    expect(fresh.kind).toBe('pandoc-exit');
    expect(warm.kind).toBe('pandoc-exit');
    expect(warm.status).toBe(1);
    expect(warm.stderr).toMatch(/ERROR|rror/);
    const w = classify(broken, false, `exit status: ${warm.status}`, warm.stderr);
    const f = classify(broken, false, `exit status: ${fresh.status}`, fresh.stderr);
    expect(codes(w)).toEqual(['Q-20-3']);
    expect(codes(f)).toEqual(['Q-20-3']);
  });

  /** A request that runs one probe filter over a one-paragraph document, with `env` as the request env. */
  function probeRequest(base: PandocRequest, env: Record<string, string>, expr: string, shareRoot: string, extraLua = ''): PandocRequest {
    const dir = base.json_path.slice(0, base.json_path.lastIndexOf('/'));
    const filter = `${extraLua}function Pandoc(d) return pandoc.Pandoc({pandoc.Para{pandoc.Str(${expr})}}, d.meta) end`;
    const doc = '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"x"}]}]}';
    const e = new TextEncoder();
    return {
      ...base,
      env: { ...base.env, ...env },
      argv: ['pandoc', '-f', 'json', '-t', 'plain', '--data-dir', `${shareRoot}/pandoc/datadir`, '-L', `${dir}/probe.lua`, '-o', base.output_path, base.json_path],
      files: [...base.files.filter((f) => f.path !== base.json_path), { path: base.json_path, bytes: e.encode(doc) }, { path: `${dir}/probe.lua`, bytes: e.encode(filter) }],
    };
  }
  const text = (r: { ok: boolean; output?: Uint8Array; stderr: string }) => {
    if (!r.ok) throw new Error(r.stderr);
    return new TextDecoder().decode(r.output).trim();
  };

  it('the env preamble delivers a different env to every render on one warm instance', async () => {
    const base = await build(qmd('Hello.'), 'd.qmd');
    const root = base.share_tree_path;
    const warm = await WarmPandoc.create(module);
    const expr = 'tostring(os.getenv("Q2_PROBE")) .. "|" .. tostring(os.getenv("SOURCE_DATE_EPOCH")) .. "|" .. tostring(os.getenv("QUARTO_PROJECT_DIR"))';
    const got: string[] = [];
    for (const [p, sde] of [['one', '111'], ['two', '222'], ['one', '111']]) {
      got.push(text(await warm.run(probeRequest(base, { Q2_PROBE: p, SOURCE_DATE_EPOCH: sde }, expr, root), shareTree)));
    }
    // A key the request does not carry is nil, as under an empty WASI environment.
    expect(got).toEqual(['one|111|nil', 'two|222|nil', 'one|111|nil']);
    const fresh = text(await execute(probeRequest(base, { Q2_PROBE: 'one', SOURCE_DATE_EPOCH: '111' }, expr, root), shareTree, { module }));
    expect(fresh).toBe('one|111|nil');
  });

  it('values with quotes, backslashes, newlines, NUL and non-ASCII reach Lua byte for byte', async () => {
    const base = await build(qmd('Hello.'), 'd.qmd');
    const warm = await WarmPandoc.create(module);
    const expr = 'tostring(os.getenv("Q2_PROBE")):gsub(".", function(c) return string.format("%02x", c:byte()) end)';
    const value = 'a"b\\c\nd\0eé\u{1f600}\'';
    const hex = [...new TextEncoder().encode(value)].map((b) => b.toString(16).padStart(2, '0')).join('');
    expect(text(await warm.run(probeRequest(base, { Q2_PROBE: value }, expr, base.share_tree_path), shareTree))).toBe(hex);
  });

  it('a failing init.lua line reports the same line number as a fresh run', async () => {
    const base = await build(qmd('Hello.'), 'd.qmd');
    const broken: ShareTree = {
      ...shareTree,
      files: shareTree.files.map((f) => (f.path === 'pandoc/datadir/init.lua' ? { ...f, bytes: new TextEncoder().encode('-- one\n-- two\nerror("boom")\n') } : f)),
    };
    const fresh = await execute(base, broken, { module });
    const warm = await (await WarmPandoc.create(module)).run(base, broken);
    expect(fresh.ok || warm.ok).toBe(false);
    // pandoc names a string chunk after its first line, so the warm chunk is `[string "do local e=..."]` where the fresh
    // one is `[string "-- one..."]` (the plan expected the name unchanged; the line number is what is preserved).
    const line = (r: { stderr: string }) => /\]:(\d+): boom/.exec(r.stderr)?.[1];
    expect(line(fresh)).toBe('3');
    expect(line(warm)).toBe('3');
    expect(warm.stderr).toContain('[string "do local e=');
  });

  it("a warning written the way Q1's warn() writes it (ANSI-wrapped, on io.stderr) reaches stderr and classifies like a fresh run's", async () => {
    const base = await build(qmd('Hello.'), 'd.qmd');
    const request = probeRequest(base, {}, '"x"', base.share_tree_path, 'io.stderr:write("\\27[33mWARNING (probe.lua:1) probe warning\\n\\27[39m")\n');
    const fresh = await execute(request, shareTree, { module });
    const warm = await (await WarmPandoc.create(module)).run(request, shareTree);
    if (!fresh.ok || !warm.ok) throw new Error(`failed: ${fresh.stderr}\n${warm.stderr}`);
    expect(warm.stderr).toContain('WARNING (probe.lua:1) probe warning');
    expect(modProgram(warm.stderr)).toBe(modProgram(fresh.stderr));
    const w = classify(request, true, 'exit status: 0', warm.stderr);
    expect(w).toEqual(classify(request, true, 'exit status: 0', fresh.stderr));
    expect(codes(w)).toEqual(['Q-11-1']);
  });

  // ---- Task 7: warm versus fresh over everything the Rust builder can emit ----

  /** A small seeded PRNG (mulberry32), so the shuffled order is the same on every run. */
  const prng = (seed: number) => () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const shuffled = <T,>(xs: T[], rand: () => number) => {
    const a = xs.slice();
    for (let i = a.length - 1; i > 0; i--) {
      const j = Math.floor(rand() * (i + 1));
      [a[i], a[j]] = [a[j], a[i]];
    }
    return a;
  };
  const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

  it('is byte-equal to a fresh run, with the same classification and zero fallbacks, over the recordings, the argv matrix and the warning fixtures in a shuffled order', async () => {
    const corpus: { name: string; request: PandocRequest }[] = [];
    for (const d of recordedDocs().filter((d) => d.format === 'typst')) corpus.push({ name: d.name, request: await build(d.files, d.qmd) });
    for (const d of matrixDocs()) {
      corpus.push({ name: `${d.name} (typst)`, request: await build(d.files, 'doc.qmd') });
      corpus.push({ name: `${d.name} (pdf)`, request: await build(d.files, 'doc.qmd', 'pdf', ['Libertinus Serif']) });
    }
    corpus.push({ name: 'warning: unconvertible TeX', request: await build(qmd('$$\\unknowncmd{x}$$'), 'd.qmd') });
    corpus.push({ name: 'warning: missing image', request: await build(qmd('![alt](missing.png)'), 'd.qmd') });
    expect(corpus.length).toBeGreaterThanOrEqual(36);

    // The fresh reference, once per document.
    const fresh = new Map<string, ExecuteResult>();
    for (const c of corpus) fresh.set(c.name, await execute(c.request, shareTree, { module }));
    for (const [name, r] of fresh) if (!r.ok) throw new Error(`fresh ${name}: ${r.kind}\n${r.stderr}`);

    // Two shuffled rounds, then the same document twice in a row.
    const rand = prng(20261003);
    const order = [...shuffled(corpus, rand), ...shuffled(corpus, rand)];
    const twice = corpus[Math.floor(rand() * corpus.length)];
    order.push(twice, twice);

    const session = await WarmSession.create(module);
    let fallbacks = 0;
    const warmMs: number[] = [];
    const freshMs: number[] = [];
    for (const c of order) {
      const f = fresh.get(c.name)!;
      const w = await session.run(c.request, shareTree);
      if (!w.ok || !f.ok) throw new Error(`${c.name}: warm ${w.ok ? 'ok' : `${w.kind}\n${w.stderr}`}`);
      expect(Buffer.from(w.output).equals(Buffer.from(f.output)), `${c.name}: output bytes differ`).toBe(true);
      expect(modProgram(w.stderr), `${c.name}: stderr differs`).toBe(modProgram(f.stderr));
      const cw = classify(c.request, true, 'exit status: 0', w.stderr);
      const cf = classify(c.request, true, 'exit status: 0', f.stderr);
      expect(cw, `${c.name}: classification differs`).toEqual(cf);
      if (w.stats?.fallback) fallbacks++;
      warmMs.push((w.stats?.mountMs ?? 0) + (w.stats?.runMs ?? 0));
      freshMs.push((f.stats?.mountMs ?? 0) + (f.stats?.instanceMs ?? 0) + (f.stats?.runMs ?? 0));
    }
    expect(fallbacks, 'every request must take the warm path').toBe(0);
    expect(session.created).toBe(1);
    // The warm medians for the record (this is a test process on a busy machine: read as magnitudes).
    console.log(`PARITY ${order.length} warm runs over ${corpus.length} documents: warm median ${median(warmMs)} ms (mount + run), fresh median ${median(freshMs)} ms (mount + instantiate + run)`);
  });
});
