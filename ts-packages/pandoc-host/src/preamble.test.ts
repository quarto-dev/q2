import { describe, expect, it } from 'vitest';
import { INIT_LUA, buildEnvPreamble, withPreamble } from './preamble.ts';
import { bytes, emptyShareTree } from './golden.test-util.ts';
import type { ShareTree } from './types.ts';

const tree = (init: Uint8Array): ShareTree => ({ ...emptyShareTree('v1'), files: [{ path: 'filters/main.lua', bytes: bytes('-- main\n') }, { path: INIT_LUA, bytes: init }] });
const dec = new TextDecoder();
const initOf = (t: ShareTree) => t.files.find((f) => f.path === INIT_LUA)!.bytes;

describe('buildEnvPreamble', () => {
  it('is one line with no trailing newline and no top-level local', () => {
    const p = buildEnvPreamble({ A: '1', QUARTO_FILTER_PARAMS: 'eyJ4IjoxfQ==' });
    expect(p).not.toMatch(/[\r\n]/);
    expect(p.startsWith('do ') && p.endsWith(' end')).toBe(true);
    expect(p).toContain('["A"]="1"');
  });
  it('escapes quotes, backslashes, newlines, NUL and non-ASCII as decimal bytes', () => {
    const p = buildEnvPreamble({ K: 'a"b\\c\nd\0eé' });
    expect(p).toContain('"a\\034b\\092c\\010d\\000e\\195\\169"');
    expect(p).not.toMatch(/[\r\n\0]/);
  });
  it('an empty env still replaces os.getenv', () => expect(buildEnvPreamble({})).toBe('do local e={} os.getenv=function(k) return e[k] end end'));
});

describe('withPreamble', () => {
  const env = { SOURCE_DATE_EPOCH: '1' };
  it('is preamble ++ original byte for byte, joined to the first line by a space', () => {
    const original = bytes('if pandoc.system.os == "mingw32" then\n  x()\nend\n');
    const out = initOf(withPreamble(tree(original), env));
    const pre = bytes(`${buildEnvPreamble(env)} `);
    expect(Array.from(out)).toEqual([...pre, ...original]);
    const lines = dec.decode(out).split('\n');
    expect(lines).toHaveLength(4); // the same line count as the original (3 lines + trailing empty)
    expect(lines[0].endsWith(' if pandoc.system.os == "mingw32" then')).toBe(true);
    expect(lines.slice(1)).toEqual(['  x()', 'end', '']);
  });
  it('keeps a BOM and invalid UTF-8', () => {
    const original = new Uint8Array([0xef, 0xbb, 0xbf, 0x2d, 0x2d, 0xff, 0xfe, 0x0a]);
    const out = initOf(withPreamble(tree(original), env));
    expect(Array.from(out.slice(out.length - original.length))).toEqual(Array.from(original));
  });
  it('does not mutate the input tree, its entry or the version', () => {
    const t = tree(bytes('x\n'));
    const before = { files: t.files.slice(), first: t.files[1].bytes.slice(), version: t.share_tree_version };
    const out = withPreamble(t, env);
    expect(t.files).toEqual(before.files);
    expect(Array.from(t.files[1].bytes)).toEqual(Array.from(before.first));
    expect(out).not.toBe(t);
    expect(out.files).not.toBe(t.files);
    expect(out.files[0]).toBe(t.files[0]); // other entries are shared, not copied
    expect(out.share_tree_version).toBe(before.version);
  });
  it('throws when the share tree has no init.lua', () => {
    expect(() => withPreamble(emptyShareTree('v'), env)).toThrow(/init\.lua/);
  });
});
