import type { AstProps } from '../../framework';
import {
    extractMetaBool,
    extractMetaString,
    getMetaPath,
} from '../../framework';

/**
 * Built-in `__title_block__` synthetic-registry entry (Plan 2D Phase 7,
 * markup updated by the title-block parity epic bd-gx9cic8z P1).
 *
 * Mirrors the Rust built-in `title-block` / `title-metadata` template
 * partials (`TITLE_BLOCK_PARTIAL` / `TITLE_METADATA_PARTIAL` in
 * `crates/quarto-core/src/template.rs`) — Q1-parity DOM: subtitle
 * carries `lead`, the `quarto-title-meta` grid children are bare divs,
 * author/date contents are `<p>`-wrapped (`p.date`), and the abstract
 * uses `div.block-title` inside `div.abstract`.
 *
 * **Inputs.** Reads the metadata the pipeline's
 * `AuthorsNormalizeTransform` derives (the preview pipeline runs it):
 * - `rendered.has-title-block` — gates the whole `<header>`;
 * - `by-author` — normalized author list (`name.literal` per entry);
 * - `labels.*` — heading labels (pluralized / `*-title`-overridden).
 * Hardcoded fallbacks remain for direct-render contexts where the
 * transform didn't run.
 *
 * **Prop shape.** Receives `AstProps` (`{ ast, onNavigateToDocument,
 * setAst }`), the same shape registered under the `Ast` key — NOT the
 * `NodeArgs<…>` shape used by per-tag entries. The title block
 * operates on document-level state (`ast.meta`), not on a node in
 * the AST. `FormatRegistry` at `framework/types.ts` types
 * `__title_block__?: AstComponent`.
 *
 * **Composition.** To wrap the built-in, import it from
 * `window.__Q2_PREVIEW_RENDERER__.PreviewTitleBlock`; the same global
 * exposes `extractMetaString` and the other framework helpers.
 *
 * **Known fidelity gap.** A multi-paragraph abstract renders as one
 * `<p>` here (blocks flattened to text) while the Rust side emits one
 * `<p>` per paragraph. Tracked with the P2 rich-author work
 * (bd-ez0hiowa), which brings block-fidelity meta rendering.
 */
export const PreviewTitleBlock = ({ ast }: AstProps) => {
    const meta = ast.meta ?? {};

    // Mirror TITLE_BLOCK_PARTIAL's $if(rendered.has-title-block)$
    // gate. The transform sets it when any of title / subtitle /
    // authors / date / abstract exists.
    const hasTitleBlock =
        extractMetaBool(getMetaPath(meta, ['rendered', 'has-title-block'])) ===
        true;
    if (!hasTitleBlock) return null;

    const title = extractMetaString(meta.title);
    const subtitle = extractMetaString(meta.subtitle);
    const date = extractMetaString(meta.date);
    const abstract = extractMetaString(meta.abstract);
    const authors = extractByAuthorNames(meta['by-author']);

    const label = (key: string, fallback: string): string =>
        extractMetaString(getMetaPath(meta, ['labels', key])) ?? fallback;

    return (
        <header
            id="title-block-header"
            className="quarto-title-block default"
        >
            <div className="quarto-title">
                {title ? <h1 className="title">{title}</h1> : null}
                {subtitle ? (
                    <p className="subtitle lead">{subtitle}</p>
                ) : null}
            </div>
            {/* Like Q1 (and TITLE_METADATA_PARTIAL), the grid div is
                always emitted when the title block renders, even if
                all its cells are empty. */}
            <div className="quarto-title-meta">
                {authors.length > 0 ? (
                    <div>
                        <div className="quarto-title-meta-heading">
                            {label(
                                'authors',
                                authors.length > 1 ? 'Authors' : 'Author',
                            )}
                        </div>
                        <div className="quarto-title-meta-contents">
                            {authors.map((name, i) => (
                                <p key={i}>{name}</p>
                            ))}
                        </div>
                    </div>
                ) : null}
                {date ? (
                    <div>
                        <div className="quarto-title-meta-heading">
                            {label('published', 'Published')}
                        </div>
                        <div className="quarto-title-meta-contents">
                            <p className="date">{date}</p>
                        </div>
                    </div>
                ) : null}
            </div>
            {abstract ? (
                <div>
                    <div className="abstract">
                        <div className="block-title">
                            {label('abstract', 'Abstract')}
                        </div>
                        <p>{abstract}</p>
                    </div>
                </div>
            ) : null}
        </header>
    );
};

/**
 * Extract the display names from a normalized `by-author` MetaList
 * (entries are MetaMaps shaped `{ name: { literal } }`, written by
 * `AuthorsNormalizeTransform`). Returns `[]` for missing/wrong shapes.
 */
function extractByAuthorNames(byAuthor: unknown): string[] {
    if (!byAuthor || typeof byAuthor !== 'object') return [];
    const m = byAuthor as { t?: string; c?: unknown };
    if (m.t !== 'MetaList' || !Array.isArray(m.c)) return [];
    const out: string[] = [];
    for (const entry of m.c) {
        // Entries are MetaMaps; getMetaPath's first step expects the
        // plain top-level record, so wrap the entry to reuse its
        // MetaMap walking for the ['name', 'literal'] descent.
        const literal = extractMetaString(
            getMetaPath({ entry }, ['entry', 'name', 'literal']),
        );
        if (literal !== undefined && literal.length > 0) out.push(literal);
    }
    return out;
}
