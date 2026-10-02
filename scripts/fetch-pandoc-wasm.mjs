#!/usr/bin/env node
// Fetch the pinned pandoc.wasm (resources/pandoc-wasm.json), verify both SHA-256s and
// place it where the tests and the hub-client build read it:
//   .cache/pandoc-wasm/pandoc.wasm               decompressed (vitest reads this)
//   hub-client/public/pandoc/pandoc.wasm.gz      the served asset (opaque gzip, design D6)
//
//   node scripts/fetch-pandoc-wasm.mjs [--require] [--from-wasm <file>]
//
// Default: when the asset cannot be fetched or verified, print why and exit 0 (local
// dev without network). --require exits 1 instead (CI, `cargo xtask verify`, builds).
// --from-wasm uses an already-downloaded decompressed wasm (still SHA-checked).
// Extraction uses fflate, not `unzip` (cross-platform rule). Runbook: dev-docs/pandoc-wasm-bump.md.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync, gzipSync, unzipSync } from 'fflate';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const constants = JSON.parse(readFileSync(path.join(repo, 'resources/pandoc-wasm.json'), 'utf8'));
export const cacheDir = path.join(repo, '.cache/pandoc-wasm');
export const wasmPath = path.join(cacheDir, 'pandoc.wasm');
export const gzPath = path.join(repo, 'hub-client/public/pandoc/pandoc.wasm.gz');

const sha256 = (b) => createHash('sha256').update(b).digest('hex');
const readIfExists = (p) => (existsSync(p) ? readFileSync(p) : null);
const writeAtomic = (p, data) => {
  mkdirSync(path.dirname(p), { recursive: true });
  const tmp = `${p}.${process.pid}.tmp`;
  writeFileSync(tmp, data);
  renameSync(tmp, p);
};

async function obtainWasm(fromWasm) {
  if (fromWasm) {
    const wasm = readFileSync(fromWasm);
    if (sha256(wasm) !== constants.wasm_sha256) throw new Error(`${fromWasm}: wasm sha256 ${sha256(wasm)} != ${constants.wasm_sha256}`);
    return wasm;
  }
  const cached = readIfExists(wasmPath);
  if (cached && sha256(cached) === constants.wasm_sha256) return cached;

  const version = /^pandoc-(.+)\.wasm\.zip$/.exec(constants.asset_name)?.[1];
  if (!version) throw new Error(`cannot read the pandoc version from asset_name ${constants.asset_name}`);
  const url = `https://github.com/jgm/pandoc/releases/download/${version}/${constants.asset_name}`;
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) throw new Error(`GET ${url}: ${res.status} ${res.statusText}`);
  const zip = new Uint8Array(await res.arrayBuffer());
  if (sha256(zip) !== constants.upstream_zip_sha256)
    throw new Error(`${url}: zip sha256 ${sha256(zip)} != ${constants.upstream_zip_sha256} (a proxy may have altered the download)`);
  const entry = Object.entries(unzipSync(zip)).find(([name]) => name.endsWith('/pandoc.wasm') || name === 'pandoc.wasm');
  if (!entry) throw new Error(`${constants.asset_name} has no pandoc.wasm`);
  const wasm = entry[1];
  if (sha256(wasm) !== constants.wasm_sha256) throw new Error(`extracted wasm sha256 ${sha256(wasm)} != ${constants.wasm_sha256}`);
  return wasm;
}

export async function fetchPandocWasm({ fromWasm } = {}) {
  const wasm = await obtainWasm(fromWasm);
  if (!existsSync(wasmPath) || sha256(readFileSync(wasmPath)) !== constants.wasm_sha256) writeAtomic(wasmPath, wasm);
  // The .gz is not a root of trust (gzip output varies); it only has to decompress to the pinned wasm.
  const gz = readIfExists(gzPath);
  let gzOk = false;
  if (gz) {
    try {
      gzOk = sha256(gunzipSync(gz)) === constants.wasm_sha256;
    } catch {
      gzOk = false;
    }
  }
  if (!gzOk) writeAtomic(gzPath, gzipSync(wasm, { level: 9 }));
  return { wasmPath, gzPath };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const require = args.includes('--require');
  const i = args.indexOf('--from-wasm');
  try {
    const r = await fetchPandocWasm({ fromWasm: i >= 0 ? args[i + 1] : undefined });
    console.log(`pandoc.wasm ok: ${path.relative(repo, r.wasmPath)}, ${path.relative(repo, r.gzPath)}`);
  } catch (e) {
    console.error(`pandoc.wasm unavailable: ${e instanceof Error ? e.message : e}`);
    if (require) process.exit(1);
    console.error('(skipping: pass --require to fail instead; real-wasm tests and "Download as" are unavailable)');
  }
}
