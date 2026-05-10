# Plan 2D — q2-preview body container + title block

**Date:** 2026-05-10
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** M2 polish. Closes the gap between q2-preview's bare-fragment output and the HTML pipeline's `#quarto-content > main.content#quarto-document-content` wrapper plus its `<header id="title-block-header">` document chrome, so theme CSS that targets `.page-layout-article`, `.content`, `body.fullcontent`, and `.quarto-title-block` selectors lands on real elements in the iframe DOM.

## Goal

Add the document-level body container AND the document-title block to q2-preview, mirroring the HTML pipeline's wrapper structure (`crates/quarto-core/src/template.rs:140-256`) and reading title/author/date/subtitle/abstract directly from `ast.meta`. After 2D lands:

- The iframe DOM has `<div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-{layout}"><main class="content" id="quarto-document-content">{title-block?}{blocks}</main></div>` wrapping rendered blocks (or no wrapper when `minimal: true` / `theme: none/pandoc` / `format: revealjs`).
- The iframe `<body>` element carries the user's `body-classes` override or the literal default `fullcontent`, applied imperatively on document mount.
- When `meta.title` is set, a `<header id="title-block-header" class="quarto-title-block default">` is emitted before the body blocks, containing the title (and optional subtitle, author, date, abstract) in the same element/class structure as the Rust HTML template (`template.rs:211-240`).
- Theme CSS rules that target `body.fullcontent .content`, `.page-layout-full`, `.quarto-title-block .title`, `.quarto-title-meta-author`, etc. land on real elements without theme forks.
- Sidebar / navbar / TOC / footer chrome remains deferred (they require pipeline transforms that q2-preview's `Q2_PREVIEW_TRANSFORM_EXCLUDED` currently elides — a separate plan adds them and revisits body-classes computation).
- `<head>` meta tags (`<meta name="author">`, `<meta name="dcterms.date">`, `<meta name="keywords">`, `<meta name="description">`, `<link rel="canonical">`) remain deferred — they affect SEO/print-preview but not visible chrome, and the iframe `<head>` is owned by `q2-preview.html` + `entry.tsx`, not `PreviewDocument`. A follow-up plan can wire them.

## Checklist

### Phase 6 — Body container

(Phase numbering continues from Plan 2C's Phase 4 + Phase 5.)

- [ ] **6.1** Lift `extractMetaString` to a shared util — promote the private helper from `hub-client/src/components/render/ReactAstSlideRenderer.tsx:350` to `hub-client/src/utils/meta.ts` (NEW, ~30 LOC including extension to `extractMetaBool` and `MetaBlocks` walk). Update the one existing caller (`ReactAstSlideRenderer.tsx:220-221`) to import from the new location. **First commit of Phase 6**, per the "enumeration before consumers" rule.
- [ ] **6.2** Extend `q2-preview/PreviewDocument.tsx` with the body-container wrapper (~50 LOC). Read `page-layout`, `body-classes`, `minimal`, `theme`, `format` from `ast.meta` via the new util. Apply `body-classes` to `document.body.className` imperatively in a `useEffect`. Emit the wrapper structure unless `minimal: true` OR `theme === 'none'` OR `theme === 'pandoc'` OR `format === 'revealjs'` (those paths skip the wrapper; the first three match Rust's `is_minimal_html()` check at `template.rs:501`, and revealjs is q2-preview-specific defensive). **Minimal-mode title synthesis**: re-implement the Rust `title-block` transform's minimal-mode branch (`transforms/title_block.rs:42-95`) on the React side — when minimal AND `meta.title` is set AND no level-1 `Header` exists in `ast.blocks`, prepend a synthetic `<h1>{title}</h1>` inside the bare Fragment. Rust's transform is in `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1052`), so without this React-side synthesis, q2-preview's minimal mode silently drops the title. RevealJS is left untouched (q2-debug owns slide chrome).
- [ ] **6.3** Vitest unit tests for `extractMetaString` / `extractMetaBool` against the typed Pandoc Meta variants (`MetaString`, `MetaInlines`, `MetaBlocks`, `MetaBool`, missing key). Per-snapshot tests on `PreviewDocument` for: default (article + fullcontent), `page-layout: full`, `body-classes: custom-cls`, `minimal: true` (no wrapper), `theme: none` (no wrapper), **`minimal: true` + `title` set + no body Header → synthetic `<h1>` prepended**, **`minimal: true` + `title` + body has user-authored level-1 Header → no synthetic `<h1>` (avoid duplicate)**, **`format: revealjs` + `title` → no synthetic `<h1>` (q2-debug owns slide chrome)**.
- [ ] **6.4** Smoke-all q2-preview fixtures — extend `crates/quarto/tests/smoke-all/q2-preview/` (directory exists post-2B) with:
  - `body-container-default.qmd` (single-doc; no `page-layout` set; assert `div#quarto-content.page-layout-article` and `body.fullcontent`).
  - `body-container-full-layout.qmd` (single-doc; `page-layout: full`; assert `div.page-layout-full`).
  - `body-container-override.qmd` (single-doc; `body-classes: custom-cls`; assert `body.custom-cls` AND no `body.fullcontent`).
  - `body-container-minimal.qmd` (single-doc; `minimal: true`; assert no `div#quarto-content` and no `<main>` wrapper).
  - `body-container-minimal-title.qmd` (single-doc; `minimal: true`, `title: "Doc"`, body has only a paragraph and no level-1 header; assert a synthetic `<h1>Doc</h1>` is rendered before the paragraph and locks the React-side replication of `transforms/title_block.rs`'s minimal-mode branch).

  All fixtures use `_quarto.tests.run.requires_js: true` so the Playwright runner picks them up. Pattern matches Plan 2C item 5.2.

### Phase 7 — Title block

Phase 7 sits on top of Phase 6's wrapper: when present, the `<header id="title-block-header">` lives inside `<main class="content">`, before any rendered body blocks. If the wrapper is skipped (`minimal: true`, `theme: none/pandoc`, `format: revealjs`), the title block is also skipped — matches the Rust minimal template (`template.rs:80-111`), which has no title block.

- [ ] **7.1** Extend `hub-client/src/utils/meta.ts` with `extractMetaStringList` (~15 LOC). Reads a `MetaList` of `MetaInlines`/`MetaString` entries and returns `string[]` (empty when missing or wrong shape). Used by the title-block author rendering to support YAML list form (`author: [Alice, Bob]`). Single-author shapes (`MetaString` / `MetaInlines`) continue to use `extractMetaString`. Vitest unit tests cover MetaList → string[], single MetaString → undefined (caller's job to fall back), missing → undefined, wrong shape → undefined.
- [ ] **7.2** New file `hub-client/src/components/render/q2-preview/PreviewTitleBlock.tsx` (~70 LOC). Reads `meta.title` / `meta.subtitle` / `meta.author` / `meta.date` / `meta.abstract` via the `extractMeta*` helpers; emits the `<header id="title-block-header">` structure mirroring `template.rs:211-240` byte-for-byte:
   - `<header id="title-block-header" class="quarto-title-block default">` only when `title` resolves (matches `$if(title)$`).
   - `<div class="quarto-title">` containing `<h1 class="title">{title}</h1>` and optional `<p class="subtitle">{subtitle}</p>`.
   - `<div class="quarto-title-meta">` only when an author resolves (matches `$if(author)$`); inside it, exactly one `<div class="quarto-title-meta-author">` (heading "Author"; multi-author lists get empty-string-concatenated to match Rust's broken-but-consistent behavior — see §"Out of scope: Multi-author rendering UX") and a nested `<div class="quarto-title-meta-date">` (heading "Published") only when `date` is also set.
   - `<div class="abstract">` with `<div class="abstract-title">Abstract</div>` and the abstract text, only when `abstract` resolves.
   - **Mirrors the Rust quirk**: date renders only when at least one author is present (`template.rs:225` puts the date `$if(date)$` block inside the `$if(author)$` block). Document the quirk inline so a future "fix the Rust template" plan can flip both at once.
   - **Does NOT lift Pandoc filter outputs into the title block**: the Rust template inserts `$author$` as a stringified inlines walk, which loses inline emphasis — q2-preview matches by using `inlinesToPlainText` for v1. Block-level abstract rendering and richer inline-markup-preserving title rendering are deferred follow-ups; the v1 fidelity matches what the Rust HTML format currently produces.
- [ ] **7.3** Register `PreviewTitleBlock` in `previewRegistry` (`registry.ts:30-40`) under the synthetic key `'__title_block__'`, sibling of Plan 2C's `__fallback__: Custom.Fallback` line. Resolve it from `PreviewDocument.tsx` via `const { registry } = useContext(RegistryContext); const TitleBlock = registry['__title_block__'] ?? PreviewTitleBlock;` (matches the dispatchers' access pattern at `dispatchers.tsx:39`). Mount `<TitleBlock meta={meta} />` inside `<main class="content">`, before `{children}`. Because the merged `mergedPreviewRegistry = { ...previewRegistry, ...customRegistry }` site in `entry.tsx` already layers user TSX exports over built-ins, no extra wiring is needed for user overrides. Extend `registry.test.ts`'s namespace-disjoint assertion to include `__title_block__` in the "expected synthetic keys" check (mirrors how Plan 2C's `'every expected CustomNode component is exported from ./custom'` test pins `__fallback__`'s presence). When the wrapper is skipped (minimal / theme: none / theme: pandoc / format: revealjs), the title block is also skipped — falling through to the bare Fragment / minimal-mode `<h1>` synthesis.
- [ ] **7.4** Vitest unit tests for `PreviewTitleBlock` (`PreviewTitleBlock.test.tsx`, NEW). Covered cases:
   - No title → renders `null` (no `<header>` element).
   - Title only → `<header>` + `<h1 class="title">`; no `<p class="subtitle">`; no `<div class="quarto-title-meta">`; no `<div class="abstract">`.
   - Title + subtitle → adds `<p class="subtitle">`.
   - Title + author (string) → adds `<div class="quarto-title-meta">` with one `<div class="quarto-title-meta-author">`.
   - Title + author (MetaList of two) → still exactly ONE `<div class="quarto-title-meta-author">`, with `quarto-title-meta-contents` text equal to the empty-string-joined names (matches Rust's broken-but-consistent behavior).
   - Title + author + date → date appears as `<div class="quarto-title-meta-date">` nested inside the author meta wrapper.
   - Title + date but no author → date does NOT render (mirrors the Rust quirk; explicit lock-in test so a future "support date without author" change is a deliberate regression).
   - Title + abstract → adds `<div class="abstract">` with the `<div class="abstract-title">Abstract</div>` heading.
   - Title with inline emphasis (`title: *World*` parses to MetaInlines with Emph) → renders as plain text (matches Rust today; locks the v1 fidelity choice).
   - **User override via registry** — vitest integration test (next to the existing 2C override tests) registers a stub `__title_block__: () => <div data-testid="custom-title">x</div>` in a `__Q2_PREVIEW_RENDERER__` shape, mounts a doc with `title` set, and asserts `data-testid="custom-title"` is present and the built-in `<header id="title-block-header">` is NOT.
- [ ] **7.5** Smoke-all q2-preview fixtures for the title block (single-doc, all under `crates/quarto/tests/smoke-all/q2-preview/`):
  - `title-block-default.qmd` (`title: "Doc"`). Positives: `['header#title-block-header.quarto-title-block', 'h1.title']`. Negatives: `['p.subtitle', 'div.quarto-title-meta', 'div.abstract']`.
  - `title-block-full.qmd` (`title`, `subtitle`, `author`, `date`, `abstract` all set). Positives: `['header#title-block-header', 'h1.title', 'p.subtitle', 'div.quarto-title-meta-author', 'div.quarto-title-meta-date', 'div.abstract', 'div.abstract-title']`. Negatives: none.
  - `title-block-no-title.qmd` (no `title`, with `author: "Jane Doe"` and `date: "2026-05-10"` set, body contains a single `Some content.` paragraph so the `'p'` selector has something to match). Positives: `['p']`. Negatives: `['header#title-block-header', 'div.quarto-title-meta']` (locks "no title → no chrome at all").
  - `title-block-multi-author.qmd` (`author: [Alice, Bob]`, plus `title` and `date`). Positives: `['div.quarto-title-meta-author']`. Negatives: none. The fixture's separately-asserted text content is the empty-string-joined `AliceBob` (locks Rust parity — we deliberately do NOT diverge to one-block-per-name; see §"Out of scope: Multi-author rendering UX").
  - `title-block-date-no-author.qmd` (`title` + `date`, no `author`). Positives: `['header#title-block-header']`. Negatives: `['div.quarto-title-meta', 'div.quarto-title-meta-date']`. Locks the Rust quirk that date alone (without author) is suppressed.

### Phase 8 — Verification

- [ ] **8.1** Run `cargo xtask verify --e2e` before declaring 2D complete (per project CLAUDE.md "End-to-end verification before declaring success"). Default `cargo xtask verify` skips the Playwright runner; without `--e2e` the smoke fixtures landed in 6.4 and 7.5 are not exercised. Also do a manual browser session against a running hub for sanity; record the invocation and an inspected-output snippet in the implementation transcript or this plan's checklist comments. Manual session must include at least one document with title + author + date set, so the title-block render is visually inspected against the Rust HTML output.

## Scope

### In scope

#### `hub-client/src/utils/meta.ts` — shared meta helpers (NEW)

Three helpers — `extractMetaString` is lifted from `ReactAstSlideRenderer.tsx:350` (q2-debug's existing private helper); `extractMetaBool` and `extractMetaStringList` are new in 2D:

```ts
import type { PandocAST } from '../components/render/framework/types';

type Meta = PandocAST['meta'];

/**
 * Extract a string from a Pandoc Meta value. Handles MetaString,
 * MetaInlines, and MetaBlocks. Returns undefined for missing keys,
 * MetaBool, MetaList, or MetaMap (which can't reasonably be coerced
 * to a string).
 *
 * The MetaBlocks branch matches Rust's `config_value_to_template_value`
 * fallthrough to `blocks_to_text` (template.rs:610-614) — needed for
 * `abstract: |` block-scalar YAML, which parses as MetaBlocks.
 *
 * Existing callers: q2-debug slide title/author, q2-preview body
 * container, q2-preview title block (title/subtitle/author/date/abstract).
 */
export function extractMetaString(meta: unknown): string | undefined {
    if (!meta || typeof meta !== 'object') return undefined;
    const m = meta as { t?: string; c?: unknown };
    if (m.t === 'MetaString' && typeof m.c === 'string') return m.c;
    if (m.t === 'MetaInlines' && Array.isArray(m.c)) {
        return inlinesToPlainText(m.c as InlineNode[]);
    }
    if (m.t === 'MetaBlocks' && Array.isArray(m.c)) {
        // Match Rust's blocks_to_text fallthrough — block boundaries
        // collapse to a single space, same fidelity loss as Rust today.
        return blocksToPlainText(m.c as BlockNode[]);
    }
    return undefined;
}

/**
 * Extract a boolean from a Pandoc Meta value. Treats both MetaBool and
 * MetaString("true" | "false") as valid — the YAML parser produces one
 * or the other depending on quoting.
 */
export function extractMetaBool(meta: unknown): boolean | undefined {
    if (!meta || typeof meta !== 'object') return undefined;
    const m = meta as { t?: string; c?: unknown };
    if (m.t === 'MetaBool' && typeof m.c === 'boolean') return m.c;
    if (m.t === 'MetaString' && (m.c === 'true' || m.c === 'false')) return m.c === 'true';
    return undefined;
}
```

The `inlinesToPlainText` re-use means q2-preview's `MetaInlines` → string coercion handles bold / emph / code inline markup in the layout key — same behavior as the Rust template's variable substitution, which sees the rendered Inlines as a string.

**Why shared util, not q2-preview-local**: q2-debug's slide renderer already uses `extractMetaString`. Lifting avoids two copies that drift independently. q2-debug's existing call site is updated to import from the new location in the same commit (one-line diff). Future format-render targets (revealjs slides, future docx, etc.) get the helper for free.

**Phase 7 addition: `extractMetaStringList`** (~15 LOC) — reads a `MetaList` whose entries are each `MetaString` or `MetaInlines`, returning `string[]`. Returns empty array for missing / wrong shape (callers default to single-author via `extractMetaString` first, then fall through). Used by `PreviewTitleBlock` for `author: [Alice, Bob]` YAML form.

```ts
/**
 * Extract a string list from a Pandoc MetaList. Each list entry is
 * coerced via the same MetaString/MetaInlines logic as extractMetaString.
 * Returns an empty array for missing keys, MetaString (use extractMetaString
 * for that single-value shape), or wrong shape.
 */
export function extractMetaStringList(meta: unknown): string[] {
    if (!meta || typeof meta !== 'object') return [];
    const m = meta as { t?: string; c?: unknown };
    if (m.t !== 'MetaList' || !Array.isArray(m.c)) return [];
    const out: string[] = [];
    for (const entry of m.c) {
        const s = extractMetaString(entry);
        if (s !== undefined) out.push(s);
    }
    return out;
}
```

#### `q2-preview/PreviewDocument.tsx` — body container

Replace the current Fragment-only body with the page-layout-aware wrapper. Source structure (annotated against the Rust template):

```tsx
import { useContext, useEffect } from 'react';
import { RegistryContext } from '../framework/RegistryContext';
import { renderChildren } from '../framework';
import type { PandocAST } from '../framework';
import { extractMetaString, extractMetaBool } from '../../../utils/meta';
import { PreviewTitleBlock } from './PreviewTitleBlock';

export const PreviewDocument = ({
    ast,
    onNavigateToDocument,
    setAst,
}: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => {
    const meta = ast.meta ?? {};

    // Mirror Rust template.rs:415-417: page-layout defaults to "article".
    const pageLayout = extractMetaString(meta['page-layout']) ?? 'article';

    // Mirror Rust template.rs:177 + 423-429 precedence:
    //   1. User override (frontmatter `body-classes`).
    //   2. SidebarRenderTransform output — DEFERRED (Plan 2E).
    //   3. Literal "fullcontent" default.
    const bodyClasses = extractMetaString(meta['body-classes']) ?? 'fullcontent';

    // Mirror Rust template.rs:501 is_minimal_html(): minimal: true OR
    // theme: none/pandoc → skip the entire wrapper.
    const minimal =
        extractMetaBool(meta.minimal) === true ||
        extractMetaString(meta.theme) === 'none' ||
        extractMetaString(meta.theme) === 'pandoc';

    // RevealJS divergence: q2-preview doesn't render slides today (q2-debug
    // covers revealjs), but if a doc with format: revealjs ever lands in
    // q2-preview the wrapper would be wrong. Skip the wrapper for safety.
    const isRevealjs = extractMetaString(meta.format) === 'revealjs';

    // Apply body-classes to document.body imperatively. Mirrors the
    // applyTheme pattern at entry.tsx:116-131 for predictable iframe-side
    // DOM management. useEffect (vs module-scope) is fine here because
    // body-classes don't have applyTheme's race condition with the first
    // mount — they can land on the React commit.
    useEffect(() => {
        const previous = document.body.className;
        document.body.className = bodyClasses;
        return () => {
            // Restore on unmount so test re-mounts don't accumulate classes.
            document.body.className = previous;
        };
    }, [bodyClasses]);

    // Resolve the title-block component via the registry so user TSX
    // can override it under the synthetic '__title_block__' key
    // (Phase 7.3). Falls back to the built-in component when the user
    // hasn't registered an override. Pattern matches the dispatchers'
    // `useContext(RegistryContext).registry` access at dispatchers.tsx:39.
    //
    // Hook order: this useContext MUST be called unconditionally before
    // any of the early returns below — React's rules-of-hooks require
    // hooks to fire in the same order on every render. Reading the
    // registry in the minimal/revealjs branches (where TitleBlock is
    // unused) is wasted but harmless — useContext is cheap.
    const { registry } = useContext(RegistryContext);
    const TitleBlock = (registry['__title_block__'] ??
        PreviewTitleBlock) as typeof PreviewTitleBlock;

    // The current PreviewDocument hands the parsed AST + setAst to
    // renderChildren via the framework's "Ast"-shaped detection. Casts
    // mirror the existing 2B/2C version of this file — `renderChildren`
    // is typed for BlockNode | InlineNode but special-cases PandocAST.
    const children = renderChildren({
        node: ast as any,
        setLocalAst: setAst as any,
        onNavigateToDocument,
    });

    if (isRevealjs) {
        // q2-debug owns slide chrome; q2-preview's revealjs branch
        // is purely defensive and does NOT add a synthetic title.
        return <>{children}</>;
    }

    if (minimal) {
        // Re-implement the Rust `title-block` transform's minimal-mode
        // branch on the React side. Rust's transform (excluded from
        // q2-preview's pipeline via Q2_PREVIEW_TRANSFORM_EXCLUDED at
        // pipeline.rs:1052; behavior at transforms/title_block.rs:42-95)
        // prepends a level-1 Header from meta.title to ast.blocks when
        // there's no existing level-1 header, but only in minimal mode.
        // Without this, q2-preview's minimal mode silently drops the
        // title. We synthesize the <h1> here instead of mutating the AST
        // so unwrap-customNodes / setLocalAst paths stay clean.
        const title = extractMetaString(meta.title);
        const hasLevel1Header = (ast.blocks ?? []).some(
            (b) => (b as { t?: string; c?: unknown }).t === 'Header'
                && Array.isArray((b as { c?: unknown[] }).c)
                && ((b as { c: unknown[] }).c[0] === 1),
        );
        return (
            <>
                {title && !hasLevel1Header ? <h1>{title}</h1> : null}
                {children}
            </>
        );
    }

    return (
        <div
            id="quarto-content"
            className={`quarto-container page-columns page-rows-contents page-layout-${pageLayout}`}
        >
            <main className="content" id="quarto-document-content">
                <TitleBlock meta={meta} />
                {children}
            </main>
        </div>
    );
};
```

**Key invariants — body container** (matching `template.rs:185-209`):

- The `<div id="quarto-content">` carries exactly four classes in order: `quarto-container page-columns page-rows-contents page-layout-{layout}`. Theme CSS treats this list as load-bearing.
- The inner `<main>` carries `class="content" id="quarto-document-content"`. Both the class and the id are CSS-targeted by the bundled theme.
- The body-class precedence is user override → literal default; the SidebarRenderTransform-computed body-classes (`nav-sidebar floating` / `nav-sidebar docked`) are **explicitly out of scope** because `SidebarRenderTransform` isn't in q2-preview's pipeline (`Q2_PREVIEW_TRANSFORM_EXCLUDED`). When sidebar lands in a follow-up plan, that plan re-derives body-classes via the Option A pattern (typed field on `RenderResponse`) and wires q2-preview's resolver to be `bodyClassesOverride ?? extractMetaString(meta['body-classes']) ?? 'fullcontent'` — same precedence as `template.rs:419-429`.

**Phase 7 wiring**: `<PreviewTitleBlock meta={meta} />` is mounted as the FIRST child of `<main class="content">`. It returns `null` when `meta.title` doesn't resolve, so the no-title path produces a clean `<main>` containing only `{children}` (matches Rust template's `$if(title)$ … $endif$` gate at `template.rs:211/240`). Phase 6's "skip the wrapper" branches (minimal / theme / revealjs) also skip the title block — falling through to the bare Fragment makes that automatic.

#### `q2-preview/PreviewTitleBlock.tsx` — title block (NEW, Phase 7)

Mirrors `template.rs:211-240` byte-for-byte. Reads from `ast.meta` only — no Rust-side typed fields, consistent with Phase 6's Option B. Source (annotated against the Rust template):

```tsx
import type { PandocAST } from '../framework/types';
import {
    extractMetaString,
    extractMetaStringList,
} from '../../../utils/meta';

interface PreviewTitleBlockArgs {
    meta: PandocAST['meta'];
}

export const PreviewTitleBlock = ({ meta }: PreviewTitleBlockArgs) => {
    // Mirror Pandoc's `$if(title)$` falsy semantics: missing key,
    // explicit empty string, and other non-string shapes all suppress
    // the title block. `if (!title)` covers undefined and "" together.
    // Matches Rust template.rs:211; explicit empty-string lock is in
    // PreviewTitleBlock.test.tsx.
    const title = extractMetaString(meta.title);
    if (!title) return null;

    const subtitle = extractMetaString(meta.subtitle);

    // Match Rust template.rs:219-225: one `<div class="quarto-title-meta-author">`,
    // never multiple. For YAML list form (`author: [Alice, Bob]`) Rust
    // stringifies the TemplateValue::List as the empty-string-joined
    // concatenation ("AliceBob") — q2-preview matches by joining with
    // empty string. When Rust fixes multi-author rendering (proper
    // structured authors / separator policy / "Authors" plural heading),
    // q2-preview mirrors in the same plan; until then both sides
    // share the broken-but-consistent behavior so theme CSS doesn't
    // need to special-case q2-preview.
    const author: string | undefined = (() => {
        const single = extractMetaString(meta.author);
        if (single !== undefined) return single;
        const list = extractMetaStringList(meta.author);
        return list.length > 0 ? list.join('') : undefined;
    })();

    const date = extractMetaString(meta.date);

    // Rust today stringifies abstract via inlines_to_text/blocks_to_text
    // (template.rs:582-668). q2-preview matches with extractMetaString;
    // block-shape abstracts (paragraph + paragraph) collapse to plain
    // text the same way Rust does. Block-rendering fidelity is a
    // deliberate follow-up — see §"Out of scope".
    const abstract = extractMetaString(meta.abstract);

    return (
        <header
            id="title-block-header"
            className="quarto-title-block default"
        >
            <div className="quarto-title">
                <h1 className="title">{title}</h1>
                {subtitle ? <p className="subtitle">{subtitle}</p> : null}
            </div>
            {/*
              Rust quirk replicated: $if(date)$ is INSIDE $if(author)$
              at template.rs:225, so a doc with date but no author
              renders no date. Mirrored here to lock parity; flipping
              both is a follow-up plan.
            */}
            {author !== undefined ? (
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
```

**Key invariants** (matching `template.rs:211-240`):

- The wrapper `<header>` has exactly two classes: `quarto-title-block default`, plus `id="title-block-header"`.
- `<h1>` carries class `title` (singular, matches Rust); subtitle is `<p class="subtitle">`.
- The author wrapper `<div class="quarto-title-meta">` is only emitted when an author resolves; it contains exactly one `<div class="quarto-title-meta-author">` (matches Rust — multi-author lists are concatenated into a single block, see §"Out of scope: Multi-author rendering UX"). That block carries `<div class="quarto-title-meta-heading">Author</div>` + `<div class="quarto-title-meta-contents">{author}</div>` in that order.
- The date sub-block `<div class="quarto-title-meta-date">` uses heading "Published" (not "Date" — matches Rust line 227).
- The abstract block uses `<div class="abstract">` with an inner `<div class="abstract-title">Abstract</div>` heading.
- All English labels ("Author", "Published", "Abstract") are hardcoded literals — same as Rust. i18n is out of scope for v1; when Rust grows it (probably via `language-` keys in metadata), q2-preview mirrors. **Mark a TODO at the heading literals** so a grep finds them when the Rust-side i18n change lands.

### Out of scope

- **Sidebar / TOC / navbar / page-footer chrome rendering**. Each of these requires running a Rust pipeline transform that q2-preview currently excludes (sidebar-resolve, etc.). Each is a follow-up plan.
- **SidebarRenderTransform-computed body-classes** (`nav-sidebar floating` / `nav-sidebar docked`). Tied to the sidebar plan above. The body-classes precedence in 2D is user-override-or-literal-default; the typed-field-on-`RenderResponse` plumbing for transform-computed classes lands when sidebar lands.
- **Quarto Bootstrap grid layout details** (`page-columns`'s actual CSS-grid definitions, margin-sidebar tracks, etc.). The classes are emitted; theme CSS owns the visual interpretation. q2-preview ships no per-format CSS for these.
- **Custom `page-layout` values defined by user themes**. The wrapper passes the value verbatim into the class name — same as the Rust template. Theme CSS owns interpretation.
- **`format: revealjs` rendering**. q2-preview detects and skips the wrapper, but does not render slides. q2-debug owns revealjs.
- **Slide-deck-style chrome** (slide indicator, slide nav). RevealJS-only; not in scope here.
- **`<head>` meta tags driven by metadata** (`<meta name="author">`, `<meta name="dcterms.date">`, `<meta name="keywords">`, `<meta name="description">`, `<link rel="canonical">`, generator meta). The iframe `<head>` is owned by `hub-client/public/q2-preview.html` + `entry.tsx`'s theme/title injection. Adding metadata-driven `<head>` tags is its own wiring (a postMessage payload extension, or a static read at boot) — defer to a follow-up. Visible chrome and theme CSS targeting in `<body>` are unaffected.
- **`<title>` (browser tab title) sourced from `pagetitle`/`title`**. `entry.tsx` already sets the iframe's `<title>` from the AST today — so the tab title for q2-preview is correct as long as upstream meta-merge populates `pagetitle`. If a regression surfaces (e.g. tab title reads "Untitled" while title block reads correctly), it's a wiring issue in `entry.tsx`, not a `PreviewDocument`/`PreviewTitleBlock` concern. Verify during Phase 8.1's manual session and flag separately if broken.
- **Block-rendered (not stringified) abstracts**. v1 stringifies `MetaBlocks` abstracts via `blocksToPlainText` to match Rust's `blocks_to_text` (template.rs:610-614). Real block rendering (paragraphs, lists, etc.) inside `<div class="abstract">` is deferred — when Rust upgrades, q2-preview's component switches to `<Node>` walks over `meta.abstract`'s blocks. Class taxonomy stays the same.
- **Inline-markup-preserving title rendering**. Rust strips emphasis from titles today (`config_value_to_template_value` → `inlines_to_text`); q2-preview matches. When the Rust template grows an HTML-emit path for title inlines, q2-preview switches `<h1 class="title">` to render via `<InlineNode>` walks. Until then, italic/bold/code in titles are stripped on both sides.
- **i18n of "Author" / "Published" / "Abstract" labels**. Hardcoded literals match Rust; both flip together when the Rust template grows i18n.
- **Multi-author rendering UX**. Rust today emits exactly one `<div class="quarto-title-meta-author">` with all names concatenated as one string (`AliceBob`, no separator) — the doctemplate engine stringifies `TemplateValue::List` as the empty-join. q2-preview matches verbatim (single block, empty-string-joined names). When Rust grows proper multi-author support (structured author objects, separator policy, possibly an "Authors" plural heading), q2-preview mirrors in the same plan — no per-format CSS fork needed in the meantime.

### Defensive variants

- **Missing `ast.meta`**: `ast.meta ?? {}` defaults to empty object; all `extractMeta*` helpers return undefined; defaults kick in for every key. The wrapper still renders with `page-layout-article` and `body.fullcontent`, and `PreviewTitleBlock` returns `null` (no title block).
- **Non-string `page-layout` value** (e.g. user wrote `page-layout: 42` accidentally): `extractMetaString` returns undefined for `MetaBool` / `MetaList` / `MetaMap`; fallback to `article`. No type coercion.
- **Whitespace-only `body-classes`** (e.g. `body-classes: " "`): the helper returns the whitespace string verbatim; `document.body.className = " "` produces a body with no classes (browser ignores whitespace-only class). Matches Rust template behavior.

## Design decisions

- **Option B (read `ast.meta` in JS), not Option A (Rust-side typed field).** The HTML pipeline reads `page-layout` directly from `ast.meta` at template-render time (`template.rs:415`); q2-preview matches that pattern. Existing precedent: `ReactRenderer.tsx:211` (format detection) and `ReactAstSlideRenderer.tsx:220-221` (slide title/author) already read `ast.meta` directly. Adds zero Rust / WASM-bridge plumbing for v1. **The Option A pattern is reserved for transform-computed values** (sidebar body-classes, future stage outputs) — when those land, `RenderResponse` grows a typed field and q2-preview's resolver layers it before the `ast.meta` read.
- **Body-class application via `useEffect`, not module-scope imperative.** `applyTheme` at `entry.tsx:116-131` lives at module scope because it has to fire before any React mount (the parent posts theme + AST from sibling `useEffect`s on the same `iframeReady` transition; if theme posts first the message would be dropped). Body-classes have no such race — they read from the AST that's already mounted, so the React commit fires before any user can observe a class mismatch. Keeps document-level concerns inside `PreviewDocument` rather than spreading across `entry.tsx` + the wrapper component.
- **Wrapper structure mirrors `template.rs` byte-for-byte.** Same element + class list + nesting. The justification is the same as Plan 2C's "Visual + structural parity target": Bootstrap-flavored theme CSS targets the wrapper's selectors, and any divergence forces a per-format CSS fork. The cost of strict mirroring is low — three classes + an id + an inner `<main>` — and the payoff is "load Quarto's compiled theme CSS and it just works".
- **`minimal` / `theme: none` / `theme: pandoc` skip the wrapper.** Mirrors `is_minimal_html()` at `template.rs:501`. The minimal HTML template at `template.rs:80-111` omits `#quarto-content` and `<main>` entirely; q2-preview's PreviewDocument matches by returning the bare Fragment. If a user opts into `minimal: true`, they expect no chrome — q2-preview shouldn't add any.
- **`format: revealjs` also skips the wrapper.** q2-preview is not the slide renderer (q2-debug owns slides), but a doc with `format: revealjs` could end up here if the user toggles formats. Skipping the wrapper keeps the layout from interfering with whatever the eventual slide renderer expects.
- **`extractMetaString` lifted to `utils/meta.ts`, not q2-preview-local.** q2-debug's slide renderer already uses it; lifting avoids parallel copies. The new file is `hub-client/src/utils/meta.ts` (sibling of `customRegistry.ts`, `iframeLinkHandlers.ts`, `atomicCustomNodes.ts`). One-line update to the existing q2-debug import.
- **Cleanup on `useEffect` unmount.** Test re-mounts (vitest, Playwright) need a clean body class slate; restoring the previous `document.body.className` on unmount keeps the suite hermetic. Production iframe unmounts only on document switch — same restore is benign there.

## Multi-plan contracts

### Consumed: Plan 2B / 2C (PreviewDocument + registry both shipped)

`PreviewDocument.tsx` ships in Plan 2B as the registry's `Ast` entry; Plan 2C extends `previewRegistry` with the unified Pandoc-tag / CustomNode dispatcher pattern, the `__fallback__` synthetic key, and the namespace-disjoint policy locked at `registry.test.ts`. Both have landed on `feature/q2-preview` (the q2-preview-work branch is current as of 2026-05-10).

Plan 2D extends `PreviewDocument.tsx` with the wrapper + minimal-mode title synthesis (Phase 6) and adds `PreviewTitleBlock` registered under `'__title_block__'` (Phase 7). The synthetic-key precedent is already established by 2C's `__fallback__`. The PreviewDocument prop shape stays at `{ ast, setAst, onNavigateToDocument }` (matching the framework `AstProps` at `framework/types.ts:155-159`); 2D does NOT migrate to `NodeArgs<PandocAST>`.

### Provided: foundation for sidebar / navbar / footer plans

When Plan 2E (sidebar) lands, the `<div id="quarto-content">` wrapper is the slot the sidebar `<nav>` and `<div id="quarto-margin-sidebar">` siblings live alongside `<main class="content">`. The wrapper structure assumed by 2D is exactly the structure those follow-up plans will extend, so 2E adds elements without restructuring the wrapper.

When the typed-field-on-`RenderResponse` pattern is used for transform-computed body-classes (Plan 2E or later), q2-preview's resolver layers the typed field above the `ast.meta` read:

```ts
// Future shape (Plan 2E):
const bodyClasses =
    props.bodyClassesOverride ??                            // Plan 2E: typed field on RenderResponse
    extractMetaString(meta['body-classes']) ??              // Plan 2D: user override
    'fullcontent';                                          // Plan 2D: literal default
```

Mirrors `template.rs:419-429` precedence.

### Soft activation dependencies

None for 2D. The wrapper's structure is fully determined by `ast.meta` reads, and `ast.meta` is already plumbed end-to-end (Plan 2A's WASM bridge, Plan 2B's iframe entry, no changes needed).

## Test plan

### Test-tier conventions

Same tiers as Plans 2B / 2C: vitest unit / vitest integration / smoke-all WASM / Playwright e2e. The project-context coverage rule does **not** apply to 2D — the body container is computed entirely from `ast.meta`, which is identical in single-doc and project mode (the orchestrator merges the same metadata in both paths). Single-doc fixtures are sufficient.

### Vitest unit tests (`utils/meta.test.ts`)

NEW file at `hub-client/src/utils/meta.test.ts`:

- `extractMetaString` returns string for `MetaString`, walks `MetaInlines` via `inlinesToPlainText`, walks `MetaBlocks` via `blocksToPlainText` (covers `abstract: |` block-scalar shape), returns undefined for `MetaBool` / `MetaList` / `MetaMap` / null / undefined / wrong-shape.
- `extractMetaBool` returns boolean for `MetaBool`, parses `MetaString("true" | "false")`, returns undefined for other types.
- `extractMetaStringList` (Phase 7) returns `string[]` for `MetaList` of `MetaString`/`MetaInlines`, empty array for missing, wrong shape, or single-value forms (`MetaString` directly).

### Vitest snapshot tests (`q2-preview/PreviewDocument.test.tsx`)

NEW file (or extend if already present post-2B/2C):

- Default render (no metadata): assert wrapper with `page-layout-article` and `<main class="content" id="quarto-document-content">`. Body class is `fullcontent`.
- `page-layout: full`: assert wrapper carries `page-layout-full`.
- `page-layout: custom`: assert wrapper carries `page-layout-custom` (verbatim — the wrapper doesn't validate against an enum).
- `body-classes: my-class`: assert `document.body.className === 'my-class'` AND that `fullcontent` is NOT applied.
- `minimal: true`: assert NO wrapper element rendered (just children inside a Fragment).
- `theme: none`: assert NO wrapper element rendered.
- `theme: pandoc`: assert NO wrapper element rendered.
- `format: revealjs`: assert NO wrapper element rendered.
- Cleanup: mount → unmount → assert `document.body.className` is restored to its pre-mount value.

### Vitest snapshot tests (`q2-preview/PreviewTitleBlock.test.tsx`) — Phase 7

NEW file at `hub-client/src/components/render/q2-preview/PreviewTitleBlock.test.tsx`:

- No title (meta empty) → `render` returns `null`. No `<header>` in the DOM.
- Title only → `<header id="title-block-header" class="quarto-title-block default">` with `<h1 class="title">` inside `<div class="quarto-title">`. Asserts no `<p class="subtitle">`, no `<div class="quarto-title-meta">`, no `<div class="abstract">`.
- Title + subtitle → adds `<p class="subtitle">`.
- Title + author (single MetaString) → exactly one `<div class="quarto-title-meta-author">` inside `<div class="quarto-title-meta">`.
- Title + author (MetaList of two) → exactly one `<div class="quarto-title-meta-author">` whose `.quarto-title-meta-contents` text is the empty-string-joined names (matches Rust).
- Title + author + date → `<div class="quarto-title-meta-date">` rendered inside the meta wrapper, with heading "Published".
- Title + date but NO author → date is NOT rendered (locks the Rust quirk; explicit regression test for the deliberate divergence).
- Title + abstract → `<div class="abstract">` with `<div class="abstract-title">Abstract</div>` and the abstract text.
- Title with inline emphasis (MetaInlines `[Str("Hello"), Space, Emph([Str("World")])]`) → `<h1 class="title">Hello World</h1>` (emphasis stripped, matches Rust).
- User override via `__title_block__` registry key → stub component is mounted in place of `PreviewTitleBlock`, and the built-in `<header id="title-block-header">` is NOT in the DOM.
- Empty-string title (`title: ""` parses as `MetaString("")`) → renders `null`, no `<header>` element. Locks Pandoc's `$if(title)$` falsy semantics for the empty-string case (treated identically to missing-key).

### Smoke-all q2-preview fixtures

Pattern matches Plan 2C item 5.2 — `_quarto.tests.run.requires_js: true` + `ensureHtmlElements` assertions:

The `ensureHtmlElements` schema (verified at `hub-client/e2e/helpers/smokeAllDiscovery.ts:124-129` and `smokeAllAssertions.ts:122-138`) is a two-element YAML array: first inner array is selectors that must match (`toBeAttached`), second is selectors that must NOT match (`toHaveCount(0)`). Negative selectors are first-class in the harness; 2D uses both halves:

```yaml
_quarto:
  tests:
    run:
      requires_js: true
    q2-preview:
      ensureHtmlElements:
        - ['div#quarto-content.page-layout-article', 'main.content#quarto-document-content', 'body.fullcontent']
        - []  # no negatives for the default case
```

Per-fixture assertions:

- **`q2-preview/body-container-default.qmd`**: no `page-layout` / `body-classes` / `minimal`. Positives: `['div#quarto-content.page-layout-article', 'main.content#quarto-document-content', 'body.fullcontent']`. Negatives: none.
- **`q2-preview/body-container-full-layout.qmd`**: `page-layout: full`. Positives: `['div.page-layout-full', 'main.content']`. Negatives: `['div.page-layout-article']` (defends against the default leaking through).
- **`q2-preview/body-container-override.qmd`**: `body-classes: custom-cls`. Positives: `['body.custom-cls']`. Negatives: `['body.fullcontent']` (locks the override-replaces-default precedence).
- **`q2-preview/body-container-minimal.qmd`**: `minimal: true`, no `title`. Positives: `['p']` (some content rendered). Negatives: `['div#quarto-content', 'main.content', 'h1']` (locks the wrapper-skip behavior AND that no synthetic h1 is added when there's no title).
- **`q2-preview/body-container-minimal-title.qmd`**: `minimal: true`, `title: "Doc"`, body is a single paragraph (no level-1 header authored). Positives: `['h1', 'p']`. Negatives: `['div#quarto-content', 'main.content', 'header#title-block-header']`. Locks the React-side title synthesis: chrome wrapper is skipped, `<header id="title-block-header">` is skipped, but the title still appears as a body `<h1>` — matching Rust's minimal-mode `title-block` transform output.

#### Phase 7 — title-block fixtures

- **`q2-preview/title-block-default.qmd`**: `title: "Doc"` only. Positives: `['header#title-block-header.quarto-title-block', 'h1.title']`. Negatives: `['p.subtitle', 'div.quarto-title-meta', 'div.abstract']`.
- **`q2-preview/title-block-full.qmd`**: `title`, `subtitle`, `author: "Jane Doe"`, `date: "2026-05-10"`, `abstract: "Hello."` all set. Positives: `['header#title-block-header', 'h1.title', 'p.subtitle', 'div.quarto-title-meta-author', 'div.quarto-title-meta-date', 'div.abstract', 'div.abstract-title']`. Negatives: none.
- **`q2-preview/title-block-no-title.qmd`**: no `title`, but `author: "Jane Doe"` and `date: "2026-05-10"` set. Positives: `['p']` (body content rendered). Negatives: `['header#title-block-header', 'div.quarto-title-meta']` (locks "no title → no chrome at all").
- **`q2-preview/title-block-multi-author.qmd`**: `author: [Alice, Bob]`, `title: "Doc"`, `date: "2026-05-10"`. Positives: `['div.quarto-title-meta-author']` (exactly one match expected). Negatives: none. The fixture also asserts `.quarto-title-meta-contents` text equals `AliceBob` (empty-string-joined, matches Rust's `TemplateValue::List` stringification — see §"Out of scope: Multi-author rendering UX"). Locks Rust parity, not divergence.
- **`q2-preview/title-block-date-no-author.qmd`**: `title: "Doc"`, `date: "2026-05-10"`, no `author`. Positives: `['header#title-block-header']`. Negatives: `['div.quarto-title-meta', 'div.quarto-title-meta-date']` (locks the Rust quirk that date alone is suppressed).

### Visual sanity check (manual)

During Phase 8.1's manual browser session:

- Open a multi-element fixture (the one Plan 2C ships at `q2-preview/multi-element-doc.qmd`) in q2-preview through a running hub.
- Confirm in DevTools that the body-container wrapper is in place, classes correct.
- Reload with theme CSS applied; confirm the document looks like the HTML pipeline's output (theme CSS targeting `body.fullcontent .content` should land).
- Toggle `page-layout: full` in frontmatter, save, confirm the iframe re-renders with `page-layout-full`.

Record the inspected output snippet in the implementation transcript.

## Risk areas

- **Theme CSS expectation drift**. Theme CSS is bundled by the Rust pipeline and shipped through `theme_fingerprint` / `applyTheme`. If the bundled theme CSS uses a selector q2-preview doesn't emit (e.g. `body.fullcontent .content > article`), 2D's wrapper won't satisfy it. Mitigation: the smoke fixtures assert the wrapper structure post-render; the manual visual check at 8.1 catches the rest. If a divergence is found, the fix is to extend the wrapper, not the theme CSS.
- **Wrapper drift between Rust template and PreviewDocument**. The classes / element / nesting pattern is replicated in two places (template.rs literal HTML, PreviewDocument.tsx JSX). If template.rs changes its wrapper (e.g. adds `quarto-document-grid` to the outer container's class list), q2-preview won't pick it up automatically. Mitigation: doc-comment the PreviewDocument wrapper with an explicit "MIRRORS template.rs:185-209 — keep in sync" pin and a line ref. Same drift-detection caveat as Plan 2C's `quartoClasses.ts`.
- **`document.body.className` collision with iframe-host CSS — verified safe**. The iframe host (`hub-client/public/q2-preview.html:28`) declares `<body>` with no class attribute. The host CSS at lines 11-14 only sets `margin: 0; padding: 0` on the bare `body` selector, no class dependency. `useEffect`'s `document.body.className = bodyClasses` is a clean overwrite — no host class to preserve, no iframe-host CSS rule that depends on a class being present. The cleanup-on-unmount restores the previous value (which is the empty string on first mount); subsequent re-mounts restore the prior `bodyClasses` cleanly.
- **Body-class restore on unmount races with re-mount**. If the iframe unmounts and immediately re-mounts (e.g. doc switch), the cleanup-then-reapply sequence may briefly flash `fullcontent` between the old doc's class and the new one. Visually invisible at React's commit cadence (microsecond), but worth noting if a future bug points at brief unstyled flashes. Mitigation: if it bites, switch to module-scope imperative management like `applyTheme`.
- **Non-string `format` field**. `format` in `ast.meta` is the merged-format ID; should always be `MetaString`. If a future pipeline change makes it `MetaInlines`, the revealjs-skip branch still works (extractMetaString handles both), but worth a regression test if format-detection is ever centralized.
- **Minimal-mode section structure divergence (known, minor)**. In Rust minimal mode the synthetic title Header is added BEFORE the section-structure transform runs, so it ends up wrapped in a `<section level1>` along with following content. In q2-preview the title-block transform is excluded entirely; the React-side synthesis emits `<h1>{title}</h1>` AFTER section-structure has run, so the `<h1>` is a sibling of (not nested inside) the body's section wrappers. Visual difference is normally invisible — minimal-mode CSS doesn't depend on the section nesting — but worth flagging if a regression points at minimal-mode title positioning. Mitigation: the smoke fixture asserts the `<h1>` is present without pinning section nesting.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `hub-client/src/utils/meta.ts` (NEW — `extractMetaString` + `extractMetaBool` + `extractMetaStringList`) | ~50 |
| `q2-preview/PreviewDocument.tsx` extension (wrapper + useEffect + title-block mount) | ~32 |
| `q2-preview/PreviewTitleBlock.tsx` (NEW — title block) | ~70 |
| q2-debug import update (`ReactAstSlideRenderer.tsx:350` lifted) | ~3 |
| `utils/meta.test.ts` (NEW) — unit tests for all three helpers | ~70 |
| `q2-preview/PreviewDocument.test.tsx` extension — wrapper snapshot tests | ~80 |
| `q2-preview/PreviewTitleBlock.test.tsx` (NEW) — title-block snapshot tests | ~90 |
| Smoke-all q2-preview fixtures (5 body-container + 5 title-block, all single-doc) | ~100 |
| **Total** | **~495** |

Still comfortable for a focused session. About double the original 2D scope; the title-block work is small but multiplied by the number of conditional branches. **Phase 7 depends on Phase 6**: `<PreviewTitleBlock>` mounts inside Phase 6's `<main class="content">`, and the "skip the wrapper" logic (minimal / theme: none / revealjs) is what makes the title block also disappear in those modes. Phase 6 must land first.

**Sub-ordering**:
1. `utils/meta.ts` + `meta.test.ts` (with all three helpers including `extractMetaStringList`) land first — q2-debug import update bundled in the same commit.
2. Phase 6: `PreviewDocument.tsx` body-wrapper + tests + body-container smoke fixtures.
3. Phase 7: `PreviewTitleBlock.tsx` + tests + title-block smoke fixtures + the one-line `PreviewDocument.tsx` mount edit.
4. Phase 8: `cargo xtask verify --e2e` + manual browser session covering both wrapper and title block.

## Dependencies

### Hard dependencies

- **Plan 2B** ✅ — ships `PreviewDocument.tsx` as the registry's `Ast` entry and `inlinesToPlainText` / `blocksToPlainText` (used by `extractMetaString`). 2D extends `PreviewDocument.tsx`.
- **Plan 2C** ✅ (landed 2026-05-10) — ships `previewRegistry` with the merged Pandoc-tag / CustomNode key namespace, the `__fallback__` synthetic-key precedent, and the namespace-disjoint test in `registry.test.ts`. 2D's `'__title_block__'` synthetic key follows that precedent.
- **Plan 2A** ✅ — q2-preview surface scaffolding, `theme_fingerprint` plumbing (the iframe already receives theme CSS that targets the wrapper).
- **Plan 1** ✅ — pipeline + format detection. `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1049-1072`) defines what runs in q2-preview's render path; 2D inherits, and Phase 6.2 specifically re-implements the excluded `title-block` transform's minimal-mode branch on the React side.

### Soft / activation dependencies

None. The wrapper structure is fully determined by `ast.meta` reads available today.

### Blocks

Nothing structurally. The future sidebar plan (Plan 2E or similar) sits on top of 2D's wrapper — 2E adds `<nav>` and `<div id="quarto-margin-sidebar">` siblings alongside `<main class="content">`, so the wrapper structure 2D ships is exactly the structure 2E extends. 2D blocks 2E temporally.

## Related beads issues

None tracked specifically for this work. The `body-classes` and "Layout / chrome components" deferrals in Plans 2A and 2C are notes-only, not beads issues.

## Notes

- This plan is the smallest possible step toward HTML-pipeline visual parity for q2-preview. Sidebar / TOC / navbar / page-footer each warrant their own plans (each requires unblocking a Rust pipeline transform on the q2-preview side, which is its own design surface). 2D ships the foundation those sit on.
- `extractMetaString` already exists in q2-debug's slide renderer; the lift is opportunistic — once it's at `utils/meta.ts`, future format renderers (e.g. a docx preview, a PDF preview hypothetically) get the helper for free without re-importing from q2-debug.
- `extractMetaBool` is new. Could be deferred until a real consumer needs it (today only `minimal` reads it, and a `=== 'true'`-style string check would do). Including it here because (a) the `minimal: true` YAML form parses to MetaBool, not MetaString, so the type-correct version is needed; (b) the additional 6 LOC + 4 test cases is cheap, and bundling avoids a separate "add extractMetaBool" commit later.

## References

### Rust side (read during implementation; not modified by 2D)

- `crates/quarto-core/src/template.rs:140-256` — `FULL_HTML_TEMPLATE` (the wrapper structure 2D mirrors).
- `crates/quarto-core/src/template.rs:177` — `<body class="$if(body-classes)$$body-classes$$else$fullcontent$endif$">` (body-class precedence reference).
- `crates/quarto-core/src/template.rs:185-209` — `<div id="quarto-content">` wrapper definition (element + classes).
- `crates/quarto-core/src/template.rs:211-240` — title block (`<header id="title-block-header">` + title / subtitle / quarto-title-meta / abstract). Phase 7 mirrors byte-for-byte.
- `crates/quarto-core/src/template.rs:415-417` — `page-layout` template-variable injection with default.
- `crates/quarto-core/src/template.rs:419-429` — body-classes computation precedence (user override → `rendered.navigation.body-classes` → literal default).
- `crates/quarto-core/src/template.rs:501` — `is_minimal_html()` check.
- `crates/quarto-core/src/template.rs:80-111` — `MINIMAL_HTML_TEMPLATE` (the no-wrapper / no-title-block variant 2D matches when minimal is set).
- `crates/quarto-core/src/template.rs:582-668` — `config_value_to_template_value` + `inlines_to_text` + `blocks_to_text` (the plain-text stringify path the title block consumes today; locks the v1 fidelity choice).
- `crates/quarto-core/src/transforms/sidebar_render.rs:88-97` — SidebarRenderTransform body-classes computation (out of scope for 2D; documented for the future sidebar plan).
- `crates/quarto-core/src/transforms/title_block.rs:42-95` — `TitleBlockTransform`. In full HTML mode it short-circuits (template emits the chrome). In minimal mode it prepends a synthetic level-1 Header from `meta.title` to `ast.blocks`. The transform is in `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1052`) — q2-preview re-implements its minimal-mode branch in `PreviewDocument.tsx` (Phase 6.2).
- `crates/quarto-core/src/pipeline.rs:1049-1072` — `Q2_PREVIEW_TRANSFORM_EXCLUDED` deny-list. Names `"title-block"` (and the chrome transforms) here.

### hub-client side (modified by 2D)

- `hub-client/src/utils/meta.ts` (NEW) — `extractMetaString`, `extractMetaBool`, `extractMetaStringList`.
- `hub-client/src/utils/meta.test.ts` (NEW) — unit tests for all three helpers.
- `hub-client/src/components/render/q2-preview/PreviewDocument.tsx` — extend with body-container wrapper + useEffect; mount `<PreviewTitleBlock>` inside `<main>`.
- `hub-client/src/components/render/q2-preview/PreviewDocument.test.tsx` (extend if exists, NEW otherwise) — wrapper snapshot tests.
- `hub-client/src/components/render/q2-preview/PreviewTitleBlock.tsx` (NEW) — title-block component (Phase 7).
- `hub-client/src/components/render/q2-preview/PreviewTitleBlock.test.tsx` (NEW) — title-block snapshot tests (Phase 7).
- `hub-client/src/components/render/ReactAstSlideRenderer.tsx:350` — remove the local `extractMetaString` definition; update the import at `:220-221` to use `utils/meta.ts`.
- `crates/quarto/tests/smoke-all/q2-preview/body-container-{default,full-layout,override,minimal,minimal-title}.qmd` (NEW) — body-container smoke fixtures (Phase 6).
- `crates/quarto/tests/smoke-all/q2-preview/title-block-{default,full,no-title,multi-author,date-no-author}.qmd` (NEW) — title-block smoke fixtures (Phase 7).
- `hub-client/src/components/render/q2-preview/registry.ts` — add `__title_block__: PreviewTitleBlock` next to `__fallback__: Custom.Fallback` (line 34).
- `hub-client/src/components/render/q2-preview/registry.test.ts` — extend the synthetic-key presence assertion to include `__title_block__`.

### hub-client side (read-only references during implementation)

- `hub-client/public/q2-preview.html:28` — iframe `<body>` declaration (verified to carry no class — `useEffect` overwrite is safe).
- `hub-client/e2e/helpers/smokeAllDiscovery.ts:124-129` — `parseTwoArraySpec` defines the `ensureHtmlElements` YAML schema (two arrays: positives + negatives).
- `hub-client/e2e/helpers/smokeAllAssertions.ts:122-138` — `ensureHtmlElements` runner; positives use `toBeAttached`, negatives use `toHaveCount(0)`.
- `hub-client/src/components/render/framework/types.ts:155-167` — `AstProps` (the prop shape `PreviewDocument` receives) and `FormatRegistry` (Record<string, …> with required `Ast`, `Block`, `Inline` keys; arbitrary string keys allowed for synthetic entries like `__fallback__`, `__title_block__`).
- `hub-client/src/components/render/framework/RegistryContext.tsx:19-22` — registry context shape and access pattern.
- `hub-client/src/components/render/q2-preview/dispatchers.tsx:39` / `:78-82` — `useContext(RegistryContext).registry` lookup and `__fallback__` precedent for synthetic-key resolution.
- `hub-client/src/components/render/q2-preview/registry.ts:30-40` — current `previewRegistry` shape; 2D adds the `__title_block__` line.
- `hub-client/src/components/render/q2-preview/PreviewDocument.tsx` (current) — Plan 2B/2C version; 2D's Phase 6.2 rewrites it.

## Revision history

- **2026-05-10**: initial draft. Decision context: chose Option B (read `ast.meta` in JS) over Option A (typed field on `RenderResponse`) for v1 because (a) `ast.meta` reads are already idiomatic in the codebase (`ReactRenderer.tsx:211`, `ReactAstSlideRenderer.tsx:220-221`), (b) sidebar-derived body-classes — the only fields that would justify Option A's plumbing — aren't computed in q2-preview's pipeline today, so Option A would buy nothing for v1, (c) Option B keeps the plan scoped to hub-client with no Rust changes. Body-class application via `useEffect` (not module-scope imperative) chosen because there's no race-against-first-mount like `applyTheme` has — the AST is already parsed by the time the React commit runs. The Option A → typed field on `RenderResponse` pattern is reserved for the future sidebar plan, where it's load-bearing.

- **2026-05-10 (risk-area resolution)**: two risk areas flagged for "verify before implementation" in the initial draft are now resolved:
  - **Iframe host body class**: verified that `hub-client/public/q2-preview.html:28` declares `<body>` with no class attribute and host CSS only targets the bare `body` selector. `useEffect`'s `document.body.className = bodyClasses` overwrite is safe; no host-CSS dependency to preserve. Risk-area entry downgraded from "verify before implementation" to "verified safe" with a file:line citation.
  - **`ensureHtmlElements` negative selectors**: verified that the harness already supports negatives via the two-array YAML schema (`smokeAllDiscovery.ts:124-129` for parsing; `smokeAllAssertions.ts:122-138` for the runner). Negatives use Playwright's `toHaveCount(0)`. Smoke-fixture descriptions in §Test plan rewritten to use the actual two-array schema with both positive and negative selector lists per fixture.

- **2026-05-10 (title-block extension)**: scope extended to also mirror the HTML template's title block (`<header id="title-block-header">` and the title/subtitle/quarto-title-meta-author/quarto-title-meta-date/abstract subtree at `template.rs:211-240`). Rationale: with body-container parity in place, the next visible chrome gap between q2-preview and the HTML format is the title block; theme CSS targeting `.quarto-title-block .title`, `.quarto-title-meta-author`, and `.abstract` lands on real elements only after this work. Decisions:
  - **Phase 7 added** with its own `PreviewTitleBlock.tsx` component, a new `extractMetaStringList` helper in `utils/meta.ts`, snapshot tests, and 5 smoke-all fixtures. Verification (formerly 6.5) becomes Phase 8.
  - **Mirror Rust quirks deliberately**: date-suppressed-without-author is locked in with an explicit regression-test fixture so a future "fix Rust quirk" plan flips both sides at once. Inline-emphasis stripping in titles is also locked in.
  - **No deliberate divergences** from the Rust HTML format. Multi-author rendering matches Rust's broken-but-consistent behavior (one block, names empty-string-joined to `AliceBob`) so theme CSS doesn't need a q2-preview special case and the proper-multi-author fix lands once on both sides in a future plan.
  - **`<head>` meta tags / abstract block rendering / inline-markup-preserving title rendering / i18n** are explicitly out of scope and listed under §Out of scope so they aren't accidentally pulled in.
  - Estimated scope grows from ~233 to ~495 LOC; still a single focused session.

- **2026-05-10 (consistency pass)**: review-driven cleanups to the title-block extension:
  - **Reverted multi-author divergence to Rust parity**: `PreviewTitleBlock` emits exactly one `<div class="quarto-title-meta-author">` with names empty-string-joined for list-form authors (matches Rust's `TemplateValue::List` stringification). The "one block per name" idea contradicted the plan's stated mirror-byte-for-byte philosophy and would have created a heading-pluralization design question (`Author` vs `Authors`) that has no Rust precedent to mirror. Multi-author UX is now a single deferred follow-up that lands once on both sides.
  - **Filled the missing fifth fixture** (`title-block-date-no-author.qmd`) into the Phase 7.5 checklist — the Test plan section and References list already had it, but the checklist did not.
  - **Renumbered stale "Phase 6.5" references** to "Phase 8.1" in the Visual sanity check section and a Risk-areas mitigation note.
  - **Re-anchored the body-container "Key invariants"** to sit directly under the `PreviewDocument.tsx` source code (was orphaned below `PreviewTitleBlock.tsx` after the Phase 7 insertion).
  - **Corrected the Phase 6/7 independence claim** in Estimated scope: Phase 7 mounts inside Phase 6's `<main>` and inherits its skip branches, so Phase 6 must land first.
  - **Pinned `title-block-no-title.qmd` body content** to a single literal paragraph so the `'p'` selector matches deterministically.
  - **Clarified `<title>` (browser-tab title) status**: not a PreviewDocument concern — `entry.tsx` owns it today; verify during Phase 8.1's manual session, no scope change for 2D.

- **2026-05-10 (design-question resolutions)**: 5 open design questions resolved:
  1. **PreviewDocument prop shape** — confirmed against the post-2C-landed file: stays `{ ast, setAst, onNavigateToDocument }` (matching `framework/types.ts:155-159` `AstProps`), NOT `NodeArgs<PandocAST>`. Phase 6.2's example code uses the existing shape verbatim. No coordination dependency.
  2. **Abstract as `MetaBlocks`** — `extractMetaString` extended to walk `MetaBlocks` via `blocksToPlainText`, mirroring Rust's `blocks_to_text` fallthrough at `template.rs:610-614`. Out-of-scope entry rewritten from "MetaBlocks unsupported" to "block-rendered abstracts deferred (stringification matches Rust)".
  3. **Title-block transform exclusion (the user's hint)** — confirmed `"title-block"` is in `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1052`). In full HTML mode the transform short-circuits anyway (template emits the chrome) so q2-preview is aligned. In **minimal mode** the Rust transform prepends a synthetic `<h1>` to `ast.blocks` (`transforms/title_block.rs:42-95`); without React-side replication, q2-preview's minimal mode silently drops the title. Phase 6.2 now re-implements the minimal-mode branch in `PreviewDocument.tsx`'s minimal branch, with its own vitest cases and a `body-container-minimal-title.qmd` smoke fixture. RevealJS is left untouched (q2-debug owns slide chrome).
  4. **`PreviewTitleBlock` registered as a renderer** — registered under synthetic key `'__title_block__'` in `previewRegistry` (sibling of Plan 2C's `__fallback__: Custom.Fallback`). Resolved via `useContext(RegistryContext).registry['__title_block__'] ?? PreviewTitleBlock` from `PreviewDocument.tsx` — same access pattern as `dispatchers.tsx:39`. `mergedPreviewRegistry` already layers user TSX exports over built-ins, so user override flows through the existing 2C merge site with no extra wiring. `registry.test.ts` is extended to pin the synthetic-key presence.
  5. **Empty-string title falsy semantics** — committed to Pandoc's `$if(title)$` behavior: missing key, empty string, and non-string shapes all suppress the title block. Doc-comment in `PreviewTitleBlock.tsx` cites the source; explicit empty-string regression test added to Phase 7.4.

- **2026-05-10 (post-2C-landing reread)**: Plan 2C landed on the branch in this same session. Re-read 2D against the actual post-landing files (`PreviewDocument.tsx`, `registry.ts`, `dispatchers.tsx`, `framework/types.ts`, `RegistryContext.tsx`, `Ast.tsx`, `registry.test.ts`) and tightened:
  - Removed the Phase 6.0 "coordinate prop shape with 2C" step (answered by the file as-it-stands).
  - Replaced the example's `useRegistry()` placeholder with the real `useContext(RegistryContext).registry` pattern; imports updated accordingly.
  - Replaced `NodeArgs<PandocAST>` with `{ ast, setAst, onNavigateToDocument }`; rewrote `renderChildren` call to use the existing `node: ast as any, setLocalAst: setAst as any` cast that the current 2B/2C file uses.
  - Phase 7.3 now references the actual `registry.ts:30-40` line range and the existing `__fallback__: Custom.Fallback` line as the precedent to mirror.
  - Multi-plan contracts §"Consumed: Plan 2B / 2C" replaced — 2C is no longer a parallel session, it's a hard dependency.
  - Hard dependencies list now includes 2C explicitly.
  - References §hub-client read-only side now includes `framework/types.ts`, `RegistryContext.tsx`, `dispatchers.tsx`, `registry.ts`, and the existing `PreviewDocument.tsx`.
