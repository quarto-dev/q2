/**
 * @vitest-environment jsdom
 *
 * book-projects P8 checklist: `CrossrefResolvedRef` calls
 * `onNavigateToDocument` with the correct target when `owning_chapter_path`
 * is present (a cross-chapter resolution from book preview's
 * `StaticProjectAnalyzer` sweep), and renders the existing same-page `href`
 * unchanged — with no navigation call — when it's absent (a same-chapter
 * resolution, and every real non-preview book render).
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';

import { CrossrefResolvedRef } from './CrossrefResolvedRef';
import type { CustomInlineNode } from '../../framework';

afterEach(cleanup);

function makeNode(plain_data: Record<string, unknown>): CustomInlineNode {
    return {
        t: 'CustomInline',
        type_name: 'CrossrefResolvedRef',
        attr: ['', [], []],
        plain_data,
        slots: {},
    };
}

describe('CrossrefResolvedRef navigation', () => {
    it('same-chapter (local) resolution: same-page href, no onNavigateToDocument call', () => {
        const onNavigateToDocument = vi.fn();
        const node = makeNode({
            identifier: 'fig-one',
            ref_type: 'fig',
            kind: 'Figure',
            resolved: true,
            resolved_number: '1.1',
            order: { section: [], order: 1 },
        });

        render(
            <CrossrefResolvedRef
                node={node}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={() => {}}
            />,
        );

        const link = screen.getByRole('link');
        expect(link.getAttribute('href')).toBe('#fig-one');
        expect(link.textContent).toBe('Figure\u{a0}1.1');

        fireEvent.click(link);
        expect(onNavigateToDocument).not.toHaveBeenCalled();
    });

    it('cross-chapter resolution: same-page href, but clicking navigates to the owning chapter', () => {
        const onNavigateToDocument = vi.fn();
        const node = makeNode({
            identifier: 'fig-one',
            ref_type: 'fig',
            kind: 'Figure',
            resolved: true,
            resolved_number: '1.1',
            order: { section: [], order: 1 },
            owning_chapter_path: 'ch1.qmd',
        });

        render(
            <CrossrefResolvedRef
                node={node}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={() => {}}
            />,
        );

        const link = screen.getByRole('link');
        // href stays the plain same-page anchor — navigation is driven by
        // the onClick handler, not by swapping the href.
        expect(link.getAttribute('href')).toBe('#fig-one');

        fireEvent.click(link);
        expect(onNavigateToDocument).toHaveBeenCalledTimes(1);
        expect(onNavigateToDocument).toHaveBeenCalledWith('ch1.qmd', 'fig-one');
    });

    it('unresolved ref: no navigation handler regardless of owning_chapter_path', () => {
        const onNavigateToDocument = vi.fn();
        const node = makeNode({
            identifier: 'fig-missing',
            ref_type: 'fig',
            kind: 'Figure',
            resolved: false,
        });

        render(
            <CrossrefResolvedRef
                node={node}
                onNavigateToDocument={onNavigateToDocument}
                setLocalAst={() => {}}
            />,
        );

        const link = screen.getByRole('link');
        expect(link.textContent).toBe('?fig-missing?');

        fireEvent.click(link);
        expect(onNavigateToDocument).not.toHaveBeenCalled();
    });
});
