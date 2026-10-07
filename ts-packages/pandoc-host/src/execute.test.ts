import { describe, expect, it } from 'vitest';
import { execute } from './execute.ts';
import { checkInputs } from './shared.ts';
import { bytes, goldenRequest } from './golden.test-util.ts';
import type { PandocRequest, ShareTree } from './types.ts';

// The input check runs before `WebAssembly.instantiate`, so a dummy module is enough: these
// tests need no pandoc.wasm and never skip.
const module = {} as WebAssembly.Module;
const sha256 = async (b: Uint8Array) =>
  [...new Uint8Array(await crypto.subtle.digest('SHA-256', b as Uint8Array<ArrayBuffer>))].map((x) => x.toString(16).padStart(2, '0')).join('');

const PATH = '/__q2_share__/import/source.docx';
const job = async (content = bytes('docx bytes')) => {
  const g = goldenRequest();
  const request: PandocRequest = { ...g, host_inputs: [{ path: PATH, sha256: await sha256(content), size: content.length }] };
  const shareTree: ShareTree = { share_tree_version: g.share_tree_version, files: [{ path: 'filters/main.lua', bytes: bytes('-- vendored') }] };
  return { request, shareTree, content };
};

describe('execute: host inputs', () => {
  it('a missing input fails as invalid-request with input-mismatch, naming the path', async () => {
    const { request, shareTree } = await job();
    const r = await execute(request, shareTree, { module });
    expect(!r.ok && r.kind).toBe('invalid-request');
    expect(!r.ok && r.diagnostics).toMatchObject([{ origin: 'host', kind: 'error', code: 'input-mismatch', path: PATH }]);
  });

  it('an input of the wrong size is a mismatch', async () => {
    const { request, shareTree } = await job();
    const r = await execute(request, shareTree, { module, inputs: { [PATH]: bytes('short') } });
    expect(!r.ok && r.diagnostics[0]).toMatchObject({ code: 'input-mismatch', message: expect.stringContaining('declares 10') });
  });

  it('an input of the right size and the wrong bytes is a mismatch', async () => {
    const { request, shareTree } = await job();
    const r = await execute(request, shareTree, { module, inputs: { [PATH]: bytes('DOCX BYTES') } });
    expect(!r.ok && r.kind).toBe('invalid-request');
    expect(!r.ok && r.diagnostics[0]).toMatchObject({ code: 'input-mismatch', message: expect.stringContaining('sha256') });
  });

  it('an input supplied under another path counts as missing', async () => {
    const { request, shareTree, content } = await job();
    const r = await execute(request, shareTree, { module, inputs: { '/__q2_share__/import/other.docx': content } });
    expect(!r.ok && r.diagnostics[0]).toMatchObject({ code: 'input-mismatch', path: PATH });
  });

  it('a request that breaks a mount rule fails validation before the inputs are looked at', async () => {
    const { request, shareTree } = await job();
    request.host_inputs![0].path = '/tmp/source.docx';
    const r = await execute(request, shareTree, { module });
    expect(!r.ok && r.diagnostics[0]).toMatchObject({ code: 'reserved-path' });
  });
});

describe('checkInputs', () => {
  it('accepts matching bytes, and a request without host inputs', async () => {
    const { request, content } = await job();
    expect(await checkInputs(request, { [PATH]: content })).toEqual([]);
    expect(await checkInputs(goldenRequest())).toEqual([]);
  });
  it('does not mistake an Object.prototype name for a supplied input', async () => {
    const { request } = await job();
    request.host_inputs![0].path = '/__q2_share__/import/constructor';
    expect(await checkInputs(request, {})).toHaveLength(1);
  });
});
