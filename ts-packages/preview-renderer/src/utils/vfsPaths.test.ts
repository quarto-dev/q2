import { describe, it, expect } from 'vitest';
import {
    resolveRelativePath,
    relativePathBetween,
    normalizePath,
    guessMimeType,
} from './vfsPaths';

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

describe('relativePathBetween', () => {
    it('returns the bare filename for a target in the same directory', () => {
        expect(relativePathBetween('posts/hello.qmd', 'posts/photo.png')).toBe(
            'photo.png',
        );
    });

    it('walks up to reach a target in the parent directory', () => {
        expect(relativePathBetween('posts/hello.qmd', 'photo.png')).toBe(
            '../photo.png',
        );
    });

    it('walks up and back down to reach a sibling directory', () => {
        expect(
            relativePathBetween('posts/hello.qmd', 'images/photo.png'),
        ).toBe('../images/photo.png');
    });

    it('returns the bare filename when both are at the root', () => {
        expect(relativePathBetween('hello.qmd', 'photo.png')).toBe(
            'photo.png',
        );
    });

    it('descends from a root file into a subdirectory', () => {
        expect(relativePathBetween('hello.qmd', 'images/photo.png')).toBe(
            'images/photo.png',
        );
    });

    it('walks up multiple levels', () => {
        expect(relativePathBetween('a/b/c/doc.qmd', 'a/x.png')).toBe(
            '../../x.png',
        );
    });

    it('descends below the current directory', () => {
        expect(relativePathBetween('a/b/doc.qmd', 'a/b/c/d.png')).toBe(
            'c/d.png',
        );
    });

    it('does not treat a segment-name prefix as a shared directory', () => {
        // 'post' and 'posts' share a string prefix but are different dirs
        expect(relativePathBetween('post/doc.qmd', 'posts/photo.png')).toBe(
            '../posts/photo.png',
        );
    });

    it('tolerates leading slashes on either argument', () => {
        expect(relativePathBetween('/posts/hello.qmd', '/photo.png')).toBe(
            '../photo.png',
        );
        expect(relativePathBetween('posts/hello.qmd', '/photo.png')).toBe(
            '../photo.png',
        );
        expect(relativePathBetween('/posts/hello.qmd', 'photo.png')).toBe(
            '../photo.png',
        );
    });

    it('normalizes `.` and `..` segments in the inputs', () => {
        expect(
            relativePathBetween('posts/./hello.qmd', 'posts/../photo.png'),
        ).toBe('../photo.png');
    });

    it('handles conflict-renamed (hash-suffixed) upload paths', () => {
        expect(
            relativePathBetween('posts/hello.qmd', 'photo-1a2b3c4d.png'),
        ).toBe('../photo-1a2b3c4d.png');
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
