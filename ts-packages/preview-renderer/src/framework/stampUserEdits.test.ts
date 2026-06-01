/**
 * Tests for the `stampUserEdits` walker (Plan 7f Phase 3).
 *
 * The walker stamps `s: USER_EDIT_SOURCE_INFO_ID` on every AST node a
 * `setLocalAst` call introduces without an existing `s:`. Preserved
 * subtrees (rebuilt-wrapper case from Phase 2) keep their original `s:`.
 */

import { describe, it, expect } from 'vitest';
import { stampUserEdits } from './dispatch';
import { USER_EDIT_SOURCE_INFO_ID } from '../types/sourceInfo';
import type { BlockNode, CustomBlockNode, InlineNode } from './types';

describe('stampUserEdits', () => {
    it('stamps a freshly-constructed Span with USER_EDIT_SOURCE_INFO_ID', () => {
        // User affordance constructs a Span from scratch — no `s:` on
        // the new node or its children.
        const newSpan: InlineNode = {
            t: 'Span',
            c: [['', [], []], [{ t: 'Str', c: 'hello' }]],
        } as InlineNode;

        const stamped = stampUserEdits(newSpan) as any;
        expect(stamped.s).toBe(USER_EDIT_SOURCE_INFO_ID);
        // Children also get stamped because they're fresh too.
        expect(stamped.c[1][0].s).toBe(USER_EDIT_SOURCE_INFO_ID);
    });

    it('preserves the original `s:` on a rebuilt-wrapper case', () => {
        // The Phase 2 spread-fix means rebuilt parents already carry `s:`.
        // `stampUserEdits` must not overwrite it. Children that already
        // have `s:` (preserved subtree) must keep their values.
        const rebuilt = {
            t: 'Para',
            s: 42,
            c: [
                { t: 'Str', s: 1, c: 'a' },
                { t: 'Str', s: 2, c: 'b' },
            ],
        } as any;

        const stamped = stampUserEdits(rebuilt) as any;
        expect(stamped.s).toBe(42);
        expect(stamped.c[0].s).toBe(1);
        expect(stamped.c[1].s).toBe(2);
    });

    it('stamps only the new child when a wrapper rebuild splices one in', () => {
        // The typical Phase 2 path: existing parent (s: 42), existing
        // child at index 0 (s: 1), new child at index 1 with no `s:`.
        const node = {
            t: 'Emph',
            s: 42,
            c: [
                { t: 'Str', s: 1, c: 'kept' },
                { t: 'Str', c: 'new' },
            ],
        } as any;

        const stamped = stampUserEdits(node) as any;
        expect(stamped.s).toBe(42);
        expect(stamped.c[0].s).toBe(1);
        expect(stamped.c[1].s).toBe(USER_EDIT_SOURCE_INFO_ID);
    });

    it('recurses into CustomBlock slots and stamps nested nodes', () => {
        // User affordance constructs a new Callout via setLocalAst.
        // Nested nodes inside slots must all be stamped.
        const newCallout: CustomBlockNode = {
            t: 'CustomBlock',
            type_name: 'Callout',
            slots: {
                title: { kind: 'inlines', value: [{ t: 'Str', c: 'Note' } as InlineNode] },
                content: {
                    kind: 'blocks',
                    value: [{ t: 'Para', c: [{ t: 'Str', c: 'body' }] } as BlockNode],
                },
            },
            plain_data: { type: 'note', icon: true, appearance: 'default' },
            attr: ['', [], []],
        };

        const stamped = stampUserEdits(newCallout) as any;
        expect(stamped.s).toBe(USER_EDIT_SOURCE_INFO_ID);
        expect(stamped.slots.title.value[0].s).toBe(USER_EDIT_SOURCE_INFO_ID);
        expect(stamped.slots.content.value[0].s).toBe(USER_EDIT_SOURCE_INFO_ID);
        // Para's child Str also stamped recursively.
        expect(stamped.slots.content.value[0].c[0].s).toBe(USER_EDIT_SOURCE_INFO_ID);
    });

    it('recurses into `block` and `inline` (single-value) CustomNode slots', () => {
        const node: CustomBlockNode = {
            t: 'CustomBlock',
            type_name: 'FloatRefTarget',
            slots: {
                content: { kind: 'block', value: { t: 'Para', c: [{ t: 'Str', c: 'body' }] } as BlockNode },
            },
            plain_data: {},
            attr: ['', [], []],
        };

        const stamped = stampUserEdits(node) as any;
        expect(stamped.s).toBe(USER_EDIT_SOURCE_INFO_ID);
        expect(stamped.slots.content.value.s).toBe(USER_EDIT_SOURCE_INFO_ID);
        expect(stamped.slots.content.value.c[0].s).toBe(USER_EDIT_SOURCE_INFO_ID);
    });

    it('walks nested arrays in `c:` (Header, Link, BulletList shapes)', () => {
        // Header.c = [level: number, Attr, InlineNode[]]. The inline
        // array sits at tuple position 2 — without nested-array walking
        // these inlines would not be stamped.
        const header = {
            t: 'Header',
            c: [1, ['', [], []], [{ t: 'Str', c: 'Heading' }]],
        } as any;

        const stamped = stampUserEdits(header) as any;
        expect(stamped.s).toBe(USER_EDIT_SOURCE_INFO_ID);
        expect(stamped.c[2][0].s).toBe(USER_EDIT_SOURCE_INFO_ID);

        // BulletList.c is BlockNode[][] — items are arrays of blocks.
        const bullet = {
            t: 'BulletList',
            c: [[{ t: 'Para', c: [{ t: 'Str', c: 'a' }] }]],
        } as any;

        const stampedBullet = stampUserEdits(bullet) as any;
        expect(stampedBullet.s).toBe(USER_EDIT_SOURCE_INFO_ID);
        expect(stampedBullet.c[0][0].s).toBe(USER_EDIT_SOURCE_INFO_ID);
        expect(stampedBullet.c[0][0].c[0].s).toBe(USER_EDIT_SOURCE_INFO_ID);
    });

    it('is idempotent — re-stamping a stamped subtree is a no-op', () => {
        const newSpan: InlineNode = { t: 'Span', c: [['', [], []], [{ t: 'Str', c: 'hi' }]] } as InlineNode;
        const once = stampUserEdits(newSpan) as any;
        const twice = stampUserEdits(once) as any;
        expect(twice.s).toBe(once.s);
        expect(twice.c[1][0].s).toBe(once.c[1][0].s);
    });
});
