import { describe, expect, it } from 'vitest';
import { bytes, goldenRequest } from './golden.test-util.ts';
import { DEFAULT_LIMITS } from './limits.ts';
import type { PandocRequest, ShareTree } from './types.ts';
import { validateRequest } from './validate.ts';

const base = (): { req: PandocRequest; tree: ShareTree } => {
  const req = goldenRequest();
  return {
    req,
    tree: { share_tree_version: req.share_tree_version, files: [{ path: 'filters/main.lua', bytes: bytes('-- vendored') }] },
  };
};
const codes = (req: PandocRequest, tree: ShareTree, opts = {}) => validateRequest(req, tree, opts).map((d) => d.code);
const paths = (req: PandocRequest, tree: ShareTree, opts = {}) => validateRequest(req, tree, opts).map((d) => d.path);

describe('the golden request', () => {
  it('passes every rule', () => {
    const { req, tree } = base();
    expect(validateRequest(req, tree)).toEqual([]);
  });
});

describe('shadowing and roots', () => {
  it('rejects a resource_refs path under the share root (a project filters/main.lua cannot shadow the vendored one)', () => {
    const { req, tree } = base();
    req.resource_refs.push({ path: `${req.share_tree_path}/filters/main.lua`, bytes: bytes('-- evil') });
    const d = validateRequest(req, tree);
    expect(d.map((x) => x.code)).toContain('reserved-path');
    expect(d.map((x) => x.path)).toContain(`${req.share_tree_path}/filters/main.lua`);
  });
  it('allows a project filters/main.lua at its own path', () => {
    const { req, tree } = base();
    req.resource_refs.push({ path: '/project/filters/main.lua', bytes: bytes('-- mine') });
    expect(validateRequest(req, tree)).toEqual([]);
  });
  it('rejects a share_root other than the constants value', () => {
    const { req, tree } = base();
    expect(codes({ ...req, share_root: '/elsewhere' }, tree)).toContain('share-root-mismatch');
  });
  it('rejects a share_tree_path that is not <share_root>/pandoc-share', () => {
    const { req, tree } = base();
    expect(codes({ ...req, share_tree_path: '/__q2_share__/other' }, tree)).toContain('share-root-mismatch');
  });
  it('rejects an argv path outside the share root and the project root', () => {
    const { req, tree } = base();
    req.argv.push('--reference-doc=/etc/passwd');
    expect(paths(req, tree)).toContain('/etc/passwd');
    const r2 = base();
    r2.req.argv.push('-L', '/other/filter.lua');
    expect(codes(r2.req, r2.tree)).toContain('path-outside-root');
  });
  it('does not mistake a -V value starting with a comment for a path', () => {
    const { req, tree } = base();
    req.argv.push('-V', 'highlighting-definitions=/* Function definitions */\n#let x = 1');
    expect(validateRequest(req, tree)).toEqual([]);
  });
  it('rejects a resource_refs path that normalizes outside the project root', () => {
    const { req, tree } = base();
    req.resource_refs.push({ path: '/project/../etc/x.png', bytes: bytes('x') });
    expect(codes(req, tree)).toContain('path-not-normalized');
    const r2 = base();
    r2.req.resource_refs.push({ path: '/other/x.png', bytes: bytes('x') });
    expect(codes(r2.req, r2.tree)).toContain('path-outside-root');
  });
  it('rejects a backslash path', () => {
    const { req, tree } = base();
    req.dirs.push('C:\\proj');
    expect(codes(req, tree)).toContain('path-not-normalized');
  });
  it('rejects a share tree entry that climbs out', () => {
    const { req, tree } = base();
    tree.files.push({ path: '../evil.lua', bytes: bytes('x') });
    expect(codes(req, tree)).toContain('path-not-normalized');
  });
  it('rejects a mismatched share_tree_version', () => {
    const { req, tree } = base();
    expect(codes(req, { ...tree, share_tree_version: '0'.repeat(64) })).toEqual(['share-tree-mismatch']);
  });
});

