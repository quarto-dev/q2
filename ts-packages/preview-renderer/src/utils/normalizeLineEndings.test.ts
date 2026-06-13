import { describe, it, expect } from 'vitest';
import { normalizeLineEndings } from './normalizeLineEndings';

describe('normalizeLineEndings', () => {
    it('converts \\r\\n to \\n', () => {
        expect(normalizeLineEndings('foo\r\nbar')).toBe('foo\nbar');
    });

    it('leaves lone \\n untouched', () => {
        expect(normalizeLineEndings('foo\nbar')).toBe('foo\nbar');
    });

    it('leaves lone \\r untouched (only the pair is normalized)', () => {
        expect(normalizeLineEndings('foo\rbar')).toBe('foo\rbar');
    });

    it('is idempotent', () => {
        const s = 'foo\nbar\nbaz';
        expect(normalizeLineEndings(normalizeLineEndings(s))).toBe(normalizeLineEndings(s));
    });

    it('handles multiple \\r\\n pairs', () => {
        expect(normalizeLineEndings('a\r\nb\r\nc')).toBe('a\nb\nc');
    });
});
