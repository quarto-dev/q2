import * as ajv2020 from 'ajv/dist/2020.js';
import { describe, expect, it } from 'vitest';
import { DEFAULT_LIMITS, DEFAULT_SHARE_ROOT, SUPPORTED_SCHEMA_VERSION } from './limits.ts';
import { goldenRequest, readRepoJson } from './golden.test-util.ts';
import { checkShape } from './validate.ts';
import type { PandocRequest } from './types.ts';

// Every key of PandocRequest; the `satisfies` below fails to compile if one is misspelled
// and the exhaustiveness check fails if one is missing.
const REQUEST_FIELDS = [
  'schema_version', 'kind', 'job_id', 'writer', 'argv', 'env', 'files', 'dirs', 'resource_refs',
  'share_root', 'share_tree_path', 'doc_dir', 'project_root', 'output_path', 'stage_name', 'json_path',
  'post', 'expected_pandoc_wasm_sha256', 'share_tree_version', 'typst_available_fonts',
] as const satisfies readonly (keyof PandocRequest)[];
type Missing = Exclude<keyof PandocRequest, (typeof REQUEST_FIELDS)[number]>;
const _exhaustive: Missing extends never ? true : never = true;
void _exhaustive;

describe('PandocRequest vs the published schema', () => {
  const schema = readRepoJson('crates/quarto-core/schemas/pandoc-request.schema.json');

  it('the type lists exactly the schema properties', () => {
    expect([...REQUEST_FIELDS].sort()).toEqual(Object.keys(schema.properties).sort());
  });

  it('the golden validates against the schema', () => {
    // ajv is CommonJS: under node16 resolution the class is the module's `default`.
    const Ajv2020 = (ajv2020 as unknown as { default: typeof import('ajv/dist/2020.js').default }).default;
    const ajv = new Ajv2020({ strict: false });
    const validate = ajv.compile(schema);
    expect(validate(readRepoJson('crates/quarto-core/schemas/pandoc-request.golden.json')), JSON.stringify(validate.errors)).toBe(true);
  });

  it('the decoded golden passes the host shape check', () => {
    expect(checkShape(goldenRequest())).toEqual([]);
  });

  it('refuses an unknown schema_version', () => {
    const d = checkShape({ ...goldenRequest(), schema_version: 2 });
    expect(d.map((x) => x.code)).toEqual(['unsupported-schema-version']);
  });
});

describe('constants file', () => {
  it('limits, share root and schema version match resources/pandoc-wasm.json and the schema', () => {
    const c = readRepoJson('resources/pandoc-wasm.json');
    expect(DEFAULT_LIMITS).toEqual(c.limits);
    expect(DEFAULT_SHARE_ROOT).toBe(c.share_root);
    expect(readRepoJson('crates/quarto-core/schemas/pandoc-request.schema.json').properties.schema_version.const).toBe(SUPPORTED_SCHEMA_VERSION);
  });
});
