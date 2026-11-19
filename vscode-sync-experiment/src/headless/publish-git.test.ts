import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { Repo } from '@automerge/automerge-repo/slim';
import { readIndexFields, viewOf, type Sidecar } from '@quarto/quarto-git-epochs';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';
import { cleanupGitTempDirs, git, repoWith, rootCommits } from '../core/clone/git-fixtures';
import { findDoc, webSocketAdapter } from '../core/repo';
import { startHub } from '../core/test-support';
import { ensureWasm } from '../core/wasm';
import { publishGit } from './publish-git';

let hub: TestHub;
beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
afterAll(() => hub.stop());
afterEach(() => cleanupGitTempDirs());

describe('publishGit', () => {
  it('writes a version 4 index with the sidecar, the repo identity, and one HEAD-blob entry per tracked file', async () => {
    const dir = repoWith({ 'index.qmd': 'hi\n', 'sub/a.qmd': 'a\n' });
    const pub = await publishGit(dir, hub.url, hub);
    const reader = new Repo({ network: [webSocketAdapter(hub.url)] });
    try {
      const index = (await findDoc<IndexDocument>(reader, pub.indexDocId)).doc()!;
      const f = readIndexFields(index);
      expect(index.version).toBe(4);
      expect(f.repo).toEqual({ roots: rootCommits(dir), prefix: '' });
      const sidecar = (await findDoc<Sidecar>(reader, f.epochs!)).doc()!;
      const entries = viewOf(sidecar).entries.sort((a, b) => a.path.localeCompare(b.path));
      expect(entries.map((e) => [e.path, e.blob, e.gen])).toEqual([
        ['index.qmd', git(dir, 'rev-parse', 'HEAD:index.qmd'), 0],
        ['sub/a.qmd', git(dir, 'rev-parse', 'HEAD:sub/a.qmd'), 0],
      ]);
      expect(index.files['index.qmd']).toBe(entries[0]!.doc);
    } finally {
      await reader.shutdown().catch(() => {});
      await pub.stop();
    }
  });
});

describe('joinAsClone and joinAsMirror', () => {
  it('a clone attaches and follows a doc edit to disk; a mirror gets every file; an unrelated folder is refused', async () => {
    const { HeadlessPeer } = await import('./peer');
    const { cloneRepo } = await import('../core/clone/git-fixtures');
    const { waitFor, readText, tempDir } = await import('../core/test-support');
    const { Repo } = await import('@automerge/automerge-repo/slim');
    const path = await import('node:path');
    const origin = repoWith({ 'index.qmd': 'hi\n' });
    const pub = await HeadlessPeer.publishGit(origin, hub.url, hub);
    const peers: { stop(): Promise<void> }[] = [];
    try {
      const clone = cloneRepo(origin);
      const c = await HeadlessPeer.joinAsClone(clone, hub.url, pub.indexDocId);
      peers.push(c);
      expect(Object.keys(c.cloneState()!.attached)).toEqual(['index.qmd']);
      const edit = new Repo({ network: [(await import('../core/repo')).webSocketAdapter(hub.url)] });
      try {
        const h = await findDoc<{ text: string }>(edit, pub.docs['index.qmd']!.docId);
        h.change((d) => void (d.text = 'edited\n'));
        await hub.hubHasHeads(h.documentId, h.heads() as string[], 15_000);
        await waitFor(async () => (await readText(path.join(clone, 'index.qmd'))) === 'edited\n', 'the edit on the clone disk');
      } finally {
        await edit.shutdown().catch(() => {});
      }
      const mirrorDir = await tempDir('ge10-mirror-');
      peers.push(await HeadlessPeer.joinAsMirror(mirrorDir, hub.url, pub.indexDocId));
      expect(await readText(path.join(mirrorDir, 'index.qmd'))).toBe('edited\n');
      await expect(HeadlessPeer.joinAsClone(repoWith({ 'x.qmd': 'x\n' }, 'unrelated'), hub.url, pub.indexDocId)).rejects.toThrow(/identity-mismatch/);
    } finally {
      for (const p of peers) await p.stop().catch(() => {});
      await pub.stop();
    }
  });
});
