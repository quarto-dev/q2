/**
 * The mirror follower (GE-9, G20, G26): a real `ProjectSync` on fakes follows a
 * headless owner through the in-process test hub. Specs 3 and 4.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import {
  NOTICE_UNAVAILABLE,
  REPLACED_MANY_FILES,
  REPLACED_THIS_FILE,
  REPLACED_ONE_FILE,
  STRANDED_EDITS,
  formatNotice,
} from '@quarto/quarto-git-epochs';
import type { DocumentId } from '@automerge/automerge-repo';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { createProjectFolder } from './create-folder';
import { HeadlessOwner } from '../headless/owner';
import { readProjectMeta } from './meta';
import { ProjectSync, type SyncOptions } from './project-sync';
import { identity, makeFakeHost, readText, startHub, tempDir, waitFor, type FakeHost } from './test-support';
import { ensureWasm } from './wasm';

let hub: TestHub;
let parent: string;
let host: FakeHost;
let owner: HeadlessOwner;
const syncs: ProjectSync[] = [];

beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(async () => {
  await hub.stop();
});
beforeEach(async () => {
  parent = await tempDir('ge9-follow-');
  host = makeFakeHost();
});
afterEach(async () => {
  for (const s of syncs.splice(0)) await s.dispose().catch(() => {});
  await owner?.stop();
  await fs.rm(parent, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

async function mirrorOf(files: Record<string, string>, options: SyncOptions = {}) {
  owner = await HeadlessOwner.create(hub, files);
  const root = await createProjectFolder(owner.indexDocId, parent, host.log, { syncServer: hub.url });
  const meta = await readProjectMeta(root);
  const sync = new ProjectSync(root, meta!, identity, host, options);
  syncs.push(sync);
  await sync.start();
  return { root, sync };
}

const messages = () => host.notifier.messages.map((m) => m.message);
async function hubText(docId: string): Promise<string | undefined> {
  const h = await hub.repo.find<{ text: string }>(`automerge:${docId}` as never);
  return h.doc()?.text;
}
const docIds = (sync: ProjectSync) => sync.inventory().map((i) => i.docId);

describe('mirror follower', () => {
  it('a flip with no buffer open writes the new text to disk and drops the retired doc from the listing', async () => {
    const { root, sync } = await mirrorOf({ 'a.qmd': 'A1\n' });
    const oldDoc = owner.docIdOf('a.qmd')!;
    expect(docIds(sync)).toContain(oldDoc);
    const newDoc = owner.createDocFor('a.qmd', 'A2\n');
    owner.flipPath('a.qmd', newDoc);
    await owner.flush();
    await waitFor(async () => (await readText(path.join(root, 'a.qmd'))) === 'A2\n', 'the new text on disk');
    await waitFor(() => !docIds(sync).includes(oldDoc), 'the retired doc leaving the listing');
    expect(docIds(sync)).toContain(newDoc);
  });

  it('a flip while a dirty buffer is open keeps the typed characters in the old doc, counts them, and converges after done', async () => {
    const { root, sync } = await mirrorOf({ 'a.qmd': 'A1\n' });
    const abs = path.join(root, 'a.qmd');
    host.buffers.open(abs, 'A1\n');
    await waitFor(() => host.buffers.bindings.has(abs), 'the buffer binding');
    const oldDoc = owner.docIdOf('a.qmd')!;
    // The new doc is not on the hub yet, so the swap stays pending while the user types.
    const newDoc = owner.createDocOffline('A2\n');
    owner.flipPath('a.qmd', newDoc);
    await owner.flush();
    await new Promise((r) => setTimeout(r, 200)); // the follower has seen the flip and is pending
    host.buffers.type(abs, 'A1\ntyped');
    host.buffers.type(abs, 'A1\ntyped more');
    await waitFor(async () => (await hubText(oldDoc)) === 'A1\ntyped more', 'the typed text in the old doc');
    expect(host.buffers.texts.get(abs)).toBe('A1\ntyped more'); // the buffer is not touched while pending
    expect(messages()).toEqual([]);

    await owner.publishOffline();
    await waitFor(() => host.buffers.texts.get(abs) === 'A2\n', 'the buffer converging on the new doc');
    expect(await readText(abs)).toBe('A2\n');
    await waitFor(() => messages().length === 1, 'the aggregate notice');
    expect(messages()[0]).toBe(`${REPLACED_THIS_FILE} ${formatNotice(STRANDED_EDITS, { n: 2 })}`);
    expect(await hubText(oldDoc)).toBe('A1\ntyped more'); // the stranded edits stay in the old doc
    expect(docIds(sync)).not.toContain(oldDoc);
  });

  it('two flips before the first swap finishes skip to the final state', async () => {
    const { root } = await mirrorOf({ 'a.qmd': 'A1\n' });
    const first = owner.createDocOffline('A2\n'); // never published: the first swap cannot finish
    owner.flipPath('a.qmd', first);
    await owner.flush();
    await new Promise((r) => setTimeout(r, 100));
    const final = owner.createDocFor('a.qmd', 'A3\n');
    owner.flipPath('a.qmd', final);
    await owner.flush();
    await waitFor(async () => (await readText(path.join(root, 'a.qmd'))) === 'A3\n', 'the final text on disk');
    await new Promise((r) => setTimeout(r, 300));
    expect(await readText(path.join(root, 'a.qmd'))).toBe('A3\n');
    expect(messages()).toEqual([REPLACED_ONE_FILE.replace('{path}', 'a.qmd')]);
  });

  it('an unavailable new doc fires unavailable after the timeout, keeps the old doc, and a retry swaps once the doc arrives', async () => {
    const { root } = await mirrorOf({ 'a.qmd': 'A1\n' }, { swapTimeoutMs: 300 });
    const newDoc = owner.createDocOffline('A2\n');
    owner.flipPath('a.qmd', newDoc);
    await owner.flush();
    await waitFor(() => messages().includes(NOTICE_UNAVAILABLE), 'the unavailable notice');
    expect(await readText(path.join(root, 'a.qmd'))).toBe('A1\n');
    expect(host.notifier.messages.find((m) => m.message === NOTICE_UNAVAILABLE)?.level).toBe('warn');

    await owner.publishOffline();
    // Any later index change that still lists the doc starts a fresh load.
    owner.applyBatch({ additions: [{ path: 'z.qmd', text: 'Z\n' }] });
    await owner.flush();
    await waitFor(async () => (await readText(path.join(root, 'a.qmd'))) === 'A2\n', 'the retried swap');
  });

  it('the owner removing an entry deletes the mirror file, and adding an entry creates it', async () => {
    const { root } = await mirrorOf({ 'a.qmd': 'A1\n', 'b.qmd': 'B1\n' });
    owner.applyBatch({ removals: ['b.qmd'], additions: [{ path: 'c.qmd', text: 'C1\n' }] });
    await owner.flush();
    await waitFor(async () => (await readText(path.join(root, 'b.qmd'))) === null, 'b.qmd deleted');
    await waitFor(async () => (await readText(path.join(root, 'c.qmd'))) === 'C1\n', 'c.qmd created');
    expect(messages()).toEqual([]); // additions and removals are not version switches
  });
});

describe('notices (G26)', () => {
  it('one aggregate message per batch, without the stranded sentence when nothing was stranded', async () => {
    const { root } = await mirrorOf({ 'a.qmd': 'A1\n', 'b.qmd': 'B1\n', 'c.qmd': 'C1\n' });
    owner.applyBatch({
      flips: [
        { path: 'a.qmd', text: 'A2\n' },
        { path: 'b.qmd', text: 'B2\n' },
      ],
    });
    await owner.flush();
    await waitFor(async () => (await readText(path.join(root, 'b.qmd'))) === 'B2\n', 'both flips on disk');
    await waitFor(() => messages().length >= 1, 'the notice');
    await new Promise((r) => setTimeout(r, 200));
    expect(messages()).toEqual([formatNotice(REPLACED_MANY_FILES, { n: 2 })]);
    expect(messages()[0]).not.toContain('stayed with the previous version');
  });
});

describe('moved message, event log, and legacy folders', () => {
  it('sends the moved message on the old binding before disposing it (G25)', async () => {
    const { root } = await mirrorOf({ 'a.qmd': 'A1\n' });
    const abs = path.join(root, 'a.qmd');
    host.buffers.open(abs, 'A1\n');
    await waitFor(() => host.buffers.bindings.has(abs), 'the buffer binding');
    owner.flipPath('a.qmd', owner.createDocFor('a.qmd', 'A2\n'));
    await owner.flush();
    await waitFor(() => host.buffers.texts.get(abs) === 'A2\n', 'the swap');
    expect(host.buffers.bindingEvents).toEqual([`moved:${abs}`, `dispose:${abs}`]);
    expect(host.buffers.bindings.has(abs)).toBe(true); // rebound to the new doc
  });

  it('logs a flipped event with the old and new doc ids', async () => {
    const { root } = await mirrorOf({ 'a.qmd': 'A1\n' });
    const oldDoc = owner.docIdOf('a.qmd')!;
    const newDoc = owner.createDocFor('a.qmd', 'A2\n');
    owner.flipPath('a.qmd', newDoc);
    await owner.flush();
    await waitFor(async () => (await readText(path.join(root, 'a.qmd'))) === 'A2\n', 'the swap');
    const lines = (await readText(path.join(root, '.quarto-hub', 'events.jsonl')))!.split('\n').filter(Boolean).map((l) => JSON.parse(l));
    expect(lines.find((e) => e.event === 'flipped')).toMatchObject({ path: 'a.qmd', source: 'hub', from: oldDoc, to: newDoc });
  });

  it('a folder created before the mode existed starts as a mirror and follows a flip', async () => {
    owner = await HeadlessOwner.create(hub, { 'a.qmd': 'A1\n' });
    const root = await createProjectFolder(owner.indexDocId, parent, host.log, { syncServer: hub.url });
    // Rewrite project.json the way an older extension wrote it: no `mode`.
    const metaPath = path.join(root, '.quarto-hub', 'project.json');
    const old = JSON.parse((await readText(metaPath))!);
    delete old.mode;
    await fs.writeFile(metaPath, JSON.stringify(old));
    const meta = await readProjectMeta(root);
    expect(meta?.mode).toBe('mirror');
    const sync = new ProjectSync(root, meta!, identity, host);
    syncs.push(sync);
    await sync.start();
    expect(sync.mode).toBe('mirror');
    owner.flipPath('a.qmd', owner.createDocFor('a.qmd', 'A2\n'));
    await owner.flush();
    await waitFor(async () => (await readText(path.join(root, 'a.qmd'))) === 'A2\n', 'the flip on disk');
  });

  it('createProjectFolder writes mode: mirror', async () => {
    owner = await HeadlessOwner.create(hub, { 'a.qmd': 'A1\n' });
    const root = await createProjectFolder(owner.indexDocId, parent, host.log, { syncServer: hub.url });
    expect(JSON.parse((await readText(path.join(root, '.quarto-hub', 'project.json')))!).mode).toBe('mirror');
  });
});
