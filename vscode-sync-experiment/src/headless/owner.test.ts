/**
 * The headless owner (GE-9): test code that plays the project owner against the
 * test hub by writing `files[path]` changes on the index. A second client must
 * see each operation. GE-12 builds the real owner.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { readText, startHub, tempDir, waitFor } from '../core/test-support';
import { ensureWasm } from '../core/wasm';
import { HeadlessOwner } from './owner';
import { HeadlessPeer } from './peer';

let hub: TestHub;
const owners: HeadlessOwner[] = [];
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
  for (const o of owners.splice(0)) await o.stop().catch(() => {});
  for (const d of dirs.splice(0)) await fs.rm(d, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

async function setup(files: Record<string, string>) {
  const owner = await HeadlessOwner.create(hub, files);
  owners.push(owner);
  const dir = await tempDir('ge9-owner-');
  dirs.push(dir);
  const peer = await HeadlessPeer.join(hub.url, owner.indexDocId, dir);
  peers.push(peer);
  return { owner, peer, dir };
}

describe('HeadlessOwner', () => {
  it('a flip reaches a second client', async () => {
    const { owner, dir } = await setup({ 'a.qmd': 'A1\n' });
    expect(await readText(path.join(dir, 'a.qmd'))).toBe('A1\n');
    const docId = owner.createDocFor('a.qmd', 'A2\n');
    owner.flipPath('a.qmd', docId);
    await owner.flush();
    await waitFor(async () => (await readText(path.join(dir, 'a.qmd'))) === 'A2\n', 'the flipped text on the peer');
  });

  it('removePath and relistPath reach a second client', async () => {
    const { owner, dir } = await setup({ 'a.qmd': 'A1\n', 'b.qmd': 'B1\n' });
    const bDoc = owner.docIdOf('b.qmd')!;
    owner.removePath('b.qmd');
    await owner.flush();
    await waitFor(async () => (await readText(path.join(dir, 'b.qmd'))) === null, 'b.qmd removed on the peer');
    owner.relistPath('b.qmd', bDoc);
    await owner.flush();
    await waitFor(async () => (await readText(path.join(dir, 'b.qmd'))) === 'B1\n', 'b.qmd back on the peer');
  });

  it('applyBatch is one index change: flips, removals, relists, and additions together', async () => {
    const { owner, dir } = await setup({ 'a.qmd': 'A1\n', 'b.qmd': 'B1\n' });
    const before = owner.indexHeads();
    owner.applyBatch({
      flips: [{ path: 'a.qmd', text: 'A2\n' }],
      removals: ['b.qmd'],
      additions: [{ path: 'c.qmd', text: 'C1\n' }],
    });
    const after = owner.indexHeads();
    expect(after).toHaveLength(1);
    expect(after).not.toEqual(before);
    await owner.flush();
    await waitFor(async () => (await readText(path.join(dir, 'a.qmd'))) === 'A2\n', 'flip');
    await waitFor(async () => (await readText(path.join(dir, 'c.qmd'))) === 'C1\n', 'addition');
    await waitFor(async () => (await readText(path.join(dir, 'b.qmd'))) === null, 'removal');
  });
});
