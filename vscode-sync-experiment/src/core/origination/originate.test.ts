/** GE-12 specs 4, 5, 6, 8: the offline build, the registry bootstrap, the heads wait, the sidecar race, and resume. */
import * as A from '@automerge/automerge';
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import { CURRENT_SCHEMA_VERSION, createIndexDocumentContent, type IndexDocument } from '@quarto/quarto-automerge-schema';
import { readIndexFields, viewOf, type Sidecar } from '@quarto/quarto-git-epochs';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { cleanupGitTempDirs, git, put, repoWith } from '../clone/git-fixtures';
import { RegistryClient } from '../clone/registry-client';
import { readProjectMeta } from '../meta';
import { findDoc, makeRepo, webSocketAdapter } from '../repo';
import { identity, startHub, tempDir, waitFor } from '../test-support';
import { ensureWasm } from '../wasm';
import { buildOffline, originate, type OriginateInput } from './originate';
import { selectFiles } from './select';
import { waitForServerHeads } from './wait-heads';

let hub: TestHub;
beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(() => hub.stop());
const cleanup: (() => Promise<unknown>)[] = [];
afterEach(async () => {
  for (const c of cleanup.splice(0)) await c().catch(() => {});
  cleanupGitTempDirs();
});

const input = async (dir: string, extra: Partial<OriginateInput> = {}): Promise<OriginateInput> => ({
  dir,
  selection: await selectFiles(dir),
  server: hub.url,
  identity,
  ...extra,
});

describe('buildOffline: the bootstrap content (spec 5)', () => {
  it('a git project: version 4 index with epochs, owner, repo, identity, one doc per file, and HEAD-blob entries at generation 0', async () => {
    const dir = repoWith({ 'index.qmd': 'hi\n', 'sub/a.qmd': 'a\n', 'logo.png': Buffer.from([1, 2, 3]) });
    put(dir, 'sub/a.qmd', 'dirty a\n'); // uncommitted: published as on disk, registered under the HEAD blob
    const b = await buildOffline(await input(dir));
    cleanup.push(() => b.repo.shutdown());
    const idx = b.index.doc()!;
    expect(idx.version).toBe(CURRENT_SCHEMA_VERSION);
    expect(idx.version).toBe(4);
    const f = readIndexFields(idx);
    expect(f.owner).toBe(b.meta.ownerId);
    expect(f.owner).toMatch(/\S{8,}/);
    expect(f.epochs).toBe(b.sidecar!.documentId);
    expect(f.repo).toEqual({ roots: [git(dir, 'rev-list', '--max-parents=0', 'HEAD')], prefix: '' });
    expect(idx.identities[identity.userId]).toEqual({ name: identity.userName, color: identity.userColor });
    expect(Object.keys(idx.files).sort()).toEqual(['index.qmd', 'logo.png', 'sub/a.qmd']);

    const entries = viewOf(b.sidecar!.doc()!).entries.sort((x, y) => x.path.localeCompare(y.path));
    expect(entries.map((e) => [e.path, e.blob, e.gen])).toEqual([
      ['index.qmd', git(dir, 'rev-parse', 'HEAD:index.qmd'), 0],
      ['logo.png', git(dir, 'rev-parse', 'HEAD:logo.png'), 0],
      ['sub/a.qmd', git(dir, 'rev-parse', 'HEAD:sub/a.qmd'), 0],
    ]);
    for (const e of entries) expect(idx.files[e.path]).toBe(e.doc);

    const dirtyDoc = (await findDoc<{ text: string }>(b.repo, idx.files['sub/a.qmd']!)).doc()!;
    expect(dirtyDoc.text).toBe('dirty a\n');
    const bin = (await findDoc<{ content: Uint8Array; mimeType: string; hash: string }>(b.repo, idx.files['logo.png']!)).doc()!;
    expect(Array.from(bin.content)).toEqual([1, 2, 3]);
    expect(bin.mimeType).toBe('image/png');
    expect(bin.hash).toMatch(/^[0-9a-f]{64}$/);
  });

  it('writes project.json first: clone mode with the repo identity, the owner id, the name, and the file', async () => {
    const dir = repoWith({ 'index.qmd': 'hi\n', '_quarto.yml': 'project:\n  title: "My Site"\n' });
    const b = await buildOffline(await input(dir));
    cleanup.push(() => b.repo.shutdown());
    const meta = await readProjectMeta(dir);
    expect(meta).toMatchObject({ indexDocId: b.index.documentId, syncServer: hub.url, mode: 'clone', ownerId: b.meta.ownerId, name: 'My Site', file: 'index.qmd' });
    expect(meta!.repo).toEqual({ roots: expect.any(Array), prefix: '' });
  });

  it('a plain folder: mirror mode, no repo and no sidecar, but still an owner', async () => {
    const dir = await tempDir('ge12-plain-');
    put(dir, 'a.qmd', 'a\n');
    put(dir, 'notes/b.qmd', 'b\n');
    const b = await buildOffline(await input(dir));
    cleanup.push(() => b.repo.shutdown());
    const idx = b.index.doc()!;
    expect(b.sidecar).toBeNull();
    expect(readIndexFields(idx)).toEqual({ owner: b.meta.ownerId });
    expect(Object.keys(idx.files).sort()).toEqual(['a.qmd', 'notes/b.qmd']);
    const meta = await readProjectMeta(dir);
    expect(meta).toMatchObject({ mode: 'mirror', ownerId: b.meta.ownerId, name: path.basename(dir), file: 'a.qmd' });
    expect(meta!.repo).toBeUndefined();
  });

  it('file picks the first .qmd when there is no index.qmd, else the first file', async () => {
    const dir = await tempDir('ge12-file-');
    put(dir, 'b.qmd', 'b');
    put(dir, 'a.qmd', 'a');
    put(dir, 'z.png', 'x');
    const b = await buildOffline(await input(dir));
    cleanup.push(() => b.repo.shutdown());
    expect(b.meta.file).toBe('a.qmd');
    const dir2 = await tempDir('ge12-file2-');
    put(dir2, 'only.png', 'x');
    const b2 = await buildOffline(await input(dir2));
    cleanup.push(() => b2.repo.shutdown());
    expect(b2.meta.file).toBe('only.png');
  });
});

