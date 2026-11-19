/**
 * GE-10 specs 8 to 14 and 16: detach, startup, rebind, the sidecar race, the late reload, retired and superseded docs,
 * the moved message, the event log, and hostile keys. Real git repositories, a test hub, fakes for the editor.
 * Permanent-subset candidates: none.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { ADOPT_CONFIRM } from '@quarto/quarto-git-epochs';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { cleanupGitTempDirs, commitAll, put } from './git-fixtures';
import { abs, editDisk, git, makeCloneEnv, readDisk, type CloneEnv } from './clone-harness';
import { readRecord } from './record';
import { rebindFolder } from './rebind';
import { findDoc } from '../repo';
import { readProjectMeta } from '../meta';
import { publishGit } from '../../headless/publish-git';
import { sleep, startHub, waitFor } from '../test-support';
import { ensureWasm } from '../wasm';

let hub: TestHub;
const envs: CloneEnv[] = [];
beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(() => hub.stop());
afterEach(async () => {
  for (const e of envs.splice(0)) await e.stop();
  cleanupGitTempDirs();
});
async function env(files: Record<string, string | Buffer>, prepare?: (clone: string, origin: string) => void | Promise<void>, noRegistry = false) {
  const e = await makeCloneEnv(hub, files, { prepare, noRegistry });
  envs.push(e);
  return e;
}
const sideBranch = (clone: string) => {
  git(clone, 'switch', '-q', '-c', 'side');
  put(clone, 'a.qmd', 'A2\n');
  commitAll(clone, 'a2');
  git(clone, 'switch', '-q', 'main');
};
const blobOf = (e: CloneEnv, rel: string, rev = 'HEAD') => git(e.clone, 'rev-parse', `${rev}:${rel}`);
const docIdOf = (e: CloneEnv, rel: string) => e.sync.cloneState()!.attached[rel];
const events = async (e: CloneEnv) =>
  (await fs.readFile(path.join(e.clone, '.quarto-hub', 'events.jsonl'), 'utf8').catch(() => ''))
    .split('\n')
    .filter(Boolean)
    .map((l) => JSON.parse(l) as { event: string; path: string; from?: string; to?: string });

/** Leave the clone with `a.qmd` detached: on `side`, a shared doc under that blob, and a dirty file. */
async function detachedA(e: CloneEnv) {
  const other = await e.other.register('a.qmd', blobOf(e, 'a.qmd', 'side'), 'A2 shared\n');
  await waitFor(() => e.sync.registryView().entries.some((x) => x.doc === other), 'registration');
  git(e.clone, 'switch', '-q', 'side');
  await fs.writeFile(abs(e, 'a.qmd'), 'my own edit\n');
  await e.settle(() => e.sync.cloneState()!.detached.includes('a.qmd'));
  return other;
}

