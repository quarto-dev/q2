import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const hook: { rename?: (from: string, to: string) => Promise<void> } = {};
vi.mock('node:fs/promises', async (importOriginal) => {
  const real = await importOriginal<typeof import('node:fs/promises')>();
  return { ...real, rename: (from: string, to: string) => (hook.rename ?? real.rename)(from, to) };
});
import { tempDir } from './test-support';
import { renameFolderIntoPlace } from './create-folder';

const refuse = (code: string) => Object.assign(new Error(`${code}: operation not permitted, rename`), { code });

let dir: string;
let from: string;
let to: string;
beforeEach(async () => {
  dir = await tempDir('ge16a-ren-');
  from = path.join(dir, '.quarto-hub-tmp-x');
  to = path.join(dir, 'project');
  await fs.mkdir(from);
  await fs.writeFile(path.join(from, 'a.qmd'), 'a');
});
afterEach(async () => {
  hook.rename = undefined;
  await fs.rm(dir, { recursive: true, force: true });
});

describe('renameFolderIntoPlace (B3: Windows refuses a directory rename while a handle is open inside it)', () => {
  it('retries a rename refused with EPERM, then succeeds', async () => {
    let calls = 0;
    const real = (await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises')).rename;
    hook.rename = async (a, b) => {
      if (++calls <= 2) throw refuse('EPERM');
      return real(a, b);
    };
    await renameFolderIntoPlace(from, to);
    expect(calls).toBe(3);
    expect(await fs.readFile(path.join(to, 'a.qmd'), 'utf8')).toBe('a');
  });

  it('waits out a handle that is released after 200 ms', async () => {
    const real = (await vi.importActual<typeof import('node:fs/promises')>('node:fs/promises')).rename;
    const releasedAt = Date.now() + 200;
    hook.rename = async (a, b) => {
      if (Date.now() < releasedAt) throw refuse('EPERM');
      return real(a, b);
    };
    await renameFolderIntoPlace(from, to);
    expect((await fs.stat(path.join(to, 'a.qmd'))).isFile()).toBe(true);
  });

  it('gives up on a permanent EBUSY, leaves the temp folder in place, and throws', async () => {
    hook.rename = async () => {
      throw refuse('EBUSY');
    };
    await expect(renameFolderIntoPlace(from, to)).rejects.toMatchObject({ code: 'EBUSY' });
    expect(await fs.readFile(path.join(from, 'a.qmd'), 'utf8')).toBe('a');
    await expect(fs.stat(to)).rejects.toMatchObject({ code: 'ENOENT' });
  });

  it('does not retry an error that is not a busy folder', async () => {
    let calls = 0;
    hook.rename = async () => {
      calls++;
      throw Object.assign(new Error('ENOENT'), { code: 'ENOENT' });
    };
    await expect(renameFolderIntoPlace(from, to)).rejects.toMatchObject({ code: 'ENOENT' });
    expect(calls).toBe(1);
  });
});
