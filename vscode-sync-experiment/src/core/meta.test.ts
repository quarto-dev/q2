import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { InvalidProjectMetaError, readProjectMeta, writeProjectMeta, type ProjectMode } from './meta';
import { tempDir } from './test-support';

let root: string;
beforeEach(async () => {
  root = await tempDir('ge9-meta-');
});
afterEach(async () => {
  await fs.rm(root, { recursive: true, force: true });
});

async function writeRaw(json: unknown): Promise<void> {
  await fs.mkdir(path.join(root, '.quarto-hub'), { recursive: true });
  await fs.writeFile(path.join(root, '.quarto-hub', 'project.json'), JSON.stringify(json));
}

const base = { indexDocId: 'abc', syncServer: 'ws://x', createdAt: '2026-01-01T00:00:00.000Z' };

describe('project mode (G19)', () => {
  it('reads a folder with no mode as a mirror', async () => {
    await writeRaw(base);
    expect((await readProjectMeta(root))?.mode).toBe('mirror');
  });

  it.each<ProjectMode>(['mirror', 'editor-only', 'clone'])('round-trips %s', async (mode) => {
    await writeProjectMeta(root, { ...base, mode });
    expect(await readProjectMeta(root)).toEqual({ ...base, mode });
    expect(JSON.parse(await fs.readFile(path.join(root, '.quarto-hub', 'project.json'), 'utf8')).mode).toBe(mode);
  });

  it('rejects an unknown mode with a message that names the value and the choices', async () => {
    await writeRaw({ ...base, mode: 'sideways' });
    const err = await readProjectMeta(root).catch((e) => e);
    expect(err).toBeInstanceOf(InvalidProjectMetaError);
    expect((err as Error).message).toContain('"sideways"');
    expect((err as Error).message).toContain('mirror, editor-only, clone');
  });

  it('still returns null for a missing or unparseable file', async () => {
    expect(await readProjectMeta(root)).toBeNull();
    await fs.mkdir(path.join(root, '.quarto-hub'), { recursive: true });
    await fs.writeFile(path.join(root, '.quarto-hub', 'project.json'), '{nope');
    expect(await readProjectMeta(root)).toBeNull();
  });
});
