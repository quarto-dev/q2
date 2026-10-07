import { describe, expect, it } from 'vitest';
import { commitImport, type ImportStorageDeps } from './importStorage';
import type { ImportDiagnostic, ImportOutcome } from '../pandoc/importService';

/** An in-memory project index with the same observable behaviour as the sync layer's three calls. */
function fakeSync(opts: { existing?: Record<string, string>; failOn?: string; failDelete?: string; renameTo?: Record<string, string> } = {}) {
  const index = new Map<string, string>(Object.entries(opts.existing ?? {}));
  const hashes = new Map<string, string>();
  const calls: string[] = [];
  let n = 0;
  const deps: ImportStorageDeps = {
    async createBinaryFile(path, bytes) {
      calls.push(`bin:${path}`);
      if (opts.failOn === path) throw new Error('disk on fire');
      const hash = Array.from(bytes).join(',');
      if (index.has(path) && hashes.get(path) === hash) return { docId: index.get(path)!, path, deduplicated: true };
      const actual = opts.renameTo?.[path] ?? (index.has(path) ? `${path}-renamed` : path);
      const docId = `doc${++n}`;
      index.set(actual, docId);
      hashes.set(actual, hash);
      return { docId, path: actual, deduplicated: false };
    },
    async createFileIfAbsent(path, content) {
      calls.push(`qmd:${path}`);
      if (opts.failOn === path) throw new Error('disk on fire');
      if (index.has(path)) return { created: false };
      const docId = `doc${++n}`;
      index.set(path, docId);
      hashes.set(path, content);
      return { created: true, docId };
    },
    deleteFile(path) {
      calls.push(`del:${path}`);
      if (opts.failDelete === path) throw new Error('cannot delete');
      index.delete(path);
    },
    indexedDocId: (path) => index.get(path),
  };
  return { deps, index, calls };
}

const warn: ImportDiagnostic = { origin: 'rust', kind: 'warning', code: 'Q-24-4', title: 'Reader warning', problem: 'p' };
const outcome = (media: string[], diagnostics: ImportDiagnostic[] = [warn]): Extract<ImportOutcome, { ok: true }> => ({
  ok: true,
  qmd: '# Doc\n',
  media: media.map((projectPath, i) => ({ projectPath, bytes: Uint8Array.from([i + 1]), mimeType: 'image/png' })),
  diagnostics,
});
const codes = (r: { diagnostics: ImportDiagnostic[] }) => r.diagnostics.map((d) => (d.origin === 'host' ? d.code : d.code));