describe('detach (spec 8)', () => {
  it('a detached Binding sends no edits and applies no remote changes; the status bar shows the count', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, sideBranch);
    const a = abs(e, 'a.qmd');
    const other = await e.other.register('a.qmd', blobOf(e, 'a.qmd', 'side'), 'A2 shared\n');
    await waitFor(() => e.sync.registryView().entries.some((x) => x.doc === other), 'registration');
    e.host.buffers.open(a, 'A\n');
    await waitFor(() => e.host.buffers.bindings.has(a), 'the binding');
    git(e.clone, 'switch', '-q', 'side');
    await fs.writeFile(a, 'my own edit\n');
    await e.settle(() => e.sync.cloneState()!.detached.includes('a.qmd'));
    const docBefore = e.sync.docText('a.qmd');
    e.host.buffers.type(a, 'typed while detached\n');
    await e.other.edit(other, 'someone else\n');
    await sleep(300);
    expect(e.sync.docText('a.qmd')).toBe(docBefore);
    expect(e.host.buffers.getText(a)).toBe('typed while detached\n');
    expect(e.host.notifier.statuses.filter(Boolean).pop()).toBe('Quarto Hub: clone · 1 detached');
  });

  it('adopt asks first when the file differs from its blob, then takes the shared doc; declining changes nothing', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, sideBranch);
    const other = await detachedA(e);
    e.host.notifier.choices.push('Cancel');
    expect(await e.sync.adopt('a.qmd')).toBe(false);
    expect(e.host.notifier.messages.some((m) => m.message === ADOPT_CONFIRM)).toBe(true);
    expect(await readDisk(e, 'a.qmd')).toBe('my own edit\n');
    e.host.notifier.choices.push('Replace');
    expect(await e.sync.adopt('a.qmd')).toBe(true);
    expect(await readDisk(e, 'a.qmd')).toBe('A2 shared\n');
    expect(e.sync.cloneState()!.detached).toEqual([]);
    expect(docIdOf(e, 'a.qmd')).toBe(other);
    expect((await events(e)).map((x) => x.event)).toEqual(expect.arrayContaining(['switched', 'detached', 'reattached']));
  });

  it('re-attaches automatically when the contents become equal', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, sideBranch);
    await detachedA(e);
    await editDisk(e, 'a.qmd', 'A2 shared\n');
    await waitFor(() => e.sync.cloneState()!.detached.length === 0, 're-attach');
    expect((await events(e)).some((x) => x.event === 'reattached')).toBe(true);
  });

  it('a detached file that is later committed gets a doc under the new blob and is shareable from then on', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, sideBranch);
    await detachedA(e);
    commitAll(e.clone, 'commit my edit');
    await e.settle(() => e.sync.cloneState()!.detached.length === 0);
    const blob = blobOf(e, 'a.qmd');
    const entry = e.sync.registryView().entries.find((x) => x.path === 'a.qmd' && x.blob === blob)!;
    expect(entry).toBeTruthy();
    expect(docIdOf(e, 'a.qmd')).toBe(entry.doc);
    expect(e.sync.docText('a.qmd')).toBe('my own edit\n');
  });
});

describe('startup rule (spec 9)', () => {
  it('HEAD unchanged merges offline edits as today', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const doc = docIdOf(e, 'a.qmd');
    await e.sync.dispose();
    await fs.writeFile(abs(e, 'a.qmd'), 'edited offline\n');
    await e.restart();
    expect(e.sync.docText('a.qmd')).toBe('edited offline\n');
    expect(docIdOf(e, 'a.qmd')).toBe(doc);
  });

  it('HEAD changed by an offline git switch runs arrival and merges nothing from disk into a doc', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, sideBranch);
    const doc = docIdOf(e, 'a.qmd')!;
    await e.sync.dispose();
    git(e.clone, 'switch', '-q', 'side');
    await e.restart();
    expect(docIdOf(e, 'a.qmd')).not.toBe(doc); // a doc was created from the disk for the new blob...
    expect((await findDoc<{ text: string }>(e.other.repo, doc)).doc()!.text).toBe('A\n'); // ...and the old doc was left alone
    expect((await readRecord(e.clone))?.head).toBe(git(e.clone, 'rev-parse', 'HEAD'));
  });

  it('the documented gap: an offline reset --hard with an unchanged HEAD merges the reset content as an edit', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const doc = docIdOf(e, 'a.qmd')!;
    await e.other.edit(doc, 'A live\n');
    await waitFor(async () => (await readDisk(e, 'a.qmd')) === 'A live\n', 'live edit on disk');
    await e.sync.dispose();
    git(e.clone, 'reset', '-q', '--hard');
    await e.restart();
    expect(docIdOf(e, 'a.qmd')).toBe(doc);
    expect(e.sync.docText('a.qmd')).toBe('A\n'); // pinned: the live edit is overwritten, as the contract says
  });

  it('a detached file stays detached across a restart', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, sideBranch);
    await detachedA(e);
    expect((await readRecord(e.clone))?.detached).toEqual(['a.qmd']);
    await e.restart();
    expect(e.sync.cloneState()!.detached).toEqual(['a.qmd']);
    expect(await readDisk(e, 'a.qmd')).toBe('my own edit\n');
  });
});

