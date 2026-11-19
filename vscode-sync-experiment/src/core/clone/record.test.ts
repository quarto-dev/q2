import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
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
});
