/**
 * Unit tests for scripts/stage-keyring.mjs (bd-c6l13j79): the staging
 * rules for the keyring loader + platform addon packages that ship
 * inside the bundle's mini node_modules.
 *
 * Offline by construction: fixture @napi-rs trees are fabricated in a
 * temp dir and the network fetcher is injected as a fake. The real
 * `npm pack` fetcher is exercised by the release workflow (whose
 * launcher-info assertion checks the staged platform list per target).
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
// @ts-expect-error — plain .mjs module without type declarations
import { parsePlatformList, keyringVersion, stageKeyring } from '../scripts/stage-keyring.mjs';

let tmp: string;
let srcDir: string;
let outDir: string;

/** Fabricate an installed @napi-rs/<name> package. */
function installPackage(name: string, opts: { version?: string; addon?: boolean } = {}) {
  const dir = path.join(srcDir, name);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(
    path.join(dir, 'package.json'),
    JSON.stringify({ name: `@napi-rs/${name}`, version: opts.version ?? '1.3.0' }),
  );
  if (opts.addon) {
    fs.writeFileSync(path.join(dir, `keyring.${name.replace(/^keyring-/, '')}.node`), 'ELF-ish');
  }
}

beforeEach(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'keyring-staging-test-'));
  srcDir = path.join(tmp, 'src', '@napi-rs');
  outDir = path.join(tmp, 'out', '@napi-rs');
  fs.mkdirSync(srcDir, { recursive: true });
});

afterEach(() => {
  fs.rmSync(tmp, { recursive: true, force: true });
});

describe('parsePlatformList', () => {
  it('returns null for unset/blank (= stage what is installed)', () => {
    expect(parsePlatformList(undefined)).toBeNull();
    expect(parsePlatformList('')).toBeNull();
    expect(parsePlatformList('  ')).toBeNull();
  });

  it('splits, trims, and drops empties', () => {
    expect(parsePlatformList('darwin-x64, darwin-arm64 ,,')).toEqual([
      'darwin-x64',
      'darwin-arm64',
    ]);
  });
});

describe('keyringVersion', () => {
  it('reads the loader package version', () => {
    installPackage('keyring', { version: '9.9.9' });
    expect(keyringVersion(srcDir)).toBe('9.9.9');
  });
});

describe('stageKeyring', () => {
  it('without an explicit list, stages loader + every installed platform', () => {
    installPackage('keyring');
    installPackage('keyring-darwin-arm64', { addon: true });
    const staged = stageKeyring({ napiSrcDir: srcDir, outNapiDir: outDir, platforms: null });
    expect(staged).toEqual(['keyring', 'keyring-darwin-arm64']);
    expect(fs.existsSync(path.join(outDir, 'keyring', 'package.json'))).toBe(true);
    expect(
      fs.existsSync(path.join(outDir, 'keyring-darwin-arm64', 'keyring.darwin-arm64.node')),
    ).toBe(true);
  });

  it('explicit list: stages exactly loader + requested platforms, not other installed ones', () => {
    installPackage('keyring');
    installPackage('keyring-darwin-arm64', { addon: true });
    installPackage('keyring-linux-x64-gnu', { addon: true });
    const staged = stageKeyring({
      napiSrcDir: srcDir,
      outNapiDir: outDir,
      platforms: ['darwin-arm64'],
    });
    expect(staged).toEqual(['keyring', 'keyring-darwin-arm64']);
    expect(fs.existsSync(path.join(outDir, 'keyring-linux-x64-gnu'))).toBe(false);
  });

  it('fetches requested platforms that are not installed, at the loader version', () => {
    installPackage('keyring', { version: '1.3.0' });
    installPackage('keyring-darwin-arm64', { addon: true });
    const fetched: Array<[string, string]> = [];
    const staged = stageKeyring({
      napiSrcDir: srcDir,
      outNapiDir: outDir,
      platforms: ['darwin-arm64', 'darwin-x64'],
      fetchPackage: (name: string, version: string, destDir: string) => {
        fetched.push([name, version]);
        fs.mkdirSync(destDir, { recursive: true });
        fs.writeFileSync(path.join(destDir, 'package.json'), '{}');
        fs.writeFileSync(path.join(destDir, 'keyring.darwin-x64.node'), 'ELF-ish');
      },
    });
    expect(fetched).toEqual([['@napi-rs/keyring-darwin-x64', '1.3.0']]);
    expect(staged).toEqual(['keyring', 'keyring-darwin-arm64', 'keyring-darwin-x64']);
  });

  it('fails closed when a requested platform cannot be fetched', () => {
    installPackage('keyring');
    installPackage('keyring-darwin-arm64', { addon: true });
    expect(() =>
      stageKeyring({
        napiSrcDir: srcDir,
        outNapiDir: outDir,
        platforms: ['linux-riscv64-gnu'],
        fetchPackage: () => {
          throw new Error('registry unreachable');
        },
      }),
    ).toThrow(/keyring-linux-riscv64-gnu.*fetch failed.*registry unreachable/);
  });

  it('fails closed when a staged platform package lacks its .node addon', () => {
    installPackage('keyring');
    installPackage('keyring-darwin-arm64', { addon: false });
    expect(() =>
      stageKeyring({ napiSrcDir: srcDir, outNapiDir: outDir, platforms: ['darwin-arm64'] }),
    ).toThrow(/no \.node addon/);
  });

  it('fails when the loader itself is missing', () => {
    installPackage('keyring-darwin-arm64', { addon: true });
    expect(() =>
      stageKeyring({ napiSrcDir: srcDir, outNapiDir: outDir, platforms: null }),
    ).toThrow(/loader not found/);
  });

  it('fails when nothing but the loader would be staged', () => {
    installPackage('keyring');
    expect(() =>
      stageKeyring({ napiSrcDir: srcDir, outNapiDir: outDir, platforms: null }),
    ).toThrow(/at least one platform package/);
  });
});
