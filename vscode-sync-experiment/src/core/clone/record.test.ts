import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// `rename` goes through this hook so a test can make it fail like Windows does; unset, it is the real one.
const hook = vi.hoisted(() => ({ rename: null as null | ((from: string, to: string) => Promise<void>) }));
vi.mock('node:fs/promises', async (importOriginal) => {
  const real = await importOriginal<typeof import('node:fs/promises')>();
  return { ...real, rename: (from: string, to: string) => (hook.rename ?? real.rename)(from, to) };
});
import { tempDir } from '../test-support';
import { RECORD_FILE, readRecord, removeRecord, writeRecord } from './record';

let dir: string;
beforeEach(async () => {
  dir = await tempDir('ge10-rec-');
});
afterEach(() => fs.rm(dir, { recursive: true, force: true }));

describe('the persisted record (spec 9)', () => {
  it('round-trips head, blobs, and the detached set', async () => {
    expect(await readRecord(dir)).toBeNull();
    await writeRecord(dir, { head: 'h1', blobs: { 'a.qmd': 'b1' }, detached: ['z.qmd', 'a.qmd'] });
    expect(await readRecord(dir)).toEqual({ version: 1, head: 'h1', blobs: { 'a.qmd': 'b1' }, detached: ['a.qmd', 'z.qmd'] });
    await writeRecord(dir, { head: 'h2', blobs: {} });
    expect((await readRecord(dir))?.head).toBe('h2');
    expect((await fs.readdir(path.join(dir, '.quarto-hub'))).filter((n) => n.endsWith('.tmp'))).toEqual([]);
  });

  it('treats a corrupt or foreign-version file as no record, and remove clears it', async () => {
    await fs.mkdir(path.join(dir, '.quarto-hub'));
    await fs.writeFile(path.join(dir, '.quarto-hub', RECORD_FILE), '{not json');
    expect(await readRecord(dir)).toBeNull();
    await fs.writeFile(path.join(dir, '.quarto-hub', RECORD_FILE), JSON.stringify({ version: 9, head: 'x', blobs: {} }));
    expect(await readRecord(dir)).toBeNull();
    await writeRecord(dir, { head: 'h', blobs: {} });
    await removeRecord(dir);
    expect(await readRecord(dir)).toBeNull();
  });

  it('retries a rename refused with EPERM (a Windows watcher or scanner holding the target), then succeeds', async () => {
    const real = await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises');
    let calls = 0;
    hook.rename = async (from, to) => {
      if (++calls <= 2) throw Object.assign(new Error('operation not permitted'), { code: 'EPERM' });
      return real.rename(from, to);
    };
    try {
      await writeRecord(dir, { head: 'h3', blobs: {} });
    } finally {
      hook.rename = null;
    }
    expect(calls).toBe(3);
    expect((await readRecord(dir))?.head).toBe('h3');
    expect((await fs.readdir(path.join(dir, '.quarto-hub'))).filter((n) => n.endsWith('.tmp'))).toEqual([]);
  });

  it('gives up after repeated EPERM, leaves no temp file, and keeps the old record', async () => {
    await writeRecord(dir, { head: 'old', blobs: {} });
    hook.rename = async () => {
      throw Object.assign(new Error('busy'), { code: 'EBUSY' });
    };
    try {
      await expect(writeRecord(dir, { head: 'new', blobs: {} })).rejects.toMatchObject({ code: 'EBUSY' });
    } finally {
      hook.rename = null;
    }
    expect((await readRecord(dir))?.head).toBe('old');
    expect((await fs.readdir(path.join(dir, '.quarto-hub'))).filter((n) => n.endsWith('.tmp'))).toEqual([]);
  });
});
