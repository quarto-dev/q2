import { describe, expect, it } from 'vitest';
import { arrive, normalizeText } from './arrival.js';
import { relativeProjectPath, toForwardSlashes } from './paths.js';
import { registryKey } from './registry.js';

describe('Windows-shaped inputs, run natively on every platform', () => {
  it('toForwardSlashes turns backslashes into slashes', () => {
    expect(toForwardSlashes('docs\\site\\a.qmd')).toBe('docs/site/a.qmd');
  });
  it('relativeProjectPath handles drive letters, case, and mixed separators', () => {
    expect(relativeProjectPath('C:\\Users\\x\\proj', 'C:\\Users\\x\\proj\\sub\\a.qmd')).toBe('sub/a.qmd');
    expect(relativeProjectPath('c:/Users/x/proj/', 'C:\\Users\\x\\proj\\a.qmd')).toBe('a.qmd');
  });
  it('relativeProjectPath handles UNC prefixes', () => {
    expect(relativeProjectPath('\\\\server\\share\\proj', '\\\\server\\share\\proj\\a.qmd')).toBe('a.qmd');
  });
  it('relativeProjectPath returns null outside the root, and for a sibling that shares a prefix', () => {
    expect(relativeProjectPath('C:\\proj', 'C:\\other\\a.qmd')).toBeNull();
    expect(relativeProjectPath('/home/x/proj', '/home/x/project2/a.qmd')).toBeNull();
  });
  it('relativeProjectPath works for POSIX paths', () => {
    expect(relativeProjectPath('/home/x/proj', '/home/x/proj/a/b.qmd')).toBe('a/b.qmd');
  });
  it('registry keys use / and reject a native Windows path', () => {
    expect(registryKey(toForwardSlashes('a\\b.qmd'), 'b', 'd')).toContain('a/b.qmd');
    expect(() => registryKey('a\\b.qmd', 'b', 'd')).toThrow();
  });
  it('CRLF disk content equals LF doc content after normalization (core.autocrlf)', () => {
    const disk = normalizeText('one\r\ntwo\r\n');
    const out = arrive({ peer: 'extension', kind: 'text', disk, doc: 'one\ntwo\n', blob: 'x', attached: 'none' });
    expect(out.step).toBe('3');
  });
});
