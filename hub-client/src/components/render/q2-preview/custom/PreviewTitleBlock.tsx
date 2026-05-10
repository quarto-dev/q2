import type { AstProps } from '../../framework';
import { extractMetaString, extractMetaStringList } from '../../framework';

/**
 * Built-in `__title_block__` synthetic-registry entry (Plan 2D Phase 7).
 *
 * Mirrors the Rust HTML template's `<header id="title-block-header">`
 * subtree at `crates/quarto-core/src/template.rs:211-240` byte-for-byte.
 *
 * **Prop shape.** Receives `AstProps` (`{ ast, onNavigateToDocument,
 * setAst }`), the same shape registered under the `Ast` key — NOT the
 * `NodeArgs<…>` shape used by per-tag entries. The title block
 * operates on document-level state (`ast.meta`), not on a node in
 * the AST. `FormatRegistry` at `framework/types.ts` types
 * `__title_block__?: AstComponent`, so a user TSX override that
 * destructures `{ meta }` or `{ node }` by reflex will fail to
 * compile.
 *
 * **Built-in behavior.** Reads `ast.meta` and ignores `setAst` /
 * `onNavigateToDocument`. A user override that wants editable title
 * blocks can call `setAst`; one that wants click-to-navigate on the
 * title can call `onNavigateToDocument`.
 *
 * **Composition.** To wrap the built-in (add a DOI / license /
 * download button without re-implementing the whole `<header>`),
 * import it from `window.__Q2_PREVIEW_RENDERER__.PreviewTitleBlock`
 * (exposed by Phase 7.3.1) and render it alongside your own
 * extensions. The same global exposes `extractMetaString` and the
 * other framework helpers so the override can coerce `ast.meta`
 * values without re-implementing the walks.
 *
 * **Pandoc-falsy semantics.** Missing keys, empty strings, and
 * non-string shapes all suppress the relevant optional element
 * (`subtitle`, `author`, `date`, `abstract`). Matches the Rust
 * template's `$if(x)$` gates, where Pandoc treats `""` as falsy.
 */
export const PreviewTitleBlock = ({ ast }: AstProps) => {
    const meta = ast.meta ?? {};

    // Mirror Rust template.rs:211: $if(title)$ gates the entire
    // <header>. Missing / empty-string / non-string title all
    // suppress the title block.
    const title = extractMetaString(meta.title);
    if (!title) return null;

    const subtitle = extractMetaString(meta.subtitle);

    // Match Rust template.rs:219-225: one <div class="quarto-title-meta-author">,
    // never multiple. For YAML list form (`author: [Alice, Bob]`) Rust
    // stringifies the TemplateValue::List as the empty-string-joined
    // concatenation ("AliceBob") — q2-preview matches by joining
    // with empty string. Locked by §"Out of scope: Multi-author
    // rendering UX" in the plan; when Rust fixes multi-author
    // rendering, both sides flip together.
    const author: string | undefined = (() => {
        const single = extractMetaString(meta.author);
        if (single !== undefined && single.length > 0) return single;
        const list = extractMetaStringList(meta.author);
        if (list.length === 0) return undefined;
        const joined = list.join('');
        return joined.length > 0 ? joined : undefined;
    })();

    const date = extractMetaString(meta.date);
    const abstract = extractMetaString(meta.abstract);

    return (
        <header
            id="title-block-header"
            className="quarto-title-block default"
        >
            <div className="quarto-title">
                {/* TODO(i18n): "Author" / "Published" / "Abstract" are
                    hardcoded literals — flip to extractMetaString(
                    meta.labels?.<key>) when Rust grows a
                    LanguageResolveStage. See plan §"Out of scope: i18n". */}
                <h1 className="title">{title}</h1>
                {subtitle ? <p className="subtitle">{subtitle}</p> : null}
            </div>
            {/*
              Rust quirk replicated: $if(date)$ is INSIDE $if(author)$
              at template.rs:225, so a doc with date but no author
              renders no date. Mirrored to lock Rust parity; flipping
              both is a follow-up plan.
            */}
            {author ? (
                <div className="quarto-title-meta">
                    <div className="quarto-title-meta-author">
                        <div className="quarto-title-meta-heading">
                            Author
                        </div>
                        <div className="quarto-title-meta-contents">
                            {author}
                        </div>
                    </div>
                    {date ? (
                        <div className="quarto-title-meta-date">
                            <div className="quarto-title-meta-heading">
                                Published
                            </div>
                            <div className="quarto-title-meta-contents">
                                {date}
                            </div>
                        </div>
                    ) : null}
                </div>
            ) : null}
            {abstract ? (
                <div className="abstract">
                    <div className="abstract-title">Abstract</div>
                    {abstract}
                </div>
            ) : null}
        </header>
    );
};
