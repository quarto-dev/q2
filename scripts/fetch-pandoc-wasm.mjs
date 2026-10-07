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
//
// The same run also provides the typst compiler's assets (host phase H7; resources/typst-wasm.json):
//   hub-client/public/typst/typst.wasm.gz   the typst.ts compiler wasm from node_modules (sha-checked)
//   hub-client/public/typst/fonts.bin.gz    the typst-assets default fonts, from the pinned crates.io crate
//   .cache/typst-assets/fonts.bin           the same fonts, uncompressed (vitest reads this)
// --from-crate <file> uses an already-downloaded typst-assets .crate (still SHA-checked).
//
// And the stock pdf.js viewer (host phase H9; resources/pdfjs-viewer.json):
//   hub-client/public/pdfjs/{web,build}/    the release's viewer, trimmed, with pdf.worker.mjs patched
//                                           to a constant document fingerprint (see patchWorker)
// --from-pdfjs <file> uses an already-downloaded release zip (still SHA-checked).
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync, gzipSync, unzipSync } from 'fflate';
import { createRequire } from 'node:module';

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

// ---- typst compiler assets (H7) ----------------------------------------------------------

const typstConstants = JSON.parse(readFileSync(path.join(repo, 'resources/typst-wasm.json'), 'utf8'));
export const typstCacheDir = path.join(repo, '.cache/typst-assets');
export const typstFontsPath = path.join(typstCacheDir, 'fonts.bin');
export const typstWasmGzPath = path.join(repo, 'hub-client/public/typst/typst.wasm.gz');
export const typstFontsGzPath = path.join(repo, 'hub-client/public/typst/fonts.bin.gz');

/** The files of a ustar/pax tarball (regular files only): `[name, bytes]`. */
function untar(tar) {
  const files = [];
  const text = (b, from, to) => new TextDecoder().decode(b.subarray(from, to)).replace(/\0.*$/s, '');
  let at = 0;
  while (at + 512 <= tar.length) {
    const header = tar.subarray(at, at + 512);
    if (header.every((x) => x === 0)) break;
    const size = parseInt(text(header, 124, 136).trim() || '0', 8);
    const type = String.fromCharCode(header[156] || 48);
    const prefix = text(header, 345, 500);
    const name = prefix ? `${prefix}/${text(header, 0, 100)}` : text(header, 0, 100);
    at += 512;
    if (type === '0') files.push([name, tar.subarray(at, at + size)]);
    at += Math.ceil(size / 512) * 512;
  }
  return files;
}

/** The fonts bundle format of `@quarto/typst-host` (`packFonts`): u32 count, then u32 length + bytes per font. */
function packFonts(fonts) {
  const out = new Uint8Array(4 + fonts.reduce((n, f) => n + 4 + f.length, 0));
  const view = new DataView(out.buffer);
  view.setUint32(0, fonts.length, true);
  let k = 4;
  for (const f of fonts) {
    view.setUint32(k, f.length, true);
    out.set(f, k + 4);
    k += 4 + f.length;
  }
  return out;
}

async function obtainFonts(fromCrate) {
  const fc = typstConstants.fonts_crate;
  const crateFile = `${fc.name}-${fc.version}.crate`;
  let crate;
  if (fromCrate) crate = readFileSync(fromCrate);
  else {
    const cached = readIfExists(path.join(typstCacheDir, crateFile));
    if (cached && sha256(cached) === fc.crate_sha256) return packFontsFromCrate(cached, fc);
    const url = `https://static.crates.io/crates/${fc.name}/${crateFile}`;
    const res = await fetch(url, { redirect: 'follow' });
    if (!res.ok) throw new Error(`GET ${url}: ${res.status} ${res.statusText}`);
    crate = new Uint8Array(await res.arrayBuffer());
  }
  if (sha256(crate) !== fc.crate_sha256) throw new Error(`${crateFile}: sha256 ${sha256(crate)} != ${fc.crate_sha256}`);
  writeAtomic(path.join(typstCacheDir, crateFile), crate);
  return packFontsFromCrate(crate, fc);
}