describe('originate: the publish order (spec 4)', () => {
  it('connects only after the docs and registry entries exist, and returns ok only once the server holds every head', async () => {
    const dir = repoWith({ 'index.qmd': 'hi\n', 'a.qmd': 'a\n' });
    const seen: string[] = [];
    const r = await originate(await input(dir), {
      connect(b, server) {
        // The recording fake network: at this moment everything must already exist, and nothing is connected.
        seen.push(`connect:${server}`);
        expect(b.repo.networkSubsystem.adapters.length).toBe(0);
        expect(Object.keys(b.index.doc()!.files).sort()).toEqual(['a.qmd', 'index.qmd']);
        expect(viewOf(b.sidecar!.doc()!).entries).toHaveLength(2);
        expect(readIndexFields(b.index.doc()!).epochs).toBe(b.sidecar!.documentId);
        b.repo.networkSubsystem.addNetworkAdapter(webSocketAdapter(server));
      },
    });
    expect(seen).toEqual([`connect:${hub.url}`]);
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    // Ground truth: the hub holds every doc at the heads the folder's own storage holds.
    const local = makeRepo(dir, null);
    cleanup.push(() => local.shutdown());
    const index = await findDoc<IndexDocument>(local, r.indexDocId);
    for (const docId of [r.indexDocId, readIndexFields(index.doc()).epochs!, ...Object.values(index.doc()!.files)]) {
      const h = await findDoc<unknown>(local, docId);
      expect(await hub.hubHasHeads(docId, h.heads() as string[], 5_000)).toBe(true);
    }
  });

  it('a server that never confirms is a timeout, and the folder stays bound with its data in local storage', async () => {
    const dir = repoWith({ 'index.qmd': 'hi\n' });
    const r = await originate(await input(dir), { connect: () => {}, waitMs: 250 });
    expect(r.ok).toBe(false);
    if (r.ok) return;
    expect(r.reason).toBe('timeout');
    expect(r.pending.length).toBeGreaterThan(0);
    expect((await readProjectMeta(dir))?.indexDocId).toBe(r.indexDocId);
    const local = makeRepo(dir, null);
    cleanup.push(() => local.shutdown());
    expect(Object.keys((await findDoc<IndexDocument>(local, r.indexDocId)).doc()!.files)).toEqual(['index.qmd']);
  });
});