describe('rebind (spec 10)', () => {
  it('after a confirm deletes the store and the record and rewrites project.json; a declined confirm deletes nothing', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    await e.sync.dispose();
    const store = path.join(e.clone, '.quarto-hub', 'automerge');
    expect((await fs.readdir(store)).length).toBeGreaterThan(0);
    const declined = await rebindFolder({ root: e.clone, indexDocId: e.pub.indexDocId, confirm: async () => false });
    expect(declined).toMatchObject({ ok: false, reason: 'declined' });
    expect((await fs.readdir(store)).length).toBeGreaterThan(0);
    expect(await readRecord(e.clone)).not.toBeNull();
    let shutdown = 0;
    const ok = await rebindFolder({ root: e.clone, indexDocId: e.pub.indexDocId, confirm: async () => true, shutdown: async () => void shutdown++ });
    expect(ok).toMatchObject({ ok: true });
    expect(shutdown).toBe(1);
    expect(await fs.stat(store).then(() => true, () => false)).toBe(false);
    expect(await readRecord(e.clone)).toBeNull();
    expect((await readProjectMeta(e.clone))?.mode).toBe('clone');
    await e.restart(); // the first-run rule: arrival for every file
    expect(Object.keys(e.sync.cloneState()!.attached)).toEqual(['a.qmd']);
  });

  it('a clone refuses to rebind to a project of another repository', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const dir = (await import('./git-fixtures')).repoWith({ 'a.qmd': 'A\n' }, 'unrelated');
    const other = await publishGit(dir, hub.url, hub);
    try {
      await e.sync.dispose();
      const r = await rebindFolder({ root: e.clone, indexDocId: other.indexDocId, confirm: async () => true });
      expect(r).toMatchObject({ ok: false, reason: 'identity-mismatch' });
      expect((await readProjectMeta(e.clone))?.indexDocId).toBe(e.pub.indexDocId);
    } finally {
      await other.stop();
    }
  });

  it('refuses to start sync when the repository no longer matches the recorded identity', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    await e.sync.dispose();
    const meta = (await readProjectMeta(e.clone))!;
    await fs.writeFile(path.join(e.clone, '.quarto-hub', 'project.json'), JSON.stringify({ ...meta, repo: { roots: ['0'.repeat(40)], prefix: '' } }));
    await expect(e.restart()).rejects.toThrow(/root commit|same repository/);
  });
});