describe('mount rules (design: Contracts)', () => {
  it('a path in two of share tree / files / resource_refs / dirs is rejected, naming the path', () => {
    const a = base();
    a.req.files.push({ path: `${a.req.share_tree_path}/filters/main.lua`, bytes: bytes('x') });
    expect(paths(a.req, a.tree)).toContain(`${a.req.share_tree_path}/filters/main.lua`);
    expect(codes(a.req, a.tree)).toContain('mount-conflict');

    const b = base();
    b.req.files.push({ path: '/project/dup.png', bytes: bytes('1') });
    b.req.resource_refs.push({ path: '/project/dup.png', bytes: bytes('2') });
    expect(codes(b.req, b.tree)).toContain('mount-conflict');
    expect(paths(b.req, b.tree)).toContain('/project/dup.png');
  });
  it('a file/directory collision is rejected', () => {
    const a = base();
    a.req.dirs.push('/project/figure.png'); // also a resource_refs file
    expect(codes(a.req, a.tree)).toContain('mount-conflict');
    const b = base();
    b.req.resource_refs.push({ path: '/project/figure.png/inner.txt', bytes: bytes('x') });
    expect(paths(b.req, b.tree)).toContain('/project/figure.png');
  });
  it('resource_refs under the share root or /tmp, and files under /tmp, are rejected', () => {
    const a = base();
    a.req.resource_refs.push({ path: '/tmp/x.png', bytes: bytes('x') }, { path: '/__q2_share__/y.png', bytes: bytes('x') });
    expect(codes(a.req, a.tree).filter((c) => c === 'reserved-path')).toHaveLength(2);
    const b = base();
    b.req.files.push({ path: '/tmp/f', bytes: bytes('x') });
    expect(codes(b.req, b.tree)).toContain('reserved-path');
  });
  it('/tmpfoo is not /tmp', () => {
    const { req, tree } = base();
    req.files.push({ path: '/__q2_share__/tmpfoo', bytes: bytes('x') });
    req.resource_refs.push({ path: '/project/tmpfoo/a.png', bytes: bytes('x') });
    expect(validateRequest(req, tree)).toEqual([]);
  });
  it('identical duplicates within a list are tolerated; differing ones are not', () => {
    const a = base();
    a.req.resource_refs.push({ ...a.req.resource_refs[0], bytes: a.req.resource_refs[0].bytes.slice() });
    expect(validateRequest(a.req, a.tree)).toEqual([]);
    const b = base();
    b.req.resource_refs.push({ path: b.req.resource_refs[0].path, bytes: bytes('different') });
    expect(codes(b.req, b.tree)).toContain('mount-conflict');
  });
});

describe('limits', () => {
  const small = { image_bytes: 10, reference_doc_bytes: 20, total_bytes: 1_000_000 };
  it('an oversized image is rejected, naming it', () => {
    const { req, tree } = base();
    req.resource_refs.push({ path: '/project/big.png', bytes: new Uint8Array(11) });
    const d = validateRequest(req, tree, { limits: small });
    expect(d.map((x) => [x.code, x.path])).toContainEqual(['limit-exceeded', '/project/big.png']);
  });
  it('an image at the limit passes', () => {
    const { req, tree } = base();
    req.resource_refs.push({ path: '/project/ok.png', bytes: new Uint8Array(10) });
    expect(validateRequest(req, tree, { limits: small })).toEqual([]);
  });
  it('reference-doc has its own limit, and the image limit does not apply to it', () => {
    const { req, tree } = base();
    req.argv.push('--reference-doc', '/project/ref.docx');
    req.resource_refs.push({ path: '/project/ref.docx', bytes: new Uint8Array(15) });
    expect(validateRequest(req, tree, { limits: small })).toEqual([]);
    const r2 = base();
    r2.req.argv.push('--reference-doc=/project/ref.docx');
    r2.req.resource_refs.push({ path: '/project/ref.docx', bytes: new Uint8Array(21) });
    expect(paths(r2.req, r2.tree, { limits: small })).toContain('/project/ref.docx');
  });
  it('the total payload is limited, and the diagnostic names the largest file', () => {
    const { req, tree } = base();
    const all = [...tree.files, ...req.files, ...req.resource_refs];
    const existing = all.reduce((n, f) => n + f.bytes.length, 0);
    const biggest = Math.max(...all.map((f) => f.bytes.length));
    req.resource_refs.push({ path: '/project/data.bin', bytes: new Uint8Array(biggest + 1000) });
    const d = validateRequest(req, tree, { limits: { ...small, image_bytes: 1e9, total_bytes: existing + 10 } });
    expect(d.map((x) => [x.code, x.path])).toContainEqual(['limit-exceeded', '/project/data.bin']);
  });
  it('default limits are the constants-file values', () => {
    expect(DEFAULT_LIMITS.image_bytes).toBe(25 * 1024 * 1024);
  });
});