function packFontsFromCrate(crate, fc) {
  const marker = `${fc.name}-${fc.version}/${fc.fonts_dir}`;
  const fonts = untar(gunzipSync(crate))
    .filter(([name]) => name.startsWith(marker) && /\.(otf|ttf)$/i.test(name))
    .sort(([a], [b]) => (a < b ? -1 : 1))
    .map(([, bytes]) => bytes);
  if (fonts.length === 0) throw new Error(`${fc.name}-${fc.version}.crate has no fonts under ${fc.fonts_dir}`);
  return packFonts(fonts);
}

export async function fetchTypstAssets({ fromCrate } = {}) {
  const wasmFile = createRequire(import.meta.url).resolve(`${typstConstants.wasm_package}/wasm`);
  const wasm = readFileSync(wasmFile);
  if (sha256(wasm) !== typstConstants.wasm_sha256)
    throw new Error(`${wasmFile}: typst wasm sha256 ${sha256(wasm)} != ${typstConstants.wasm_sha256} (is ${typstConstants.wasm_package}@${typstConstants.typst_ts_version} installed?)`);
  const gz = readIfExists(typstWasmGzPath);
  let gzOk = false;
  if (gz) {
    try {
      gzOk = sha256(gunzipSync(gz)) === typstConstants.wasm_sha256;
    } catch {
      gzOk = false;
    }
  }
  if (!gzOk) writeAtomic(typstWasmGzPath, gzipSync(wasm, { level: 9 }));

  const fonts = await obtainFonts(fromCrate);
  if (sha256(fonts) !== typstConstants.fonts_bundle_sha256)
    throw new Error(`the packed default fonts have sha256 ${sha256(fonts)}, not the pinned ${typstConstants.fonts_bundle_sha256} (resources/typst-wasm.json)`);
  const bundle = readIfExists(typstFontsPath);
  if (!bundle || sha256(bundle) !== sha256(fonts)) writeAtomic(typstFontsPath, fonts);
  const fgz = readIfExists(typstFontsGzPath);
  let fgzOk = false;
  if (fgz) {
    try {
      fgzOk = sha256(gunzipSync(fgz)) === sha256(fonts);
    } catch {
      fgzOk = false;
    }
  }
  if (!fgzOk) writeAtomic(typstFontsGzPath, gzipSync(fonts, { level: 9 }));
  return { wasmGzPath: typstWasmGzPath, fontsGzPath: typstFontsGzPath, fontsPath: typstFontsPath };
}

// ---- pdf.js viewer (H9) ------------------------------------------------------------------

const pdfjsConstants = JSON.parse(readFileSync(path.join(repo, 'resources/pdfjs-viewer.json'), 'utf8'));
export const pdfjsDir = path.join(repo, 'hub-client/public/pdfjs');
export const pdfjsStamp = path.join(pdfjsDir, '.stamp');

// The viewer keys its saved zoom/scroll by the document fingerprint: the trailer /ID, which typst
// derives from the content, so it changes on every edit. A constant fingerprint makes a recompile
// look like the same document (Q1's preview does the same; design T7). The host clears the saved
// history when it switches to a different file.
const FINGERPRINT_GETTER = 'return shadow(this, "fingerprints", [hashOriginal.toHex(), hashModified?.toHex() ?? null]);';

export function patchWorker(source, fingerprint = pdfjsConstants.stable_fingerprint) {
  const first = source.indexOf(FINGERPRINT_GETTER);
  if (first < 0 || source.indexOf(FINGERPRINT_GETTER, first + 1) >= 0)
    throw new Error('pdf.worker.mjs: the fingerprints getter is not found exactly once (did the pinned pdf.js version change? update patchWorker)');
  return source.replace(FINGERPRINT_GETTER, `return shadow(this, "fingerprints", ["${fingerprint}", null]);`);
}

// Dropped from the release: source maps, the scripting sandbox (typst PDFs carry no JS), the debugger,
// and the sample document.
const keepPdfjsFile = (name) =>
  (name.startsWith('web/') || name.startsWith('build/')) &&
  !name.endsWith('.map') &&
  !name.endsWith('/pdf.sandbox.mjs') &&
  !name.startsWith('web/debugger.') &&
  !name.startsWith('web/compressed.') &&
  !name.endsWith('/');

