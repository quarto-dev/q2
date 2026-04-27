/**
 * Tests for validateProjectPath
 */

import { describe, it, expect } from 'vitest';
import { validateProjectPath } from './validateProjectPath';

describe('validateProjectPath', () => {
  describe('valid paths', () => {
    it('accepts a simple filename', () => {
      expect(validateProjectPath('foo.png')).toBeNull();
    });

    it('accepts a nested path', () => {
      expect(validateProjectPath('images/foo.png')).toBeNull();
    });

    it('accepts a deeply nested path', () => {
      expect(validateProjectPath('_quarto/grammars/toml/toml.wasm')).toBeNull();
    });

    it('accepts empty string (project root)', () => {
      expect(validateProjectPath('')).toBeNull();
    });

    it('accepts a dotfile', () => {
      expect(validateProjectPath('.gitignore')).toBeNull();
    });

    it('accepts a dotfile in a subdirectory', () => {
      expect(validateProjectPath('config/.env')).toBeNull();
    });

    it('accepts filenames with hyphens and underscores', () => {
      expect(validateProjectPath('foo-bar_baz.png')).toBeNull();
    });
  });

  describe('leading slash', () => {
    it('rejects a leading slash', () => {
      expect(validateProjectPath('/foo.png')).toMatch(/leading.*slash|absolute/i);
    });

    it('rejects a leading slash on a nested path', () => {
      expect(validateProjectPath('/images/foo.png')).toMatch(/leading.*slash|absolute/i);
    });
  });

  describe('relative path segments', () => {
    it('rejects a "." segment', () => {
      expect(validateProjectPath('./foo.png')).toMatch(/\.|relative|segment/i);
    });

    it('rejects a ".." segment at the start', () => {
      expect(validateProjectPath('../foo.png')).toMatch(/\.\.|relative|segment/i);
    });

    it('rejects a ".." segment in the middle', () => {
      expect(validateProjectPath('images/../foo.png')).toMatch(/\.\.|relative|segment/i);
    });

    it('rejects a bare "."', () => {
      expect(validateProjectPath('.')).toMatch(/\.|relative|segment/i);
    });

    it('rejects a bare ".."', () => {
      expect(validateProjectPath('..')).toMatch(/\.\.|relative|segment/i);
    });
  });

  describe('forbidden characters', () => {
    it('rejects "<" in filename', () => {
      expect(validateProjectPath('foo<bar.png')).toMatch(/invalid.*char|forbidden/i);
    });

    it('rejects ">" in filename', () => {
      expect(validateProjectPath('foo>bar.png')).toMatch(/invalid.*char|forbidden/i);
    });

    it('rejects ":" in filename', () => {
      expect(validateProjectPath('foo:bar.png')).toMatch(/invalid.*char|forbidden/i);
    });

    it('rejects a quote in filename', () => {
      expect(validateProjectPath('foo"bar.png')).toMatch(/invalid.*char|forbidden/i);
    });

    it('rejects "|" in filename', () => {
      expect(validateProjectPath('foo|bar.png')).toMatch(/invalid.*char|forbidden/i);
    });

    it('rejects "?" in filename', () => {
      expect(validateProjectPath('foo?bar.png')).toMatch(/invalid.*char|forbidden/i);
    });

    it('rejects "*" in filename', () => {
      expect(validateProjectPath('foo*bar.png')).toMatch(/invalid.*char|forbidden/i);
    });

    it('rejects a backslash in filename', () => {
      expect(validateProjectPath('foo\\bar.png')).toMatch(/invalid.*char|forbidden/i);
    });

    it('rejects forbidden characters inside a subdirectory name', () => {
      expect(validateProjectPath('sub<dir/foo.png')).toMatch(/invalid.*char|forbidden/i);
    });
  });

  describe('empty segments', () => {
    it('rejects double slashes', () => {
      expect(validateProjectPath('foo//bar.png')).toMatch(/empty.*segment|double.*slash/i);
    });

    it('rejects a trailing slash', () => {
      expect(validateProjectPath('foo/')).toMatch(/empty.*segment|trailing.*slash/i);
    });
  });
});
