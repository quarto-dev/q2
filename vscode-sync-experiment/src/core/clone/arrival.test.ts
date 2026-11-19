/**
 * GE-10 specs 5 to 7 and 14: arrival driven by a real `git switch` in a temp repository against a registry on a test
 * hub; a commit moves the doc forward; removals and untracked files.
 * Permanent-subset candidates: none (these are core tests; the Playwright flows cover the real window).
 */
import { mkdirSync } from 'node:fs';
import * as fs from 'node:fs/promises';
import { FILES_DETACHED, FILES_DETACHED_ACTIONS, FILES_JOINED_LIVE, FILES_JOINED_LIVE_ACTIONS, formatNotice } from '@quarto/quarto-git-epochs';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { cleanupGitTempDirs, commitAll, put } from './git-fixtures';
import { abs, editDisk, git, makeCloneEnv, readDisk, type CloneEnv } from './clone-harness';
import { sleep, waitFor } from '../test-support';
import { ensureWasm } from '../wasm';
import { startHub } from '../test-support';

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
async function env(files: Record<string, string | Buffer>, prepare?: (clone: string, origin: string) => void | Promise<void>) {
  const e = await makeCloneEnv(hub, files, { prepare });
  envs.push(e);
  return e;
}

const docIdOf = (e: CloneEnv, rel: string) => e.sync.cloneState()!.attached[rel];
const blobOf = (e: CloneEnv, rel: string, rev = 'HEAD') => git(e.clone, 'rev-parse', `${rev}:${rel}`);
const notices = (e: CloneEnv) => e.host.notifier.messages;

