/**
 * Test-only adapter from R0's pandoc recordings
 * (crates/quarto-core/tests/fixtures/pandoc-recordings/) to a PandocRequest + share tree,
 * for the pandoc-host tests, until request phase R2's export exists.
 *
 * Recordings keep the share tree at /__q2_share__ and pipeline temp files at /__q2_tmp__;
 * a real request has the share tree at <share_root>/pandoc-share and the temp files under
 * the share root. So the adapter relocates every occurrence (argv, env, the decoded
 * QUARTO_FILTER_PARAMS blob, small text inputs): share tree -> /__q2_share__/pandoc-share,
 * temp -> /__q2_share__. Node-only (reads the fixtures from disk).
 */
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import type { PandocRequest, ShareTree } from '@quarto/pandoc-host';

export const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
export const RECORDINGS = path.join(REPO, 'crates/quarto-core/tests/fixtures/pandoc-recordings/recordings');
export const CONSTANTS = JSON.parse(readFileSync(path.join(REPO, 'resources/pandoc-wasm.json'), 'utf8'));
export const WASM_PATH = path.join(REPO, '.cache/pandoc-wasm/pandoc.wasm');

const SHARE = '/__q2_share__';
const TREE = `${SHARE}/pandoc-share`;
const TEXT_EXT = /\.(json|txt|html|typ|css|lua)$/;

export function pandocWasmAvailable(): boolean {
  return existsSync(WASM_PATH);
}

/** Relocate recording paths into the request layout. */
export function relocate(s: string): string {
  return s.replaceAll(SHARE, '\u0000S').replaceAll('/__q2_tmp__', SHARE).replaceAll('\u0000S', TREE);
}

/** The text a native reference run embedded (it ran at /tmp/q2-pandoc-replay/<name>/), in request layout. */
export function relocateReferenceText(text: string, name: string): string {
  const base = `/tmp/q2-pandoc-replay/${name}/`;
  return text
    .replaceAll(`${base}__q2_share__`, TREE)
    .replaceAll(`${base}__q2_tmp__`, SHARE)
    .replaceAll(`${base}__q2_doc__`, '/__q2_doc__')
    .replaceAll(`${base}__q2_out__`, '/__q2_out__');
}

export function recordingNames(): string[] {
  return readdirSync(RECORDINGS)
    .filter((n) => n !== 'share' && existsSync(path.join(RECORDINGS, n, 'argv.json')))
    .sort();
}

const readJson = (p: string) => JSON.parse(readFileSync(p, 'utf8'));
const u8 = (b: Buffer) => new Uint8Array(b.buffer, b.byteOffset, b.byteLength).slice();

export interface LoadedRecording {
  name: string;
  format: string;
  request: PandocRequest;
  shareTree: ShareTree;
  /** Native reference output (byte-comparison reference). */
  referencePath: string;
  outputName: string;
}

export function loadRecording(name: string): LoadedRecording {
  const dir = path.join(RECORDINGS, name);
  const argv: string[] = readJson(path.join(dir, 'argv.json')).map(relocate);
  const envRaw: Record<string, string> = readJson(path.join(dir, 'env.json'));
  const meta = readJson(path.join(dir, 'meta.json'));

  const env: Record<string, string> = {};
  for (const [k, v] of Object.entries(envRaw)) {
    if (k === 'QUARTO_FILTER_PARAMS') {
      const json = Buffer.from(v, 'base64').toString('utf8');
      env[k] = Buffer.from(relocate(json), 'utf8').toString('base64');
    } else env[k] = relocate(v);
  }

  const files: PandocRequest['files'] = [];
  const resourceRefs: PandocRequest['resource_refs'] = [];
  for (const m of readJson(path.join(dir, 'manifest.json')) as { path: string }[]) {
    if (!m.path.startsWith('fs/')) continue;
    const mount = '/' + m.path.slice('fs/'.length);
    let data = readFileSync(path.join(dir, m.path));
    if (mount.startsWith('/__q2_tmp__/')) {
      if (TEXT_EXT.test(mount)) data = Buffer.from(relocate(data.toString('utf8')), 'utf8');
      files.push({ path: relocate(mount), bytes: u8(data) });
    } else resourceRefs.push({ path: mount, bytes: u8(data) });
  }

  const treeId: string = meta.share_tree;
  const shareDir = path.join(RECORDINGS, 'share');
  const shareFiles = (readJson(path.join(shareDir, `${treeId}.manifest.json`)) as { path: string }[]).map((m) => ({
    path: m.path,
    bytes: u8(readFileSync(path.join(shareDir, treeId, m.path))),
  }));

  const outputPath = relocate(meta.output);
  const writer = argv[argv.indexOf('-t') + 1];
  const request: PandocRequest = {
    schema_version: 1,
    kind: 'pandoc',
    job_id: '0'.repeat(16),
    writer,
    argv,
    env,
    files,
    dirs: ['/tmp', path.posix.dirname(outputPath)],
    resource_refs: resourceRefs,
    share_root: CONSTANTS.share_root,
    share_tree_path: TREE,
    doc_dir: '/__q2_doc__',
    project_root: '/__q2_doc__',
    output_path: outputPath,
    stage_name: 'pandoc-write',
    json_path: relocate('/__q2_tmp__/pandoc-input.json'),
    post: 'none',
    expected_pandoc_wasm_sha256: CONSTANTS.wasm_sha256,
    share_tree_version: treeId,
    typst_available_fonts: null,
  };
  return {
    name,
    format: writer,
    request,
    shareTree: { share_tree_version: treeId, files: shareFiles },
    referencePath: path.join(dir, meta.reference),
    outputName: path.basename(meta.output),
  };
}
