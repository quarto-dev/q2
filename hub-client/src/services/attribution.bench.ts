/**
 * Attribution performance baseline (Phase A).
 *
 * Synthetic microbenchmarks that isolate the attribution service from
 * Automerge's internals (mocked `diff()` + `decodeHeads()`). Workloads
 * span the relevant axes: document size, history length, edit pattern.
 * Output pins down which deferred follow-up (RLE, block-max, typed
 * arrays, incremental byteToCharMap) actually moves the needle before
 * we spend effort implementing any of them.
 *
 * Run: cd hub-client && npm run bench
 *
 * Not part of CI. This file uses the `.bench.ts` suffix which is
 * excluded from the unit-test include glob, so it only runs when
 * explicitly targeted.
 *
 * @vitest-environment node
 */
import { describe, it, vi } from 'vitest';

vi.mock('@automerge/automerge', async () => {
  const actual = await vi.importActual<typeof import('@automerge/automerge')>('@automerge/automerge');
  return { ...actual, diff: vi.fn() };
});
vi.mock('@automerge/automerge-repo', async () => {
  const actual = await vi.importActual<typeof import('@automerge/automerge-repo')>('@automerge/automerge-repo');
  return { ...actual, decodeHeads: vi.fn((heads: unknown) => heads) };
});

import { diff } from '@automerge/automerge';
import {
  buildAttributionMap,
  updateAttributionMap,
  makeCharArraySource,
} from './attribution';
import type { AttributionMap, AttributionSource, CharAttribution } from './attribution';
import {
  buildRunListAttribution,
  updateRunListAttribution,
  makeRunListSource,
} from './attribution-runs';
import type { RunListAttribution } from './attribution-runs';

const mockDiff = vi.mocked(diff);

// ---------------------------------------------------------------------------
// workload types + generators
// ---------------------------------------------------------------------------

interface MockPatch {
  action: 'splice' | 'del' | 'put';
  path: [string, number] | [string];
  value?: string;
  length?: number;
}

interface MockHistoryEntry {
  heads: string[];
  actor: string;
  time: number;
  patches: MockPatch[];
}

function createMockHandle(entries: MockHistoryEntry[]) {
  const history = entries.map(e => e.heads);
  const metadataMap = new Map<string, { time: number; actor: string }>();
  for (const e of entries) metadataMap.set(e.heads[0], { time: e.time, actor: e.actor });
  return {
    history: () => history,
    metadata: (hash: string) => metadataMap.get(hash),
    doc: () => ({ text: '' }),
  };
}

function wireMockDiff(entries: MockHistoryEntry[]): void {
  const byHeads = new Map<string, MockPatch[]>();
  for (const e of entries) byHeads.set(JSON.stringify(e.heads), e.patches);
  mockDiff.mockImplementation((_doc, _before, after) =>
    (byHeads.get(JSON.stringify(after)) ?? []) as unknown as ReturnType<typeof diff>,
  );
}

function mkHead(tag: string, i: number): string[] {
  return [`${tag}_h${i}`];
}

function genAppend(N: number, actors: number, tag: string): MockHistoryEntry[] {
  const out: MockHistoryEntry[] = [];
  for (let i = 0; i < N; i++) {
    out.push({
      heads: mkHead(tag, i),
      actor: `a${i % actors}`,
      time: 1_700_000_000_000 + i,
      patches: [{ action: 'splice', path: ['text', i], value: 'x' }],
    });
  }
  return out;
}

function genPrepend(N: number, actors: number, tag: string): MockHistoryEntry[] {
  const out: MockHistoryEntry[] = [];
  for (let i = 0; i < N; i++) {
    out.push({
      heads: mkHead(tag, i),
      actor: `a${i % actors}`,
      time: 1_700_000_000_000 + i,
      patches: [{ action: 'splice', path: ['text', 0], value: 'x' }],
    });
  }
  return out;
}

function genRandom(N: number, actors: number, tag: string, seed = 42): MockHistoryEntry[] {
  let s = seed;
  const rand = () => {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    return s / 0x7fffffff;
  };
  const out: MockHistoryEntry[] = [];
  let docLen = 0;
  for (let i = 0; i < N; i++) {
    const pos = Math.floor(rand() * (docLen + 1));
    out.push({
      heads: mkHead(tag, i),
      actor: `a${i % actors}`,
      time: 1_700_000_000_000 + i,
      patches: [{ action: 'splice', path: ['text', pos], value: 'x' }],
    });
    docLen++;
  }
  return out;
}

function genBulk(N: number, tag: string): MockHistoryEntry[] {
  return [{
    heads: mkHead(tag, 0),
    actor: 'a0',
    time: 1_700_000_000_000,
    patches: [{ action: 'splice', path: ['text', 0], value: 'x'.repeat(N) }],
  }];
}

