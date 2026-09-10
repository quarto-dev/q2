/**
 * Q2's port of Quarto 1's `suggestColumn` / `setMainColumn`
 * (`format-html-bootstrap.ts:1008-1019` and `:1959-1983`), mirroring the
 * Rust twin in `render_with_compiled_template`
 * (`crates/quarto-core/src/template.rs`), which feeds the same rule into
 * the `$main-column$` template variable — keep the two in sync
 * (bd-no-toc-reserves-margin-column-s8nonx0w).
 *
 * Under `page-layout: full`, `<main>` spans whichever margins are free
 * instead of the body reclaiming them with `fullcontent`; Quarto 1 applies
 * one remedy or the other, never both. The banner title tracks the same
 * class, because `.page-columns > *` and `.page-columns .column-body`
 * resolve to the same tracks — an unmoved banner would fall out of
 * alignment with a moved `<main>`.
 *
 * Lives in its own module so `PreviewDocument` and `PreviewTitleBlock`
 * cannot drift apart, and so the Rust twin has a single counterpart to
 * stay in sync with.
 */
import { extractMetaBool, extractMetaString, getMetaPath } from '../framework';

const nonEmpty = (v: string | undefined): boolean =>
    v !== undefined && v !== '';

export function suggestMainColumn(
    meta: Record<string, unknown>,
): string | undefined {
    const pageLayout = extractMetaString(meta['page-layout']) ?? 'article';
    if (pageLayout !== 'full') return undefined;

    const hasToc = nonEmpty(
        extractMetaString(getMetaPath(meta, ['rendered', 'navigation', 'toc'])),
    );
    const hasMarginCategories = nonEmpty(
        extractMetaString(
            getMetaPath(meta, ['rendered', 'navigation', 'margin_categories']),
        ),
    );
    // A relocated TOC (`toc-location: left`/`body`) leaves the right
    // margin free, so the suggestion follows where the TOC is actually
    // rendered — unlike the `fullcontent` decision, which follows Quarto
    // 1 in keying on whether a TOC exists at all.
    //
    // The preview's own markup does not yet honour `toc-relocated` —
    // `PreviewDocument` renders `TocSlot` into the right margin whenever
    // a TOC exists (a pre-existing gap, unrelated to this computation).
    // Mirroring the Rust is still the right call: the sync contract is
    // that both engines derive the same classes from the same metadata,
    // so closing that markup gap will make the preview agree without
    // touching this file.
    const tocRelocated =
        extractMetaBool(
            getMetaPath(meta, ['rendered', 'navigation', 'toc-relocated']),
        ) === true;
    // `toc-left` is the standalone regime's flag; a website page with
    // `toc-location: left` gets `toc-in-sidebar` and its TOC merged into
    // `rendered.navigation.sidebar`, so that case is covered by the
    // sidebar term below. Mirrors the Rust comment in template.rs.
    const leftToc =
        hasToc &&
        tocRelocated &&
        extractMetaBool(
            getMetaPath(meta, ['rendered', 'navigation', 'toc-left']),
        ) === true;
    const leftUsed =
        leftToc ||
        nonEmpty(
            extractMetaString(
                getMetaPath(meta, ['rendered', 'navigation', 'sidebar']),
            ),
        );
    // `#quarto-margin-sidebar` carries content only when the TOC lands
    // there, or when listing categories do.
    const rightUsed = hasMarginCategories || (hasToc && !tocRelocated);

    if (leftUsed && rightUsed) return 'column-body';
    if (leftUsed) return 'column-page-right';
    if (rightUsed) return 'column-page-left';
    return 'column-page';
}
