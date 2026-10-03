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

// ---- document-import recordings (crates/quarto-core/tests/fixtures/import-recordings/) ----------------

export const IMPORT_RECORDINGS = path.join(REPO, 'crates/quarto-core/tests/fixtures/import-recordings');
const IMPORT_DIR = `${SHARE}/import`;
/** SHA-256 over zero share-tree entries: the import request's empty share tree. */
const EMPTY_SHARE_TREE_VERSION = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';

/** Every import recording's name (`<name>-<format>`), sorted. */
export function importRecordingNames(): string[] {
  return readdirSync(IMPORT_RECORDINGS)
    .filter((n) => existsSync(path.join(IMPORT_RECORDINGS, n, 'argv.json')))
    .sort();
}

export interface ImportMediaFile {
  /** Path relative to `media/` (and to the extract dir). */
  rel: string;
  /** `/__q2_share__/import/media/<rel>`: the path pandoc wrote it at, and the `Image` target prefix. */
  pandocPath: string;
  sha256: string;
  bytes: Uint8Array;
}

export interface LoadedImportRecording {
  name: string;
  /** The recorded interface-2 argv. */
  argv: string[];
  /** The request built by hand from `argv.json`, following interface 2: the source as a host input, the extract dir collected, an empty share tree. */
  request: PandocRequest;
  shareTree: ShareTree;
  /** Where `execute` mounts the source (`inputs` key). */
  sourcePath: string;
  source: Uint8Array;
  /** Native pandoc's exit status. */
  status: number;
  /** Native pandoc's `out.json`, parsed; `null` when pandoc failed (no `pandoc.json`). */
  pandocJson: unknown | null;
  stderr: string;
  /** The files `--extract-media` wrote, with their sha256 from `manifest.json`. */
  media: ImportMediaFile[];
}

export function loadImportRecording(name: string): LoadedImportRecording {
  const dir = path.join(IMPORT_RECORDINGS, name);
  const argv: string[] = readJson(path.join(dir, 'argv.json'));
  const manifest = readJson(path.join(dir, 'manifest.json')) as { path: string; sha256: string; size: number }[];
  const sourcePath = argv[argv.length - 1];
  const sourceEntry = manifest.find((m) => m.path === path.posix.basename(sourcePath));
  if (!sourceEntry) throw new Error(`${name}: manifest.json has no ${path.posix.basename(sourcePath)}`);
  const source = u8(readFileSync(path.join(dir, sourceEntry.path)));
  const outPath = argv[argv.indexOf('-o') + 1];
  const extractDir = (argv.find((a) => a.startsWith('--extract-media=')) as string).slice('--extract-media='.length);
  const writer = argv[argv.indexOf('-t') + 1];

  const request: PandocRequest = {
    schema_version: 1,
    kind: 'pandoc',
    job_id: '0'.repeat(16),
    writer,
    argv,
    env: { SOURCE_DATE_EPOCH: '1700000000' },
    files: [],
    // No `/tmp`: a reader run with no filters never writes there (T7 ran every recording both ways).
    dirs: [],
    resource_refs: [],
    share_root: CONSTANTS.share_root,
    share_tree_path: TREE,
    doc_dir: IMPORT_DIR,
    project_root: IMPORT_DIR,
    output_path: outPath,
    stage_name: 'import',
    json_path: sourcePath,
    post: 'none',
    expected_pandoc_wasm_sha256: CONSTANTS.wasm_sha256,
    share_tree_version: EMPTY_SHARE_TREE_VERSION,
    typst_available_fonts: null,
    host_inputs: [{ path: sourcePath, sha256: sourceEntry.sha256, size: sourceEntry.size }],
    collect_dirs: [extractDir],
  };
  const jsonPath = path.join(dir, 'pandoc.json');
  return {
    name,
    argv,
    request,
    shareTree: { share_tree_version: EMPTY_SHARE_TREE_VERSION, files: [] },
    sourcePath,
    source,
    status: readJson(path.join(dir, 'status.json')).status,
    pandocJson: existsSync(jsonPath) ? readJson(jsonPath) : null,
    stderr: readFileSync(path.join(dir, 'stderr.txt'), 'utf8'),
    media: manifest
      .filter((m) => m.path.startsWith('media/'))
      .map((m) => {
        const rel = m.path.slice('media/'.length);
        return { rel, pandocPath: `${extractDir}/${rel}`, sha256: m.sha256, bytes: u8(readFileSync(path.join(dir, m.path))) };
      }),
  };
}