/**
 * Realistic workload: Automerge batches keystrokes per change, so `B` chars
 * per history entry is closer to production than 1 char/entry. Total doc
 * size = N chars in ceil(N/B) history entries, appending.
 */
function genBatchedAppend(N: number, B: number, actors: number, tag: string): MockHistoryEntry[] {
  const out: MockHistoryEntry[] = [];
  let pos = 0;
  let i = 0;
  while (pos < N) {
    const chunk = Math.min(B, N - pos);
    out.push({
      heads: mkHead(tag, i),
      actor: `a${i % actors}`,
      time: 1_700_000_000_000 + i,
      patches: [{ action: 'splice', path: ['text', pos], value: 'x'.repeat(chunk) }],
    });
    pos += chunk;
    i++;
  }
  return out;
}

// ---------------------------------------------------------------------------
// measurement helpers
// ---------------------------------------------------------------------------

async function timeBuildMs(handle: ReturnType<typeof createMockHandle>, iters: number): Promise<number> {
  await buildAttributionMap(handle as never, 'text');
  const t0 = performance.now();
  for (let i = 0; i < iters; i++) await buildAttributionMap(handle as never, 'text');
  return (performance.now() - t0) / iters;
}

async function timeBuildRunsMs(handle: ReturnType<typeof createMockHandle>, iters: number): Promise<number> {
  await buildRunListAttribution(handle as never, 'text');
  const t0 = performance.now();
  for (let i = 0; i < iters; i++) await buildRunListAttribution(handle as never, 'text');
  return (performance.now() - t0) / iters;
}

function timeUpdateMs(map: AttributionMap, handle: ReturnType<typeof createMockHandle>, iters: number): number {
  updateAttributionMap(map, handle as never, 'text');
  const t0 = performance.now();
  for (let i = 0; i < iters; i++) updateAttributionMap(map, handle as never, 'text');
  return (performance.now() - t0) / iters;
}

function timeUpdateRunsMs(state: RunListAttribution, handle: ReturnType<typeof createMockHandle>, iters: number): number {
  updateRunListAttribution(state, handle as never, 'text');
  const t0 = performance.now();
  for (let i = 0; i < iters; i++) updateRunListAttribution(state, handle as never, 'text');
  return (performance.now() - t0) / iters;
}

function timeQueryNsPerOp(source: AttributionSource, totalBytes: number, rangeSize: number, iters: number): number {
  // Warmup
  for (let i = 0; i < 100; i++) source.queryByteRange(0, 0, rangeSize);
  let s = 0xdeadbeef | 0;
  const t0 = performance.now();
  for (let i = 0; i < iters; i++) {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    const start = s % Math.max(1, totalBytes - rangeSize);
    source.queryByteRange(0, start, start + rangeSize);
  }
  const ms = performance.now() - t0;
  return (ms * 1_000_000) / iters; // ns / op
}

/** Count unique CharAttribution object references — indicates dedup via .fill(ref). */
function countUniqueRefs(entries: CharAttribution[]): number {
  const seen = new Set<CharAttribution>();
  for (const e of entries) seen.add(e);
  return seen.size;
}

function fmt(ms: number): string {
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)} s`;
  if (ms >= 1) return `${ms.toFixed(2)} ms`;
  return `${(ms * 1000).toFixed(1)} µs`;
}

function fmtNs(ns: number): string {
  if (ns >= 1_000_000) return `${(ns / 1_000_000).toFixed(2)} ms`;
  if (ns >= 1000) return `${(ns / 1000).toFixed(2)} µs`;
  return `${ns.toFixed(0)} ns`;
}

function fmtBytes(b: number): string {
  if (b >= 1024 * 1024) return `${(b / (1024 * 1024)).toFixed(1)} MB`;
  if (b >= 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${b} B`;
}

// ---------------------------------------------------------------------------
// the suite
// ---------------------------------------------------------------------------

const BUILD_SIZES = [1_000, 5_000, 20_000, 100_000] as const;
const PATTERNS = [
  { name: 'append',  gen: genAppend },
  { name: 'prepend', gen: genPrepend },
  { name: 'random',  gen: genRandom },
] as const;

function pickIters(N: number, base: number): number {
  if (N >= 100_000) return Math.max(2, Math.floor(base / 10));
  if (N >= 20_000) return Math.max(5, Math.floor(base / 4));
  if (N >= 5_000) return Math.max(10, Math.floor(base / 2));
  return base;
}

