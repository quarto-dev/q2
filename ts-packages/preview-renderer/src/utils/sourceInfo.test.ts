import { describe, test, expect } from 'vitest';
import { entryFor, isAtomicSourceInfo, ATOMIC_KINDS } from './sourceInfo';
import type { SourceInfoPool } from '../types/sourceInfo';


// Build a representative pool covering each wire code shipped by the
// Rust writer post-Plan-5. Code 5 is unassigned — no entry exists.
const samplePool: SourceInfoPool = [
    { t: 0, r: [0, 10], d: 0 },                                            // 0: Original
    { t: 1, r: [3, 7], d: 0 },                                             // 1: Substring (parent_id 0)
    { t: 2, r: [0, 20], d: [[0, 0, 10], [0, 10, 10]] },                   // 2: Concat
    { t: 3, r: [5, 15], d: ['filter.lua', 42] },                          // 3: Legacy (string-headed FilterProvenance)
    { t: 3, r: [10, 20], d: [0] },                                         // 4: Legacy (numeric-headed Transformed)
    { t: 4, r: [0, 0], d: { by: { kind: 'sectionize' } } },               // 5: Generated, no anchors, no data
    { t: 4, r: [0, 0], d: {                                                // 6: Generated with anchor
        by: { kind: 'shortcode', data: { name: 'meta' } },
        from: [{ role: 'invocation', si_id: 0 }],
    } },
];

describe('entryFor', () => {
    test('returns the entry at node.s', () => {
        expect(entryFor({ s: 0 }, samplePool)).toEqual(samplePool[0]);
        expect(entryFor({ s: 3 }, samplePool)).toEqual(samplePool[3]);
        expect(entryFor({ s: 6 }, samplePool)).toEqual(samplePool[6]);
    });

    test('returns undefined when node lacks an s field', () => {
        expect(entryFor({}, samplePool)).toBeUndefined();
    });

    test('returns undefined when s is out of bounds', () => {
        expect(entryFor({ s: 99 }, samplePool)).toBeUndefined();
    });

    test('returns undefined when pool is undefined', () => {
        expect(entryFor({ s: 0 }, undefined)).toBeUndefined();
    });
});

describe('isAtomicSourceInfo', () => {
    const atomicKinds = new Set(['shortcode']);

    test('returns true for Generated (code 4) when by.kind is atomic', () => {
        // samplePool[6] has by.kind === 'shortcode'.
        expect(isAtomicSourceInfo({ s: 6 }, samplePool, atomicKinds)).toBe(true);
    });

    test('returns false for Generated (code 4) when by.kind is not atomic', () => {
        // samplePool[5] has by.kind === 'sectionize'.
        expect(isAtomicSourceInfo({ s: 5 }, samplePool, atomicKinds)).toBe(false);
    });

    test.each([0, 1, 2, 3, 4])('returns false for non-Generated code %d', (idx) => {
        expect(isAtomicSourceInfo({ s: idx }, samplePool, atomicKinds)).toBe(false);
    });

    test('returns false when entry is missing', () => {
        expect(isAtomicSourceInfo({}, samplePool, atomicKinds)).toBe(false);
    });

    test('treats absent `from` as empty (canonical access pattern)', () => {
        // Build a pool with one Generated entry that has no `from` field
        // at all — the writer omits it when the anchor list is empty.
        const pool: SourceInfoPool = [
            { t: 4, r: [0, 0], d: { by: { kind: 'shortcode' } } },
        ];
        expect(isAtomicSourceInfo({ s: 0 }, pool, atomicKinds)).toBe(true);
        // `entry.d.from ?? []` is the canonical access pattern for
        // consumers that want to iterate the anchor list.
        const entry = entryFor({ s: 0 }, pool);
        if (entry?.t === 4) {
            expect(entry.d.from ?? []).toEqual([]);
        } else {
            throw new Error('expected code-4 entry');
        }
    });
});

describe('ATOMIC_KINDS', () => {
    test('is exported as a ReadonlySet', () => {
        expect(ATOMIC_KINDS).toBeInstanceOf(Set);
    });

    test('contains the Plan-4 atomic-kind set', () => {
        // Mirrors `By::is_atomic_kind` on the Rust side
        // (crates/quarto-source-map/src/source_info.rs).
        expect(ATOMIC_KINDS.has('filter')).toBe(true);
        expect(ATOMIC_KINDS.has('shortcode')).toBe(true);
        expect(ATOMIC_KINDS.has('title-block')).toBe(true);
        expect(ATOMIC_KINDS.has('tree-sitter-postprocess')).toBe(true);
    });

    test('excludes known non-atomic kinds', () => {
        expect(ATOMIC_KINDS.has('sectionize')).toBe(false);
        expect(ATOMIC_KINDS.has('user-edit')).toBe(false);
        expect(ATOMIC_KINDS.has('include')).toBe(false);
    });
});