describe('the registry races and retirement (specs 11, 13)', () => {
  it('a clean peer whose doc loses canonical selection re-attaches to the winner', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, undefined, true);
    const mine = docIdOf(e, 'a.qmd')!;
    const winner = await e.other.register('a.qmd', blobOf(e, 'a.qmd'), 'A\n', -1);
    await e.sync.registryView(); // (a view read; the change event drives re-evaluation)
    await waitFor(() => docIdOf(e, 'a.qmd') === winner, 'the clean loser re-attaching');
    expect(winner).not.toBe(mine);
  });

  it('a loser whose disk differs from the canonical doc detaches', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, (clone) => put(clone, 'a.qmd', 'dirty before start\n'), true);
    const winner = await e.other.register('a.qmd', blobOf(e, 'a.qmd'), 'A\n', -1);
    await waitFor(() => e.sync.cloneState()!.detached.includes('a.qmd'), 'the dirty loser detaching');
    expect(await readDisk(e, 'a.qmd')).toBe('dirty before start\n');
    expect(winner).toBeTruthy();
  });

  it('two peers creating the sidecar at once end on one sidecar and one doc per file', { timeout: 90_000 }, async () => {
    const origin = (await import('./git-fixtures')).repoWith({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' });
    const pub = await publishGit(origin, hub.url, hub, { registry: false });
    const { cloneRepo } = await import('./git-fixtures');
    const { attachFolder } = await import('./attach-flow');
    const { ProjectSync } = await import('../project-sync');
    const { makeFakeHost, identity, ManualClock } = await import('../test-support');
    const peers: InstanceType<typeof ProjectSync>[] = [];
    const roots = [cloneRepo(origin), cloneRepo(origin)];
    try {
      for (const r of roots) expect((await attachFolder({ dir: r, serverUrl: hub.url, indexDocId: pub.indexDocId })).ok).toBe(true);
      await Promise.all(
        roots.map(async (r) => {
          const s = new ProjectSync(r, (await readProjectMeta(r))!, identity, makeFakeHost(), { clone: { clock: new ManualClock() } });
          peers.push(s);
          await s.start();
        }),
      );
      await waitFor(
        () =>
          ['a.qmd', 'b.qmd'].every((f) => peers[0]!.cloneState()!.attached[f] === peers[1]!.cloneState()!.attached[f]) &&
          peers[0]!.registryView().entries.length > 0,
        'both peers on the same docs',
        45_000,
      );
      const sidecars = peers.map((p) => (p as unknown as { index: { doc(): { epochs?: unknown } } }).index.doc().epochs);
      expect(String(sidecars[0])).toBe(String(sidecars[1])); // one sidecar
      for (const p of peers) expect(p.registryView().retired).toEqual([]);

    } finally {
      for (const s of peers) await s.dispose().catch(() => {});
      await pub.stop();
    }
  });

  it('a file attached to a doc another peer retired stays attached and live, and is listed as superseded', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const doc = docIdOf(e, 'a.qmd')!;
    await e.other.retire('a.qmd', blobOf(e, 'a.qmd'), doc);
    await waitFor(() => e.sync.cloneState()!.superseded.includes('a.qmd'), 'superseded');
    expect(docIdOf(e, 'a.qmd')).toBe(doc);
    expect(e.host.notifier.statuses.filter(Boolean).pop()).toContain('1 superseded');
    await e.other.edit(doc, 'still live\n');
    await waitFor(async () => (await readDisk(e, 'a.qmd')) === 'still live\n', 'live edit still arrives');
  });

  it('the repair rule re-registers a doc that vanished from the registry, and never a retired one', async () => {
    const e = await env({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' });
    const aDoc = docIdOf(e, 'a.qmd')!;
    const bDoc = docIdOf(e, 'b.qmd')!;
    await e.other.dropEntry('a.qmd', blobOf(e, 'a.qmd'), aDoc);
    await waitFor(() => e.sync.registryView().entries.some((x) => x.doc === aDoc), 'a.qmd re-registered');
    await e.other.retire('b.qmd', blobOf(e, 'b.qmd'), bDoc);
    await e.other.dropEntry('b.qmd', blobOf(e, 'b.qmd'), bDoc);
    await sleep(500);
    expect(e.sync.registryView().entries.some((x) => x.doc === bDoc)).toBe(false);
  });
});

describe('the late reload (spec 12)', () => {
  it('while the gate is closed a whole-file rewrite of an open buffer is not spliced into the doc', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const a = abs(e, 'a.qmd');
    e.host.buffers.open(a, 'A\n');
    await waitFor(() => e.host.buffers.bindings.has(a), 'the binding');
    await fs.writeFile(path.join(e.clone, '.git', 'index.lock'), ''); // git is working
    await e.clock.advance(300);
    expect(e.sync.cloneState()!.gateClosed).toBe(true);
    e.host.buffers.type(a, 'the checkout rewrote this buffer\n'); // VS Code reloads the file
    await sleep(100);
    expect(e.sync.docText('a.qmd')).toBe('A\n');
    await fs.rm(path.join(e.clone, '.git', 'index.lock'));
  });
});

