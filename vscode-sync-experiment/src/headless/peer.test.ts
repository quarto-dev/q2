/**
 * Spec 5 (GE-8): peer A publishes three files to the test hub, peer B joins by
 * index id and sees them on its disk, and an edit on A reaches B. The hub is
 * in-process; the headless peers use the real `fs.watch`.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { readText, startHub, tempDir, waitFor } from '../core/test-support';
import { ensureWasm } from '../core/wasm';
import { HeadlessPeer } from './peer';

let hub: TestHub;
const peers: HeadlessPeer[] = [];
const dirs: string[] = [];

beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(async () => {
  await hub.stop();
});
afterEach(async () => {
  for (const p of peers.splice(0)) await p.stop().catch(() => {});
  for (const d of dirs.splice(0)) await fs.rm(d, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

async function newDir(): Promise<string> {
  const d = await tempDir('ge8-peer-');
  dirs.push(d);
  return d;
}

describe('HeadlessPeer', () => {
  it('publishes three files, a second peer joins and sees them, and an edit reaches it', async () => {
    const dirA = await newDir();
    await fs.mkdir(path.join(dirA, 'docs'));
    await fs.writeFile(path.join(dirA, 'index.qmd'), '# Home\n');
    await fs.writeFile(path.join(dirA, 'docs/guide.qmd'), 'guide\n');
    await fs.writeFile(path.join(dirA, 'logo.png'), new Uint8Array([137, 80, 78, 71, 1, 2, 3]));

    const a = await HeadlessPeer.publish(dirA, hub.url);
    peers.push(a);
    // Presence is not delivery: wait until the hub holds A's heads.
    for (const { docId, heads } of a.inventory()) {
      expect(await hub.hubHasHeads(docId, heads, 15_000)).toBe(true);
    }

    const dirB = await newDir();
    const b = await HeadlessPeer.join(hub.url, a.indexDocId, dirB);
    peers.push(b);
    expect(await readText(path.join(dirB, 'index.qmd'))).toBe('# Home\n');
    expect(await readText(path.join(dirB, 'docs/guide.qmd'))).toBe('guide\n');
    expect(new Uint8Array(await fs.readFile(path.join(dirB, 'logo.png')))).toEqual(new Uint8Array([137, 80, 78, 71, 1, 2, 3]));

    await fs.writeFile(path.join(dirA, 'index.qmd'), '# Home, edited on A\n');
    await waitFor(async () => (await readText(path.join(dirB, 'index.qmd'))) === '# Home, edited on A\n', "A's edit on B's disk");

    // And a new file on A.
    await fs.writeFile(path.join(dirA, 'new.qmd'), 'new on A\n');
    await waitFor(async () => (await readText(path.join(dirB, 'new.qmd'))) === 'new on A\n', "A's new file on B's disk");
  });

  it('does not publish ignored paths', async () => {
    const dirA = await newDir();
    await fs.mkdir(path.join(dirA, '.git'));
    await fs.writeFile(path.join(dirA, '.git/config'), 'x');
    await fs.writeFile(path.join(dirA, 'a.qmd'), 'A\n');
    const a = await HeadlessPeer.publish(dirA, hub.url);
    peers.push(a);
    for (const { docId, heads } of a.inventory()) expect(await hub.hubHasHeads(docId, heads, 15_000)).toBe(true);
    const index = await hub.repo.find<{ files: Record<string, string> }>(`automerge:${a.indexDocId}` as never);
    expect(Object.keys(index.doc()!.files)).toEqual(['a.qmd']);
  });
});
