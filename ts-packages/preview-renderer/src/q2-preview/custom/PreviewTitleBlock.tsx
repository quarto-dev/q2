import type { AstProps, BlockNode, InlineNode } from '../../framework';
import {
    extractMetaBool,
    extractMetaString,
    extractMetaStringList,
    getMetaPath,
    inlinesToPlainText,
} from '../../framework';

/**
 * Built-in `__title_block__` synthetic-registry entry (Plan 2D Phase 7,
 * markup updated by the title-block parity epic bd-gx9cic8z P1/P2/P3).
 *
 * Mirrors the Rust built-in `title-block` / `title-metadata` /
 * `_title-meta-author` template partials (`TITLE_BLOCK_PARTIAL` /
 * `TITLE_METADATA_PARTIAL` / `TITLE_META_AUTHOR_PARTIAL` in
 * `crates/quarto-core/src/template.rs`) — Q1-parity DOM: subtitle
 * carries `lead`, the `quarto-title-meta` grid children are bare divs,
 * author/date contents are `<p>`-wrapped (`p.date`), the abstract
 * uses `div.block-title` inside `div.abstract`, and structured
 * authors render Q1's two-column `.quarto-title-meta-author` grid
 * with url-linked names, degrees, an email icon anchor, and an ORCID
 * badge anchor (both inline SVGs, design decision Q8).
 *
 * **Inputs.** Reads the metadata the pipeline's
 * `AuthorsNormalizeTransform` derives (the preview pipeline runs it):
 * - `rendered.has-title-block` — gates the whole `<header>`;
 * - `by-author` — normalized author list (`name.literal`, `url`,
 *   `email`, `orcid`, `degrees`, denormalized `affiliations`);
 * - `labels.*` — heading labels (pluralized / `*-title`-overridden);
 * - `quarto-template-params.title-block-categories` — the category
 *   chips gate (P3, bd-j6huijli), written unless the document sets
 *   `title-block-categories: false`.
 * P3 also renders the raw metadata-grid fields: `date-modified`
 * (Modified cell), `doi` (Doi cell, linked to doi.org), `keywords`
 * (trailing block), `description` (block below the title, suppressed
 * by `hide-description`), and `categories`.
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
 * **Known fidelity gap.** Inline markup inside title-block fields
 * (emphasis in an abstract paragraph, say) flattens to plain text
 * here while the Rust side renders it as HTML. Paragraph structure
 * is preserved (one `<p>` per abstract paragraph, since P2).
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
    const dateModified = extractMetaString(meta['date-modified']);
    const doi = extractMetaString(meta.doi);
    const keywords = extractMetaStringList(meta.keywords);
    const description = extractMetaString(meta.description);
    const hideDescription =
        extractMetaBool(meta['hide-description']) === true;
    const abstractParagraphs = extractParagraphs(meta.abstract);
    const authors = extractByAuthors(meta['by-author']);
    const hasAffiliations = authors.some((a) => a.affiliations.length > 0);

    // Mirror TITLE_BLOCK_PARTIAL's
    // $if(quarto-template-params.title-block-categories)$ gate —
    // `AuthorsNormalizeTransform` writes the param unless the document
    // sets `title-block-categories: false`. The fallback reads the raw
    // option for direct-render contexts where the transform didn't run
    // (absent means enabled, Q1's default).
    const categoriesParam = extractMetaBool(
        getMetaPath(meta, [
            'quarto-template-params',
            'title-block-categories',
        ]),
    );
    const categoriesEnabled =
        categoriesParam ??
        extractMetaBool(meta['title-block-categories']) !== false;
    const categories = categoriesEnabled
        ? extractMetaStringList(meta.categories)
        : [];

    const label = (key: string, fallback: string): string =>
        extractMetaString(getMetaPath(meta, ['labels', key])) ?? fallback;

    // Banner mode (P5, bd-364ol5lu): `TitleBannerTransform` writes the
    // flag; the markup mirrors TITLE_BLOCK_PARTIAL's banner branch —
    // title/subtitle/description/categories inside
    // div.quarto-title-banner > div.quarto-title.column-body, the meta
    // grids below the banner, page-columns page-full on header +
    // banner div, and (Q1 banner-partial parity) NO hide-description
    // gate.
    const banner =
        extractMetaBool(
            getMetaPath(meta, ['rendered', 'title-block-banner']),
        ) === true;

    const categoryChips =
        categories.length > 0 ? (
            <div className="quarto-categories">
                {categories.map((category, i) => (
                    <div className="quarto-category" key={i}>
                        {category}
                    </div>
                ))}
            </div>
        ) : null;

    const descriptionBlock = description ? (
        <div>
            <div className="description">{description}</div>
        </div>
    ) : null;

    if (banner) {
        return (
            <header
                id="title-block-header"
                className="quarto-title-block default page-columns page-full"
            >
                <div className="quarto-title-banner page-columns page-full">
                    <div className="quarto-title column-body">
                        {title ? <h1 className="title">{title}</h1> : null}
                        {subtitle ? (
                            <p className="subtitle lead">{subtitle}</p>
                        ) : null}
                        {descriptionBlock}
                        {categoryChips}
                    </div>
                </div>
                <TitleMetaGrids
                    authors={authors}
                    hasAffiliations={hasAffiliations}
                    date={date}
                    dateModified={dateModified}
                    doi={doi}
                    keywords={keywords}
                    abstractParagraphs={abstractParagraphs}
                    label={label}
                />
            </header>
        );
    }

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
                {categoryChips}
            </div>
            {/* Q11 gate: hide-description suppresses the block (set by
                Q1's book pipeline for chapter pages; nothing sets it
                in Q2 yet). */}
            {!hideDescription ? descriptionBlock : null}
            <TitleMetaGrids
                authors={authors}
                hasAffiliations={hasAffiliations}
                date={date}
                dateModified={dateModified}
                doi={doi}
                keywords={keywords}
                abstractParagraphs={abstractParagraphs}
                label={label}
            />
        </header>
    );
};

