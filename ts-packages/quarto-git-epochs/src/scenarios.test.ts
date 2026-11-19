/**
 * Runs every scenario step list in fixtures/scenarios against a real temp repo with the real git, and
 * compares the classifier's verdict after each step. The simulated peer treats a `write` step as the
 * author's editor edit (synced at once); git steps change the disk without syncing. After each git step the
 * peer decides, then re-baselines. Set SCENARIO_TRACE=1 to print every step's verdict.
 */
import { execFileSync } from 'node:child_process';
import { readdirSync, rmSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { classify, type ChangedFile } from './classify.js';
import { FIXTURES, readJson } from './fixture-io.js';
import { contentKey, createGitProbe } from './probe.js';
import type { ReflogMark } from './probe-types.js';
import { cleanupTempDirs, put, tempDir } from './repo-helper.js';

afterAll(cleanupTempDirs);

const files = readdirSync(join(FIXTURES, 'scenarios')).filter((f) => f.endsWith('.json'));
const trace = process.env.SCENARIO_TRACE === '1';

async function runScenario(s: any): Promise<{ step: string; verdict: string }[]> {
  const dir = tempDir('ge-scenario-');
  const probe = createGitProbe({ cwd: dir });
  const synced = new Map<string, string>();
  let lastBlobs = new Map<string, string>();
  let lastHead: string | null = null;
  let mark: ReflogMark | null = null;
  const results: { step: string; verdict: string }[] = [];

  for (const step of s.steps) {
    const label = JSON.stringify(step.git ?? step.write?.path ?? step.remove ?? step.writeBinary?.path);
    if (step.write) {
      put(dir, step.write.path, step.write.content);
      synced.set(step.write.path, contentKey(Buffer.from(step.write.content)));
      continue;
    }
    if (step.writeBinary) {
      const bytes = Buffer.from(step.writeBinary.hex, 'hex');
      put(dir, step.writeBinary.path, bytes);
      synced.set(step.writeBinary.path, contentKey(bytes));
      continue;
    }
    if (step.remove) {
      rmSync(join(dir, step.remove));
      synced.delete(step.remove);
      continue;
    }
    // A git step.
    let failed = false;
    try {
      execFileSync('git', step.git, { cwd: dir, stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (e) {
      failed = true;
      if (!step.expectFail) throw e;
    }
    if (step.expectFail) expect(failed, `${label} was expected to fail`).toBe(true);

    await probe.ready();
    let verdict: string;
    const head = await probe.headSha();
    if (head === null) {
      verdict = 'none';
    } else if (probe.cheapState().markers.length > 0) {
      verdict = 'none'; // the gate is closed: no decision yet
    } else {
      const slice = await probe.reflogSince(mark);
      if (head === lastHead && (slice?.subjects.length ?? 0) === 0) {
        verdict = 'none';
      } else {
        const tracked = await probe.lsTree();
        const blobBytes = await probe.catBlobs(tracked.map((t) => t.blob));
        const changed: ChangedFile[] = [];
        const nextBlobs = new Map<string, string>();
        const nextSynced = new Map<string, string>();
        for (const t of tracked) {
          nextBlobs.set(t.path, t.blob);
          let disk = '';
          try {
            disk = contentKey(readFileSync(join(dir, t.path)));
          } catch {
            /* removed from disk */
          }
          nextSynced.set(t.path, disk);
          if (t.blob !== lastBlobs.get(t.path) || disk !== (synced.get(t.path) ?? '')) {
            changed.push({ path: t.path, disk, synced: synced.get(t.path) ?? '', blob: contentKey(blobBytes.get(t.blob)!) });
          }
        }
        // A file that left the tree (a checkout removed it) is a change too: its disk is gone, its doc is not.
        for (const [p, oldBlob] of lastBlobs) {
          if (nextBlobs.has(p) || oldBlob === undefined) continue;
          let disk = '';
          try {
            disk = contentKey(readFileSync(join(dir, p)));
          } catch {
            /* gone */
          }
          if (disk !== (synced.get(p) ?? '')) changed.push({ path: p, disk, synced: synced.get(p) ?? '', blob: '' });
          synced.delete(p);
        }
        verdict = classify({ peer: 'extension', reflog: slice ? slice.subjects : null, changed });
        lastBlobs = nextBlobs;
        for (const [p, k] of nextSynced) synced.set(p, k);
        if (slice?.tail) mark = slice.tail;
        lastHead = head;
        if (trace) console.log(`  ${s.id}: ${label} -> ${verdict} [${slice?.subjects.join(' | ')}] changed=${changed.map((c) => c.path)}`);
      }
      if (head !== lastHead) lastHead = head;
    }
    if (trace && verdict === 'none') console.log(`  ${s.id}: ${label} -> none`);
    results.push({ step: label, verdict });
    if (step.expect) expect(verdict, `${s.id}: after ${label}`).toBe(step.expect.verdict);
  }
  return results;
}

describe('scenario step lists against real temp repos', () => {
  it('finds the scenarios', () => expect(files.length).toBeGreaterThanOrEqual(11));
  for (const f of files) {
    const s = readJson(`scenarios/${f}`);
    it(`${s.id}: ${s.title}`, async () => {
      const results = await runScenario(s);
      const expected = s.steps.filter((x: any) => x.expect).length;
      expect(results.length).toBeGreaterThanOrEqual(expected);
    }, 60_000);
  }
});
