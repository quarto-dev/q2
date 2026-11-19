/**
 * GE-14, the cross-implementation registry check. The Rust hub writes the sidecar document; the TypeScript package
 * decodes it here, from a real hub run, through a raw `Repo` connected to the hub's `/ws` (the path Gate 0 item 1
 * proved). `ts-packages/quarto-git-epochs/fixtures/golden/from-hub-run.bin` is that document, frozen: the TypeScript
 * golden tests and the Rust golden test (`crates/quarto-git-epochs/tests/integration/golden.rs`) both decode every
 * sidecar in `manifest.json`, so one committed file checks both directions of the Rust and TypeScript pair.
 *
 * With `UPDATE_GOLDEN=1` the spec rewrites the `.bin` and its manifest entry from the live hub; without it the spec
 * checks the live hub against the committed manifest entry (paths, blobs, and generations as `int`).
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from '@playwright/test';
import { MAIN_FILES, pngBytes } from '../../../../q2-preview-spa/e2e/helpers/gitRepo';
import { registryKey } from '@quarto/quarto-git-epochs';
import { Scene, expect } from '../../helpers/cross-client';
import { dumpSidecar } from '../../helpers/registry-dump';
import { ensureWasm } from '../../helpers/hub';

const here = path.dirname(fileURLToPath(import.meta.url));
const GOLDEN = path.resolve(here, '..', '..', '..', '..', 'ts-packages', 'quarto-git-epochs', 'fixtures', 'golden');
const FILE = 'from-hub-run.bin';

let scene: Scene;
test.beforeAll(async () => {
  await ensureWasm();
});
test.beforeEach(() => {
  scene = new Scene();
});
test.afterEach(async () => {
  await scene.close();
});

test('@permanent the sidecar a real hub writes decodes in TypeScript: one entry per tracked file, generations as int, nothing retired', async () => {
  const hub = await scene.trackingHub();
  const tracked = [...Object.keys(MAIN_FILES), 'img/logo.png'].sort();
  let paths: string[] = [];
  await expect
    .poll(
      async () => {
        paths = (await dumpSidecar(hub.wsUrl, hub.indexDocId)).entries.map((e) => e.path).sort();
        return paths;
      },
      { timeout: 60_000, message: 'the hub registered every tracked file' },
    )
    .toEqual(tracked);
  const dump = await dumpSidecar(hub.wsUrl, hub.indexDocId);
  expect(dump.retired).toEqual([]);
  for (const e of dump.entries) {
    expect(Number.isInteger(e.gen), `${e.path}: generation ${String(e.gen)}`).toBe(true);
    expect(e.gen).toBe(0);
    expect(e.blob).toMatch(/^[0-9a-f]{40}$/);
    // The blob is the one git has for that path at HEAD.
    expect(e.blob).toBe(hub.repo.git('rev-parse', `HEAD:${e.path}`).trim());
  }
  // The fixture's blob ids repeat across runs: the same bytes, the same ids.
  expect(hub.repo.git('rev-parse', 'HEAD:img/logo.png').trim()).toBe(hub.repo.git('hash-object', 'img/logo.png').trim());
  expect(pngBytes(10).equals(hub.repo.logoMain)).toBe(true);

  const entries = dump.entries
    .map((e) => ({ key: registryKey(e.path, e.blob, e.doc), gen: e.gen }))
    .sort((a, b) => a.key.localeCompare(b.key));
  const manifestFile = path.join(GOLDEN, 'manifest.json');
  if (process.env.UPDATE_GOLDEN === '1') {
    await fs.writeFile(path.join(GOLDEN, FILE), dump.bytes);
    const manifest = JSON.parse(await fs.readFile(manifestFile, 'utf8'));
    manifest[FILE] = { entries, retired: [] };
    await fs.writeFile(manifestFile, JSON.stringify(manifest, null, 2) + '\n');
    scene.log(`rewrote ${FILE} (${dump.bytes.length} bytes, ${entries.length} entries)`);
    return;
  }
  // Without UPDATE_GOLDEN the committed document must describe the same shape: the same paths and blob ids, generation 0.
  const manifest = JSON.parse(await fs.readFile(manifestFile, 'utf8'));
  const committed = (manifest[FILE]?.entries ?? []) as { key: string; gen: number }[];
  const shape = (xs: { key: string; gen: number }[]) => xs.map((x) => `${x.key.split('\u0000').slice(0, 2).join('@')}#${x.gen}`).sort();
  expect(shape(entries)).toEqual(shape(committed));
});
