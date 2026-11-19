/** The registry client: the sidecar handle, lazy creation, entries as single changes, and the creation race (specs 6, 11, 13). */
import { Repo, type DocHandle } from '@automerge/automerge-repo/slim';
import { createIndexDocumentContent, type IndexDocument } from '@quarto/quarto-automerge-schema';
import { afterEach, describe, expect, it } from 'vitest';
import { ensureWasm } from '../wasm';
import { RegistryClient } from './registry-client';

const repos: Repo[] = [];
afterEach(async () => {
  for (const r of repos.splice(0)) await r.shutdown().catch(() => {});
});

async function setup() {
  await ensureWasm();
  const repo = new Repo();
  repos.push(repo);
  const index = repo.create<IndexDocument>(createIndexDocumentContent());
  return { repo, index };
}

describe('RegistryClient', () => {
  it('creates the sidecar lazily, writes the index key once, and reads entries back', async () => {
    const { repo, index } = await setup();
    const rc = new RegistryClient(repo, index);
    expect(index.doc()!.epochs).toBeUndefined();
    await rc.ensure();
    const id = String(index.doc()!.epochs);
    expect(id).toBeTruthy();
    await rc.ensure();
    expect(String(index.doc()!.epochs)).toBe(id);
    rc.register({ path: 'a.qmd', blob: 'b1', doc: 'd1', gen: 0 });
    rc.register({ path: 'a.qmd', blob: 'b1', doc: 'd1', gen: 0 }); // put-if-absent
    expect(rc.view().entries).toEqual([{ path: 'a.qmd', blob: 'b1', doc: 'd1', gen: 0 }]);
    expect(rc.canonical('a.qmd', 'b1')?.doc).toBe('d1');
    expect(rc.nextGeneration('a.qmd')).toBe(1);
  });

  it('moveForward is one change that keeps the generation; retire is skipped by canonical selection', async () => {
    const { repo, index } = await setup();
    const rc = new RegistryClient(repo, index);
    await rc.ensure();
    rc.register({ path: 'a.qmd', blob: 'b1', doc: 'd1', gen: 3 });
    const before = rc.heads().length;
    rc.moveForward({ path: 'a.qmd', oldBlob: 'b1', newBlob: 'b2', doc: 'd1' });
    expect(rc.view().entries).toEqual([{ path: 'a.qmd', blob: 'b2', doc: 'd1', gen: 3 }]);
    expect(rc.heads().length).toBe(before); // one linear change on top
    rc.register({ path: 'a.qmd', blob: 'b2', doc: 'd0', gen: 4 });
    expect(rc.canonical('a.qmd', 'b2')?.doc).toBe('d1');
    rc.retire({ path: 'a.qmd', blob: 'b2', doc: 'd1' });
    expect(rc.canonical('a.qmd', 'b2')?.doc).toBe('d0');
  });

  it('a lost sidecar race adopts the winner and reports it so attached docs can be re-registered', async () => {
    const { repo, index } = await setup();
    const a = new RegistryClient(repo, index);
    const switched: string[] = [];
    a.onSidecarSwitched = (id) => switched.push(id);
    await a.ensure();
    const mine = String(index.doc()!.epochs);
    // Another peer wins the index key with its own sidecar.
    const winner = repo.create<{ version: number; entries: Record<string, number>; retired: Record<string, number> }>({ version: 1, entries: {}, retired: {} });
    index.change((d) => void (d.epochs = winner.documentId));
    await a.refresh();
    expect(switched).toEqual([winner.documentId]);
    expect(a.sidecarId()).toBe(winner.documentId);
    expect(a.sidecarId()).not.toBe(mine);
  });

  it('repair re-registers a doc that vanished and never a retired one', async () => {
    const { repo, index } = await setup();
    const rc = new RegistryClient(repo, index);
    await rc.ensure();
    rc.repair({ path: 'a.qmd', doc: 'd1', gen: 2 }, 'b1');
    expect(rc.view().entries).toEqual([{ path: 'a.qmd', blob: 'b1', doc: 'd1', gen: 2 }]);
    rc.retire({ path: 'b.qmd', blob: 'x', doc: 'd9' });
    rc.repair({ path: 'b.qmd', doc: 'd9', gen: 1 }, 'x');
    expect(rc.view().entries.some((e) => e.doc === 'd9')).toBe(false);
  });
});

export type { DocHandle };