/** Props for the shared metadata-grid fragment. */
interface TitleMetaGridsProps {
    authors: PreviewAuthor[];
    hasAffiliations: boolean;
    date?: string;
    dateModified?: string;
    doi?: string;
    keywords: string[];
    abstractParagraphs: string[];
    label: (key: string, fallback: string) => string;
}

/**
 * The metadata grids below the title — the `title-metadata` partial's
 * output, shared verbatim by the default and banner layouts (in Q1
 * both title-block partials call `$title-metadata.html()$`).
 */
const TitleMetaGrids = ({
    authors,
    hasAffiliations,
    date,
    dateModified,
    doi,
    keywords,
    abstractParagraphs,
    label,
}: TitleMetaGridsProps) => (
    <>
        {/* Mirror TITLE_METADATA_PARTIAL: with affiliations the
            authors move to the two-column grid; without, they
            stay a cell of the plain grid (Q1's
            $if(by-affiliation)$ / $elseif(by-author)$ split). */}
        {hasAffiliations ? (
            <div className="quarto-title-meta-author">
                <div className="quarto-title-meta-heading">
                    {label(
                        'authors',
                        authors.length > 1 ? 'Authors' : 'Author',
                    )}
                </div>
                <div className="quarto-title-meta-heading">
                    {label(
                        'affiliations',
                        countAffiliations(authors) > 1
                            ? 'Affiliations'
                            : 'Affiliation',
                    )}
                </div>
                {authors.map((author, i) => (
                    <AuthorAffiliationRow key={i} author={author} />
                ))}
            </div>
        ) : null}
        {/* Like Q1 (and TITLE_METADATA_PARTIAL), the grid div is
            always emitted when the title block renders, even if
            all its cells are empty. */}
        <div className="quarto-title-meta">
            {!hasAffiliations && authors.length > 0 ? (
                <div>
                    <div className="quarto-title-meta-heading">
                        {label(
                            'authors',
                            authors.length > 1 ? 'Authors' : 'Author',
                        )}
                    </div>
                    <div className="quarto-title-meta-contents">
                        {authors.map((author, i) => (
                            <p key={i}>
                                <AuthorInline author={author} />
                            </p>
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
            {dateModified ? (
                <div>
                    <div className="quarto-title-meta-heading">
                        {label('modified', 'Modified')}
                    </div>
                    <div className="quarto-title-meta-contents">
                        <p className="date-modified">{dateModified}</p>
                    </div>
                </div>
            ) : null}
            {doi ? (
                <div>
                    <div className="quarto-title-meta-heading">
                        {label('doi', 'Doi')}
                    </div>
                    <div className="quarto-title-meta-contents">
                        <p className="doi">
                            <a href={`https://doi.org/${doi}`}>{doi}</a>
                        </p>
                    </div>
                </div>
            ) : null}
        </div>
        {abstractParagraphs.length > 0 ? (
            <div>
                <div className="abstract">
                    <div className="block-title">
                        {label('abstract', 'Abstract')}
                    </div>
                    {abstractParagraphs.map((text, i) => (
                        <p key={i}>{text}</p>
                    ))}
                </div>
            </div>
        ) : null}
        {keywords.length > 0 ? (
            <div>
                <div className="keywords">
                    <div className="block-title">
                        {label('keywords', 'Keywords')}
                    </div>
                    <p>{keywords.join(', ')}</p>
                </div>
            </div>
        ) : null}
    </>
);

/** One normalized `by-author` entry's display surface. */
interface PreviewAuthor {
    name: string;
    url?: string;
    email?: string;
    orcid?: string;
    degrees: string[];
    affiliations: PreviewAffiliation[];
}

interface PreviewAffiliation {
    name?: string;
    url?: string;
}

/** One row of the two-column grid: author cell + affiliations cell. */
const AuthorAffiliationRow = ({ author }: { author: PreviewAuthor }) => (
    <>
        <div className="quarto-title-meta-contents">
            <p className="author">
                <AuthorInline author={author} />
            </p>
        </div>
        <div className="quarto-title-meta-contents">
            {author.affiliations.map((aff, i) => (
                <p className="affiliation" key={i}>
                    {aff.url ? (
                        <a href={aff.url}>{aff.name}</a>
                    ) : (
                        aff.name
                    )}
                </p>
            ))}
        </div>
    </>
);

/**
 * One author's inline rendering — the `_title-meta-author` partial:
 * name (linked when `url` is set) with degrees inside the link, then
 * the email and ORCID icon anchors.
 */
const AuthorInline = ({ author }: { author: PreviewAuthor }) => {
    const display =
        author.degrees.length > 0
            ? `${author.name}, ${author.degrees.join(', ')}`
            : author.name;
    return (
        <>
            {author.url ? <a href={author.url}>{display}</a> : display}
            {author.email ? (
                <>
                    {' '}
                    <a
                        href={`mailto:${author.email}`}
                        className="quarto-title-author-email"
                    >
                        <EnvelopeIcon />
                    </a>
                </>
            ) : null}
            {author.orcid ? (
                <>
                    {' '}
                    <a
                        href={`https://orcid.org/${author.orcid}`}
                        className="quarto-title-author-orcid"
                        aria-label={`ORCID profile for ${author.name}`}
                    >
                        <OrcidIcon />
                    </a>
                </>
            ) : null}
        </>
    );
};

/**
 * Bootstrap Icons `envelope`, inlined (the icon font only ships with
 * website projects). Byte-for-byte the SVG the Rust
 * `TITLE_META_AUTHOR_PARTIAL` emits.
 */
const EnvelopeIcon = () => (
    <svg
        xmlns="http://www.w3.org/2000/svg"
        width="16"
        height="16"
        fill="currentColor"
        className="bi bi-envelope"
        viewBox="0 0 16 16"
        aria-hidden="true"
        focusable="false"
    >
        <path d="M0 4a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H2a2 2 0 0 1-2-2zm2-1a1 1 0 0 0-1 1v.217l7 4.2 7-4.2V4a1 1 0 0 0-1-1zm13 2.383-4.708 2.825L15 11.105zm-.034 6.876-5.64-3.471L8 9.583l-1.326-.795-5.64 3.47A1 1 0 0 0 2 13h12a1 1 0 0 0 .966-.741M1 11.105l4.708-2.897L1 5.383z" />
    </svg>
);

/**
 * The ORCID iD glyph in ORCID brand green, inlined (design decision
 * Q8 — Q1 used a base64 PNG). Byte-for-byte the SVG the Rust
 * `TITLE_META_AUTHOR_PARTIAL` emits.
 */
const OrcidIcon = () => (
    <svg
        xmlns="http://www.w3.org/2000/svg"
        width="16"
        height="16"
        fill="#A6CE39"
        viewBox="0 0 24 24"
        aria-hidden="true"
        focusable="false"
    >
        <path d="M12 0C5.372 0 0 5.372 0 12s5.372 12 12 12 12-5.372 12-12S18.628 0 12 0zM7.369 4.378c.525 0 .947.431.947.947s-.422.947-.947.947a.95.95 0 0 1-.947-.947c0-.525.422-.947.947-.947zm-.722 3.038h1.444v10.041H6.647V7.416zm3.562 0h3.9c3.712 0 5.344 2.653 5.344 5.025 0 2.578-2.016 5.025-5.325 5.025h-3.919V7.416zm1.444 1.303v7.444h2.297c3.272 0 4.022-2.484 4.022-3.722 0-2.016-1.284-3.722-4.097-3.722h-2.222z" />
    </svg>
);

/** Count the affiliation cells across all authors (for the fallback
 * Affiliation/Affiliations pluralization when `labels` is absent). */
function countAffiliations(authors: PreviewAuthor[]): number {
    return authors.reduce((n, a) => n + a.affiliations.length, 0);
}

/**
 * Extract the display surface from a normalized `by-author` MetaList
 * (entries are MetaMaps shaped `{ name: { literal }, url?, email?,
 * orcid?, degrees?, affiliations? }`, written by
 * `AuthorsNormalizeTransform`). Entries without a non-empty
 * `name.literal` are dropped. Returns `[]` for missing/wrong shapes.
 */
function extractByAuthors(byAuthor: unknown): PreviewAuthor[] {
    if (!byAuthor || typeof byAuthor !== 'object') return [];
    const m = byAuthor as { t?: string; c?: unknown };
    if (m.t !== 'MetaList' || !Array.isArray(m.c)) return [];
    const out: PreviewAuthor[] = [];
    for (const entry of m.c) {
        // Entries are MetaMaps; getMetaPath's first step expects the
        // plain top-level record, so wrap the entry to reuse its
        // MetaMap walking.
        const wrapped = { entry };
        const at = (...path: string[]) =>
            getMetaPath(wrapped, ['entry', ...path]);
        const name = extractMetaString(at('name', 'literal'));
        if (name === undefined || name.length === 0) continue;
        out.push({
            name,
            url: extractMetaString(at('url')),
            email: extractMetaString(at('email')),
            orcid: extractMetaString(at('orcid')),
            degrees: extractMetaStringList(at('degrees')),
            affiliations: extractAffiliations(at('affiliations')),
        });
    }
    return out;
}

/** Extract the denormalized affiliation list of one by-author entry. */
function extractAffiliations(value: unknown): PreviewAffiliation[] {
    if (!value || typeof value !== 'object') return [];
    const m = value as { t?: string; c?: unknown };
    if (m.t !== 'MetaList' || !Array.isArray(m.c)) return [];
    const out: PreviewAffiliation[] = [];
    for (const entry of m.c) {
        const wrapped = { entry };
        const name = extractMetaString(getMetaPath(wrapped, ['entry', 'name']));
        const url = extractMetaString(getMetaPath(wrapped, ['entry', 'url']));
        if (name === undefined && url === undefined) continue;
        out.push({ name, url });
    }
    return out;
}

/**
 * Split a title-block field into display paragraphs: MetaBlocks →
 * one string per Para/Plain block (Q1 parity: the Rust side emits
 * one `<p>` per abstract paragraph); other Meta shapes → a single
 * paragraph via the plain-text coercion. Empty/missing → `[]`.
 */
function extractParagraphs(value: unknown): string[] {
    if (!value || typeof value !== 'object') return [];
    const m = value as { t?: string; c?: unknown };
    if (m.t === 'MetaBlocks' && Array.isArray(m.c)) {
        const out: string[] = [];
        for (const block of m.c as BlockNode[]) {
            const b = block as { t?: string; c?: unknown };
            if ((b.t === 'Para' || b.t === 'Plain') && Array.isArray(b.c)) {
                const text = inlinesToPlainText(b.c as InlineNode[]);
                if (text.length > 0) out.push(text);
            }
        }
        return out;
    }
    const text = extractMetaString(value);
    return text !== undefined && text.length > 0 ? [text] : [];
}
