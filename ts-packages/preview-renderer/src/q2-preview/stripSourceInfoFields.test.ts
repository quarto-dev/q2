/**
 * Unit tests for the `s`-field stripping assumption used by
 * `commitSubtreeEdit` in entry.tsx.
 *
 * `apply_node_edit` (Rust) throws `InvalidSourceInfoRef` when the
 * replacement subtree JSON contains `s` pool-index fields, because the
 * replacement doc carries no pool.  `commitSubtreeEdit` strips them with
 * a JSON.stringify replacer before sending.  These tests verify that the
 * replacer actually removes `s` at every nesting depth.
 */

import { describe, test, expect } from 'vitest';

/**
 * The exact replacer used in commitSubtreeEdit (entry.tsx).
 * Strips both `s` (block/inline SourceInfo pool-index) and `a` (AttrSourceInfo
 * object whose `id`/`classes`/`kvs` values are also pool indices).
 */
function stripSourceInfoFields<T>(block: T): T {
    return JSON.parse(JSON.stringify(block, (key, value) =>
        key === 's' || key === 'a' ? undefined : value,
    )) as T;
}

describe('s-field stripping for commitSubtreeEdit', () => {
    test('strips s from a flat block', () => {
        const block = { t: 'Para', s: 42, c: [] };
        const result = stripSourceInfoFields(block);
        expect('s' in result).toBe(false);
        expect(result.t).toBe('Para');
    });

    test('strips s from deeply nested children', () => {
        const block = {
            t: 'Div',
            s: 546,
            c: [['id', [], []], [
                { t: 'Header', s: 100, c: [2, ['', [], []], [{ t: 'Str', c: 'backlog' }]] },
                { t: 'BulletList', s: 200, c: [
                    [{ t: 'Plain', s: 300, c: [{ t: 'Str', c: 'item one' }] }],
                    [{ t: 'Plain', s: 301, c: [{ t: 'Str', c: 'item two' }] }],
                ]},
            ]],
        };
        const result = stripSourceInfoFields(block);
        const json = JSON.stringify(result);
        expect(json).not.toMatch(/"s":/);
    });

    test('strips a (AttrSourceInfo) fields — the root cause of InvalidSourceInfoRef(546)', () => {
        // The untransformed AST writes an "a" field alongside "s" on every
        // block/inline.  The "a" object contains pool indices for the
        // attribute's id, classes, and kvs source locations.  Rust's
        // read_attr_source calls from_json_ref on each of those values; with
        // an empty pool that throws InvalidSourceInfoRef even when all "s"
        // fields are absent.
        const block = {
            t: 'Div',
            s: 545,
            a: { id: 546, classes: [547], kvs: [[548, 549]] },
            c: [['', ['kanban'], []], [
                { t: 'Header', s: 10, a: { id: null, classes: [], kvs: [] },
                  c: [2, ['', [], []], [{ t: 'Str', c: 'backlog' }]] },
            ]],
        };
        const result = stripSourceInfoFields(block);
        const json = JSON.stringify(result);
        expect(json).not.toMatch(/"s":/);
        expect(json).not.toMatch(/"a":/);
        // Structural content must survive
        expect((result as any).t).toBe('Div');
        expect((result as any).c[0]).toEqual(['', ['kanban'], []]);
    });

    test('preserves all non-s fields at every depth', () => {
        const block = {
            t: 'Div',
            s: 546,
            c: [['id', ['kanban'], []], [
                { t: 'Header', s: 1, c: [2, ['', [], []], [{ t: 'Str', c: 'doing' }]] },
            ]],
        };
        const result = stripSourceInfoFields(block);
        expect(result.t).toBe('Div');
        expect((result as any).c[0]).toEqual(['id', ['kanban'], []]);
        expect((result as any).c[1][0].t).toBe('Header');
        expect((result as any).c[1][0].c[0]).toBe(2);
    });

    test('strips s from root-level block (the kanban Div itself)', () => {
        // This is the exact scenario: kanban.tsx does
        //   const modified = structuredClone(resolved.sourceNode);
        //   modified.c[1] = newBlocks;
        //   edit.commitSubtreeEdit(..., modified);
        // The root `modified` object carries `s: 546` from sourceNode.
        const kanbanDiv = { t: 'Div', s: 546, c: [['', ['kanban'], []], []] };
        const result = stripSourceInfoFields(kanbanDiv);
        expect('s' in result).toBe(false);
        expect(result.t).toBe('Div');
    });

    test('freshly-constructed blocks (no s field) pass through unchanged', () => {
        // newBlocks in kanban.tsx are constructed without `s` — verify no
        // unintended mutation occurs.
        const newBlock = {
            t: 'BulletList',
            c: [[{ t: 'Plain', c: [{ t: 'Str', c: 'item one' }] }]],
        };
        const result = stripSourceInfoFields(newBlock);
        expect(JSON.stringify(result)).toBe(JSON.stringify(newBlock));
    });

    test('wrapped doc format has no s or a fields after stripping', () => {
        const modifiedBlock = {
            t: 'Div', s: 546, a: { id: 547, classes: [], kvs: [] },
            c: [['', ['kanban'], []], [
                { t: 'Header', s: 10, a: { id: 11, classes: [], kvs: [] },
                  c: [2, ['', [], []], []] },
            ]],
        };
        const stripped = stripSourceInfoFields(modifiedBlock);
        const wrappedDoc = { 'pandoc-api-version': [1, 23, 0], meta: {}, blocks: [stripped] };
        const json = JSON.stringify(wrappedDoc);
        expect(json).not.toMatch(/"s":/);
        expect(json).not.toMatch(/"a":/);
    });
});
