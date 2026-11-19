/**
 * Editor-only mode (GE-9, G20): VS Code opened on a directory a hub serves. The
 * extension never writes a project file, and it runs the git gate: a fake probe
 * and a manual clock stand in for git and for time. Specs 5 and 6.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { EDITOR_ONLY_NO_FLIP, EDITOR_ONLY_NO_FLIP_ACTIONS, GIT_OP_EDITS_DROPPED } from '@quarto/quarto-git-epochs';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { createProjectFolder } from './create-folder';
import { HeadlessOwner } from '../headless/owner';
import { readProjectMeta, writeProjectMeta } from './meta';
import { ProjectSync, type SyncOptions } from './project-sync';
import {
  FakeGitProbe,
  ManualClock,
  identity,
  makeFakeHost,
  readText,
  startHub,
  tempDir,
  waitFor,
  type FakeHost,
} from './test-support';
import { ensureWasm } from './wasm';

let hub: TestHub;
let parent: string;
let host: FakeHost;
let owner: HeadlessOwner;
let clock: ManualClock;
let probe: FakeGitProbe;
const syncs: ProjectSync[] = [];

beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(async () => {
  await hub.stop();
});
beforeEach(async () => {
  parent = await tempDir('ge9-eo-');
  host = makeFakeHost();
  clock = new ManualClock();
  probe = new FakeGitProbe(clock);
});
afterEach(async () => {
  for (const s of syncs.splice(0)) await s.dispose().catch(() => {});
  await owner?.stop();
  await fs.rm(parent, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

/** A folder bound to the project in editor-only mode, with `a.qmd` open and bound. */
async function editorOnly(files: Record<string, string>, prepare?: (root: string) => Promise<void>, options: SyncOptions = {}) {
  owner = await HeadlessOwner.create(hub, files);
  const root = await createProjectFolder(owner.indexDocId, parent, host.log, { syncServer: hub.url });
  const meta = (await readProjectMeta(root))!;
  await writeProjectMeta(root, { ...meta, mode: 'editor-only' });
  await prepare?.(root);
  const sync = new ProjectSync(root, { ...meta, mode: 'editor-only' }, identity, host, {
    clock,
    gate: { probe, clock },
    ...options,
  });
  syncs.push(sync);
  await sync.start();
  return { root, sync, abs: path.join(root, 'a.qmd') };
}

async function openBound(abs: string, text: string) {
  host.buffers.open(abs, text);
  await waitFor(() => host.buffers.bindings.has(abs), 'the buffer binding');
}
const warns = () => host.notifier.messages.filter((m) => m.level === 'warn');
const real = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function snapshot(root: string): Promise<Record<string, string>> {
  const out: Record<string, string> = {};
  for (const e of await fs.readdir(root, { withFileTypes: true, recursive: true })) {
    if (!e.isFile()) continue;
    const abs = path.join(e.parentPath, e.name);
    const rel = path.relative(root, abs);
    if (rel.startsWith('.quarto-hub')) continue;
    const st = await fs.stat(abs);
    out[rel] = `${await fs.readFile(abs, 'utf8')}@${st.mtimeMs}`;
  }
  return out;
}

describe('editor-only follower (spec 5)', () => {
  it('never writes a project file, even when a flip changes a bound file or the owner adds and removes entries', async () => {
    const { root, abs } = await editorOnly({ 'a.qmd': 'A1\n', 'b.qmd': 'B1\n' });
    await openBound(abs, 'A1\n');
    const before = await snapshot(root);
    owner.applyBatch({
      flips: [{ path: 'a.qmd', text: 'A2\n' }],
      removals: ['b.qmd'],
      additions: [{ path: 'c.qmd', text: 'C1\n' }],
    });
    await owner.flush();
    await waitFor(() => host.buffers.texts.get(abs) === 'A2\n', 'the buffer on the new doc');
    await real(300);
    expect(await snapshot(root)).toEqual(before);
    expect(await readText(path.join(root, 'c.qmd'))).toBeNull();
    expect(await readText(path.join(root, 'b.qmd'))).toBe('B1\n');
  });

  it('does not watch the disk or reconcile it with the index', async () => {
    const { root, abs } = await editorOnly({ 'a.qmd': 'A1\n', 'b.qmd': 'B1\n' }, async (r) => {
      await fs.writeFile(path.join(r, 'extra.qmd'), 'only on disk\n'); // would be published by reconcileDisk
      await fs.rm(path.join(r, 'b.qmd')); // would be removed from the index by reconcileDisk
    });
    await fs.writeFile(abs, 'changed on disk\n');
    host.watcher.fire('change', abs); // a mirror would splice this into the doc
    host.watcher.fire('create', path.join(root, 'extra.qmd'));
    await real(400);
    expect(Object.keys((await hub.repo.find<{ files: Record<string, string> }>(`automerge:${owner.indexDocId}` as never)).doc()!.files).sort()).toEqual(['a.qmd', 'b.qmd']);
    expect(owner.textOf('a.qmd')).toBe('A1\n');
  });

  it('shows the mode in a persistent status item', async () => {
    await editorOnly({ 'a.qmd': 'A1\n' });
    expect(host.notifier.statuses).toContain('Quarto Hub: editor only');
  });

  it('rebinds an open buffer to the new doc on a flip', async () => {
    const { abs } = await editorOnly({ 'a.qmd': 'A1\n' });
    await openBound(abs, 'A1\n');
    owner.flipPath('a.qmd', owner.createDocFor('a.qmd', 'A2\n'));
    await owner.flush();
    await waitFor(() => host.buffers.texts.get(abs) === 'A2\n', 'the buffer on the new doc');
    host.buffers.type(abs, 'A2\nmore');
    await waitFor(() => owner.textOf('a.qmd') === 'A2\nmore', 'typing reaching the new doc');
  });
});

