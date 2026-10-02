import { describe, expect, it } from 'vitest';
import { isNormalizedAbsolute, isUnder, normalizeRequestPath as n } from './paths.ts';

// Pure string tests: they run the Windows forms on every OS. Mirrors the Rust tests in
// crates/quarto-core/src/pandoc_request/path.rs.
describe('normalizeRequestPath', () => {
  it('windows drive forms', () => {
    expect(n('C:\\proj\\a.qmd')).toBe('C:/proj/a.qmd');
    expect(n('c:\\proj\\a.qmd')).toBe('C:/proj/a.qmd');
    expect(n('\\\\?\\C:\\proj\\a.qmd')).toBe('C:/proj/a.qmd');
    expect(n('\\\\?\\c:\\proj\\..\\x\\.\\a.qmd')).toBe('C:/x/a.qmd');
    expect(n('C:\\')).toBe('C:/');
    expect(n('C:')).toBe('C:/');
  });
  it('unc forms', () => {
    expect(n('\\\\server\\share\\a.qmd')).toBe('//server/share/a.qmd');
    expect(n('\\\\?\\UNC\\server\\share\\d\\a.qmd')).toBe('//server/share/d/a.qmd');
    expect(n('\\\\server\\share')).toBe('//server/share/');
  });
  it('posix forms', () => {
    expect(n('/a/b/../c/./d')).toBe('/a/c/d');
    expect(n('/a//b/')).toBe('/a/b');
    expect(n('/')).toBe('/');
    expect(n('/..')).toBe('/');
    expect(n('/__q2_share__/pandoc-share')).toBe('/__q2_share__/pandoc-share');
  });
  it('relative forms stay relative', () => {
    expect(n('a/./b/../c')).toBe('a/c');
    expect(n('../a')).toBe('../a');
  });
  it('a colon in a posix name is not a drive', () => {
    expect(n('/a:b/c')).toBe('/a:b/c');
    expect(n('ab:/c')).toBe('ab:/c');
  });
});

describe('isNormalizedAbsolute / isUnder', () => {
  it('accepts only clean absolute paths', () => {
    expect(isNormalizedAbsolute('/a/b')).toBe(true);
    expect(isNormalizedAbsolute('C:/a/b')).toBe(true);
    expect(isNormalizedAbsolute('C:\\proj\\a.qmd')).toBe(false);
    expect(isNormalizedAbsolute('/a/../b')).toBe(false);
    expect(isNormalizedAbsolute('a/b')).toBe(false);
  });
  it('is component-wise', () => {
    expect(isUnder('/tmp', '/tmp')).toBe(true);
    expect(isUnder('/tmp/x', '/tmp')).toBe(true);
    expect(isUnder('/tmpfoo', '/tmp')).toBe(false);
    expect(isUnder('/__q2_share__x/a', '/__q2_share__')).toBe(false);
  });
});
