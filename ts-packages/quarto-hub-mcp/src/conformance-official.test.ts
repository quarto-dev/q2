/**
 * The official MCP conformance suite, run against our real server
 * construction through the test-only loopback HTTP listener (Phase 6,
 * bd-8iv9jty5; deferred from Phase 0 while the suite matured past
 * 0.1.x — the pinned 0.2.0-alpha carries the frozen 2026-07-28
 * requirement sets).
 *
 *   conformance server --url <loopback>/mcp --requirements 2026-07-28
 *
 * `--requirements` (not `--spec-version`) runs exactly what the
 * 2026-07-28 revision required at release, frozen in the suite's
 * requirements file — the honest backing for the registry listing's
 * conformance claim (CAP-15). The bar is an EMPTY expected-failures
 * baseline: no `--expected-failures` flag is passed, so any failing
 * check fails this test.
 *
 * What the run exercises: the suite's scenarios drive the `test_*`
 * fixture surface (`conformance-fixtures.ts`, registered only in the
 * loopback) through OUR `createServer`, the SDK's per-request HTTP
 * serving, and our Host/Origin guards — plus the wire-schema checks the
 * harness applies to every message either side sends.
 *
 * The suite spawns its clients as local processes and talks to
 * 127.0.0.1 only — no network beyond the loopback (the devDependency
 * install already happened at `npm install` time).
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { execFile } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';

import { startHttpLoopback, type HttpLoopback } from './http-loopback.js';
import { startInMemoryMcp } from './in-memory-fixture.js';
import { startTestHub, type TestHub } from './test-hub.js';

const require = createRequire(import.meta.url);
const conformanceBin = join(
  require.resolve('@modelcontextprotocol/conformance/package.json'),
  '..',
  'dist',
  'index.js',
);

describe('official MCP conformance suite (2026-07-28 requirements)', () => {
  let hub: TestHub;
  let loopback: HttpLoopback;
  let stdout: string;
  let stderr: string;
  let exitCode: number;
  let logFile: string;

  beforeAll(async () => {
    hub = await startTestHub();
    loopback = await startHttpLoopback({ serverUrl: hub.url });
    const resultsDir = mkdtempSync(join(tmpdir(), 'mcp-conformance-'));
    exitCode = 0;
    stdout = '';
    stderr = '';
    try {
      // ASYNC, on pain of universal scenario timeouts: the loopback
      // server lives on this process's event loop — a synchronous
      // execFileSync would starve it for the entire suite run and every
      // scenario would time out (the manual out-of-process driver never
      // notices the difference).
      const result = await promisify(execFile)(
        process.execPath,
        [
          conformanceBin,
          'server',
          '--url',
          loopback.url,
          '--requirements',
          '2026-07-28',
          // Not-scored scenarios (tasks extension, pending) still run
          // "for visibility" and each burns the full per-scenario
          // timeout against a server that does not implement them.
          // 10 s keeps them bounded without endangering any required
          // scenario (the slowest required waits are ~100 ms).
          '--timeout',
          '10000',
        ],
        {
          cwd: resultsDir,
          encoding: 'utf8',
          timeout: 840_000,
          // The suite's full transcript is large; the default 1 MiB
          // maxBuffer would kill the run mid-way.
          maxBuffer: 64 * 1024 * 1024,
        },
      );
      stdout = result.stdout;
      stderr = result.stderr;
    } catch (err) {
      const e = err as { stdout?: string; stderr?: string; status?: number };
      exitCode = e.status ?? 1;
      stdout = e.stdout ?? '';
      stderr = e.stderr ?? '';
    } finally {
      rmSync(resultsDir, { recursive: true, force: true });
    }
    // Always persist the suite's full output: an 8-minute run's failure
    // detail must not live only in an assertion message.
    logFile = join(tmpdir(), `hub-mcp-conformance-${Date.now()}.log`);
    writeFileSync(
      logFile,
      `exit ${exitCode}\n===== stdout =====\n${stdout}\n===== stderr =====\n${stderr}\n`,
    );
  }, 900_000);

  afterAll(async () => {
    await loopback.close();
    await hub.stop();
  });

  it('passes every required scenario with an empty expected-failures baseline', () => {
    // The suite's exit code IS the scored verdict: not-scored runs
    // (extensions, pending scenarios) are reported but cannot fail it.
    expect(
      exitCode,
      `conformance suite exited ${exitCode} (full log: ${logFile})\n--- stdout tail ---\n${stdout.slice(-4000)}`,
    ).toBe(0);
  });

  it('keeps the test_* fixture surface out of the default listing', async () => {
    // The fixtures exist so the suite can drive OUR server — they are
    // not a product surface. Only the loopback (conformanceFixtures:
    // true) registers them; a default server must list none. (The
    // loopback leg above proves the positive case by passing scenarios
    // that look the names up in tools/list.)
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      const leaked = tools.map((t) => t.name).filter((n) => n.startsWith('test_'));
      expect(leaked).toEqual([]);
    } finally {
      await f.close();
    }
  });

  it('fails only scenarios the requirements set marks not-scored', () => {
    // Belt and braces on the exit code: parse the per-scenario summary.
    // Every `✗` line must name a scenario the suite itself reported as
    // not-scored (extension / pending / added-after-release) — a scored
    // failure would appear here without a roster entry.
    const failed = [...stdout.matchAll(/^✗ (\S+):/gm)].map((m) => m[1]!);
    const notScoredSection = stdout.split('Not scored for 2026-07-28:')[1] ?? '';
    const notScored = new Set(
      [...notScoredSection.matchAll(/^\s+✗ (\S+) \(/gm)].map((m) => m[1]!),
    );
    const scoredFailures = failed.filter((name) => !notScored.has(name));
    expect(
      scoredFailures,
      `scored failures: ${scoredFailures.join(', ') || '(none)'}\n--- stdout tail ---\n${stdout.slice(-3000)}`,
    ).toEqual([]);
  });
});
