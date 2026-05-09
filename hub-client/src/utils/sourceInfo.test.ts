import { describe, test, expect } from 'vitest';
import {
    entryFor,
    isDerived,
    isAtomicSourceInfo,
    ATOMIC_SYNTHETIC_KINDS,
} from './sourceInfo';
import type { SourceInfoPool } from '../types/sourceInfo';

// Build a representative pool covering each wire code.
const samplePool: SourceInfoPool = [
    { t: 0, r: [0, 10], d: 0 },                              // 0: Original
    { t: 1, r: [3, 7], d: 0 },                               // 1: Substring (parent_id 0)
    { t: 2, r: [0, 20], d: [[0, 0, 10], [1, 10, 10]] },      // 2: Concat
    { t: 3, r: [5, 15], d: ['filter.lua', 42] },             // 3: FilterProvenance
    { t: 4, r: [0, 0], d: { kind: 'IncludeShortcode' } },    // 4: Synthetic
    { t: 5, r: [0, 0], d: { from: 0, by: { kind: 'CrossrefResolver' } } }, // 5: Derived
];

describe('entryFor', () => {
    test('returns the entry at node.s', () => {
        expect(entryFor({ s: 0 }, samplePool)).toEqual(samplePool[0]);
        expect(entryFor({ s: 3 }, samplePool)).toEqual(samplePool[3]);
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

describe('isDerived', () => {
    test('returns true for code 5 (Derived)', () => {
        expect(isDerived({ s: 5 }, samplePool)).toBe(true);
    });

    test('returns false for code 4 (Synthetic)', () => {
        expect(isDerived({ s: 4 }, samplePool)).toBe(false);
    });

    test.each([0, 1, 2, 3])('returns false for code %d', (idx) => {
        expect(isDerived({ s: idx }, samplePool)).toBe(false);
    });

    test('returns false when entry is missing', () => {
        expect(isDerived({}, samplePool)).toBe(false);
        expect(isDerived({ s: 99 }, samplePool)).toBe(false);
    });
});

describe('isAtomicSourceInfo', () => {
    const atomicKinds = new Set(['CrossrefResolver']);

    test('returns true for Derived entries (code 5)', () => {
        expect(isAtomicSourceInfo({ s: 5 }, samplePool, atomicKinds)).toBe(true);
    });

    test('returns true for Synthetic (code 4) when kind is in atomic set', () => {
        const pool: SourceInfoPool = [{ t: 4, r: [0, 0], d: { kind: 'CrossrefResolver' } }];
        expect(isAtomicSourceInfo({ s: 0 }, pool, atomicKinds)).toBe(true);
    });

    test('returns false for Synthetic (code 4) when kind is not atomic', () => {
        expect(isAtomicSourceInfo({ s: 4 }, samplePool, atomicKinds)).toBe(false);
    });

    test.each([0, 1, 2, 3])('returns false for non-Synthetic non-Derived code %d', (idx) => {
        expect(isAtomicSourceInfo({ s: idx }, samplePool, atomicKinds)).toBe(false);
    });

    test('returns false when entry is missing', () => {
        expect(isAtomicSourceInfo({}, samplePool, atomicKinds)).toBe(false);
    });
});

describe('ATOMIC_SYNTHETIC_KINDS', () => {
    test('is exported as a ReadonlySet', () => {
        expect(ATOMIC_SYNTHETIC_KINDS).toBeInstanceOf(Set);
    });

    test('is empty in 2A — Plan 4/6 will populate', () => {
        expect(ATOMIC_SYNTHETIC_KINDS.size).toBe(0);
    });
});
