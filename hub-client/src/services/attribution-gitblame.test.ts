/**
 * Unit tests for the git-blame AttributionSource adapter.
 *
 * Two kinds of tests in one file:
 *   1. Synthetic — pure JS inputs, deterministic, no dependencies.
 *   2. End-to-end — drives a real temp git repo and `git blame --porcelain`
 *      through the extracted adapter. Requires `git` on PATH.
 *
 * @vitest-environment node
 */
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { devNull, tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  parseBlamePorcelain,
  buildBlameRuns,
  makeGitBlameSource,
  blameSourceFromPorcelain,
  type BlameLine,
  type BlameRun,
} from './attribution-gitblame';
import { getNodeAttribution, type AttributionSource, type NodeAttribution } from './attribution';
import type { ActorIdentity } from './automergeSync';

// ===========================================================================
// Synthetic tests — do not require git to be installed
// ===========================================================================

describe('parseBlamePorcelain', () => {
  it('parses a single-commit single-line blame', () => {
    const porcelain =
      'abcdef0123456789abcdef0123456789abcdef01 1 1 1\n' +
      'author Alice\n' +
      'author-mail <alice@example.com>\n' +
      'author-time 1700000000\n' +
      'author-tz +0000\n' +
      'committer Alice\n' +
      'committer-mail <alice@example.com>\n' +
      'committer-time 1700000000\n' +
      'committer-tz +0000\n' +
      'summary initial\n' +
      'boundary\n' +
      'filename doc.qmd\n' +
      '\thello\n';
    expect(parseBlamePorcelain(porcelain)).toEqual<BlameLine[]>([
      { author: 'Alice', authorMail: 'alice@example.com', authorTime: 1700000000 },
    ]);
  });

  it('caches commit metadata across lines from the same commit', () => {
    const h = '0000000000000000000000000000000000000001';
    const porcelain =
      `${h} 1 1 2\n` +
      'author Alice\n' +
      'author-mail <alice@example.com>\n' +
      'author-time 1700000000\n' +
      'author-tz +0000\n' +
      'summary a\n' +
      '\tfirst\n' +
      `${h} 2 2\n` +
      '\tsecond\n';
    const result = parseBlamePorcelain(porcelain);
    expect(result).toHaveLength(2);
    expect(result[0]).toEqual({ author: 'Alice', authorMail: 'alice@example.com', authorTime: 1700000000 });
    expect(result[1]).toEqual({ author: 'Alice', authorMail: 'alice@example.com', authorTime: 1700000000 });
  });

  it('returns an empty list on empty input', () => {
    expect(parseBlamePorcelain('')).toEqual([]);
  });
});

describe('buildBlameRuns', () => {
  it('produces contiguous byte ranges matching the input text', () => {
    const blame: BlameLine[] = [
      { author: 'A', authorMail: 'a@x', authorTime: 1 },
      { author: 'B', authorMail: 'b@x', authorTime: 2 },
    ];
    const text = 'hello\nworld\n';
    expect(buildBlameRuns(blame, text)).toEqual<BlameRun[]>([
      { byteStart: 0, byteEnd: 6, actor: 'a@x', time: 1 },
      { byteStart: 6, byteEnd: 12, actor: 'b@x', time: 2 },
    ]);
  });

  it('handles multi-byte UTF-8 (CJK)', () => {
    // "世界\n" is 3+3+1 = 7 bytes
    const blame: BlameLine[] = [{ author: 'A', authorMail: 'a@x', authorTime: 1 }];
    expect(buildBlameRuns(blame, '世界\n')).toEqual<BlameRun[]>([
      { byteStart: 0, byteEnd: 7, actor: 'a@x', time: 1 },
    ]);
  });

  it('handles text without a trailing newline', () => {
    const blame: BlameLine[] = [
      { author: 'A', authorMail: 'a@x', authorTime: 1 },
      { author: 'B', authorMail: 'b@x', authorTime: 2 },
    ];
    expect(buildBlameRuns(blame, 'foo\nbar')).toEqual<BlameRun[]>([
      { byteStart: 0, byteEnd: 4, actor: 'a@x', time: 1 },
      { byteStart: 4, byteEnd: 7, actor: 'b@x', time: 2 },
    ]);
  });

  it('throws if blame and text line counts disagree', () => {
    expect(() => buildBlameRuns([], 'hello\n')).toThrow(/mismatch/);
  });
});

describe('makeGitBlameSource', () => {
  const runs: BlameRun[] = [
    { byteStart: 0, byteEnd: 10, actor: 'a', time: 1 },
    { byteStart: 10, byteEnd: 20, actor: 'b', time: 2 },
    { byteStart: 20, byteEnd: 30, actor: 'c', time: 0 },
  ];

  it('resolves a range inside one run', () => {
    expect(makeGitBlameSource(runs).queryByteRange(0, 2, 8)).toEqual({ actor: 'a', time: 1 });
  });

  it('picks the most recent attribution across spanning runs', () => {
    expect(makeGitBlameSource(runs).queryByteRange(0, 0, 30)).toEqual({ actor: 'b', time: 2 });
  });

  it('returns null for empty or inverted ranges', () => {
    const s = makeGitBlameSource(runs);
    expect(s.queryByteRange(0, 5, 5)).toBeNull();
    expect(s.queryByteRange(0, 10, 5)).toBeNull();
  });

  it('returns null for out-of-bounds byte offsets', () => {
    expect(makeGitBlameSource(runs).queryByteRange(0, 100, 200)).toBeNull();
  });

  it('returns null for non-zero fileId (one source per file)', () => {
    expect(makeGitBlameSource(runs).queryByteRange(1, 0, 10)).toBeNull();
  });

  it('returns null for an empty run list', () => {
    expect(makeGitBlameSource([]).queryByteRange(0, 0, 10)).toBeNull();
  });
});

