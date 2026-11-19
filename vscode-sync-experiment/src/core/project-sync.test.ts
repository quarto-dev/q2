/**
 * Characterization tests of today's sync engine (GE-8): they pin what the
 * code does now, so the extraction into `src/core/` is provably a no-op. Where
 * today's behavior is something a later plan replaces, the test says so.
 *
 * Each test runs a real `ProjectSync` on fakes (watcher, buffer host,
 * notifier) against an in-process test hub, with a second client (`Seeder`)
 * playing the collaborator.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import type { DocumentId } from '@automerge/automerge-repo';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { createProjectFolder } from './create-folder';
import { readProjectMeta } from './meta';
import { ProjectSync } from './project-sync';
import {
  Seeder,
  identity,
  makeFakeHost,
  readText,
  sleep,
  startHub,
  tempDir,
  waitFor,
  type FakeHost,
} from './test-support';
import { ensureWasm } from './wasm';

let hub: TestHub;
let parent: string;
let seeder: Seeder;
let host: FakeHost;
let syncs: ProjectSync[];

beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(async () => {
  await hub.stop();
});
beforeEach(async () => {
  parent = await tempDir();
  seeder = new Seeder(hub);
  host = makeFakeHost();
  syncs = [];
});
afterEach(async () => {
  for (const s of syncs) await s.dispose().catch(() => {});
  await seeder.stop();
  // automerge-repo's storage can still be writing for a moment after shutdown.
  await fs.rm(parent, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
});

const PNG = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3]);

async function hubIndex(id: string): Promise<IndexDocument> {
  const h = await hub.repo.find<IndexDocument>(`automerge:${id}` as never);
  return h.doc() as IndexDocument;
}
async function hubText(docId: string): Promise<string | undefined> {
  const h = await hub.repo.find<{ text: string }>(docId as DocumentId);
  return h.doc()?.text;
}
async function folderFrom(indexId: string): Promise<string> {
  return createProjectFolder(indexId, parent, host.log, { syncServer: hub.url });
}
async function startSync(root: string): Promise<ProjectSync> {
  const meta = await readProjectMeta(root);
  if (!meta) throw new Error('no project.json');
  const sync = new ProjectSync(root, meta, identity, host);
  syncs.push(sync);
  await sync.start();
  return sync;
}
async function events(root: string): Promise<{ event: string; path: string; source: string }[]> {
  const text = (await readText(path.join(root, '.quarto-hub', 'events.jsonl'))) ?? '';
  return text
    .split('\n')
    .filter(Boolean)
    .map((l) => JSON.parse(l));
}
const files = { 'a.qmd': 'A\n', 'docs/b.qmd': 'B\n', 'img.png': PNG };

describe('createProjectFolder', () => {
  it('writes the project, project.json, folders, and .vscode settings', async () => {
    const id = await seeder.create({ ...files, '_quarto.yml': 'title: "My Site"\n' }, ['empty']);
    const root = await folderFrom(id);
    expect(path.basename(root)).toBe('My-Site');
    expect(await readText(path.join(root, 'a.qmd'))).toBe('A\n');
    expect(await readText(path.join(root, 'docs/b.qmd'))).toBe('B\n');
    expect(new Uint8Array(await fs.readFile(path.join(root, 'img.png')))).toEqual(PNG);
    expect((await fs.stat(path.join(root, 'empty'))).isDirectory()).toBe(true);
    const meta = await readProjectMeta(root);
    expect(meta).toMatchObject({ indexDocId: id, syncServer: hub.url });
    expect(JSON.parse((await readText(path.join(root, '.vscode', 'settings.json')))!)).toMatchObject({ 'files.autoSave': 'afterDelay' });
  });

  it('falls back to a name made of the id, and never reuses an existing folder name', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const first = await folderFrom(id);
    const second = await folderFrom(id);
    expect(path.basename(first)).toBe(`quarto-hub-${id.slice(0, 8)}`);
    expect(path.basename(second)).toBe(`quarto-hub-${id.slice(0, 8)}-2`);
  });

  it('skips a hostile index key, logs it, and still writes the other keys (G36)', async () => {
    const id = await seeder.create({
      'ok.qmd': 'ok\n',
      '../evil.qmd': 'evil\n',
      '.git/hooks/pre-commit': 'evil\n',
      'sub/.quarto-hub/x': 'evil\n',
    });
    const root = await folderFrom(id);
    expect(await readText(path.join(root, 'ok.qmd'))).toBe('ok\n');
    expect(await readText(path.join(parent, 'evil.qmd'))).toBeNull();
    expect(await readText(path.join(root, '.git/hooks/pre-commit'))).toBeNull();
    expect(host.log.lines.filter((l) => l.startsWith('skipped index key'))).toHaveLength(3);
  });
});

describe('ProjectSync: startup reconciliation (reconcileDisk)', () => {
  it('folds offline disk edits into the docs, removes entries for missing files, adds new files', async () => {
    const id = await seeder.create(files);
    const root = await folderFrom(id);
    await fs.writeFile(path.join(root, 'a.qmd'), 'A edited offline\n');
    await fs.rm(path.join(root, 'docs/b.qmd'));
    await fs.writeFile(path.join(root, 'new.qmd'), 'brand new\n');
    const changedPng = new Uint8Array([...PNG, 99]);
    await fs.writeFile(path.join(root, 'img.png'), changedPng);

    await startSync(root);

    const index = await waitFor(async () => {
      const i = await hubIndex(id);
      return 'new.qmd' in i.files && !('docs/b.qmd' in i.files) ? i : null;
    }, 'the index on the hub to reflect the offline changes');
    expect(await waitFor(async () => (await hubText(index.files['a.qmd']!)) === 'A edited offline\n', 'a.qmd on the hub')).toBe(true);
    expect(await hubText(index.files['new.qmd']!)).toBe('brand new\n');
    const evs = await events(root);
    expect(evs).toContainEqual(expect.objectContaining({ event: 'edited-offline', path: 'a.qmd', source: 'local' }));
    expect(evs).toContainEqual(expect.objectContaining({ event: 'deleted', path: 'docs/b.qmd', source: 'local' }));
    expect(evs).toContainEqual(expect.objectContaining({ event: 'created', path: 'new.qmd', source: 'local' }));
    expect(evs).toContainEqual(expect.objectContaining({ event: 'edited-offline', path: 'img.png', source: 'local' }));
  });

  it('does not treat a subfolder with its own .quarto-hub/project.json as part of the project', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await fs.mkdir(path.join(root, 'mirror/.quarto-hub'), { recursive: true });
    await fs.writeFile(path.join(root, 'mirror/.quarto-hub/project.json'), '{"indexDocId":"x","syncServer":"ws://x","createdAt":"t"}');
    await fs.writeFile(path.join(root, 'mirror/x.qmd'), 'inner\n');
    await fs.writeFile(path.join(root, 'outer.qmd'), 'outer\n');
    await startSync(root);
    await waitFor(async () => 'outer.qmd' in (await hubIndex(id)).files, 'outer.qmd on the hub');
    await sleep(300);
    expect(Object.keys((await hubIndex(id)).files).sort()).toEqual(['a.qmd', 'outer.qmd']);
  });

  it('binds an already-open buffer to its doc, and the two stay in sync', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    const abs = path.join(root, 'a.qmd');
    host.buffers.open(abs, 'A\n');
    await startSync(root);
    expect(host.buffers.bindings.has(abs)).toBe(true);

    host.buffers.type(abs, 'A typed\n');
    await waitFor(async () => (await hubText((await hubIndex(id)).files['a.qmd']!)) === 'A typed\n', 'the typed edit on the hub');

    const h = await seeder.repo.find<{ text: string }>(`automerge:${(await hubIndex(id)).files['a.qmd']!}` as never);
    h.change((d) => {
      d.text = 'A typed\nremote\n';
    });
    await waitFor(() => host.buffers.getText(abs) === 'A typed\nremote\n', 'the remote edit in the buffer');
  });
});

describe('ProjectSync: disk to hub', () => {
  it('turns a created text file and a created binary file into docs and index entries', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    await fs.writeFile(path.join(root, 'c.qmd'), 'created\n');
    await fs.writeFile(path.join(root, 'p.png'), PNG);
    host.watcher.fire('create', path.join(root, 'c.qmd'));
    host.watcher.fire('create', path.join(root, 'p.png'));
    const index = await waitFor(async () => {
      const i = await hubIndex(id);
      return 'c.qmd' in i.files && 'p.png' in i.files ? i : null;
    }, 'both new files in the hub index');
    expect(await waitFor(async () => (await hubText(index.files['c.qmd']!)) === 'created\n', 'c.qmd text')).toBe(true);
    const bin = await hub.repo.find<{ content: Uint8Array; mimeType: string }>(index.files['p.png'] as DocumentId);
    expect(new Uint8Array(bin.doc()!.content)).toEqual(PNG);
    expect(bin.doc()!.mimeType).toBe('image/png');
  });

  it('prefers the open buffer text over the disk text for a created file', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    const abs = path.join(root, 'o.qmd');
    await fs.writeFile(abs, 'stale on disk\n');
    host.buffers.open(abs, 'fresh in the buffer\n'); // open before the watcher reports the file
    host.watcher.fire('create', abs);
    const index = await waitFor(async () => {
      const i = await hubIndex(id);
      return 'o.qmd' in i.files ? i : null;
    }, 'o.qmd in the hub index');
    expect(await waitFor(async () => (await hubText(index.files['o.qmd']!)) === 'fresh in the buffer\n', 'o.qmd text')).toBe(true);
    // track() bound the buffer once the handle existed.
    expect(host.buffers.bindings.has(abs)).toBe(true);
  });

  it('writes a changed (unbound) file into its doc, and ignores the echo of its own write', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    await fs.writeFile(path.join(root, 'a.qmd'), 'A changed\n');
    host.watcher.fire('change', path.join(root, 'a.qmd'));
    await waitFor(async () => (await hubText((await hubIndex(id)).files['a.qmd']!)) === 'A changed\n', 'the changed text on the hub');
    host.watcher.fire('change', path.join(root, 'a.qmd')); // the doc already equals the disk: nothing to do
  });

  it('removes the entries of a deleted file, and of every file below a deleted folder', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n', 'docs/b.qmd': 'B\n', 'docs/deep/c.qmd': 'C\n', 'keep.qmd': 'K\n' });
    const root = await folderFrom(id);
    await startSync(root);
    await fs.rm(path.join(root, 'a.qmd'));
    host.watcher.fire('delete', path.join(root, 'a.qmd'));
    await fs.rm(path.join(root, 'docs'), { recursive: true });
    host.watcher.fire('delete', path.join(root, 'docs')); // a directory arrives as one event
    await waitFor(async () => Object.keys((await hubIndex(id)).files).join() === 'keep.qmd', 'only keep.qmd left in the index');
    const evs = await events(root);
    expect(evs.filter((e) => e.event === 'deleted' && e.source === 'local').map((e) => e.path).sort()).toEqual(['a.qmd', 'docs/b.qmd', 'docs/deep/c.qmd']);
  });

  it('records an empty folder in the index, and logs it', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    await fs.mkdir(path.join(root, 'newdir'));
    host.watcher.fire('create', path.join(root, 'newdir'));
    await waitFor(async () => (await hubIndex(id)).folders?.['newdir'] === true, 'the folder entry on the hub');
    expect(await events(root)).toContainEqual(expect.objectContaining({ event: 'created', path: 'newdir/', source: 'local' }));
  });

  it('ignores events for ignored paths', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    for (const rel of ['.git/config', '.vscode/x.json', 'node_modules/p/i.js', '_site/i.html', '.DS_Store']) {
      await fs.mkdir(path.dirname(path.join(root, rel)), { recursive: true });
      await fs.writeFile(path.join(root, rel), 'x');
      host.watcher.fire('create', path.join(root, rel));
    }
    await sleep(300);
    expect(Object.keys((await hubIndex(id)).files)).toEqual(['a.qmd']);
  });
});

describe('ProjectSync: hub to disk (syncIndex)', () => {
  it('writes a file whose index entry appears, and removes a file whose entry is deleted', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    await seeder.putFile('docs/new.qmd', 'from the hub\n');
    await waitFor(async () => (await readText(path.join(root, 'docs/new.qmd'))) === 'from the hub\n', 'docs/new.qmd on disk');
    expect(await events(root)).toContainEqual(expect.objectContaining({ event: 'created', path: 'docs/new.qmd', source: 'hub' }));
    await seeder.removeFile('docs/new.qmd');
    await waitFor(async () => (await readText(path.join(root, 'docs/new.qmd'))) === null, 'docs/new.qmd gone from disk');
    expect(await events(root)).toContainEqual(expect.objectContaining({ event: 'deleted', path: 'docs/new.qmd', source: 'hub' }));
  });

  // PIN (GE-9 replaces this): when a path's doc id changes, today's code
  // unconditionally untracks, retracks the new doc, and overwrites the disk
  // file with the new doc's content, with no check of what the disk holds.
  it('overwrites the disk file when an index entry changes its doc id (unconditional untrack, retrack, write)', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    await fs.writeFile(path.join(root, 'a.qmd'), 'local unsynced text\n'); // not reported to the watcher
    const replacement = await seeder.putFile('a.qmd', 'replacement doc\n');
    expect(replacement.documentId).toBeTruthy();
    await waitFor(async () => (await readText(path.join(root, 'a.qmd'))) === 'replacement doc\n', 'a.qmd rewritten from the new doc');
  });

  // PIN (GE-10 gates this): a doc change rewrites a file that is missing on disk.
  it('rewrites a file that is missing on disk when its doc changes', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    await fs.rm(path.join(root, 'a.qmd')); // no watcher event
    const h = await seeder.repo.find<{ text: string }>(`automerge:${seeder.index.doc()!.files['a.qmd']}` as never);
    h.change((d) => {
      d.text = 'A, edited by the collaborator\n';
    });
    await waitFor(async () => (await readText(path.join(root, 'a.qmd'))) === 'A, edited by the collaborator\n', 'a.qmd rewritten from its doc');
  });

  it('creates explicit folders from the index and removes them when they leave it', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    seeder.index.change((d) => void ((d.folders ??= {})['made'] = true));
    await seeder.flush();
    await waitFor(async () => (await fs.stat(path.join(root, 'made')).catch(() => null))?.isDirectory(), 'the folder on disk');
    seeder.index.change((d) => void delete d.folders!['made']);
    await seeder.flush();
    await waitFor(async () => (await fs.stat(path.join(root, 'made')).catch(() => null)) === null, 'the folder removed');
  });

  it('skips a hostile key, logs it as a rejected event, and still applies the other keys (G36)', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    await seeder.putFile('../live-evil.qmd', 'evil\n');
    await seeder.putFile('.git/hooks/post-merge', 'evil\n');
    await seeder.putFile('fine.qmd', 'fine\n');
    await waitFor(async () => (await readText(path.join(root, 'fine.qmd'))) === 'fine\n', 'fine.qmd on disk');
    expect(await readText(path.join(parent, 'live-evil.qmd'))).toBeNull();
    expect(await readText(path.join(root, '.git/hooks/post-merge'))).toBeNull();
    const rejected = (await events(root)).filter((e) => e.event === 'rejected').map((e) => e.path).sort();
    expect(rejected).toEqual(['.git/hooks/post-merge', '../live-evil.qmd'].sort());
    expect(host.log.lines.some((l) => l.startsWith('skipped index key'))).toBe(true);
  });
});

describe('ProjectSync: lifecycle', () => {
  it('reports status through the notifier and logs a running line', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    await startSync(root);
    expect(host.log.lines.some((l) => l.startsWith('project sync running:'))).toBe(true);
    expect(host.notifier.statuses.some((s) => s?.includes('syncing'))).toBe(true);
  });

  it('stops reacting to the hub after dispose', async () => {
    const id = await seeder.create({ 'a.qmd': 'A\n' });
    const root = await folderFrom(id);
    const sync = await startSync(root);
    await sync.dispose();
    await seeder.putFile('late.qmd', 'late\n');
    await sleep(500);
    expect(await readText(path.join(root, 'late.qmd'))).toBeNull();
  });
});
