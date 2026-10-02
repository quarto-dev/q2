import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { PACKAGE_REGISTRY } from './limits.ts';
import { repo } from './fixtures.test-util.ts';

const constants = JSON.parse(readFileSync(path.join(repo, 'resources/typst-wasm.json'), 'utf8'));
const require = createRequire(import.meta.url);

describe('resources/typst-wasm.json pins the compiler', () => {
  it('names the typst.ts version this package depends on, exactly (T2)', () => {
    const own = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'));
    expect(own.dependencies['@myriaddreamin/typst-ts-web-compiler']).toBe(constants.typst_ts_version);
    expect(own.dependencies['@myriaddreamin/typst.ts']).toBe(constants.typst_ts_version);
    const installed = JSON.parse(readFileSync(require.resolve('@myriaddreamin/typst-ts-web-compiler/package.json'), 'utf8'));
    expect(installed.version).toBe(constants.typst_ts_version);
  });

  it('matches the SHA-256 of the installed wasm', () => {
    const wasm = readFileSync(require.resolve('@myriaddreamin/typst-ts-web-compiler/wasm'));
    expect(createHash('sha256').update(wasm).digest('hex')).toBe(constants.wasm_sha256);
  });

  it('pins the typst-assets crate to the version Cargo.lock uses', () => {
    const lock = readFileSync(path.join(repo, 'Cargo.lock'), 'utf8');
    const m = /name = "typst-assets"\nversion = "([^"]+)"\nsource = "[^"]+"\nchecksum = "([0-9a-f]+)"/.exec(lock);
    expect(m?.[1]).toBe(constants.fonts_crate.version);
    expect(m?.[2]).toBe(constants.fonts_crate.crate_sha256);
    expect(constants.typst_version).toBe(constants.fonts_crate.version);
  });

  it('uses the registry host the limits module names, which the CSP must allow', () => {
    expect(constants.package_registry).toBe(PACKAGE_REGISTRY);
  });
});