describe('waitForServerHeads (spec 4)', () => {
  it('is not ok while offline, times out after the cap, and is ok once the hub has the heads', async () => {
    const repo = new Repo();
    cleanup.push(() => repo.shutdown());
    const h = repo.create<{ text: string }>({ text: 'x' });
    const t0 = Date.now();
    const off = await waitForServerHeads(repo, [h as DocHandle<unknown>], 300);
    expect(off.ok).toBe(false);
    expect(off.pending).toEqual([h.documentId]);
    expect(Date.now() - t0).toBeGreaterThanOrEqual(280);
    repo.networkSubsystem.addNetworkAdapter(webSocketAdapter(hub.url));
    const on = await waitForServerHeads(repo, [h as DocHandle<unknown>], 10_000);
    expect(on).toEqual({ ok: true, pending: [] });
    expect(await hub.hubHasHeads(h.documentId, h.heads() as string[], 2_000)).toBe(true);
  });

  it('waits for a later local change too: ok only when the server heads equal the new local heads', async () => {
    const repo = new Repo({ network: [webSocketAdapter(hub.url)] });
    cleanup.push(() => repo.shutdown());
    const h = repo.create<{ text: string }>({ text: 'x' });
    expect((await waitForServerHeads(repo, [h as DocHandle<unknown>], 10_000)).ok).toBe(true);
    h.change((d) => void (d.text = 'y'));
    const r = await waitForServerHeads(repo, [h as DocHandle<unknown>], 10_000);
    expect(r.ok).toBe(true);
    expect(await hub.hubHasHeads(h.documentId, h.heads() as string[], 2_000)).toBe(true);
  });
});

describe('the sidecar race (spec 6)', () => {
  it('two peers create the sidecar offline; both converge on one pointer and the loser\'s entries end up in the winner', async () => {
    const a = new Repo();
    const index = a.create<IndexDocument>(createIndexDocumentContent());
    const b = new Repo();
    const indexB = b.import<IndexDocument>(A.save(index.doc()!), { docId: index.documentId } as never) as DocHandle<IndexDocument>;
    cleanup.push(() => a.shutdown(), () => b.shutdown());
    const ra = new RegistryClient(a, index);
    const rb = new RegistryClient(b, indexB);
    // The owner's re-register hook: when the key is lost, the loser puts its own entries into the winner.
    const mine = { a: [{ path: 'a.qmd', blob: 'b1', doc: 'da', gen: 0 }], b: [{ path: 'b.qmd', blob: 'b2', doc: 'db', gen: 0 }] };
    ra.onSidecarSwitched = () => mine.a.forEach((e) => ra.register(e));
    rb.onSidecarSwitched = () => mine.b.forEach((e) => rb.register(e));
    await ra.ensure();
    await rb.ensure();
    mine.a.forEach((e) => ra.register(e));
    mine.b.forEach((e) => rb.register(e));
    expect(ra.sidecarId()).not.toBe(rb.sidecarId());
    const stopA = ra.follow();
    const stopB = rb.follow();
    cleanup.push(async () => (stopA(), stopB()));
    a.networkSubsystem.addNetworkAdapter(webSocketAdapter(hub.url));
    b.networkSubsystem.addNetworkAdapter(webSocketAdapter(hub.url));
    await waitFor(() => ra.sidecarId() === rb.sidecarId(), 'one pointer', 30_000);
    await waitFor(() => ra.view().entries.length === 2 && rb.view().entries.length === 2, 'both entries in the winner', 30_000);
    expect(ra.view().entries.map((e) => e.path).sort()).toEqual(['a.qmd', 'b.qmd']);
  });
});

describe('resume (spec 8)', () => {
  it('stops after the offline step; reopening the folder uploads the index and every doc', async () => {
    const dir = repoWith({ 'index.qmd': 'hi\n', 'a.qmd': 'a\n' });
    const b = await buildOffline(await input(dir));
    const indexDocId = b.index.documentId;
    await b.repo.flush();
    await b.repo.shutdown(); // the interruption: nothing was connected
    expect(await hub.hubHasDoc(indexDocId, 500)).toBe(false);
    const { HeadlessPeer } = await import('../../headless/peer');
    const peer = await HeadlessPeer.open(dir);
    cleanup.push(() => peer.stop());
    expect(peer.cloneState()).not.toBeNull();
    await waitFor(async () => {
      const inv = peer.inventory();
      for (const d of inv) if (!(await hub.hubHasHeads(d.docId, d.heads, 500))) return false;
      return inv.length >= 3;
    }, 'the hub holding the index and every doc', 30_000);
  });
});
