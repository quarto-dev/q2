/**
 * Attribution consumer surface — alternate data source (real git blame).
 *
 * Feeds output from a real `git blame --porcelain` run into the same consumer
 * API that the Automerge path feeds (`getNodeAttribution` backed by an
 * `AttributionSource`). A passing run demonstrates that the consumer surface
 * admits producers with native range-based storage — no per-character
 * expansion required.
 *
 * The git-blame adapter here binary-searches a sorted run list instead of
 * flattening line records into a `CharAttribution[]`. Compare commit history
 * of this file: the prior version built a 21-entry char array from 4 blame
 * records; this version keeps them as 4 runs and queries directly in bytes.
 *
 * This file deliberately imports only data-source-agnostic symbols — no
 * `CharAttribution`, `AttributionMap`, `buildAttributionMap`, or any
 * Automerge types. The import list itself is part of the contract.
 *
 * @vitest-environment node
 */
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { devNull, tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  getNodeAttribution,
  type AttributionSource,
  type NodeAttribution,
} from './attribution';
import type { ActorIdentity } from './automergeSync';

// ---------------------------------------------------------------------------
// git driver + porcelain parser
// ---------------------------------------------------------------------------

interface BlameLine {
  author: string;
  authorMail: string;
  authorTime: number;
}

function runGit(
  cwd: string,
  args: string[],
  opts?: { author?: { name: string; email: string }; time?: number },
): string {
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    GIT_CONFIG_NOSYSTEM: '1',
    GIT_CONFIG_GLOBAL: devNull,
  };
  if (opts?.author) {
    env.GIT_AUTHOR_NAME = opts.author.name;
    env.GIT_AUTHOR_EMAIL = opts.author.email;
    env.GIT_COMMITTER_NAME = opts.author.name;
    env.GIT_COMMITTER_EMAIL = opts.author.email;
  }
  if (opts?.time !== undefined) {
    const d = `@${opts.time} +0000`;
    env.GIT_AUTHOR_DATE = d;
    env.GIT_COMMITTER_DATE = d;
  }
  return execFileSync(
    'git',
    ['-C', cwd, '-c', 'commit.gpgsign=false', ...args],
    { env, stdio: ['ignore', 'pipe', 'pipe'] },
  ).toString();
}

/**
 * Parse `git blame --porcelain` into one BlameLine per source line. Commit
 * metadata is emitted only on the first appearance of each commit; subsequent
 * lines from the same commit only carry the header. We cache metadata by
 * commit hash so every record gets fully populated.
 */
function parseBlamePorcelain(output: string): BlameLine[] {
  const records: BlameLine[] = [];
  const cache = new Map<string, BlameLine>();
  let cur: Partial<BlameLine> = {};
  let curHash: string | null = null;

  for (const line of output.split('\n')) {
    const h = line.match(/^([0-9a-f]{40}) \d+ \d+(?: \d+)?$/);
    if (h) {
      curHash = h[1];
      cur = { ...(cache.get(curHash) ?? {}) };
    } else if (line.startsWith('author ')) {
      cur.author = line.slice('author '.length);
    } else if (line.startsWith('author-mail ')) {
      cur.authorMail = line.slice('author-mail '.length).replace(/^<|>$/g, '');
    } else if (line.startsWith('author-time ')) {
      cur.authorTime = parseInt(line.slice('author-time '.length), 10);
    } else if (line.startsWith('\t')) {
      const rec = cur as BlameLine;
      if (curHash && !cache.has(curHash)) cache.set(curHash, rec);
      records.push(rec);
    }
  }
  return records;
}

// ---------------------------------------------------------------------------
// makeBlameSource — run-based AttributionSource (no per-char expansion)
// ---------------------------------------------------------------------------

interface BlameRun {
  byteStart: number; // inclusive
  byteEnd: number;   // exclusive
  actor: string;
  time: number;
}

/**
 * Build one BlameRun per source line, measuring byte lengths via TextEncoder
 * (line content may contain multi-byte UTF-8 — e.g. CJK, emoji).
 */
function buildBlameRuns(blame: BlameLine[], text: string): BlameRun[] {
  const encoder = new TextEncoder();
  const sourceLines = text.split('\n');
  const trailing = text.endsWith('\n');
  const n = trailing ? sourceLines.length - 1 : sourceLines.length;
  if (n !== blame.length) {
    throw new Error(`blame/text line mismatch: ${blame.length} blame vs ${n} text`);
  }

  const runs: BlameRun[] = [];
  let byteOffset = 0;
  for (let i = 0; i < n; i++) {
    const lineBytes = encoder.encode(sourceLines[i]).length;
    const newlineBytes = i < n - 1 || trailing ? 1 : 0;
    const byteEnd = byteOffset + lineBytes + newlineBytes;
    runs.push({
      byteStart: byteOffset,
      byteEnd,
      actor: blame[i].authorMail,
      time: blame[i].authorTime,
    });
    byteOffset = byteEnd;
  }
  return runs;
}

/**
 * AttributionSource that stores byte-ranged runs and answers queries by
 * binary-searching for the first overlapping run, then scanning overlapping
 * runs for the maximum time. O(log R + overlap) per query where R = number
 * of runs — no per-character storage.
 */
