/**
 * Stage the `@napi-rs/keyring` loader + platform addon packages into a
 * bundle's mini node_modules (bd-c6l13j79; split out of bundle.mjs so
 * the staging rules are unit-testable with an injectable fetcher).
 *
 * The `.node` addon is loaded at runtime by the *user's* node, so the
 * staged platform packages must match the **release target**, not the
 * build host. Release jobs request explicit platforms via
 * `KEYRING_PLATFORMS` (e.g. `darwin-x64,darwin-arm64`); packages not
 * installed locally are fetched with `npm pack` at the loader's exact
 * version. Without an explicit request, every locally installed
 * platform package is staged (the original dev behavior: one package
 * on a dev machine, whatever the lockfile installed).
 *
 * Fail-closed: a requested platform that can neither be copied nor
 * fetched aborts the bundle. The keyring loader does full runtime
 * platform/arch/libc detection, so co-staged platforms coexist by
 * design.
 */

import { execFileSync } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
} from 'node:fs';
import * as os from 'node:os';
import { join } from 'node:path';

/** Parse `KEYRING_PLATFORMS`-style input: comma-separated, trimmed,
 * empty entries dropped; `undefined`/blank → null (= "no explicit
 * request", stage what's installed). */
export function parsePlatformList(raw) {
  if (raw === undefined || raw === null) return null;
  const list = raw
    .split(',')
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  return list.length > 0 ? list : null;
}

/** The loader package's exact version — fetched platform packages must
 * match it (napi-rs publishes loader + addons in lockstep). */
export function keyringVersion(napiSrcDir) {
  const pkgJson = join(napiSrcDir, 'keyring', 'package.json');
  return JSON.parse(readFileSync(pkgJson, 'utf8')).version;
}

/** Default fetcher: `npm pack <name>@<version>` into a temp dir, then
 * extract the tarball's `package/` to `destDir`. Network-touching; the
 * tests inject a fake instead. */
export function npmPackFetcher(name, version, destDir) {
  const tmp = mkdtempSync(join(os.tmpdir(), 'keyring-fetch-'));
  try {
    // On Windows npm is npm.cmd, which execFileSync cannot spawn
    // without a shell (ENOENT; and Node ≥20.12 rejects .cmd without
    // shell:true outright). Arguments here are fixed package
    // specifiers, so shell quoting is not a concern. Seen in the
    // v0.1.0 dry-run (run 27448388974, windows_amd64 leg).
    const windows = process.platform === 'win32';
    execFileSync(windows ? 'npm.cmd' : 'npm', ['pack', `${name}@${version}`, '--silent'], {
      cwd: tmp,
      stdio: ['ignore', 'pipe', 'pipe'],
      shell: windows,
    });
    const tgz = readdirSync(tmp).find((f) => f.endsWith('.tgz'));
    if (!tgz) throw new Error(`npm pack produced no tarball for ${name}@${version}`);
    // tar is universal on the runners we build on (incl. Windows bsdtar).
    execFileSync('tar', ['-xzf', tgz], { cwd: tmp });
    const payload = join(tmp, 'package');
    if (!existsSync(join(payload, 'package.json'))) {
      throw new Error(`tarball for ${name}@${version} lacks package/package.json`);
    }
    cpSync(payload, destDir, { recursive: true });
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

/**
 * Stage loader + platform packages from `napiSrcDir` (the installed
 * `node_modules/@napi-rs`) into `outNapiDir`.
 *
 * @param {object} opts
 * @param {string} opts.napiSrcDir  installed @napi-rs directory
 * @param {string} opts.outNapiDir  destination @napi-rs directory
 * @param {string[]|null} opts.platforms  explicit platform list
 *   (e.g. ['darwin-x64']) or null for "all installed"
 * @param {(name: string, version: string, destDir: string) => void} [opts.fetchPackage]
 *   fetcher for platforms not installed locally (default: npm pack)
 * @returns {string[]} sorted staged package directory names
 */
export function stageKeyring({ napiSrcDir, outNapiDir, platforms, fetchPackage = npmPackFetcher }) {
  if (!existsSync(join(napiSrcDir, 'keyring', 'package.json'))) {
    throw new Error(`@napi-rs/keyring loader not found under ${napiSrcDir} — run npm install`);
  }
  mkdirSync(outNapiDir, { recursive: true });

  const installed = readdirSync(napiSrcDir).filter(
    (entry) => entry === 'keyring' || entry.startsWith('keyring-'),
  );
  const wanted =
    platforms === null
      ? installed
      : ['keyring', ...platforms.map((p) => `keyring-${p}`)];

  const staged = [];
  for (const entry of [...new Set(wanted)]) {
    const dest = join(outNapiDir, entry);
    if (installed.includes(entry)) {
      cpSync(join(napiSrcDir, entry), dest, { recursive: true, dereference: true });
    } else {
      const version = keyringVersion(napiSrcDir);
      try {
        fetchPackage(`@napi-rs/${entry}`, version, dest);
      } catch (err) {
        throw new Error(
          `failed to stage @napi-rs/${entry}@${version} (not installed locally, fetch failed): ${err.message}`,
        );
      }
    }
    staged.push(entry);
  }

  // Fail closed: every platform package must actually carry its addon.
  for (const entry of staged) {
    if (entry === 'keyring') continue;
    const hasAddon = readdirSync(join(outNapiDir, entry)).some((f) => f.endsWith('.node'));
    if (!hasAddon) {
      throw new Error(`staged @napi-rs/${entry} contains no .node addon — refusing to bundle`);
    }
  }
  if (!staged.includes('keyring') || staged.length < 2) {
    throw new Error(
      `expected @napi-rs/keyring plus at least one platform package, got: ${staged.join(', ')}`,
    );
  }
  return staged.sort();
}
