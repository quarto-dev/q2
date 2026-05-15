/**
 * @vitest-environment jsdom
 */
import { describe, it, expect } from 'vitest';
import { buildActorStyles, cssEscape } from './attribution';
import type { NodeAttributionIdentity } from './AttributionLookupContext';

const sample = (
    sid: number,
    actor: string,
    name: string,
    color: string,
): [number, NodeAttributionIdentity] => [
    sid,
    { actor, name, color, time: 0 },
];

describe('buildActorStyles', () => {
    it('returns empty string when lookup is null', () => {
        expect(buildActorStyles(null)).toBe('');
    });

    it('returns empty string when lookup has no entries', () => {
        expect(buildActorStyles(new Map())).toBe('');
    });

    it('emits one rule per distinct actor, sorted ascending', () => {
        // Three sid entries, two distinct actors. bob appears at the
        // higher sid but must emit first (alphabetical).
        const lookup = new Map<number, NodeAttributionIdentity>([
            sample(1, 'bob', 'Bob', '#88CCEE'),
            sample(2, 'alice', 'Alice', '#CC6677'),
            sample(3, 'alice', 'Alice', '#CC6677'),
        ]);
        const css = buildActorStyles(lookup);
        const aliceAt = css.indexOf('[data-attr-actor="alice"]');
        const bobAt = css.indexOf('[data-attr-actor="bob"]');
        expect(aliceAt).toBeGreaterThanOrEqual(0);
        expect(bobAt).toBeGreaterThanOrEqual(0);
        expect(aliceAt).toBeLessThan(bobAt);
        // Exactly one rule per actor — two `data-attr-actor=` selectors.
        expect((css.match(/\[data-attr-actor=/g) ?? []).length).toBe(2);
        expect(css).toContain('--attr-color: #CC6677');
        expect(css).toContain('--attr-name: "Alice"');
        expect(css).toContain('--attr-color: #88CCEE');
        expect(css).toContain('--attr-name: "Bob"');
    });
});

describe('cssEscape', () => {
    it('passes safe characters through unchanged', () => {
        expect(cssEscape('alice@example.com')).toBe('alice@example.com');
        expect(cssEscape("Alice O'Hara")).toBe("Alice O'Hara");
        expect(cssEscape('日本語')).toBe('日本語');
    });

    it('escapes quote, backslash, newline, and carriage return', () => {
        expect(cssEscape('a"b\\c\nd\re')).toBe('a\\"b\\\\c\\A d\\D e');
    });
});
