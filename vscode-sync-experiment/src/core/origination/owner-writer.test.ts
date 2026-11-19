/**
 * GE-12 spec 7: the owner against real temporary repositories and the test hub. The owner folder is originated with
 * `originate()` and runs a `ProjectSync` in clone mode with a manual clock; a reader repo watches the index the way a
 * follower does and counts its changes.
 */
import * as A from '@automerge/automerge';
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import { asString, viewOf, type Sidecar } from '@quarto/quarto-git-epochs';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { attachFolder } from '../clone/attach-flow';
import { cleanupGitTempDirs, cloneRepo, commitAll, git, put, repoWith } from '../clone/git-fixtures';
import { readProjectMeta } from '../meta';
import { ProjectSync } from '../project-sync';
import { findDoc, webSocketAdapter } from '../repo';
import { ManualClock, identity, makeFakeHost, sleep, startHub, waitFor } from '../test-support';
import { ensureWasm } from '../wasm';
import { originate } from './originate';
import { OwnerPointerWriter } from './owner-writer';
import { selectFiles } from './select';

let hub: TestHub;
beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(() => hub.stop());
const stops: (() => Promise<unknown>)[] = [];
afterEach(async () => {
  for (const s of stops.splice(0)) await s().catch(() => {});
  cleanupGitTempDirs();
});

interface OwnerEnv {
  dir: string;
  sync: ProjectSync;
  clock: ManualClock;
  indexId: string;
  reader: Repo;
  readerIndex: DocHandle<IndexDocument>;
  /** Number of changes on the index as the reader sees them. */
  changes(): number;
  files(): Record<string, string>;
  settle(condition?: () => boolean | Promise<boolean>): Promise<void>;
}

async function ownerEnv(files: Record<string, string | Buffer>, prepare?: (dir: string) => void): Promise<OwnerEnv> {
  const dir = repoWith(files);
  prepare?.(dir);
  const r = await originate({ dir, selection: await selectFiles(dir), server: hub.url, identity });
  if (!r.ok) throw new Error('origination did not finish');
  const clock = new ManualClock();
  const sync = new ProjectSync(dir, (await readProjectMeta(dir))!, identity, makeFakeHost(), { clone: { clock } });
  await sync.start();
  stops.push(() => sync.dispose());
  const reader = new Repo({ network: [webSocketAdapter(hub.url)] });
  stops.push(() => reader.shutdown());
  const readerIndex = await findDoc<IndexDocument>(reader, r.indexDocId);
  const env: OwnerEnv = {
    dir,
    sync,
    clock,
    indexId: r.indexDocId,
    reader,
    readerIndex,
    changes: () => A.getAllChanges(readerIndex.doc()!).length,
    files: () => Object.fromEntries(Object.entries(readerIndex.doc()!.files).map(([k, v]) => [k, asString(v)])),
    async settle(condition) {
      const head = git(dir, 'rev-parse', 'HEAD');
      await waitFor(
        async () => {
          await clock.advance(300);
          await sleep(20);
          const s = sync.cloneState();
          return condition ? condition() : s?.head === head && !s.gateClosed;
        },
        'the owner controller to decide',
        20_000,
      );
    },
  };
  return env;
}

const sideBranch = (dir: string, edit: (dir: string) => void) => {
  git(dir, 'switch', '-q', '-c', 'side');
  edit(dir);
  commitAll(dir, 'side');
  git(dir, 'switch', '-q', 'main');
};

