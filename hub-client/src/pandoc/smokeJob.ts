/**
 * A tiny markdown-to-plain pandoc job (one paragraph through a pass-through filter), for
 * tests that exercise the loader and worker lifecycle rather than any format: the vitest
 * wasm suites and the `VITE_E2E` page hook (src/test-hooks.ts).
 */
import type { PandocRequest, ShareTree } from '@quarto/pandoc-host';
import constants from '../../../resources/pandoc-wasm.json';

const SHARE = constants.share_root;
const TREE = `${SHARE}/pandoc-share`;
const enc = new TextEncoder();

export const PANDOC_WASM_SHA256: string = constants.wasm_sha256;

export function smokeJob(md = 'Hello\n', sha = PANDOC_WASM_SHA256): { request: PandocRequest; shareTree: ShareTree } {
  const out = `${SHARE}/out.txt`;
  const request: PandocRequest = {
    schema_version: 1,
    kind: 'pandoc',
    job_id: '0'.repeat(16),
    writer: 'plain',
    argv: ['pandoc', '-f', 'markdown', '-t', 'plain', '-L', `${TREE}/probe.lua`, '-o', out, `${SHARE}/in.md`],
    env: { SOURCE_DATE_EPOCH: '1700000000' },
    files: [{ path: `${SHARE}/in.md`, bytes: enc.encode(md) }],
    dirs: ['/tmp'],
    resource_refs: [],
    share_root: SHARE,
    share_tree_path: TREE,
    doc_dir: '/proj',
    project_root: '/proj',
    output_path: out,
    stage_name: 'pandoc-write',
    json_path: `${SHARE}/in.md`,
    post: 'none',
    expected_pandoc_wasm_sha256: sha,
    share_tree_version: 'smoke',
    typst_available_fonts: null,
  };
  const shareTree: ShareTree = {
    share_tree_version: 'smoke',
    files: [{ path: 'probe.lua', bytes: enc.encode('function Pandoc(d) return d end') }],
  };
  return { request, shareTree };
}
