/**
 * Vitest unit tests for `framework/meta.ts` (Plan 2D Phase 6.0f).
 */

import { describe, expect, it } from 'vitest';
import {
    extractMetaString,
    extractMetaBool,
    extractMetaStringList,
} from './meta';

describe('extractMetaString', () => {
    it('returns the string for MetaString', () => {
        expect(extractMetaString({ t: 'MetaString', c: 'hello' })).toBe('hello');
    });

    it('walks Str/Space inside MetaInlines', () => {
        expect(
            extractMetaString({
                t: 'MetaInlines',
                c: [
                    { t: 'Str', c: 'Hello' },
                    { t: 'Space' },
                    { t: 'Str', c: 'world' },
                ],
            }),
        ).toBe('Hello world');
    });

    it('recurses into Emph/Strong/Code/Link inside MetaInlines', () => {
        expect(
            extractMetaString({
                t: 'MetaInlines',
                c: [
                    { t: 'Emph', c: [{ t: 'Str', c: 'Hi' }] },
                    { t: 'Space' },
                    { t: 'Strong', c: [{ t: 'Str', c: 'there' }] },
                ],
            }),
        ).toBe('Hi there');

        expect(
            extractMetaString({
                t: 'MetaInlines',
                c: [
                    { t: 'Code', c: [['', [], []], 'foo'] },
                ],
            }),
        ).toBe('foo');

        expect(
            extractMetaString({
                t: 'MetaInlines',
                c: [
                    {
                        t: 'Link',
                        c: [
                            ['', [], []],
                            [{ t: 'Str', c: 'click' }],
                            ['/x', ''],
                        ],
                    },
                ],
            }),
        ).toBe('click');
    });

    it('walks MetaBlocks via blocksToPlainText', () => {
        expect(
            extractMetaString({
                t: 'MetaBlocks',
                c: [
                    {
                        t: 'Para',
                        c: [
                            { t: 'Str', c: 'Abstract' },
                            { t: 'Space' },
                            { t: 'Str', c: 'text.' },
                        ],
                    },
                ],
            }),
        ).toBe('Abstract text.');
    });

    it('returns undefined for MetaBool / MetaList / MetaMap', () => {
        expect(extractMetaString({ t: 'MetaBool', c: true })).toBeUndefined();
        expect(extractMetaString({ t: 'MetaList', c: [] })).toBeUndefined();
        expect(extractMetaString({ t: 'MetaMap', c: {} })).toBeUndefined();
    });

    it('returns undefined for null / undefined / non-object / wrong shape', () => {
        expect(extractMetaString(null)).toBeUndefined();
        expect(extractMetaString(undefined)).toBeUndefined();
        expect(extractMetaString('hello')).toBeUndefined();
        expect(extractMetaString(42)).toBeUndefined();
        expect(extractMetaString({})).toBeUndefined();
    });
});

describe('extractMetaBool', () => {
    it('returns the boolean for MetaBool', () => {
        expect(extractMetaBool({ t: 'MetaBool', c: true })).toBe(true);
        expect(extractMetaBool({ t: 'MetaBool', c: false })).toBe(false);
    });

    it('parses MetaString("true" | "false")', () => {
        expect(extractMetaBool({ t: 'MetaString', c: 'true' })).toBe(true);
        expect(extractMetaBool({ t: 'MetaString', c: 'false' })).toBe(false);
    });

    it('returns undefined for other MetaString values', () => {
        expect(extractMetaBool({ t: 'MetaString', c: 'yes' })).toBeUndefined();
        expect(extractMetaBool({ t: 'MetaString', c: '' })).toBeUndefined();
    });

    it('returns undefined for MetaInlines / other shapes / missing', () => {
        expect(
            extractMetaBool({ t: 'MetaInlines', c: [{ t: 'Str', c: 'true' }] }),
        ).toBeUndefined();
        expect(extractMetaBool({ t: 'MetaList', c: [] })).toBeUndefined();
        expect(extractMetaBool(null)).toBeUndefined();
        expect(extractMetaBool(undefined)).toBeUndefined();
    });
});

describe('extractMetaStringList', () => {
    it('returns the strings for a MetaList of MetaString', () => {
        expect(
            extractMetaStringList({
                t: 'MetaList',
                c: [
                    { t: 'MetaString', c: 'Alice' },
                    { t: 'MetaString', c: 'Bob' },
                ],
            }),
        ).toEqual(['Alice', 'Bob']);
    });

    it('coerces MetaInlines entries via extractMetaString', () => {
        expect(
            extractMetaStringList({
                t: 'MetaList',
                c: [
                    {
                        t: 'MetaInlines',
                        c: [{ t: 'Str', c: 'Carol' }],
                    },
                    {
                        t: 'MetaInlines',
                        c: [
                            { t: 'Str', c: 'Dr.' },
                            { t: 'Space' },
                            { t: 'Str', c: 'Eve' },
                        ],
                    },
                ],
            }),
        ).toEqual(['Carol', 'Dr. Eve']);
    });

    it('keeps empty-string entries (does not filter)', () => {
        expect(
            extractMetaStringList({
                t: 'MetaList',
                c: [
                    { t: 'MetaString', c: 'Alice' },
                    { t: 'MetaString', c: '' },
                ],
            }),
        ).toEqual(['Alice', '']);
    });

    it('returns [] for a single MetaString (use extractMetaString for that shape)', () => {
        expect(
            extractMetaStringList({ t: 'MetaString', c: 'Alice' }),
        ).toEqual([]);
    });

    it('returns [] for missing / wrong shape', () => {
        expect(extractMetaStringList(undefined)).toEqual([]);
        expect(extractMetaStringList(null)).toEqual([]);
        expect(extractMetaStringList({})).toEqual([]);
        expect(extractMetaStringList({ t: 'MetaBool', c: true })).toEqual([]);
    });
});