describe('commitImport', () => {
  it('writes the images first and the qmd last, and passes the report through', async () => {
    const { deps, index, calls } = fakeSync();
    const r = await commitImport(outcome(['a_media/1.png', 'a_media/2.png']), 'a.qmd', deps);
    expect(r).toMatchObject({ ok: true, qmdPath: 'a.qmd', qmd: '# Doc\n', diagnostics: [warn] });
    expect(r.ok && r.qmdDocId).toBe(index.get('a.qmd'));
    expect(calls).toEqual(['bin:a_media/1.png', 'bin:a_media/2.png', 'qmd:a.qmd']);
    expect([...index.keys()].sort()).toEqual(['a.qmd', 'a_media/1.png', 'a_media/2.png']);
  });

  it('succeeds with no media', async () => {
    const { deps, calls } = fakeSync();
    const r = await commitImport(outcome([], []), 'a.qmd', deps);
    expect(r.ok).toBe(true);
    expect(calls).toEqual(['qmd:a.qmd']);
  });

  it('a failing image write rolls back the earlier images, in reverse, and never writes the qmd', async () => {
    const { deps, index, calls } = fakeSync({ failOn: 'a_media/3.png' });
    const r = await commitImport(outcome(['a_media/1.png', 'a_media/2.png', 'a_media/3.png']), 'a.qmd', deps);
    expect(r.ok).toBe(false);
    expect(codes(r)).toEqual(['import-write-failed', 'Q-24-4']);
    const first = r.diagnostics[0];
    expect(first.origin === 'host' && first.message).toContain('image 3 of 3');
    expect(first.origin === 'host' && first.message).toContain('a_media/3.png');
    expect(first.origin === 'host' && first.message).toContain('disk on fire');
    expect(calls).toEqual(['bin:a_media/1.png', 'bin:a_media/2.png', 'bin:a_media/3.png', 'del:a_media/2.png', 'del:a_media/1.png']);
    expect(index.size).toBe(0);
  });

  it('a qmd that is refused (a file appeared) removes every image and names the path', async () => {
    const { deps, index } = fakeSync({ existing: { 'a.qmd': 'someone-elses' } });
    const r = await commitImport(outcome(['a_media/1.png']), 'a.qmd', deps);
    expect(r.ok).toBe(false);
    const first = r.diagnostics[0];
    expect(first.origin === 'host' && first.code).toBe('import-write-failed');
    expect(first.origin === 'host' && first.message).toContain('the document');
    expect(first.origin === 'host' && first.message).toContain('a.qmd');
    expect(first.origin === 'host' && first.message).toContain('appeared');
    // The other client's file is untouched; our image is gone.
    expect([...index.entries()]).toEqual([['a.qmd', 'someone-elses']]);
  });

  it('a throwing qmd write is a failure with cleanup', async () => {
    const { deps, index } = fakeSync({ failOn: 'a.qmd' });
    const r = await commitImport(outcome(['a_media/1.png']), 'a.qmd', deps);
    expect(r.ok).toBe(false);
    expect(codes(r)).toEqual(['import-write-failed', 'Q-24-4']);
    expect(index.size).toBe(0);
  });

  it('does not delete a deduplicated hit on rollback', async () => {
    // The project already holds identical bytes (the first image's bytes are [1]) at the first path.
    const { deps, index } = fakeSync({ existing: { 'a_media/1.png': 'existing-doc' } });
    (deps as { createBinaryFile: ImportStorageDeps['createBinaryFile'] }).createBinaryFile = (() => {
      const real = deps.createBinaryFile;
      return async (path, bytes, mime) => (path === 'a_media/1.png' ? { docId: 'existing-doc', path, deduplicated: true } : real(path, bytes, mime));
    })();
    const r = await commitImport(outcome(['a_media/1.png', 'a_media/2.png']), 'a.qmd', { ...deps, createFileIfAbsent: async () => ({ created: false }) });
    expect(r.ok).toBe(false);
    expect(index.get('a_media/1.png')).toBe('existing-doc');
    expect(index.has('a_media/2.png')).toBe(false);
  });

  it('deletes a renamed media write on rollback, and fails the import', async () => {
    // A file with different content sits at the first path; the sync layer stores ours under another name.
    const { deps, index } = fakeSync({ existing: { 'a_media/1.png': 'doc-other' } });
    const r = await commitImport(outcome(['a_media/1.png']), 'a.qmd', deps);
    expect(r.ok).toBe(false);
    const first = r.diagnostics[0];
    expect(first.origin === 'host' && first.code).toBe('import-write-failed');
    expect(first.origin === 'host' && first.message).toContain('a_media/1.png-renamed');
    expect([...index.keys()]).toEqual(['a_media/1.png']);
    expect(index.get('a_media/1.png')).toBe('doc-other');
  });

  it('does not delete an entry another client replaced, and reports it', async () => {
    const { deps, index } = fakeSync();
    const realCreate = deps.createFileIfAbsent;
    deps.createFileIfAbsent = async (path, content) => {
      // Between the image write and the qmd write, another client replaces the image entry.
      index.set('a_media/1.png', 'other-clients-doc');
      return realCreate(path, content).then(() => ({ created: false }));
    };
    const r = await commitImport(outcome(['a_media/1.png']), 'a.qmd', deps);
    expect(r.ok).toBe(false);
    expect(codes(r)).toEqual(['import-write-failed', 'import-cleanup-failed', 'Q-24-4']);
    expect(index.get('a_media/1.png')).toBe('other-clients-doc');
  });

  it('reports a cleanup deletion that fails, and still removes the rest', async () => {
    const { deps, index } = fakeSync({ failDelete: 'a_media/2.png', existing: { 'a.qmd': 'x' } });
    const r = await commitImport(outcome(['a_media/1.png', 'a_media/2.png']), 'a.qmd', deps);
    expect(r.ok).toBe(false);
    expect(codes(r)).toEqual(['import-write-failed', 'import-cleanup-failed', 'Q-24-4']);
    const cleanup = r.diagnostics[1];
    expect(cleanup.origin === 'host' && cleanup.message).toContain('a_media/2.png');
    expect(index.has('a_media/1.png')).toBe(false);
    expect(index.has('a_media/2.png')).toBe(true);
  });
});