export async function fetchPdfjsViewer({ fromZip } = {}) {
  const stamp = `${pdfjsConstants.upstream_zip_sha256}:${sha256(Buffer.from(FINGERPRINT_GETTER + pdfjsConstants.stable_fingerprint + keepPdfjsFile.toString() + 'v2'))}`;
  if (readIfExists(pdfjsStamp)?.toString() === stamp && existsSync(path.join(pdfjsDir, 'web/viewer.html'))) return { dir: pdfjsDir };
  let zip;
  if (fromZip) zip = readFileSync(fromZip);
  else {
    const cachedPath = path.join(repo, '.cache/pdfjs', pdfjsConstants.asset_name);
    const cached = readIfExists(cachedPath);
    if (cached && sha256(cached) === pdfjsConstants.upstream_zip_sha256) zip = cached;
    else {
      const url = `https://github.com/mozilla/pdf.js/releases/download/v${pdfjsConstants.version}/${pdfjsConstants.asset_name}`;
      const res = await fetch(url, { redirect: 'follow' });
      if (!res.ok) throw new Error(`GET ${url}: ${res.status} ${res.statusText}`);
      zip = new Uint8Array(await res.arrayBuffer());
      if (sha256(zip) === pdfjsConstants.upstream_zip_sha256) writeAtomic(cachedPath, zip);
    }
  }
  if (sha256(zip) !== pdfjsConstants.upstream_zip_sha256)
    throw new Error(`${pdfjsConstants.asset_name}: sha256 ${sha256(zip)} != ${pdfjsConstants.upstream_zip_sha256} (a proxy may have altered the download)`);
  const entries = Object.entries(unzipSync(zip)).filter(([name]) => keepPdfjsFile(name));
  if (!entries.some(([n]) => n === 'web/viewer.html') || !entries.some(([n]) => n === 'build/pdf.worker.mjs'))
    throw new Error(`${pdfjsConstants.asset_name} has no web/viewer.html or build/pdf.worker.mjs`);
  rmSync(pdfjsDir, { recursive: true, force: true });
  for (const [name, bytes] of entries) {
    let text = name.endsWith('.mjs') ? new TextDecoder().decode(bytes) : null;
    if (text !== null) text = text.replace(/\n\/\/# sourceMappingURL=\S+\s*$/, '\n'); // the maps are not shipped
    if (name === 'build/pdf.worker.mjs') text = patchWorker(text);
    const out = text === null ? bytes : new TextEncoder().encode(text);
    writeAtomic(path.join(pdfjsDir, name), out);
  }
  writeAtomic(pdfjsStamp, stamp);
  return { dir: pdfjsDir };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const require = args.includes('--require');
  const arg = (flag) => {
    const i = args.indexOf(flag);
    return i >= 0 ? args[i + 1] : undefined;
  };
  let failed = false;
  const attempt = async (what, fn) => {
    try {
      await fn();
    } catch (e) {
      failed = true;
      console.error(`${what} unavailable: ${e instanceof Error ? e.message : e}`);
    }
  };
  await attempt('pandoc.wasm', async () => {
    const r = await fetchPandocWasm({ fromWasm: arg('--from-wasm') });
    console.log(`pandoc.wasm ok: ${path.relative(repo, r.wasmPath)}, ${path.relative(repo, r.gzPath)}`);
  });
  await attempt('typst assets', async () => {
    const r = await fetchTypstAssets({ fromCrate: arg('--from-crate') });
    console.log(`typst assets ok: ${path.relative(repo, r.wasmGzPath)}, ${path.relative(repo, r.fontsGzPath)}, ${path.relative(repo, r.fontsPath)}`);
  });
  await attempt('pdf.js viewer', async () => {
    const r = await fetchPdfjsViewer({ fromZip: arg('--from-pdfjs') });
    console.log(`pdf.js viewer ok: ${path.relative(repo, r.dir)}`);
  });
  if (failed) {
    if (require) process.exit(1);
    console.error('(skipping: pass --require to fail instead; real-wasm tests and "Download as" are unavailable)');
  }
}