describe('OwnerPointerWriter, alone', () => {
  it('writes nothing until the heads wait finishes, then one change for the whole batch', async () => {
    const repo = new Repo();
    stops.push(() => repo.shutdown());
    const index = repo.create<IndexDocument>({ files: { 'a.qmd': 'old', 'gone.qmd': 'g' }, version: 4, identities: {}, folders: {} } as IndexDocument);
    const fresh = repo.create<{ text: string }>({ text: 'x' });
    let release: (() => void) | undefined;
    const order: string[] = [];
    const w = new OwnerPointerWriter({
      repo,
      index,
      waitForHeads: (handles) => {
        order.push(`wait:${handles.map((h) => h.documentId).join(',')}`);
        return new Promise((res) => (release = () => res({ ok: true, pending: [] })));
      },
    });
    const before = A.getAllChanges(index.doc()!).length;
    const done = w.applyDecision({ flips: [{ path: 'a.qmd', docId: fresh.documentId }], removals: ['gone.qmd'], relists: [], additions: [], resets: [] });
    await sleep(50);
    expect(order).toEqual([`wait:${fresh.documentId}`]);
    expect(A.getAllChanges(index.doc()!).length).toBe(before); // nothing written while waiting
    release!();
    await done;
    expect(A.getAllChanges(index.doc()!).length).toBe(before + 1); // flip and removal in one change
    expect(Object.fromEntries(Object.entries(index.doc()!.files).map(([k, v]) => [k, asString(v)]))).toEqual({ 'a.qmd': fresh.documentId });
  });

  it('a batch that changes nothing writes nothing', async () => {
    const repo = new Repo();
    stops.push(() => repo.shutdown());
    const index = repo.create<IndexDocument>({ files: { 'a.qmd': 'd1' }, version: 4, identities: {}, folders: {} } as IndexDocument);
    const w = new OwnerPointerWriter({ repo, index });
    const before = A.getAllChanges(index.doc()!).length;
    await w.applyDecision({ flips: [{ path: 'a.qmd', docId: 'd1' }], removals: ['absent.qmd'], relists: [], additions: [], resets: [] });
    expect(A.getAllChanges(index.doc()!).length).toBe(before);
  });
});

