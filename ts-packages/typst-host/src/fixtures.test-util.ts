import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { unpackFonts } from './fonts.ts';
import type { TypstFile } from './types.ts';

const here = path.dirname(fileURLToPath(import.meta.url));
export const repo = path.resolve(here, '../../..');
const bytes = (p: string) => new Uint8Array(readFileSync(p));

let modulePromise: Promise<WebAssembly.Module> | undefined;
/** The pinned typst.ts compiler wasm from node_modules, compiled once per test process. */
export function typstModule(): Promise<WebAssembly.Module> {
  modulePromise ??= WebAssembly.compile(bytes(createRequire(import.meta.url).resolve('@myriaddreamin/typst-ts-web-compiler/wasm')) as BufferSource);
  return modulePromise;
}

/** The typst-assets default fonts plus the vendored Font Awesome fonts, as the host loads them. */
export function defaultFonts(): Uint8Array[] {
  const bundle = path.join(repo, '.cache/typst-assets/fonts.bin');
  if (!existsSync(bundle)) throw new Error('missing .cache/typst-assets/fonts.bin: run `node scripts/fetch-pandoc-wasm.mjs --require` (needs network once)');
  return [...unpackFonts(bytes(bundle)), ...vendoredFonts()];
}

export function vendoredFonts(): Uint8Array[] {
  const dir = path.join(repo, 'resources/typst-packages/fonts');
  return readdirSync(dir)
    .filter((f) => /\.(otf|ttf)$/i.test(f))
    .sort()
    .map((f) => bytes(path.join(dir, f)));
}

function walk(dir: string, rel = ''): TypstFile[] {
  const out: TypstFile[] = [];
  for (const name of readdirSync(dir).sort()) {
    const full = path.join(dir, name);
    const r = rel ? `${rel}/${name}` : name;
    if (statSync(full).isDirectory()) out.push(...walk(full, r));
    else out.push({ path: r, bytes: bytes(full) });
  }
  return out;
}

/** The five vendored packages, paths relative to the cache root (`preview/<name>/<version>/...`). */
export function vendoredPackages(): TypstFile[] {
  return walk(path.join(repo, 'resources/typst-packages/packages'));
}

const RECORDINGS = path.join(repo, 'crates/quarto-core/tests/fixtures/pandoc-recordings/recordings');

/**
 * A recorded native pandoc typst run as a compile input: the document directory's files (images,
 * bibliographies) plus the recorded `.typ`, all mounted under `/doc`.
 */
export function recordedTypst(name: string): { main: string; files: TypstFile[] } {
  const dir = path.join(RECORDINGS, `${name}-typst`);
  const refDir = path.join(dir, 'reference');
  const typ = readdirSync(refDir).find((f) => f.endsWith('.typ'));
  if (!typ) throw new Error(`no .typ in ${refDir}`);
  const doc = path.join(dir, 'fs/__q2_doc__');
  const files = walk(doc)
    .filter((f) => !f.path.endsWith('.qmd'))
    .map((f) => ({ path: `/doc/${f.path}`, bytes: f.bytes }));
  files.push({ path: `/doc/${typ}`, bytes: bytes(path.join(refDir, typ)) });
  return { main: `/doc/${typ}`, files };
}

export const text = (s: string): Uint8Array => new TextEncoder().encode(s);

/** A `.tar.gz` of `files` the way `tar -C dir .` writes it (`./` name prefixes included). */
export async function tarGz(files: Record<string, string>): Promise<Uint8Array> {
  const { gzipSync } = await import('node:zlib');
  const blocks: Uint8Array[] = [];
  for (const [name, content] of Object.entries(files)) {
    const body = text(content);
    const header = new Uint8Array(512);
    const put = (s: string, at: number) => header.set(text(s), at);
    put(`./${name}`, 0);
    put('0000644\0', 100);
    put('0000000\0', 108);
    put('0000000\0', 116);
    put(`${body.length.toString(8).padStart(11, '0')}\0`, 124);
    put('00000000000\0', 136);
    put('        ', 148);
    header[156] = 48;
    put('ustar\0', 257);
    put('00', 263);
    const sum = header.reduce((a, b) => a + b, 0);
    put(`${sum.toString(8).padStart(6, '0')}\0 `, 148);
    blocks.push(header, body, new Uint8Array((512 - (body.length % 512)) % 512));
  }
  blocks.push(new Uint8Array(1024));
  const tar = new Uint8Array(blocks.reduce((n, b) => n + b.length, 0));
  let at = 0;
  for (const b of blocks) {
    tar.set(b, at);
    at += b.length;
  }
  return new Uint8Array(gzipSync(tar));
}

export const pkg = (name: string, version: string, lib: string) => ({
  'typst.toml': `[package]\nname = "${name}"\nversion = "${version}"\nentrypoint = "lib.typ"\n`,
  'lib.typ': lib,
});
