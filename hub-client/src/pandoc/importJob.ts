/**
 * A hand-built document-import request (epic interface 2) for the memory measurement
 * (`e2e/pandoc-import-measure.harness.spec.ts`) and its page hook. The real builder is P3's
 * `prepare_import`; this one exists so P1 can run a host-inputs request before it does, and
 * mirrors `crates/quarto-core/schemas/pandoc-request.import.golden.json`.
 */
import type { PandocRequest, ShareTree } from '@quarto/pandoc-host';
import constants from '../../../resources/pandoc-wasm.json';

const IMPORT_DIR = `${constants.share_root}/import`;
/** SHA-256 over zero share-tree entries. */
const EMPTY_SHARE_TREE_VERSION = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';

const toHex = (b: ArrayBuffer): string => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, '0')).join('');

export async function importJob(
  source: Uint8Array,
  format = 'docx',
): Promise<{ request: PandocRequest; shareTree: ShareTree; inputs: Record<string, Uint8Array> }> {
  const sourcePath = `${IMPORT_DIR}/source.${format}`;
  const sha256 = toHex(await crypto.subtle.digest('SHA-256', source as Uint8Array<ArrayBuffer>));
  const request: PandocRequest = {
    schema_version: 1,
    kind: 'pandoc',
    job_id: '0'.repeat(16),
    writer: 'json',
    argv: [
      'pandoc',
      '-f',
      format,
      ...(format === 'docx' ? ['--track-changes=all'] : []),
      '-t',
      'json',
      `--extract-media=${IMPORT_DIR}/media`,
      '-o',
      `${IMPORT_DIR}/out.json`,
      sourcePath,
    ],
    env: { SOURCE_DATE_EPOCH: '1700000000' },
    files: [],
    dirs: [],
    resource_refs: [],
    share_root: constants.share_root,
    share_tree_path: `${constants.share_root}/pandoc-share`,
    doc_dir: IMPORT_DIR,
    project_root: IMPORT_DIR,
    output_path: `${IMPORT_DIR}/out.json`,
    stage_name: 'import',
    json_path: sourcePath,
    post: 'none',
    expected_pandoc_wasm_sha256: constants.wasm_sha256,
    share_tree_version: EMPTY_SHARE_TREE_VERSION,
    typst_available_fonts: null,
    host_inputs: [{ path: sourcePath, sha256, size: source.byteLength }],
    collect_dirs: [`${IMPORT_DIR}/media`],
  };
  return {
    request,
    shareTree: { share_tree_version: EMPTY_SHARE_TREE_VERSION, files: [] },
    inputs: { [sourcePath]: source },
  };
}