describe('the owner folder (spec 7)', () => {
  it('a checkout that changes a file re-points it in exactly one new index change, and the new doc is on the server first', async () => {
    const e = await ownerEnv({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' }, (d) => sideBranch(d, (x) => put(x, 'a.qmd', 'A2\n')));
    const before = e.changes();
    const oldDoc = e.files()['a.qmd']!;
    git(e.dir, 'switch', '-q', 'side');
    await e.settle(() => e.files()['a.qmd'] !== oldDoc);
    const newDoc = e.files()['a.qmd']!;
    expect(newDoc).not.toBe(oldDoc);
    expect(e.changes()).toBe(before + 1);
    expect(e.files()['b.qmd']).toBeTruthy(); // untouched
    const h = await findDoc<{ text: string }>(e.reader, newDoc);
    expect(h.doc()!.text).toBe('A2\n');
    expect(await hub.hubHasHeads(newDoc, h.heads() as string[], 5_000)).toBe(true);
    // Switching back points at the registry's original doc again.
    git(e.dir, 'switch', '-q', 'main');
    await e.settle(() => e.files()['a.qmd'] === oldDoc);
  });

  it('removes the entry of a file git removed and re-lists it, at its registry doc, on return, each in one change', async () => {
    const e = await ownerEnv({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' }, (d) => {
      git(d, 'switch', '-q', '-c', 'nob');
      git(d, 'rm', '-q', 'b.qmd');
      commitAll(d, 'rm b');
      git(d, 'switch', '-q', 'main');
    });
    const bDoc = e.files()['b.qmd']!;
    const c0 = e.changes();
    git(e.dir, 'switch', '-q', 'nob');
    await e.settle(() => !('b.qmd' in e.files()));
    expect(e.changes()).toBe(c0 + 1);
    expect(Object.keys(e.files())).toEqual(['a.qmd']);
    git(e.dir, 'switch', '-q', 'main');
    await e.settle(() => 'b.qmd' in e.files());
    expect(e.files()['b.qmd']).toBe(bDoc);
    expect(e.changes()).toBe(c0 + 2);
  });

  it('publishes a file git added: a fresh doc, an index entry, and a registry entry', async () => {
    const e = await ownerEnv({ 'a.qmd': 'A\n' }, (d) => sideBranch(d, (x) => put(x, 'new.qmd', 'NEW\n')));
    git(e.dir, 'switch', '-q', 'side');
    await e.settle(() => 'new.qmd' in e.files());
    const doc = e.files()['new.qmd']!;
    expect((await findDoc<{ text: string }>(e.reader, doc)).doc()!.text).toBe('NEW\n');
    const sidecarId = asString(e.readerIndex.doc()!.epochs);
    const reg = viewOf((await findDoc<Sidecar>(e.reader, sidecarId)).doc()!);
    expect(reg.entries.find((x) => x.path === 'new.qmd')).toMatchObject({ doc, gen: 0, blob: git(e.dir, 'rev-parse', 'HEAD:new.qmd') });
  });

  it('a binary file that changes across a branch flips to a doc with the new content', async () => {
    const e = await ownerEnv({ 'a.qmd': 'A\n', 'p.png': Buffer.from([1, 2, 3]) }, (d) => sideBranch(d, (x) => put(x, 'p.png', Buffer.from([9, 9, 9, 9]))));
    const old = e.files()['p.png']!;
    git(e.dir, 'switch', '-q', 'side');
    await e.settle(() => e.files()['p.png'] !== old);
    const doc = (await findDoc<{ content: Uint8Array; hash: string }>(e.reader, e.files()['p.png']!)).doc()!;
    expect(Array.from(doc.content)).toEqual([9, 9, 9, 9]);
  });

  it('reset --hard after a browser edit makes a new doc from disk, retires the old entry, and re-points in one change', async () => {
    const e = await ownerEnv({ 'a.qmd': 'A\n' });
    const old = e.files()['a.qmd']!;
    const oldHandle = await findDoc<{ text: string }>(e.reader, old);
    oldHandle.change((d) => void (d.text = 'A with a browser edit\n'));
    await waitFor(async () => (await fs.readFile(path.join(e.dir, 'a.qmd'), 'utf8')) === 'A with a browser edit\n', 'the browser edit on the owner disk');
    const c0 = e.changes();
    git(e.dir, 'reset', '-q', '--hard');
    await e.settle(() => e.files()['a.qmd'] !== old);
    expect(e.changes()).toBe(c0 + 1);
    const view = viewOf((await findDoc<Sidecar>(e.reader, asString(e.readerIndex.doc()!.epochs))).doc()!);
    expect(view.retired.map((r) => r.doc)).toContain(old);
    expect((await findDoc<{ text: string }>(e.reader, e.files()['a.qmd']!)).doc()!.text).toBe('A\n');
  });

  it('a second clone with the same user id but its own folder writes nothing to files', async () => {
    const e = await ownerEnv({ 'a.qmd': 'A\n' }, (d) => sideBranch(d, (x) => put(x, 'a.qmd', 'A2\n')));
    const second = cloneRepo(e.dir);
    git(second, 'switch', '-q', 'main');
    const r = await attachFolder({ dir: second, serverUrl: hub.url, indexDocId: e.indexId });
    expect(r.ok).toBe(true);
    const clock = new ManualClock();
    const sync = new ProjectSync(second, (await readProjectMeta(second))!, identity, makeFakeHost(), { clone: { clock } });
    await sync.start();
    stops.push(() => sync.dispose());
    const before = e.changes();
    const files0 = e.files();
    git(second, 'fetch', '-q', 'origin', 'side:side');
    git(second, 'switch', '-q', 'side');
    await waitFor(async () => (await clock.advance(300), await sleep(20), sync.cloneState()?.head === git(second, 'rev-parse', 'HEAD') && !sync.cloneState()!.gateClosed), 'the second clone deciding');
    await sleep(500);
    expect(e.changes()).toBe(before);
    expect(e.files()).toEqual(files0);
    expect((await readProjectMeta(second))!.ownerId).toBeUndefined();
  });
});
