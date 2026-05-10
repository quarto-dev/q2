import { describe, it, expect } from 'vitest';
import { resolveRelativePath, normalizePath, guessMimeType } from './vfsPaths';

describe('resolveRelativePath', () => {
    it('returns absolute paths unchanged', () => {
        expect(resolveRelativePath('/project/index.qmd', '/already/abs.png')).toBe(
            '/already/abs.png',
        );
    });

    it('joins relative path against the directory of currentFile', () => {
        expect(resolveRelativePath('/project/index.qmd', 'hero.png')).toBe(
            '/project/hero.png',
        );
    });

    it('handles `..` traversal', () => {
        expect(
            resolveRelativePath('/project/sub/index.qmd', '../shared/hero.png'),
        ).toBe('/project/shared/hero.png');
    });

    it('handles `.` segments', () => {
        expect(resolveRelativePath('/project/index.qmd', './hero.png')).toBe(
            '/project/hero.png',
        );
    });

    it('handles currentFile with no slashes', () => {
        expect(resolveRelativePath('index.qmd', 'hero.png')).toBe('/hero.png');
    });
});

describe('normalizePath', () => {
    it('collapses `..` segments', () => {
        expect(normalizePath('/a/b/../c')).toBe('/a/c');
    });

    it('removes `.` segments', () => {
        expect(normalizePath('/a/./b/./c')).toBe('/a/b/c');
    });

    it('removes empty segments', () => {
        expect(normalizePath('/a//b///c')).toBe('/a/b/c');
    });

    it('always returns a leading slash', () => {
        expect(normalizePath('a/b')).toBe('/a/b');
    });

    it('swallows `..` past root', () => {
        expect(normalizePath('/../a')).toBe('/a');
    });
});

describe('guessMimeType', () => {
    it('returns image/* MIME types for known image extensions', () => {
        expect(guessMimeType('hero.png')).toBe('image/png');
        expect(guessMimeType('photo.jpg')).toBe('image/jpeg');
        expect(guessMimeType('photo.JPEG')).toBe('image/jpeg');
        expect(guessMimeType('icon.svg')).toBe('image/svg+xml');
        expect(guessMimeType('graphic.webp')).toBe('image/webp');
        expect(guessMimeType('animation.gif')).toBe('image/gif');
    });

    it('returns text/* MIME types for css/js', () => {
        expect(guessMimeType('theme.css')).toBe('text/css');
        expect(guessMimeType('script.js')).toBe('text/javascript');
    });

    it('falls back to application/octet-stream for unknown extensions', () => {
        expect(guessMimeType('mystery.xyz')).toBe('application/octet-stream');
        expect(guessMimeType('no-extension')).toBe('application/octet-stream');
    });
});
