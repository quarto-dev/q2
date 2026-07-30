/**
 * Unit tests for the sidecar-key stripping used by `commitSubtreeEdit`
 * in PreviewRoot.tsx.
 *
 * `apply_node_edit` (Rust) throws `InvalidSourceInfoRef` when the
 * replacement subtree JSON contains pool-index sidecar fields, because
 * the replacement doc carries no pool.  `commitSubtreeEdit` strips them
 * with `stripSourceInfoFields` before sending.  These tests verify that
 * the shared replacer (the real one — not a copy) removes every
 * pool-index-carrying key at every nesting depth.
 *
 * The authoritative key inventory lives in
 * `crates/pampa/src/writers/json.rs`; see also `normalize_nodes` in
 * `crates/pampa/tests/integration/lua_differential.rs`.
 */

import { describe, test, expect } from 'vitest';
import { stripSourceInfoFields } from './stripSourceInfoFields';

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

describe('sidecar keys beyond s/a (bare pool indices on specific nodes)', () => {
    test('strips targetS from Link — the "cannot edit anything with a link" bug (#441)', () => {
        // targetS is [urlRef, titleRef]: pool indices for the Link/Image
        // URL and title source spans.
        const block = {
            t: 'Para',
            s: 1,
            c: [{
                t: 'Link',
                s: 2,
                a: { id: null, classes: [], kvs: [] },
                targetS: [3, 4],
                c: [['', [], []], [{ t: 'Str', c: 'click' }], ['https://example.com', '']],
            }],
        };
        const result = stripSourceInfoFields(block);
        const json = JSON.stringify(result);
        expect(json).not.toMatch(/"targetS":/);
        // The structural target (URL + title) must survive.
        expect((result as any).c[0].c[2]).toEqual(['https://example.com', '']);
    });

    test('strips targetS from Image', () => {
        const block = {
            t: 'Para',
            s: 1,
            c: [{
                t: 'Image',
                s: 2,
                targetS: [3, null],
                c: [['', [], []], [], ['fig.png', '']],
            }],
        };
        const json = JSON.stringify(stripSourceInfoFields(block));
        expect(json).not.toMatch(/"targetS":/);
        expect(json).toMatch(/fig\.png/);
    });

    test('strips captionS from Figure (issue #442) — a bare pool index', () => {
        // Unlike targetS, captionS is a bare index (readers/json.rs
        // read_caption passes it straight to resolve_source_info).
        const block = {
            t: 'Figure',
            s: 1,
            a: { id: null, classes: [], kvs: [] },
            captionS: 7,
            c: [
                ['fig-1', [], []],
                [null, [{ t: 'Plain', s: 8, c: [{ t: 'Str', c: 'A caption' }] }]],
                [{ t: 'Plain', s: 9, c: [{ t: 'Image', s: 10, targetS: [11, null], c: [['', [], []], [], ['fig.png', '']] }] }],
            ],
        };
        const result = stripSourceInfoFields(block);
        const json = JSON.stringify(result);
        expect(json).not.toMatch(/"captionS":/);
        // Caption content itself must survive.
        expect(json).toMatch(/A caption/);
    });

    test('strips citationIdS from Cite citation objects (issue #442)', () => {
        // Citation objects carry no `t` tag; citationIdS sits next to
        // citationId and is a bare pool index read via read_opt_source_ref.
        const block = {
            t: 'Para',
            s: 1,
            c: [{
                t: 'Cite',
                s: 2,
                c: [
                    [{
                        citationId: 'knuth1984',
                        citationIdS: 12,
                        citationPrefix: [],
                        citationSuffix: [],
                        citationMode: { t: 'NormalCitation' },
                        citationNoteNum: 0,
                        citationHash: 0,
                    }],
                    [{ t: 'Str', c: '[@knuth1984]' }],
                ],
            }],
        };
        const result = stripSourceInfoFields(block);
        const json = JSON.stringify(result);
        expect(json).not.toMatch(/"citationIdS":/);
        // The citation id itself must survive.
        expect((result as any).c[0].c[0][0].citationId).toBe('knuth1984');
    });
});