describe('blameSourceFromPorcelain', () => {
  it('wires parse → buildRuns → source', () => {
    const porcelain =
      'abcdef0123456789abcdef0123456789abcdef01 1 1 1\n' +
      'author Alice\n' +
      'author-mail <alice@example.com>\n' +
      'author-time 1700000000\n' +
      '\thello\n';
    const source = blameSourceFromPorcelain(porcelain, 'hello\n');
    expect(source.queryByteRange(0, 0, 5)).toEqual({
      actor: 'alice@example.com',
      time: 1700000000,
    });
  });
});

// ===========================================================================
// End-to-end: real git repo exercises the adapter against real blame output
// ===========================================================================

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

describe('end-to-end: real git blame drives getNodeAttribution', () => {
  const ALICE = { name: 'Alice Author', email: 'alice@example.com' };
  const BOB = { name: 'Bob Contributor', email: 'bob@example.com' };
  const T_ALICE = 1_700_000_000;
  const T_BOB = 1_700_100_000;

  // line1\n   ASCII  bytes [0, 6)
  // 世界\n    CJK    bytes [6,13)
  // line3\n          bytes [13,19)   ← Bob below
  // line4\n          bytes [19,25)
  const ALICE_BLOCK = 'line1\n世界\n';
  const BOB_BLOCK = 'line3\nline4\n';
  const FULL_TEXT = ALICE_BLOCK + BOB_BLOCK;

  let tmpDir: string;
  let source: AttributionSource;

  beforeAll(() => {
    tmpDir = mkdtempSync(join(tmpdir(), 'attribution-gitblame-'));
    const fp = join(tmpDir, 'doc.qmd');

    runGit(tmpDir, ['init', '-q', '-b', 'main']);
    writeFileSync(fp, ALICE_BLOCK);
    runGit(tmpDir, ['add', 'doc.qmd']);
    runGit(tmpDir, ['commit', '-q', '-m', 'alice: initial'], { author: ALICE, time: T_ALICE });
    writeFileSync(fp, FULL_TEXT);
    runGit(tmpDir, ['add', 'doc.qmd']);
    runGit(tmpDir, ['commit', '-q', '-m', 'bob: append'], { author: BOB, time: T_BOB });

    const porcelain = runGit(tmpDir, ['blame', '--porcelain', 'doc.qmd']);
    source = blameSourceFromPorcelain(porcelain, FULL_TEXT);
  });

  afterAll(() => {
    if (tmpDir) rmSync(tmpDir, { recursive: true, force: true });
  });

  const identities: Record<string, ActorIdentity> = {
    [ALICE.email]: { name: ALICE.name, color: '#E91E63' },
    [BOB.email]: { name: BOB.name, color: '#2196F3' },
  };

  const mkReconstructor = (start: number, end: number) =>
    ({ getSourceLocation: (_id: number) => ({ fileId: 0, start, end }) }) as never;

  it('ASCII line attributes to Alice through getNodeAttribution', () => {
    const attr: NodeAttribution | null = getNodeAttribution(
      0, mkReconstructor(0, 6), source, identities,
    );
    expect(attr).toMatchObject({
      actor: ALICE.email, name: ALICE.name, color: '#E91E63', time: T_ALICE,
    });
  });

  it('multi-byte UTF-8 line resolves via the native byte ranges', () => {
    const attr = getNodeAttribution(1, mkReconstructor(6, 13), source, identities);
    expect(attr?.actor).toBe(ALICE.email);
    expect(attr?.time).toBe(T_ALICE);
  });

  it("Bob's line resolves to Bob", () => {
    const attr = getNodeAttribution(2, mkReconstructor(13, 19), source, identities);
    expect(attr).toMatchObject({
      actor: BOB.email, name: BOB.name, color: '#2196F3', time: T_BOB,
    });
  });

  it('range spanning both authors → most recent wins (Bob)', () => {
    const attr = getNodeAttribution(3, mkReconstructor(0, 25), source, identities);
    expect(attr?.actor).toBe(BOB.email);
    expect(attr?.time).toBe(T_BOB);
  });

  it('actor missing from identities falls back to first 8 chars + default color', () => {
    const attr = getNodeAttribution(0, mkReconstructor(0, 6), source, {});
    expect(attr?.name).toBe(ALICE.email.slice(0, 8));
    expect(attr?.color).toBe('#888888');
  });

  it('out-of-range source location returns null', () => {
    expect(
      getNodeAttribution(4, mkReconstructor(999, 1000), source, identities),
    ).toBeNull();
  });
});