describe('startup with no record (specs 5 step 3, 9)', () => {
  it('attaches every participating file silently to the registry doc and never merges disk into it', async () => {
    const e = await env({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' });
    const view = e.sync.registryView();
    expect(docIdOf(e, 'a.qmd')).toBe(view.entries.find((x) => x.path === 'a.qmd')!.doc);
    expect(notices(e)).toEqual([]);
  });
});

describe('arrival (spec 5)', () => {
  it('step 1: a file no doc stands for gets a doc from disk, registered at generation 0', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, (clone) => {
      git(clone, 'switch', '-q', '-c', 'side');
      put(clone, 'new.qmd', 'NEW\n');
      commitAll(clone, 'add new');
      git(clone, 'switch', '-q', 'main');
    });
    git(e.clone, 'switch', '-q', 'side');
    await e.settle();
    const entry = e.sync.registryView().entries.find((x) => x.path === 'new.qmd')!;
    expect(entry).toMatchObject({ blob: blobOf(e, 'new.qmd'), gen: 0 });
    expect(docIdOf(e, 'new.qmd')).toBe(entry.doc);
    expect(await readDisk(e, 'new.qmd')).toBe('NEW\n');
  });

  it('step 2: attached, S equals B and T differs (after the clone\'s own reset --hard): retire, create from disk, generation max+1', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const old = docIdOf(e, 'a.qmd')!;
    await e.other.edit(old, 'A with live edits\n');
    await waitFor(async () => (await readDisk(e, 'a.qmd')) === 'A with live edits\n', 'the live edit on disk');
    git(e.clone, 'reset', '-q', '--hard');
    await e.settle(() => docIdOf(e, 'a.qmd') !== old);
    const view = e.sync.registryView();
    const fresh = view.entries.find((x) => x.path === 'a.qmd' && x.doc === docIdOf(e, 'a.qmd'))!;
    expect(view.retired.map((r) => r.doc)).toContain(old);
    expect(fresh.gen).toBe(1);
    expect(await readDisk(e, 'a.qmd')).toBe('A\n');
  });

  it('step 2: attached and not (S equals B and T differs): the disk is not merged into the doc', async () => {
    const e = await env({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' });
    const a = docIdOf(e, 'a.qmd')!;
    git(e.clone, 'switch', '-q', '-c', 'noop');
    commitAll(e.clone, 'empty');
    await e.settle();
    expect(docIdOf(e, 'a.qmd')).toBe(a);
    expect(notices(e)).toEqual([]);
  });

  it('step 3: equal text attaches silently, including CRLF on disk against LF in the doc', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, (clone) => {
      git(clone, 'config', 'core.autocrlf', 'true');
      git(clone, 'switch', '-q', '-c', 'side');
      put(clone, 'a.qmd', 'A2\n');
      commitAll(clone, 'a2');
      git(clone, 'switch', '-q', 'main');
    });
    const sideBlob = blobOf(e, 'a.qmd', 'side');
    const other = await e.other.register('a.qmd', sideBlob, 'A2\n');
    await waitFor(() => e.sync.registryView().entries.some((x) => x.doc === other), 'the other peer\'s registration');
    git(e.clone, 'switch', '-q', 'side');
    await e.settle(() => docIdOf(e, 'a.qmd') === other);
    expect(notices(e).filter((m) => m.level !== 'info' || !/replaced/.test(m.message))).toEqual([]);
  });

  it('step 4: clean disk, differing doc: joins the live edits with one notice, and "use my checkout instead" restores the blob and detaches', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, (clone) => {
      git(clone, 'switch', '-q', '-c', 'side');
      put(clone, 'a.qmd', 'A2\n');
      commitAll(clone, 'a2');
      git(clone, 'switch', '-q', 'main');
    });
    const sideBlob = blobOf(e, 'a.qmd', 'side');
    const other = await e.other.register('a.qmd', sideBlob, 'A2 plus live edits\n');
    await waitFor(() => e.sync.registryView().entries.some((x) => x.doc === other), 'registration');
    e.host.notifier.choices.push('Use my checkout instead');
    git(e.clone, 'switch', '-q', 'side');
    await e.settle(() => notices(e).some((m) => m.message === formatNotice(FILES_JOINED_LIVE, { n: 1 })));
    const joined = notices(e).find((m) => m.message === formatNotice(FILES_JOINED_LIVE, { n: 1 }))!;
    expect(joined.actions).toEqual(FILES_JOINED_LIVE_ACTIONS);
    // The choice was answered by the recording notifier: the file now equals the blob and is detached.
    await waitFor(async () => (await readDisk(e, 'a.qmd')) === 'A2\n' && e.sync.cloneState()!.detached.includes('a.qmd'), 'the checkout restored and detached');
    expect(e.sync.registryView().entries.filter((x) => x.path === 'a.qmd' && x.blob === sideBlob)).toHaveLength(1); // no second doc
  });

  it('step 5: dirty and different: nothing is overwritten and the file is detached with one aggregate notice', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, (clone) => {
      git(clone, 'switch', '-q', '-c', 'side');
      put(clone, 'a.qmd', 'A2\n');
      commitAll(clone, 'a2');
      git(clone, 'switch', '-q', 'main');
    });
    const other = await e.other.register('a.qmd', blobOf(e, 'a.qmd', 'side'), 'A2 shared\n');
    await waitFor(() => e.sync.registryView().entries.some((x) => x.doc === other), 'registration');
    git(e.clone, 'switch', '-q', 'side');
    await fs.writeFile(abs(e, 'a.qmd'), 'my own edit\n'); // dirty: differs from the doc and from the blob
    await e.settle(() => e.sync.cloneState()!.detached.includes('a.qmd'));
    expect(await readDisk(e, 'a.qmd')).toBe('my own edit\n');
    const msg = notices(e).find((m) => m.message === formatNotice(FILES_DETACHED, { n: 1 }))!;
    expect(msg.actions).toEqual(FILES_DETACHED_ACTIONS);
    // A change to the shared doc does not touch the detached file.
    await e.other.edit(other, 'A2 shared again\n');
    await sleep(300);
    expect(await readDisk(e, 'a.qmd')).toBe('my own edit\n');
  });

  it('binary files compare by byte hash: equal bytes attach silently, different bytes detach', async () => {
    const png = Buffer.from([137, 80, 78, 71, 1, 2, 3]);
    const png2 = Buffer.from([137, 80, 78, 71, 9, 9, 9]);
    const e = await env({ 'i.png': png }, (clone) => {
      git(clone, 'switch', '-q', '-c', 'side');
      put(clone, 'i.png', png2);
      commitAll(clone, 'png2');
      git(clone, 'switch', '-q', 'main');
    });
    const { createHash } = await import('node:crypto');
    const hash = (b: Buffer) => createHash('sha256').update(b).digest('hex');
    const other = await e.other.registerBinary('i.png', blobOf(e, 'i.png', 'side'), new Uint8Array(png2), hash(png2));
    await waitFor(() => e.sync.registryView().entries.some((x) => x.doc === other), 'registration');
    git(e.clone, 'switch', '-q', 'side');
    await e.settle(() => docIdOf(e, 'i.png') === other);
    expect(e.sync.cloneState()!.detached).toEqual([]);
    // Back on main, with the file dirtied before the decision: the bytes match neither the doc nor the blob.
    git(e.clone, 'switch', '-q', 'main');
    put(e.clone, 'i.png', Buffer.from([7, 7, 7]));
    await e.settle(() => e.sync.cloneState()!.detached.includes('i.png'));
  });
});

