/**
 * Namespace-disjoint policy assertion (Plan 2C §Test plan).
 *
 * Pandoc tag names (`Para`, `Header`, `Image`, ...) and CustomNode
 * `type_name`s (`Callout`, `Theorem`, ...) are namespace-disjoint
 * by project policy: a future Quarto transform that wants to
 * introduce a `type_name` matching a Pandoc tag must pick a
 * different name. Both kinds key into the same `previewRegistry`
 * map (and the same `mergedPreviewRegistry` after user TSX overrides
 * layer on), so a collision would silently shadow one with the other.
 *
 * Pattern: static-set-vs-static-set assertion. Mirrors the Rust
 * precedent at `crates/quarto-core/src/pipeline.rs:1985` — the
 * `Q2_PREVIEW_TRANSFORM_EXCLUDED` validation test that catches typos
 * in the exclusion list at every build. Same pattern here for
 * shadowing in the format registry.
 *
 * The Pandoc tag set is hardcoded here (the test file owns the list).
 * `framework/types.ts` does not export a runtime-introspectable set
 * — `BlockNode` / `InlineNode` are TS unions, type-erased at runtime.
 * Keep this list in sync with `framework/types.ts`'s `BlockNode`
 * union (~`framework/types.ts:45-61`) and `InlineNode` union
 * (~`:90-112`) when Pandoc adds new tags.
 */

import { describe, expect, it } from 'vitest';
import * as Custom from './custom';
import { previewRegistry } from './registry';

// Hardcoded Pandoc tag list. Update when framework/types.ts adds tags.
const PANDOC_TAG_NAMES = new Set<string>([
    // Block
    'Para',
    'Plain',
    'Header',
    'BlockQuote',
    'OrderedList',
    'BulletList',
    'DefinitionList',
    'RawBlock',
    'HorizontalRule',
    'Table',
    'Div',
    'CodeBlock',
    'LineBlock',
    'Figure',
    'CustomBlock',
    // Inline
    'Str',
    'Emph',
    'Underline',
    'Strong',
    'Strikeout',
    'Superscript',
    'Subscript',
    'SmallCaps',
    'Quoted',
    'Cite',
    'Code',
    'Space',
    'SoftBreak',
    'LineBreak',
    'Math',
    'RawInline',
    'Link',
    'Image',
    'Note',
    'Span',
    'CustomInline',
]);

describe('q2-preview registry namespace-disjoint policy', () => {
    it('no Custom.* export name collides with a Pandoc tag name', () => {
        const customExportNames = Object.keys(Custom);
        const collisions = customExportNames.filter((n) =>
            PANDOC_TAG_NAMES.has(n),
        );
        expect(collisions).toEqual([]);
    });

    it('every expected CustomNode component is exported from ./custom', () => {
        const customExportNames = new Set(Object.keys(Custom));
        for (const name of [
            'Callout',
            'Theorem',
            'Proof',
            'FloatRefTarget',
            'Equation',
            'CrossrefResolvedRef',
            'Fallback',
            'PreviewTitleBlock',
        ]) {
            expect(customExportNames.has(name)).toBe(true);
        }
    });

    it('both synthetic registry keys point at the expected ./custom export', () => {
        // Locks the synthetic-key registrations so a future refactor
        // that drops either silently can't ship without breaking the
        // test. `__fallback__` was exercised by behavior tests since
        // 2C but never directly asserted — closing that gap here.
        expect(previewRegistry.__fallback__).toBe(Custom.Fallback);
        expect(previewRegistry.__title_block__).toBe(Custom.PreviewTitleBlock);
    });
});
