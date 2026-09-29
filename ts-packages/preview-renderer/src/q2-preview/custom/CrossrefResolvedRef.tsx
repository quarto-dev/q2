import React from 'react';
import type {
    BlockNode,
    CustomInlineNode,
    InlineNode,
    NodeArgs,
} from '../../framework';
import { Node } from '../../framework';
import { QUARTO_XREF } from '../quartoClasses';

/**
 * CrossrefResolvedRef — q2-preview port of `render_resolved_ref` at
 * `crates/quarto-core/src/transforms/crossref_render.rs:657-715`.
 *
 * Output: `<a class="quarto-xref" href="#{identifier}">{kind} {n}</a>{slot.suffix}`.
 *
 * Link-text rule:
 *   - `resolved && (resolved_number || order)` → `"{kind}\u{a0}{n}"` (NBSP
 *     between kind and number — same as Theorem; matches
 *     `crossref_render.rs:691`). `resolved_number` (book-projects P8's
 *     chapter-scoped composed string) wins over the raw `order.order`
 *     counter whenever present.
 *   - `resolved && !resolved_number && !order` → `kind` alone (rare;
 *     numbered targets always have one or the other).
 *   - `!resolved` → `"?{identifier}?"` (broken-ref affordance; matches
 *     `:695`).
 *
 * Navigation: a same-chapter (local) resolution keeps the plain same-page
 * `href="#{identifier}"` anchor. A cross-chapter resolution from book
 * preview (`owning_chapter_path` present) instead calls
 * `onNavigateToDocument(owning_chapter_path, identifier)` on click — see
 * the `plain_data` field notes below.
 *
 * **Atomic.** `isAtomicCustomNode("CrossrefResolvedRef") === true`
 * (`hub-client/src/utils/atomicCustomNodes.ts`); the framework's
 * atomic gate at `framework/dispatch.tsx:411` no-ops `setLocalAst`
 * before this component runs, so child setters here are effectively
 * pass-through. The component is read-only and renders the resolved
 * link as static JSX; the suffix slot still mounts via `<Node>` so its
 * inlines render through the registry, but writes from the suffix
 * cannot mutate the AST (atomic gate).
 *
 * `plain_data` (writer: `transforms/crossref_resolve.rs:316`; keys
 * mirrored from `crates/quarto-pandoc-types/resources/custom-node-schema.json`):
 *  - `identifier`, `ref_type`, `kind`, `resolved` (bool),
 *    `kind_source` (unused in render), `cite_mode`, `label_upper` (bool)
 *    (both unused in render — see the P2 wire-schema plan),
 *    optional `order: { section, order }`.
 *  - book-projects P8: optional `resolved_number` (a pre-composed display
 *    string, e.g. `"2.1"` — written by `crossref_resolve.rs` for a local,
 *    book-seeded resolution, or by `cross_chapter_crossref_resolve.rs` for
 *    a cross-chapter one; preferred over `order.order` for display when
 *    present, since it's chapter-scoped and `order.order` is not),
 *    optional `target_href` (cross-chapter only; unused here — hub-client
 *    navigation goes through `owning_chapter_path` + `onNavigateToDocument`
 *    instead, since `target_href` is a rendered-*output* href that means
 *    nothing to hub-client's document-based navigation), optional
 *    `owning_chapter_path` (cross-chapter, **book preview only** — a
 *    project-relative source path; absent for a same-chapter resolution
 *    and for every real, non-preview book render).
 *
 * NOTE: `cite_prefix` is a **slot** (`node.slots.cite_prefix`), not a
 * `plain_data` field — it is deliberately absent from the key array below.
 *
 * The key array is asserted against the schema (both directions) by
 * `schemaConformance.test.ts`'s T4.3.
 */

export const CROSSREF_RESOLVED_REF_PLAIN_DATA_KEYS = [
    'identifier',
    'ref_type',
    'kind',
    'resolved',
    'kind_source',
    'cite_mode',
    'label_upper',
    'order',
    'in_appendix',
    'resolved_number',
    'target_href',
    'owning_chapter_path',
] as const;

type CrossrefResolvedRefPlainData = {
    [K in (typeof CROSSREF_RESOLVED_REF_PLAIN_DATA_KEYS)[number]]?: unknown;
};

export const CrossrefResolvedRef = ({
    node,
    onNavigateToDocument,
    setLocalAst,
}: NodeArgs<CustomInlineNode>) => {
    const plain = (node.plain_data ?? {}) as CrossrefResolvedRefPlainData;
    const identifier = (plain.identifier as string | undefined) ?? '';
    const kind = (plain.kind as string | undefined) ?? '';
    const resolved = plain.resolved === true;
    const order = plain.order as { section?: number[]; order?: number } | undefined;
    const number = order?.order;
    // book-projects P8: `resolved_number` is the chapter-scoped composed
    // display string ("2.1") — prefer it over the raw `order.order` counter
    // whenever it's present. Absent for a non-book resolution, where the
    // raw counter is already the correct (flat) display value, matching
    // today's behavior exactly.
    const resolvedNumber = plain.resolved_number as string | undefined;
    const displayNumber = resolvedNumber ?? number;

    let linkText: string;
    if (!resolved) {
        linkText = `?${identifier}?`;
    } else if (displayNumber !== undefined) {
        linkText = `${kind} ${displayNumber}`;
    } else {
        linkText = kind;
    }

    // book-projects P8: a cross-chapter resolution from book preview's
    // StaticProjectAnalyzer sweep carries `owning_chapter_path` — the
    // project-relative source path of the chapter that owns this target.
    // Absent for a same-chapter (local) resolution and for every real
    // (non-preview) book render, so the same-page `href` below is
    // unchanged in both of those cases.
    const owningChapterPath = plain.owning_chapter_path as string | undefined;
    const handleClick = owningChapterPath
        ? (event: React.MouseEvent<HTMLAnchorElement>) => {
              event.preventDefault();
              onNavigateToDocument?.(owningChapterPath, identifier);
          }
        : undefined;

    const suffixSlot = node.slots.suffix;
    const suffixInlines: InlineNode[] =
        suffixSlot && suffixSlot.kind === 'inlines' ? suffixSlot.value : [];

    // Atomic — setLocalAst is no-op'd by the framework gate, but for
    // structural symmetry with the framework's renderCustomNodeChildren
    // walk we still build per-child setters. They never propagate.
    const setSuffixInline = (i: number) => (newInline: BlockNode | InlineNode) => {
        const next = suffixInlines.slice();
        next[i] = newInline as InlineNode;
        setLocalAst({
            ...node,
            slots: {
                ...node.slots,
                suffix: { kind: 'inlines', value: next },
            },
        });
    };

    return (
        <>
            <a className={QUARTO_XREF} href={`#${identifier}`} onClick={handleClick}>
                {linkText}
            </a>
            {suffixInlines.map((inl, i) => (
                <Node
                    key={i}
                    node={inl}
                    onNavigateToDocument={onNavigateToDocument}
                    setLocalAst={setSuffixInline(i)}
                />
            ))}
        </>
    );
};