describe('attribution Phase A baseline', () => {
  it('build scaling across patterns × sizes (per-char vs RLE)', { timeout: 600_000 }, async () => {
    const cols = BUILD_SIZES;
    const rows: string[][] = [];
    for (const pat of PATTERNS) {
      for (const impl of ['char', 'rle'] as const) {
        const row: string[] = [`${pat.name} · ${impl}`];
        for (const N of cols) {
          if (pat.name === 'random' && N >= 100_000) { row.push('skipped'); continue; }
          const entries = pat.gen(N, 5, `${pat.name}${N}${impl}`);
          const handle = createMockHandle(entries);
          wireMockDiff(entries);
          const ms = impl === 'char'
            ? await timeBuildMs(handle, pickIters(N, 20))
            : await timeBuildRunsMs(handle, pickIters(N, 20));
          row.push(fmt(ms));
        }
        rows.push(row);
      }
    }

    console.log('\n## Build time per cold build (char vs rle)');
    console.log(`| pattern · impl | ${cols.map(n => `N=${n.toLocaleString()}`).join(' | ')} |`);
    console.log(`| --- | ${cols.map(() => '---').join(' | ')} |`);
    for (const r of rows) console.log(`| ${r.join(' | ')} |`);
  });

  it('realistic batched-append — char vs rle', { timeout: 300_000 }, async () => {
    // Each history entry inserts B=20 chars (typical Automerge batch size
    // for continuous typing). Doc grows to N chars in N/B entries.
    const sizes = [1_000, 10_000, 100_000, 1_000_000];
    const B = 20;
    const rowChar: string[] = ['char'];
    const rowRle: string[] = ['rle'];
    for (const N of sizes) {
      const entries = genBatchedAppend(N, B, 5, `batch${N}`);
      const handle = createMockHandle(entries);
      wireMockDiff(entries);
      const iters = N >= 1_000_000 ? 2 : N >= 100_000 ? 5 : N >= 10_000 ? 20 : 100;
      rowChar.push(fmt(await timeBuildMs(handle, iters)));
      rowRle.push(fmt(await timeBuildRunsMs(handle, iters)));
    }
    console.log(`\n## Realistic batched-append build (B=${B} chars per history entry)`);
    console.log(`| impl | ${sizes.map(n => `N=${n.toLocaleString()}`).join(' | ')} |`);
    console.log(`| --- | ${sizes.map(() => '---').join(' | ')} |`);
    console.log(`| ${rowChar.join(' | ')} |`);
    console.log(`| ${rowRle.join(' | ')} |`);
  });

  it('bulk-insert scaling (single big splice) — char vs rle', { timeout: 300_000 }, async () => {
    const sizes = [10_000, 100_000, 1_000_000];
    const rowChar: string[] = ['char'];
    const rowRle: string[] = ['rle'];
    for (const N of sizes) {
      const entriesChar = genBulk(N, `bulkC${N}`);
      const handleChar = createMockHandle(entriesChar);
      wireMockDiff(entriesChar);
      const itersChar = N >= 100_000 ? 20 : 100;
      // Skip 1M for char: would thrash the 10K-chunk splice path.
      rowChar.push(N >= 1_000_000 ? 'slow' : fmt(await timeBuildMs(handleChar, itersChar)));

      const entriesRle = genBulk(N, `bulkR${N}`);
      const handleRle = createMockHandle(entriesRle);
      wireMockDiff(entriesRle);
      const itersRle = N >= 1_000_000 ? 100 : 500;
      rowRle.push(fmt(await timeBuildRunsMs(handleRle, itersRle)));
    }
    console.log('\n## Bulk-insert build — char vs rle');
    console.log(`| impl | ${sizes.map(n => `N=${n.toLocaleString()}`).join(' | ')} |`);
    console.log(`| --- | ${sizes.map(() => '---').join(' | ')} |`);
    console.log(`| ${rowChar.join(' | ')} |`);
    console.log(`| ${rowRle.join(' | ')} |`);
  });

  it('bulk-insert stack-overflow threshold', { timeout: 120_000 }, async () => {
    // Probe `entries.splice(idx, 0, ...newEntries)` in applyPatch to find
    // the largest single-patch size that doesn't blow V8's argument stack.
    // Binary search, since throwing from splice is cheap.
    let lo = 100_000;
    let hi = 1_000_000;
    let lastOk = lo;
    while (lo <= hi) {
      const N = (lo + hi) >> 1;
      const entries = genBulk(N, `probe${N}`);
      const handle = createMockHandle(entries);
      wireMockDiff(entries);
      let ok = true;
      try {
        await buildAttributionMap(handle as never, 'text');
      } catch (e) {
        if (e instanceof RangeError) ok = false;
        else throw e;
      }
      if (ok) { lastOk = N; lo = N + 10_000; }
      else { hi = N - 10_000; }
    }
    console.log(`\n## Bulk-insert limit: applyPatch overflows at N ≈ ${lastOk.toLocaleString()}+1 chars`);
  });

  it('update — one mid-doc patch on batched-append history (realistic)', { timeout: 300_000 }, async () => {
    // Source history: batched-append (B=20 chars/entry = realistic Automerge
    // batching). RLE's runs count is then ~N/B instead of N.
    const sizes = [1_000, 10_000, 100_000];
    const B = 20;
    const rowChar: string[] = ['char'];
    const rowRle: string[] = ['rle'];
    for (const N of sizes) {
      const tag = `upd${N}`;
      const entries = genBatchedAppend(N, B, 5, tag);
      const handle0 = createMockHandle(entries);
      wireMockDiff(entries);
      const map = (await buildAttributionMap(handle0 as never, 'text'))!;
      const runList = (await buildRunListAttribution(handle0 as never, 'text'))!;

      // Append one more mid-doc patch of B chars.
      entries.push({
        heads: mkHead(tag, entries.length),
        actor: 'a0',
        time: 1_700_000_000_000 + entries.length,
        patches: [{ action: 'splice', path: ['text', Math.floor(N / 2)], value: 'Z'.repeat(B) }],
      });
      const handle1 = createMockHandle(entries);
      wireMockDiff(entries);

      const iters = N >= 100_000 ? 200 : 2_000;
      rowChar.push(fmt(timeUpdateMs(map, handle1, iters)));
      rowRle.push(fmt(timeUpdateRunsMs(runList, handle1, iters)));
    }
    console.log(`\n## Update time — one mid-doc patch on batched-append (B=${B}) history`);
    console.log(`| impl | ${sizes.map(n => `N=${n.toLocaleString()}`).join(' | ')} |`);
    console.log(`| --- | ${sizes.map(() => '---').join(' | ')} |`);
    console.log(`| ${rowChar.join(' | ')} |`);
    console.log(`| ${rowRle.join(' | ')} |`);
  });

  it('query throughput on batched-append source — char vs rle', { timeout: 300_000 }, async () => {
    const sizes = [1_000, 10_000, 100_000];
    const ranges = [10, 100, 1_000];
    const B = 20;
    console.log(`\n## queryByteRange time per op (batched-append source, B=${B})`);
    console.log(`| doc N · impl | runs | ${ranges.map(r => `range=${r}`).join(' | ')} |`);
    console.log(`| --- | --- | ${ranges.map(() => '---').join(' | ')} |`);
    for (const N of sizes) {
      const entries = genBatchedAppend(N, B, 5, `q${N}`);
      const handle = createMockHandle(entries);
      wireMockDiff(entries);
      const built = (await buildAttributionMap(handle as never, 'text'))!;
      const runList = (await buildRunListAttribution(handle as never, 'text'))!;
      const byteToCharMap = new Array<number>(N + 1);
      for (let i = 0; i <= N; i++) byteToCharMap[i] = i;
      const charSource = makeCharArraySource(built.entries, byteToCharMap);
      const runSource = makeRunListSource(runList.runs, byteToCharMap);

      for (const [label, source, count] of [
        ['char', charSource, built.entries.length],
        ['rle ', runSource, runList.runs.length],
      ] as const) {
        const row: string[] = [`${N.toLocaleString()} · ${label}`, count.toLocaleString()];
        for (const r of ranges) {
          row.push(fmtNs(timeQueryNsPerOp(source, N, r, 200_000)));
        }
        console.log(`| ${row.join(' | ')} |`);
      }
    }
  });

  it('storage footprint — char entries vs rle runs', { timeout: 300_000 }, async () => {
    console.log('\n## Storage footprint — workload sweep');
    console.log('| workload | N | char entries | rle runs | reduction |');
    console.log('| --- | --- | --- | --- | --- |');
    const workloads: Array<[string, (N: number, tag: string) => MockHistoryEntry[]]> = [
      ['append (1 char/entry, worst)', (N, tag) => genAppend(N, 5, tag)],
      ['batched (B=20, realistic)',    (N, tag) => genBatchedAppend(N, 20, 5, tag)],
      ['bulk (single patch, best)',    (N, tag) => genBulk(N, tag)],
    ];
    for (const [label, gen] of workloads) {
      for (const N of [10_000, 100_000]) {
        const entries = gen(N, `mem_${label}_${N}`);
        const handle = createMockHandle(entries);
        wireMockDiff(entries);
        const built = (await buildAttributionMap(handle as never, 'text'))!;
        const runList = (await buildRunListAttribution(handle as never, 'text'))!;
        const ratio = built.entries.length / runList.runs.length;
        const reduction = ratio < 1.01
          ? '1x'
          : ratio < 10
            ? `${ratio.toFixed(1)}x fewer`
            : `${ratio.toLocaleString()}x fewer`;
        console.log(
          `| ${label} | ${N.toLocaleString()} | ${built.entries.length.toLocaleString()} | ${runList.runs.length.toLocaleString()} | ${reduction} |`,
        );
      }
    }
    // Silence unused helpers — kept for future non-ratio footprint probes.
    void countUniqueRefs;
    void fmtBytes;
  });
});