function makeBlameSource(runs: BlameRun[]): AttributionSource {
  return {
    queryByteRange(fileId, byteStart, byteEnd) {
      if (fileId !== 0) return null;
      if (byteStart >= byteEnd) return null;
      // Binary search for first run whose byteEnd > byteStart.
      let lo = 0;
      let hi = runs.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (runs[mid].byteEnd <= byteStart) lo = mid + 1;
        else hi = mid;
      }
      let best: { actor: string; time: number } | null = null;
      for (let i = lo; i < runs.length && runs[i].byteStart < byteEnd; i++) {
        if (!best || runs[i].time > best.time) {
          best = { actor: runs[i].actor, time: runs[i].time };
        }
      }
      return best;
    },
  };
}

// ---------------------------------------------------------------------------
// fixture
// ---------------------------------------------------------------------------

const ALICE = { name: 'Alice Author', email: 'alice@example.com' };
const BOB = { name: 'Bob Contributor', email: 'bob@example.com' };
// Bob's timestamp is later so any range spanning both authors resolves to Bob.
const T_ALICE = 1_700_000_000;
const T_BOB = 1_700_100_000;

// line1\n   ASCII   bytes [0, 6)   chars [0, 6)
// 世界\n    CJK     bytes [6,13)   chars [6, 9)   ← multi-byte UTF-8
// --- Alice above, Bob below --------------------------------
// line3\n           bytes [13,19)  chars [9,15)
// line4\n           bytes [19,25)  chars [15,21)
const ALICE_BLOCK = 'line1\n世界\n';
const BOB_BLOCK = 'line3\nline4\n';
const FULL_TEXT = ALICE_BLOCK + BOB_BLOCK;

let tmpDir: string;
let runs: BlameRun[];
let source: AttributionSource;

beforeAll(() => {
  tmpDir = mkdtempSync(join(tmpdir(), 'attribution-gitblame-'));
  const fp = join(tmpDir, 'doc.qmd');

  runGit(tmpDir, ['init', '-q', '-b', 'main']);
  writeFileSync(fp, ALICE_BLOCK);
  runGit(tmpDir, ['add', 'doc.qmd']);
  runGit(tmpDir, ['commit', '-q', '-m', 'alice: initial'], {
    author: ALICE,
    time: T_ALICE,
  });
  writeFileSync(fp, FULL_TEXT);
  runGit(tmpDir, ['add', 'doc.qmd']);
  runGit(tmpDir, ['commit', '-q', '-m', 'bob: append'], {
    author: BOB,
    time: T_BOB,
  });

  const blame = parseBlamePorcelain(
    runGit(tmpDir, ['blame', '--porcelain', 'doc.qmd']),
  );
  runs = buildBlameRuns(blame, FULL_TEXT);
  source = makeBlameSource(runs);
});

afterAll(() => {
  if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
});

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

const identities: Record<string, ActorIdentity> = {
  [ALICE.email]: { name: ALICE.name, color: '#E91E63' },
  [BOB.email]: { name: BOB.name, color: '#2196F3' },
};

const mkReconstructor = (start: number, end: number) =>
  ({ getSourceLocation: (_id: number) => ({ fileId: 0, start, end }) }) as never;

describe('attribution consumer surface accepts git blame as alternate source', () => {
  it('fixture sanity: blame produces 4 byte-ranged runs covering the full file', () => {
    expect(runs).toHaveLength(4);
    expect(runs[0].byteStart).toBe(0);
    expect(runs[runs.length - 1].byteEnd).toBe(25); // 6 + 7 + 6 + 6 bytes
    expect(runs[0].actor).toBe(ALICE.email);
    expect(runs[3].actor).toBe(BOB.email);
  });

  it('ASCII line attributed to Alice resolves through getNodeAttribution', () => {
    const attr: NodeAttribution | null = getNodeAttribution(
      0, mkReconstructor(0, 6), source, identities,
    );
    expect(attr).toMatchObject({
      actor: ALICE.email,
      name: ALICE.name,
      color: '#E91E63',
      time: T_ALICE,
    });
  });

  it('multi-byte UTF-8 line resolves via natively byte-ranged runs', () => {
    // "世界\n" spans bytes [6,13). The run for this line has byteStart=6,
    // byteEnd=13 — computed by TextEncoder, not by per-char expansion.
    const attr = getNodeAttribution(
      1, mkReconstructor(6, 13), source, identities,
    );
    expect(attr?.actor).toBe(ALICE.email);
    expect(attr?.name).toBe(ALICE.name);
    expect(attr?.time).toBe(T_ALICE);
  });

  it("Bob's line resolves to Bob", () => {
    const attr = getNodeAttribution(
      2, mkReconstructor(13, 19), source, identities,
    );
    expect(attr).toMatchObject({
      actor: BOB.email,
      name: BOB.name,
      color: '#2196F3',
      time: T_BOB,
    });
  });

  it('range spanning both authors → most recent wins (Bob)', () => {
    const attr = getNodeAttribution(
      3, mkReconstructor(0, 25), source, identities,
    );
    expect(attr?.actor).toBe(BOB.email);
    expect(attr?.time).toBe(T_BOB);
  });

  it('actor missing from identities falls back to first 8 chars + default color', () => {
    const empty: Record<string, ActorIdentity> = {};
    const attr = getNodeAttribution(
      0, mkReconstructor(0, 6), source, empty,
    );
    expect(attr?.actor).toBe(ALICE.email);
    expect(attr?.name).toBe(ALICE.email.slice(0, 8));
    expect(attr?.color).toBe('#888888');
  });

  it('out-of-range source location returns null', () => {
    const attr = getNodeAttribution(
      4, mkReconstructor(999, 1000), source, identities,
    );
    expect(attr).toBeNull();
  });
});