describe('move forward (spec 6)', () => {
  it('a plain commit registers the same doc under the new blob and removes the old entry, keeping the generation', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const doc = docIdOf(e, 'a.qmd')!;
    const before = blobOf(e, 'a.qmd');
    await editDisk(e, 'a.qmd', 'A edited\n');
    await waitFor(() => e.sync.docText('a.qmd') === 'A edited\n', 'the edit reaching the doc');
    commitAll(e.clone, 'edit a');
    await e.settle(() => e.sync.registryView().entries.some((x) => x.doc === doc && x.blob !== before));
    const mine = e.sync.registryView().entries.filter((x) => x.doc === doc);
    expect(mine).toEqual([{ path: 'a.qmd', blob: blobOf(e, 'a.qmd'), doc, gen: 0 }]);
    expect(docIdOf(e, 'a.qmd')).toBe(doc);
  });

  it('an amend does the same', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    const doc = docIdOf(e, 'a.qmd')!;
    await editDisk(e, 'a.qmd', 'A1\n');
    await waitFor(() => e.sync.docText('a.qmd') === 'A1\n', 'edit 1');
    commitAll(e.clone, 'one');
    await e.settle(() => e.sync.registryView().entries.some((x) => x.doc === doc && x.blob === blobOf(e, 'a.qmd')));
    await editDisk(e, 'a.qmd', 'A2\n');
    await waitFor(() => e.sync.docText('a.qmd') === 'A2\n', 'edit 2');
    git(e.clone, 'add', '-A');
    git(e.clone, 'commit', '-q', '--amend', '--no-edit');
    await e.settle(() => e.sync.registryView().entries.some((x) => x.doc === doc && x.blob === blobOf(e, 'a.qmd')));
    expect(e.sync.registryView().entries.filter((x) => x.doc === doc)).toHaveLength(1);
    expect(docIdOf(e, 'a.qmd')).toBe(doc);
  });
});

describe('removals and untracked files (spec 7)', () => {
  it('a tracked file git removes keeps its registry entry, and the doc is never written back over the absent path', async () => {
    const e = await env({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' });
    const bDoc = docIdOf(e, 'b.qmd')!;
    git(e.clone, 'rm', '-q', 'b.qmd');
    commitAll(e.clone, 'rm b');
    await e.settle();
    expect(e.sync.registryView().entries.some((x) => x.path === 'b.qmd' && x.doc === bDoc)).toBe(true);
    await e.other.edit(bDoc, 'B edited by someone\n');
    await sleep(400);
    expect(await readDisk(e, 'b.qmd')).toBeNull();
  });

  it('the pointer never creates an untracked file that is absent, and an untracked file on disk follows it', async () => {
    const e = await env({ 'a.qmd': 'A\n' });
    // The owner lists an untracked path the clone lacks: nothing is created.
    const h = e.other.repo.create<{ text: string }>({ text: 'N\n' });
    const index = await (await import('../repo')).findDoc<{ files: Record<string, string> }>(e.other.repo, e.pub.indexDocId);
    index.change((d) => void (d.files['notes.qmd'] = h.documentId));
    await e.other.flush();
    await sleep(500);
    expect(await readDisk(e, 'notes.qmd')).toBeNull();
    // A path the clone has on disk, untracked, does follow it.
    put(e.clone, 'scratch.qmd', 'old\n');
    const h2 = e.other.repo.create<{ text: string }>({ text: 'from the pointer\n' });
    index.change((d) => void (d.files['scratch.qmd'] = h2.documentId));
    await e.other.flush();
    await waitFor(async () => (await readDisk(e, 'scratch.qmd')) === 'from the pointer\n', 'the pointer doc on the existing path');
  });
});

describe('jj (spec 15)', () => {
  it('under colocated jj every changed file takes step 5: nothing is attached or registered', async () => {
    const e = await env({ 'a.qmd': 'A\n' }, (clone) => {
      git(clone, 'switch', '-q', '-c', 'side');
      put(clone, 'a.qmd', 'A2\n');
      put(clone, 'n.qmd', 'N\n');
      commitAll(clone, 'side');
      git(clone, 'switch', '-q', 'main');
      mkdirSync(`${clone}/.jj`);
    });
    // The other peer already shares both side files: without jj they would attach (step 3).
    await e.other.register('a.qmd', blobOf(e, 'a.qmd', 'side'), 'A2\n');
    await e.other.register('n.qmd', blobOf(e, 'n.qmd', 'side'), 'N\n');
    await waitFor(() => e.sync.registryView().entries.length === 3, 'both registrations');
    const before = e.sync.registryView().entries.length;
    const attachedBefore = e.sync.cloneState()!.attached;
    git(e.clone, 'switch', '-q', 'side');
    await e.settle(() => e.sync.cloneState()!.detached.length === 2);
    expect(e.sync.registryView().entries.length).toBe(before);
    expect(e.sync.cloneState()!.detached.sort()).toEqual(['a.qmd', 'n.qmd']);
    expect(e.sync.cloneState()!.attached['a.qmd']).toBe(attachedBefore['a.qmd']); // not switched to the shared doc
  });
});