describe('editor-only gate (spec 6)', () => {
  it('sends no outbound edit while a marker exists', async () => {
    const { abs } = await editorOnly({ 'a.qmd': 'A1\n' });
    await openBound(abs, 'A1\n');
    probe.lock();
    host.buffers.type(abs, 'A1\nx');
    host.buffers.type(abs, 'A1\nxy');
    await real(300);
    expect(owner.textOf('a.qmd')).toBe('A1\n');
  });

  it('after the markers clear the quiet window must pass; with every buffer equal to its doc, editing then resumes (a plain commit)', async () => {
    const { abs } = await editorOnly({ 'a.qmd': 'A1\n' });
    await openBound(abs, 'A1\n');
    probe.lock();
    await clock.advance(300); // the gate notices the marker
    probe.unlock('commit: message');
    await clock.advance(500);
    host.buffers.type(abs, 'A1\nduring quiet');
    await real(200);
    expect(owner.textOf('a.qmd')).toBe('A1\n'); // still inside the quiet window
    host.buffers.texts.set(abs, 'A1\n'); // the user undoes it; nothing differs from the doc
    await clock.advance(1500);
    host.buffers.type(abs, 'A1\nafter');
    await waitFor(() => owner.textOf('a.qmd') === 'A1\nafter', 'editing resuming');
    expect(warns()).toEqual([]); // no buffer was dirty when the gate closed
  });

  it('after a checkout it waits for the hub flip, rebinds, never writes the old text into the buffer, and then edits reach the new doc', async () => {
    const { abs } = await editorOnly({ 'a.qmd': 'A1\n' });
    await openBound(abs, 'A1\n');
    probe.lock();
    await clock.advance(300);
    host.buffers.texts.set(abs, 'A2\n'); // git rewrites the file and VS Code reloads the buffer
    probe.unlock();
    await clock.advance(2000); // quiet window passed, decision made: the buffer differs, so wait for the flip
    expect(host.buffers.texts.get(abs)).toBe('A2\n'); // the old doc's text was not written into the buffer
    owner.flipPath('a.qmd', owner.createDocFor('a.qmd', 'A2\n'));
    await owner.flush();
    await waitFor(() => host.buffers.bindings.has(abs) && host.buffers.bindings.get(abs)!.handle.doc()?.text === 'A2\n', 'the rebind');
    host.buffers.type(abs, 'A2\nnew');
    await waitFor(() => owner.textOf('a.qmd') === 'A2\nnew', 'editing resuming on the new doc');
    expect(warns()).toEqual([]);
  });

  it('without a flip it warns after 10 s, stays suspended, and offers Keep waiting and Dismiss', async () => {
    const { abs } = await editorOnly({ 'a.qmd': 'A1\n' });
    await openBound(abs, 'A1\n');
    probe.lock();
    await clock.advance(300);
    host.buffers.texts.set(abs, 'A2\n');
    probe.unlock();
    await clock.advance(2000);
    expect(warns()).toEqual([]);
    host.notifier.choices.push('Keep waiting', 'Dismiss');
    await clock.advance(10_000);
    expect(warns().map((w) => w.message)).toEqual([EDITOR_ONLY_NO_FLIP]);
    expect(warns()[0]!.actions).toEqual(EDITOR_ONLY_NO_FLIP_ACTIONS);
    await clock.advance(10_000); // "Keep waiting" re-armed the wait
    expect(warns().map((w) => w.message)).toEqual([EDITOR_ONLY_NO_FLIP, EDITOR_ONLY_NO_FLIP]);
    await clock.advance(30_000); // "Dismiss": no more warnings
    expect(warns()).toHaveLength(2);
    host.buffers.type(abs, 'A2\nstill dropped');
    await real(300);
    expect(owner.textOf('a.qmd')).toBe('A1\n'); // stays suspended
    expect(host.buffers.texts.get(abs)).toBe('A2\nstill dropped'); // and the buffer never got the old text
  });

  it('shows one "edits not kept" warning per episode when a bound buffer was dirty as the gate closed, and none otherwise', async () => {
    const { abs } = await editorOnly({ 'a.qmd': 'A1\n' });
    await openBound(abs, 'A1\n');
    host.buffers.type(abs, 'A1\ntyped'); // dirty, and accepted: the gate is open
    await waitFor(() => owner.textOf('a.qmd') === 'A1\ntyped', 'the typed edit');
    probe.lock();
    await clock.advance(300); // the gate closes with a dirty buffer
    host.buffers.type(abs, 'A1\ntyped, more'); // dropped
    host.buffers.texts.set(abs, 'A2\n');
    probe.unlock();
    await clock.advance(2000);
    owner.flipPath('a.qmd', owner.createDocFor('a.qmd', 'A2\n'));
    await owner.flush();
    await waitFor(() => warns().length === 1, 'the warning');
    expect(warns()[0]!.message).toBe(GIT_OP_EDITS_DROPPED);
    await clock.advance(20_000);
    expect(warns()).toHaveLength(1);
  });
});