describe('the moved message and the event log (specs 14, events)', () => {
  it('moved goes out on the old doc before the old Binding is disposed', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, sideBranch);
    const a = abs(e, 'a.qmd');
    const other = await e.other.register('a.qmd', blobOf(e, 'a.qmd', 'side'), 'A2\n');
    await waitFor(() => e.sync.registryView().entries.some((x) => x.doc === other), 'registration');
    e.host.buffers.open(a, 'A\n');
    await waitFor(() => e.host.buffers.bindings.has(a), 'the binding');
    git(e.clone, 'switch', '-q', 'side');
    await e.settle(() => docIdOf(e, 'a.qmd') === other);
    const ev = e.host.buffers.bindingEvents.filter((x) => x.endsWith(a));
    expect(ev.indexOf(`moved:${a}`)).toBeGreaterThanOrEqual(0);
    expect(ev.indexOf(`moved:${a}`)).toBeLessThan(ev.indexOf(`dispose:${a}`));
  });

  it('each decision writes its event to events.jsonl', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, sideBranch);
    const doc = docIdOf(e, 'a.qmd')!;
    await e.other.edit(doc, 'A live\n');
    await waitFor(async () => (await readDisk(e, 'a.qmd')) === 'A live\n', 'live edit on disk');
    git(e.clone, 'reset', '-q', '--hard');
    await e.settle(() => docIdOf(e, 'a.qmd') !== doc);
    await waitFor(async () => (await events(e)).some((x) => x.event === 'reset'), 'the reset event');
    expect((await events(e)).map((x) => x.event)).toEqual(expect.arrayContaining(['held', 'retired', 'reset']));
    await editDisk(e, 'a.qmd', 'A committed\n');
    await waitFor(() => e.sync.docText('a.qmd') === 'A committed\n', 'edit reaching the doc');
    commitAll(e.clone, 'c');
    await e.settle(() => e.sync.registryView().entries.some((x) => x.blob === blobOf(e, 'a.qmd')));
    await waitFor(async () => (await events(e)).some((x) => x.event === 'moved-forward'), 'the moved-forward event');
  });
});

describe('hostile keys (spec 16)', () => {
  it('an index key naming ../x or .vscode/tasks.json is skipped and writes nothing', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const index = await findDoc<{ files: Record<string, string> }>(e.other.repo, e.pub.indexDocId);
    const h = e.other.repo.create<{ text: string }>({ text: 'evil\n' });
    index.change((d) => {
      d.files['../escaped.qmd'] = h.documentId;
      d.files['.vscode/tasks.json'] = h.documentId;
    });
    await e.other.flush();
    await sleep(600);
    expect(await fs.stat(path.join(e.clone, '..', 'escaped.qmd')).then(() => true, () => false)).toBe(false);
    expect(await fs.stat(path.join(e.clone, '.vscode', 'tasks.json')).then(() => true, () => false)).toBe(false);
    expect(Object.keys(e.sync.cloneState()!.attached)).toEqual(['a.qmd']);
  });
});

describe('a doc live under one path only (G12)', () => {
  it('a canonical doc that is also registered under another path is forked, not shared', async () => {
    const e = await env({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' }, sideBranch);
    const bDoc = docIdOf(e, 'b.qmd')!;
    // b.qmd's doc is also registered as the doc for a.qmd's side blob: one doc under two paths.
    await e.other.registerExisting('a.qmd', blobOf(e, 'a.qmd', 'side'), bDoc, 5);
    await waitFor(() => e.sync.registryView().entries.some((x) => x.path === 'a.qmd' && x.doc === bDoc), 'the shared registration');
    await e.other.edit(bDoc, 'A2\n'); // the shared doc's text now equals what `side` puts in a.qmd (step 3 would attach it)
    const aBefore = docIdOf(e, 'a.qmd');
    await waitFor(async () => (await readDisk(e, 'b.qmd')) === 'A2\n', 'the edit on b.qmd');
    git(e.clone, 'switch', '-q', 'side');
    await e.settle(() => docIdOf(e, 'a.qmd') !== aBefore);
    const forked = docIdOf(e, 'a.qmd')!;
    expect(forked).not.toBe(bDoc);
    expect(e.sync.docText('a.qmd')).toBe('A2\n');
    expect(docIdOf(e, 'b.qmd')).toBe(bDoc);
    await e.other.edit(bDoc, 'B changed later\n');
    await sleep(300);
    expect(e.sync.docText('a.qmd')).toBe('A2\n'); // the fork diverges independently
  });
});
