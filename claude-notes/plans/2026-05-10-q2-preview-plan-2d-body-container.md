# Plan 2D — q2-preview body container + title block

**Date:** 2026-05-10
**Branch:** feature/q2-preview
**Status:** Implementation plan
**Milestone:** M2 polish. Closes the gap between q2-preview's bare-fragment output and the HTML pipeline's `#quarto-content > main.content#quarto-document-content` wrapper plus its `<header id="title-block-header">` document chrome, so theme CSS that targets `.page-layout-article`, `.content`, `body.fullcontent`, and `.quarto-title-block` selectors lands on real elements in the iframe DOM.

## Goal

Add the document-level body container AND the document-title block to q2-preview, mirroring the HTML pipeline's wrapper structure (`crates/quarto-core/src/template.rs:140-256`) and reading title/author/date/subtitle/abstract directly from `ast.meta`. After 2D lands:

- The iframe DOM has `<div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-{layout}"><main class="content" id="quarto-document-content">{title-block?}{blocks}</main></div>` wrapping rendered blocks (or no wrapper when `minimal: true` / `theme: none/pandoc`).
- The iframe `<body>` element carries the user's `body-classes` override or the literal default `fullcontent`, applied imperatively on document mount.
- When `meta.title` is set, a `<header id="title-block-header" class="quarto-title-block default">` is emitted before the body blocks, containing the title (and optional subtitle, author, date, abstract) in the same element/class structure as the Rust HTML template (`template.rs:211-240`).
- Theme CSS rules that target `body.fullcontent .content`, `.page-layout-full`, `.quarto-title-block .title`, `.quarto-title-meta-author`, etc. land on real elements without theme forks.
- Sidebar / navbar / TOC / footer chrome remains deferred (they require pipeline transforms that q2-preview's `Q2_PREVIEW_TRANSFORM_EXCLUDED` currently elides — a separate plan adds them and revisits body-classes computation).
- `<head>` meta tags (`<meta name="author">`, `<meta name="dcterms.date">`, `<meta name="keywords">`, `<meta name="description">`, `<link rel="canonical">`) remain deferred — they affect SEO/print-preview but not visible chrome, and the iframe `<head>` is owned by `q2-preview.html` + `entry.tsx`, not `PreviewDocument`. A follow-up plan can wire them.

## Checklist

### Phase 6 — Framework extraction + body container

(Phase numbering continues from Plan 2C's Phase 4 + Phase 5. Phase 6.0 is structural prep that 2D consumes; it stands on its own and could in principle land as its own plan, but it's small enough that inlining is cheaper than a separate-plan handoff.)

#### Phase 6.0 — Framework extraction (structural prep)

The 2pre restructure left two cross-cutting Pandoc-AST utilities — plain-text walks and meta coercion — in homes that don't reflect their reach: `inlinesToPlainText` / `blocksToPlainText` ended up in `q2-preview/utils.tsx` (used by every plain-text consumer including q2-preview's `Image`, `Note`, and the framework-meta extension); `extractMetaString` exists as a stripped-down private function in the slide renderer. Plan 2D needs a single `extractMetaString` everywhere; promoting it cleanly requires lifting the plain-text walks alongside it. Both belong in `framework/` — they're pure Pandoc-AST shape concerns with zero format opinions.

- [x] **6.0a** Create `hub-client/src/components/render/framework/plainText.ts` (NEW). Move `inlinesToPlainText` + `blocksToPlainText` from `q2-preview/utils.tsx:69-170` verbatim. Keep the same external behavior; only the file location changes. Update the existing q2-preview consumers to re-import from the new location: `q2-preview/inlines/Image.tsx:4` (`inlinesToPlainText`), `q2-preview/inlines/Note.tsx:5` (`blocksToPlainText`), and `q2-preview/utils.tsx`'s internal `blockText` helper that used the local definitions. After this commit, `q2-preview/utils.tsx` slims to format-specific helpers only (`lookupAssetUrl`, `renderSlot`, `makeSlotSetter`, `composeAttr`, `formatRefLabel`).
- [x] **6.0b** Create `hub-client/src/components/render/framework/meta.ts` (NEW, ~50 LOC). Define `extractMetaString`, `extractMetaBool`, `extractMetaStringList` per the source listing in §Scope. The `MetaInlines` / `MetaBlocks` branches call `inlinesToPlainText` / `blocksToPlainText` from the new sibling `framework/plainText.ts` (no layering inversion — both files are now framework-tier).
- [x] **6.0c** Update `framework/index.ts` (currently 4 lines re-exporting types/RegistryContext/Ast/dispatch). Add **flat** re-exports (`export * from './meta'` / `export * from './plainText'` / `export * from './customNode'`, NOT namespace `export * as meta from …`) for: the three `extract*` helpers from `meta.ts`, the two walks from `plainText.ts`, and `customNode` (which is in framework/ today but never re-exported — bookkeeping fix discovered during cross-check). Flat is required because every source listing later in this plan imports named symbols directly from `'../framework'` (`import { extractMetaString, renderChildren } from '../framework'`). Consumers that deep-imported `from '../framework/customNode'` keep working; new consumers prefer the barrel.
- [x] **6.0c.1** Extend the user-facing renderer surface global at `q2-preview/entry.tsx:56-64` (`window.__Q2_PREVIEW_RENDERER__`) to expose the new framework helpers: `extractMetaString`, `extractMetaBool`, `extractMetaStringList`, `inlinesToPlainText`, `blocksToPlainText`. The global is the explicit "public surface for user TSX overrides" (see `entry.tsx:50-55`'s comment about the explicit-object form being deliberate); user overrides of `__title_block__` need `extractMetaString` to coerce `meta.title` etc. without re-implementing the walk. Without this exposure, the doc-comment in `PreviewTitleBlock.tsx` that points users at `extractMetaString` would be a lie. Lands after 6.0a + 6.0b (helpers must exist before being exposed); pairs in the same commit as 6.0c so the global stays consistent with the framework barrel. `PreviewTitleBlock` itself is added to the same global in Phase 7.3.1 (it doesn't exist yet at 6.0c.1 time).
- [x] **6.0d** Replace `ReactAstSlideRenderer.tsx`'s private `extractMetaString` (lines 350-371) with `import { extractMetaString } from './framework'`. **This is a deliberate behavior change** for slide titles/authors that contain inline markup: the old walk handled only `Str`/`Space` inside `MetaInlines`, so `title: "*Hello*"` rendered `""`; the new walk recurses through `Emph`/`Strong`/`Code`/`Link`/etc., so the same input renders `"Hello"`. Verified intent: the old shape was a first-cut from commit `55c38955` (2026-02-04, "Add slides!") and never revisited; no design rationale documented; result is consumed as plain React text in `<h1>`/`<p>` — the suppression of markup content is accidental, not designed. Add a vitest case under `hub-client/src/components/render/` (next to `ReactAstSlideRenderer.tsx`) that constructs a `meta.title` of `MetaInlines [Str("Hello"), Space, Emph([Str("world")])]`, calls `parseSlides`, and asserts the resulting title-slide's `title` field is `"Hello world"`. Locks the new behavior; documents the change for future grep.
- [x] **6.0e** Consolidate the duplicate `meta.format` extraction into one call site each. `ReactRenderer.tsx:211`'s inline check (`ast?.meta?.format?.t === 'MetaString' && ast.meta.format.c === 'revealjs'`) becomes `extractMetaString(ast?.meta?.format) === 'revealjs'`. `getQ2Format.ts`'s body collapses to `extractMetaString(ast?.meta?.format)` followed by the existing `.startsWith('q2-') || === 'revealjs'` filter. **Behavior change, not strict superset**: today's `getQ2Format.ts:15` reads only the first child of `MetaInlines` (`fmt.c?.[0]?.c`); `ReactRenderer.tsx:211` matches `MetaString` only. The new path walks the full inlines list via `inlinesToPlainText`, so (a) `ReactRenderer` newly matches `MetaInlines [Str("revealjs")]` where it didn't before, and (b) `getQ2Format` and `ReactRenderer` both diverge from today on multi-child MetaInlines (e.g. `MetaInlines [Str("re"), Space, Str("vealjs")]` → old `"re"` / new `"re vealjs"`). For realistic `format` values (single-token strings) the new and old paths agree; the consolidation is benign in practice but is a deliberate behavior change, not a no-op refactor. Vitest unit cases for the new behaviors exist via the framework `meta.test.ts` in 6.0f; integration coverage is incidental.
- [x] **6.0f** Vitest unit tests for `framework/meta.ts` and `framework/plainText.ts` at `framework/meta.test.ts` (NEW) and `framework/plainText.test.ts` (NEW; or move the existing `q2-preview/utils.tsx` unit tests if they exist). Cover `extractMetaString` against `MetaString`, `MetaInlines (Str/Space)`, `MetaInlines with Emph/Strong/Code/Link`, `MetaBlocks`, `MetaBool`/`MetaList`/`MetaMap` (returns undefined), missing key. `extractMetaBool` against `MetaBool(true|false)`, `MetaString("true"|"false")`, others (undefined). `extractMetaStringList` against `MetaList of MetaString`, `MetaList of MetaInlines`, single `MetaString` (returns empty), missing (returns empty), wrong shape (returns empty).

After Phase 6.0, every cross-format Pandoc-AST utility lives in `framework/`, q2-preview's `utils.tsx` is format-specific only, the slide renderer has the same meta-coercion behavior every other consumer has, and the three duplicate `meta.format` checks collapse to one helper call. Plan 2D's Phase 6.1+ then consume the framework helpers directly with no further plumbing.

#### Phase 6.1+ — Body container

- [x] **6.1** *(was the original Phase 6.1 "lift extractMetaString" — replaced by 6.0a–f above. Renumber: the old 6.1 work is fully covered by 6.0d.)*
- [x] **6.2** Extend `q2-preview/PreviewDocument.tsx` with the body-container wrapper (~50 LOC). Read `page-layout`, `body-classes`, `minimal`, `theme` from `ast.meta` via the new util. Apply `body-classes` to `document.body.className` imperatively in a `useEffect`. Emit the wrapper structure unless `minimal: true` OR `theme === 'none'` OR `theme === 'pandoc'` (matches Rust's `is_minimal_html()` defined at `format.rs:306-318` and called from `template.rs:501`). **Minimal-mode title synthesis**: re-implement the Rust `title-block` transform's minimal-mode branch (`transforms/title_block.rs:54-110` for the impl + helpers at `:113-141`) on the React side — when minimal AND `meta.title` is set AND no level-1 `Header` exists in `ast.blocks`, prepend a synthetic `<h1>{title}</h1>` inside the bare Fragment. Rust's transform is in `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1052`), so without this React-side synthesis, q2-preview's minimal mode silently drops the title.
- [x] **6.2a** Add iframe `<title>` wiring inside `PreviewDocument.tsx`'s existing `useEffect` chain (~6 LOC). The iframe `<title>` is currently the static literal at `q2-preview.html:6` (`q2-preview Renderer (Sandboxed)`); nothing writes `document.title` from the AST today. Add a `useEffect` that reads `const next = extractMetaString(meta.pagetitle) ?? extractMetaString(meta.title);` and **only writes when `next` resolves to a non-empty string** — when the document has no title, the effect no-ops and the static `q2-preview Renderer (Sandboxed)` from `q2-preview.html:6` stays in place. Cleanup restores the previous title (which equals the snapshot taken on mount, before the write). Pseudocode:
  ```ts
  useEffect(() => {
      const next =
          extractMetaString(meta.pagetitle) ??
          extractMetaString(meta.title);
      if (!next) return; // no title → leave the static iframe title alone
      const previous = document.title;
      document.title = next;
      return () => {
          document.title = previous;
      };
  }, [meta]);
  ```
  Why this shape (vs. always writing a `'q2-preview Renderer'` fallback): it preserves the static HTML title for documents with no title, avoids first-mount-vs-subsequent-mount divergence in the cleanup-restore value, and matches the body-classes effect's "only act when there's something to act on" pattern. Mainly affects screen-reader announcements, DevTools, and pop-out scenarios; the parent-window tab title (set by `Editor.tsx:348-350` from project filename + description) is unaffected. Vitest cases:
  - Mount with `meta.title = MetaString("My Doc")` → `document.title === 'My Doc'`.
  - Mount with `meta.pagetitle = MetaString("Page")` and `meta.title = MetaString("Doc")` → `document.title === 'Page'` (pagetitle wins).
  - Mount with empty `meta` (no title, no pagetitle) → `document.title` is unchanged from the pre-mount value (test sets a known sentinel before mount and asserts it remains).
  - Mount with `meta.title = MetaString("")` → empty string is falsy, no write, sentinel preserved (locks Pandoc-falsy semantics).
  - Cleanup: mount with title → unmount → asserts the pre-mount title is restored.
- [x] **6.3** Vitest unit tests for `extractMetaString` / `extractMetaBool` against the typed Pandoc Meta variants (`MetaString`, `MetaInlines`, `MetaBlocks`, `MetaBool`, missing key). Per-snapshot tests on `PreviewDocument` for: default (article + fullcontent), `page-layout: full`, `body-classes: custom-cls`, `minimal: true` (no wrapper), `theme: none` (no wrapper), **`minimal: true` + `title` set + no body Header → synthetic `<h1>` prepended**, **`minimal: true` + `title` + body has user-authored level-1 Header → no synthetic `<h1>` (avoid duplicate)**.
- [x] **6.4** Smoke-all q2-preview fixtures — extend `crates/quarto/tests/smoke-all/q2-preview/` (directory exists post-2B) with:
  - `body-container-default.qmd` (single-doc; no `page-layout` set; assert `div#quarto-content.page-layout-article` and `body.fullcontent`).
  - `body-container-full-layout.qmd` (single-doc; `page-layout: full`; assert `div.page-layout-full`).
  - `body-classes-override.qmd` (single-doc; `body-classes: custom-cls`; assert `body.custom-cls` AND no `body.fullcontent`).
  - `body-classes-full-layout-combo.qmd` (single-doc; `body-classes: custom-cls` AND `page-layout: full` together; assert `div.page-layout-full` AND `body.custom-cls` AND no `body.fullcontent`). Locks the two-knobs-together case — each other fixture flips one knob; this catches regressions where the two flow paths interact (e.g. a future refactor that conflates body-classes' useEffect with the page-layout className).
  - `body-container-minimal.qmd` (single-doc; `minimal: true`; assert no `div#quarto-content` and no `<main>` wrapper).
  - `body-container-minimal-title.qmd` (single-doc; `minimal: true`, `title: "Doc"`, body has only a paragraph and no level-1 header; assert a synthetic `<h1>Doc</h1>` is rendered before the paragraph and locks the React-side replication of `transforms/title_block.rs`'s minimal-mode branch).

  All fixtures use `_quarto.tests.run.requires_js: true` so the Playwright runner picks them up. Pattern matches Plan 2C item 5.2.

### Phase 7 — Title block

Phase 7 sits on top of Phase 6's wrapper: when present, the `<header id="title-block-header">` lives inside `<main class="content">`, before any rendered body blocks. If the wrapper is skipped (`minimal: true`, `theme: none/pandoc`), the title block is also skipped — matches the Rust minimal template (`template.rs:80-111`), which has no title block.

- [x] **7.1** Extend `hub-client/src/components/render/framework/meta.ts` with `extractMetaStringList` (~15 LOC; landed earlier in Phase 6.0b — this Phase 7.1 entry is the consumer-side hook). Reads a `MetaList` of `MetaInlines`/`MetaString` entries and returns `string[]` (empty when missing or wrong shape). Used by the title-block author rendering to support YAML list form (`author: [Alice, Bob]`). Single-author shapes (`MetaString` / `MetaInlines`) continue to use `extractMetaString`. Vitest unit tests live alongside in `framework/meta.test.ts` (Phase 6.0f).
- [x] **7.2** New file `hub-client/src/components/render/q2-preview/custom/PreviewTitleBlock.tsx` (~70 LOC). **Resolution of Open design question §4**: lives under `./custom/` to mirror how `__fallback__: Custom.Fallback` is wired today. Export from `q2-preview/custom/index.ts` so `import * as Custom from './custom'` picks it up. **Prop shape — Resolution of Open design question §6**: receives `AstProps` (`{ ast, onNavigateToDocument, setAst }`) — the same shape registered under the `Ast` key, NOT the `NodeArgs<…>` shape used by per-tag entries. Rationale: the title block operates on document-level state (`ast.meta`), not on a node in the AST; treating it parallel to `Ast` is honest about the design and collapses three potential synthetic-key shapes to two (`Ast`/`__title_block__` = document-level via `AstProps`; `__fallback__` = node-level via `NodeArgs`). The built-in reads `ast.meta` and ignores `setAst` / `onNavigateToDocument`; a user override that wants editable title blocks can call `setAst`. Reads `meta.title` / `meta.subtitle` / `meta.author` / `meta.date` / `meta.abstract` via the `extractMeta*` helpers; emits the `<header id="title-block-header">` structure mirroring `template.rs:211-240` byte-for-byte:
   - `<header id="title-block-header" class="quarto-title-block default">` only when `title` resolves (matches `$if(title)$`).
   - `<div class="quarto-title">` containing `<h1 class="title">{title}</h1>` and optional `<p class="subtitle">{subtitle}</p>`.
   - `<div class="quarto-title-meta">` only when an author resolves (matches `$if(author)$`); inside it, exactly one `<div class="quarto-title-meta-author">` (heading "Author"; multi-author lists get empty-string-concatenated to match Rust's broken-but-consistent behavior — see §"Out of scope: Multi-author rendering UX") and a nested `<div class="quarto-title-meta-date">` (heading "Published") only when `date` is also set.
   - `<div class="abstract">` with `<div class="abstract-title">Abstract</div>` and the abstract text, only when `abstract` resolves.
   - **Mirrors the Rust quirk**: date renders only when at least one author is present (`template.rs:225` puts the date `$if(date)$` block inside the `$if(author)$` block). Document the quirk inline so a future "fix the Rust template" plan can flip both at once.
   - **Does NOT lift Pandoc filter outputs into the title block**: the Rust template inserts `$author$` as a stringified inlines walk, which loses inline emphasis — q2-preview matches by using `inlinesToPlainText` for v1. Block-level abstract rendering and richer inline-markup-preserving title rendering are deferred follow-ups; the v1 fidelity matches what the Rust HTML format currently produces.
- [x] **7.3** Register `PreviewTitleBlock` in `previewRegistry` (`registry.ts:30-40`) under the synthetic key `'__title_block__'`, sibling of Plan 2C's `__fallback__: Custom.Fallback` line at `registry.ts:34`. The registration line is `__title_block__: Custom.PreviewTitleBlock` (parallel to `__fallback__: Custom.Fallback`).

  **FormatRegistry type tightening — Resolution of Open design question §7**: extend the type at `framework/types.ts:163` to declare typed optional entries for both synthetic keys, so user-TSX overrides get compile-time prop-shape checking:
  ```ts
  export type FormatRegistry = Record<string, (props: any) => React.ReactNode> & {
      Ast: AstComponent;
      Block: DispatcherComponent;
      Inline: DispatcherComponent;
      __fallback__?: (args: NodeArgs<CustomBlockNode | CustomInlineNode>) => React.ReactNode;
      __title_block__?: AstComponent;
  };
  ```
  Optional (`?`) because not every format must register them — the runtime `??` fallback in dispatchers / PreviewDocument covers the missing case. Pandoc tag keys and CustomNode `type_name` keys keep their loose `(props: any)` typing via the `Record<string, …>` index signature, preserving the existing namespace flexibility. NodeArgs / AstComponent / DispatcherComponent / CustomBlockNode / CustomInlineNode are all already exported from `framework/types.ts`; no new imports needed.

  **Mount inside PreviewDocument**: resolve via `const { registry } = useContext(RegistryContext); const TitleBlock = registry.__title_block__ ?? Custom.PreviewTitleBlock;` (matches the dispatchers' access pattern at `dispatchers.tsx:39`). Mount `<TitleBlock ast={ast} setAst={setAst} onNavigateToDocument={onNavigateToDocument} />` inside `<main class="content">`, before `{children}`. The merged `mergedPreviewRegistry = { ...previewRegistry, ...customRegistry }` site in `entry.tsx:237-240` already layers user TSX exports over built-ins via plain object spread (verified 2026-05-10: `hub-client/src/utils/customRegistry.ts:14-20` does no key filtering, so synthetic keys with leading `__` flow through); no extra wiring is needed for user overrides.

  **Test wiring** (`registry.test.ts`):
  1. Extend the existing `'every expected CustomNode component is exported from ./custom'` test at `registry.test.ts:80-93` to include `'PreviewTitleBlock'` in the expected-names list. Locks the export from `q2-preview/custom/index.ts`.
  2. **Add a new test** asserting both synthetic-key registrations directly: `previewRegistry.__fallback__ === Custom.Fallback` AND `previewRegistry.__title_block__ === Custom.PreviewTitleBlock`. Locks both synthetic-key registrations so a future refactor that drops either silently can't ship without breaking the test. (The `__fallback__` registration is exercised by behavior tests today but never directly asserted — closes a pre-existing gap noticed during cross-check.)

  When the wrapper is skipped (minimal / theme: none / theme: pandoc), the title block is also skipped — falling through to the bare Fragment / minimal-mode `<h1>` synthesis.
- [x] **7.3.1** Extend `window.__Q2_PREVIEW_RENDERER__` (`entry.tsx:56-64`) with `PreviewTitleBlock` so user TSX overrides of `__title_block__` can **compose** the built-in instead of re-implementing it from scratch — **Resolution of Open design question §8**. Same exposure mechanism as Phase 6.0c.1 (which adds the five framework helpers); `PreviewTitleBlock` joins as the user-composable building block for the title-block chrome. Composition idiom:
  ```tsx
  // user-tsx file registered via render-components
  const { PreviewTitleBlock, extractMetaString } = window.__Q2_PREVIEW_RENDERER__;

  export const __title_block__ = ({ ast, setAst, onNavigateToDocument }) => (
      <>
          <PreviewTitleBlock
              ast={ast}
              setAst={setAst}
              onNavigateToDocument={onNavigateToDocument}
          />
          <div className="doi">DOI: {extractMetaString(ast.meta.doi)}</div>
      </>
  );
  ```
  Rejected alternatives discussed in Open design question §8. The exposure is one extra line on the explicit-object global; the same global already exposes `Node`, `Block`, `Inline`, `previewRegistry`.
- [x] **7.4** Vitest unit tests for `PreviewTitleBlock` (`PreviewTitleBlock.test.tsx`, NEW). Covered cases:
   - No title → renders `null` (no `<header>` element).
   - Title only → `<header>` + `<h1 class="title">`; no `<p class="subtitle">`; no `<div class="quarto-title-meta">`; no `<div class="abstract">`.
   - Title + subtitle → adds `<p class="subtitle">`.
   - Title + author (string) → adds `<div class="quarto-title-meta">` with one `<div class="quarto-title-meta-author">`.
   - Title + author (MetaList of two) → still exactly ONE `<div class="quarto-title-meta-author">`, with `quarto-title-meta-contents` text equal to the empty-string-joined names (matches Rust's broken-but-consistent behavior).
   - Title + author + date → date appears as `<div class="quarto-title-meta-date">` as a **sibling** of `<div class="quarto-title-meta-author">`, both children of the `<div class="quarto-title-meta">` wrapper (mirrors `template.rs:219-232` where the date block sits at the same nesting depth as the author block, both inside `quarto-title-meta`).
   - Title + date but no author → date does NOT render (mirrors the Rust quirk; explicit lock-in test so a future "support date without author" change is a deliberate regression).
   - Title + abstract → adds `<div class="abstract">` with the `<div class="abstract-title">Abstract</div>` heading.
   - Title with inline emphasis (`title: *World*` parses to MetaInlines with Emph) → renders as plain text (matches Rust today; locks the v1 fidelity choice).
   - **User override via registry — full replacement** — vitest integration test (next to the existing 2C override tests) registers a stub `__title_block__: () => <div data-testid="custom-title">x</div>` (receives `AstProps` but ignores them), mounts a doc with `title` set, and asserts `data-testid="custom-title"` is present and the built-in `<header id="title-block-header">` is NOT.
   - **User override via registry — composing the default** — registers a stub that calls `window.__Q2_PREVIEW_RENDERER__.PreviewTitleBlock` and emits a sibling element: `({ ast, setAst, onNavigateToDocument }) => <><PreviewTitleBlock ast={ast} setAst={setAst} onNavigateToDocument={onNavigateToDocument} /><div data-testid="extra">e</div></>`. Mounts a doc with `title` set; asserts BOTH the built-in `<header id="title-block-header">` AND `data-testid="extra"` are present. Locks the §8 composition idiom: the `__Q2_PREVIEW_RENDERER__.PreviewTitleBlock` exposure is load-bearing for user extensions and must not be dropped silently.
- [x] **7.5** Smoke-all q2-preview fixtures for the title block (single-doc, all under `crates/quarto/tests/smoke-all/q2-preview/`):
  - `title-block-default.qmd` (`title: "Doc"`). Positives: `['header#title-block-header.quarto-title-block', 'h1.title']`. Negatives: `['p.subtitle', 'div.quarto-title-meta', 'div.abstract']`.
  - `title-block-full.qmd` (`title`, `subtitle`, `author`, `date`, `abstract` all set). Positives: `['header#title-block-header', 'h1.title', 'p.subtitle', 'div.quarto-title-meta-author', 'div.quarto-title-meta-date', 'div.abstract', 'div.abstract-title']`. Negatives: none.
  - `title-block-no-title.qmd` (no `title`, with `author: "Jane Doe"` and `date: "2026-05-10"` set, body contains a single `Some content.` paragraph so the `'p'` selector has something to match). Positives: `['p']`. Negatives: `['header#title-block-header', 'div.quarto-title-meta']` (locks "no title → no chrome at all").
  - `title-block-multi-author.qmd` (`author: [Alice, Bob]`, plus `title` and `date`). Positives: `['div.quarto-title-meta-author']`. Negatives: none. The fixture's separately-asserted text content is the empty-string-joined `AliceBob` (locks Rust parity — we deliberately do NOT diverge to one-block-per-name; see §"Out of scope: Multi-author rendering UX").
  - `title-block-date-no-author.qmd` (`title` + `date`, no `author`). Positives: `['header#title-block-header']`. Negatives: `['div.quarto-title-meta', 'div.quarto-title-meta-date']`. Locks the Rust quirk that date alone (without author) is suppressed.

### Phase 8 — Verification

- [x] **8.1** Run `cargo xtask verify --e2e` before declaring 2D complete (per project CLAUDE.md "End-to-end verification before declaring success"). Default `cargo xtask verify` skips the Playwright runner; without `--e2e` the smoke fixtures landed in 6.4 and 7.5 are not exercised. Also do a manual browser session against a running hub for sanity; record the invocation and an inspected-output snippet in the implementation transcript or this plan's checklist comments. Manual session must include at least one document with title + author + date set, so the title-block render is visually inspected against the Rust HTML output.

## Scope

### In scope

#### `hub-client/src/components/render/framework/meta.ts` — shared meta helpers (NEW)

Three helpers — `extractMetaString` is lifted from `ReactAstSlideRenderer.tsx:350` (the slide renderer's existing private helper; the slide renderer lives at the top level of `components/render/`, NOT inside `q2-debug/`); `extractMetaBool` and `extractMetaStringList` are new in 2D:

```ts
// hub-client/src/components/render/framework/meta.ts
import type { BlockNode, InlineNode } from './types';
import { inlinesToPlainText, blocksToPlainText } from './plainText';

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
 * Existing callers: slide-renderer slide title/author, q2-preview body
 * container, q2-preview title block (title/subtitle/author/date/abstract).
 *
 * NOTE — behavior change for the slide renderer. The current local copy at
 * `ReactAstSlideRenderer.tsx:350-368` only walks `Str` and `Space`
 * inside `MetaInlines`; any `Emph` / `Strong` / `Code` / `Link` is
 * silently dropped. The lifted version delegates to `inlinesToPlainText`,
 * which recursively walks all nested inlines. Slide titles and authors
 * containing inline markup (`# *Hello*`, `# Hello \`world\``, etc.)
 * will start rendering text where they previously rendered the empty
 * string. Strict improvement, but treat as a behavior change for
 * regression-testing purposes — see Phase 6.0d below.
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

**Why shared util, not q2-preview-local**: the slide renderer (`ReactAstSlideRenderer.tsx`, top-level under `components/render/`) already uses `extractMetaString` privately. Lifting avoids two copies that drift independently. The slide-renderer call site is updated to import from `framework/meta` in the same commit (Phase 6.0d). Future format-render targets (the q2-slides migration in Plan 2E, hypothetical docx/PDF preview, etc.) get the helper for free.

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
import { renderChildren, extractMetaString, extractMetaBool } from '../framework';
import type { PandocAST } from '../framework';
import * as Custom from './custom';

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
    //   2. SidebarRenderTransform output — deferred to a future sidebar plan.
    //   3. Literal "fullcontent" default.
    const bodyClasses = extractMetaString(meta['body-classes']) ?? 'fullcontent';

    // Mirror Rust is_minimal_html() (defined at format.rs:306, called from
    // template.rs:501): minimal: true OR theme: none/pandoc → skip the
    // entire wrapper.
    const minimal =
        extractMetaBool(meta.minimal) === true ||
        extractMetaString(meta.theme) === 'none' ||
        extractMetaString(meta.theme) === 'pandoc';

    // Apply body-classes to document.body imperatively. Mirrors the
    // applyTheme pattern at entry.tsx:120-135 for predictable iframe-side
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
    // `__title_block__` is typed as `AstComponent` in FormatRegistry
    // (Phase 7.3 type tightening), so the cast is unnecessary — the
    // `??` keeps inference clean. Hook order: this useContext MUST be
    // called unconditionally before any of the early returns below —
    // React's rules-of-hooks require hooks to fire in the same order
    // on every render. Reading the registry in the minimal branch
    // (where TitleBlock is unused) is wasted but harmless.
    const { registry } = useContext(RegistryContext);
    const TitleBlock = registry.__title_block__ ?? Custom.PreviewTitleBlock;

    // The current PreviewDocument hands the parsed AST + setAst to
    // renderChildren via the framework's "Ast"-shaped detection. Casts
    // mirror the existing 2B/2C version of this file — `renderChildren`
    // is typed for BlockNode | InlineNode but special-cases PandocAST.
    const children = renderChildren({
        node: ast as any,
        setLocalAst: setAst as any,
        onNavigateToDocument,
    });

    if (minimal) {
        // Re-implement the Rust `title-block` transform's minimal-mode
        // branch on the React side. Rust's transform (excluded from
        // q2-preview's pipeline via Q2_PREVIEW_TRANSFORM_EXCLUDED at
        // pipeline.rs:1052; behavior at transforms/title_block.rs:54-110
        // with helpers at :113-141) prepends a level-1 Header from
        // meta.title to ast.blocks when
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
                <TitleBlock
                    ast={ast}
                    setAst={setAst}
                    onNavigateToDocument={onNavigateToDocument}
                />
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

**Phase 7 wiring**: `<PreviewTitleBlock ast={ast} setAst={setAst} onNavigateToDocument={onNavigateToDocument} />` is mounted as the FIRST child of `<main class="content">`. It returns `null` when `meta.title` doesn't resolve, so the no-title path produces a clean `<main>` containing only `{children}` (matches Rust template's `$if(title)$ … $endif$` gate at `template.rs:211/240`). Phase 6's "skip the wrapper" branches (minimal / theme: none / theme: pandoc) also skip the title block — falling through to the bare Fragment makes that automatic. The `AstProps` shape mirrors the `Ast` key's contract (Phase 7.2 §Prop shape).

#### `q2-preview/custom/PreviewTitleBlock.tsx` — title block (NEW, Phase 7)

Lives in `custom/` alongside every other CustomNode component (`Callout`, `Theorem`, `Proof`, `FloatRefTarget`, `Equation`, `CrossrefResolvedRef`, `Fallback`); verified 2026-05-10 that `q2-preview/custom/` is the universal home for type-keyed registry entries. Mirrors `template.rs:211-240` byte-for-byte. Reads from `ast.meta` only — no Rust-side typed fields, consistent with Phase 6's Option B. Source (annotated against the Rust template):

```tsx
import type { AstProps } from '../../framework';
import {
    extractMetaString,
    extractMetaStringList,
} from '../../framework';

/**
 * Built-in `__title_block__` synthetic-registry entry.
 *
 * Prop shape — receives `AstProps` (`{ ast, onNavigateToDocument, setAst }`),
 * the same shape registered under the `Ast` key. NOT the `NodeArgs<…>`
 * shape used by per-tag entries (`Block` / `Inline` / `Para` / …).
 * Rationale: the title block operates on document-level state
 * (`ast.meta`), not on a node in the AST. `FormatRegistry` at
 * `framework/types.ts` types `__title_block__?: AstComponent`, so a
 * user TSX override that destructures `{ meta }` or `{ node }` by
 * reflex will fail to compile (Phase 7.3 type tightening).
 *
 * The built-in reads `ast.meta` and ignores `setAst` /
 * `onNavigateToDocument`. A user override that wants editable title
 * blocks can call `setAst`; one that wants click-to-navigate behavior
 * on the title can call `onNavigateToDocument`.
 *
 * To compose this component from a user TSX override, import it
 * from `window.__Q2_PREVIEW_RENDERER__.PreviewTitleBlock` (exposed
 * by Phase 7.3.1) and render it alongside your own extensions.
 */
export const PreviewTitleBlock = ({ ast }: AstProps) => {
    const meta = ast.meta ?? {};

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
- **`format: revealjs` rendering**. Documents with `format: revealjs` route to the slide renderers (`SlideAst` / `RevealjsSlideAst`) via `ReactRenderer.tsx`'s format-dispatch and never reach `Q2PreviewIframe`/`PreviewDocument`. q2-preview therefore needs no revealjs-specific branch; if routing ever changes upstream, the plan's wrapper would emit benign-but-wrong-for-slides chrome and the fix would be a one-line skip — but that's a hypothetical, not a 2D concern.
- **Slide-deck-style chrome** (slide indicator, slide nav). RevealJS-only; not in scope here.
- **`<head>` meta tags driven by metadata** (`<meta name="author">`, `<meta name="dcterms.date">`, `<meta name="keywords">`, `<meta name="description">`, `<link rel="canonical">`, generator meta). The iframe `<head>` is owned by `hub-client/public/q2-preview.html` + `entry.tsx`'s theme/title injection. Adding metadata-driven `<head>` tags is its own wiring (a postMessage payload extension, or a static read at boot) — defer to a follow-up. Visible chrome and theme CSS targeting in `<body>` are unaffected.
- **`<title>` (browser tab title) sourced from `pagetitle`/`title`** — DEFERRED, NOT ALREADY DONE. Verified 2026-05-10: the iframe's `<title>` is the static literal `q2-preview Renderer (Sandboxed)` at `q2-preview.html:6`; nothing in `entry.tsx` or any q2-preview component writes `document.title` (only `Editor.tsx:348-350` in the parent window writes a tab title, and it uses project filename + description, not the doc's `title`/`pagetitle`). An earlier draft of this plan claimed `entry.tsx` already wired this — that claim was wrong. Wiring the iframe tab title from `meta.pagetitle` / `meta.title` is a follow-up: a one-line `useEffect` inside `PreviewDocument` would do it (`document.title = extractMetaString(meta.pagetitle) ?? extractMetaString(meta.title) ?? 'q2-preview Renderer';` with cleanup-on-unmount). Whether to bundle that into 2D or defer is **Open design question §1** below.
- **Block-rendered (not stringified) abstracts**. v1 stringifies `MetaBlocks` abstracts via `blocksToPlainText` to match Rust's `blocks_to_text` (template.rs:610-614). Real block rendering (paragraphs, lists, etc.) inside `<div class="abstract">` is deferred — when Rust upgrades, q2-preview's component switches to `<Node>` walks over `meta.abstract`'s blocks. Class taxonomy stays the same.
- **Inline-markup-preserving title rendering**. Rust strips emphasis from titles today (`config_value_to_template_value` → `inlines_to_text`); q2-preview matches. When the Rust template grows an HTML-emit path for title inlines, q2-preview switches `<h1 class="title">` to render via `<InlineNode>` walks. Until then, italic/bold/code in titles are stripped on both sides.
- **i18n of "Author" / "Published" / "Abstract" labels**. Hardcoded literals match Rust; both flip together when the Rust template grows i18n. **Research summary (verified 2026-05-10 against TS Quarto sources at `/Users/gordon/src/quarto-cli/`)**:
  - **Data model in TS Quarto**: YAML files at `src/resources/language/_language[-<locale>].yml` carry the keys `title-block-author-single`, `title-block-author-plural`, `title-block-published`, `section-title-abstract` (`_language.yml:30-34, 18`). A Lua filter `src/resources/filters/modules/authors.lua:852-906` writes them into `meta.labels.{authors,published,abstract,...}`. The HTML title-block partial reads `$labels.authors$` / `$labels.published$` / `$labels.abstract$` (`src/resources/formats/html/templates/title-metadata.html`).
  - **Active locale resolution in TS Quarto**: `lang` frontmatter (IETF tag) → optional `language:` key pointing at a YAML file or inline overrides → `_language.yml` (English) as fallback.
  - **Rust q2 status**: no language-resolution stage. `crates/quarto-core/src/template.rs:222/227/235` uses hardcoded English literals. The `lang` template variable IS plumbed but only for `<html lang="…">` (`template.rs:81/141`). No `_language*.yml` files have been ported into `resources/`. The schema declares the language-map shape (`crates/pampa/test-fixtures/schemas/definitions.yml:1321`) but nothing reads it.
  - **Why deferred from 2D**: making i18n work end-to-end requires a Rust-side `LanguageResolveStage` (port the YAMLs into `resources/`, resolve locale from `meta.lang`, write `meta.labels.*`) plus flipping `template.rs` literals to `$labels.*$` references. That's ~400 LOC + YAML data + a stage-ordering decision (must run before `MetadataMergeStage` and before `DocumentProfileStage`). Out of scope for 2D; should become its own plan whenever someone needs translated labels. **For future implementer**: when this lands, q2-preview's `PreviewTitleBlock` switches the three hardcoded label literals to `extractMetaString(meta.labels?.author) ?? 'Author'` (with English fallback) — single-commit change once Rust has produced the data.
- **Multi-author rendering UX**. Rust today emits exactly one `<div class="quarto-title-meta-author">` with all names concatenated as one string (`AliceBob`, no separator) — the doctemplate engine stringifies `TemplateValue::List` as the empty-join. q2-preview matches verbatim (single block, empty-string-joined names). When Rust grows proper multi-author support (structured author objects, separator policy, possibly an "Authors" plural heading), q2-preview mirrors in the same plan — no per-format CSS fork needed in the meantime.
- **Re-enabling the Rust `title-block` transform for minimal mode only**. 2D re-implements the transform's minimal-mode branch (`transforms/title_block.rs:54-110`) on the React side via Phase 6.2's `<h1>` synthesis. This is a deliberate divergence with a known structural cost (see §Risk areas "Minimal-mode section structure divergence"): the React-side `<h1>` is a sibling of the body's section wrappers rather than nested inside `<section level1>` like Rust's pre-section-structure insertion. The cleaner long-term fix is to **un-exclude `"title-block"` from `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1052`) but configure it to short-circuit in full-template mode** (where q2-preview's React `PreviewTitleBlock` emits the chrome) and run only in minimal mode (where it pre-pends the synthetic header to `ast.blocks` before section-structure). That keeps React owning the full-template path (Bootstrap parity, theme-CSS targeting) and gives Rust ownership of the minimal-mode AST shape (so section-structure wraps correctly). Out of scope for 2D — would require touching `Q2_PREVIEW_TRANSFORM_EXCLUDED`, validating that the transform's full-template short-circuit interacts cleanly with React's chrome (no double-h1), and removing Phase 6.2's React-side synthesis. Worth filing as a follow-up beads issue if the section-nesting divergence ever bites.

### Defensive variants

- **Missing `ast.meta`**: `ast.meta ?? {}` defaults to empty object; all `extractMeta*` helpers return undefined; defaults kick in for every key. The wrapper still renders with `page-layout-article` and `body.fullcontent`, and `PreviewTitleBlock` returns `null` (no title block).
- **Non-string `page-layout` value** (e.g. user wrote `page-layout: 42` accidentally): `extractMetaString` returns undefined for `MetaBool` / `MetaList` / `MetaMap`; fallback to `article`. No type coercion.
- **Multi-token `page-layout` value** (e.g. `page-layout: "full grid"`): `extractMetaString` returns the literal string verbatim; React interpolates it into `className=\`… page-layout-${value}\``, producing `page-layout-full grid` — i.e. two CSS classes (`page-layout-full` and `grid`). Matches Rust's `template.rs:185` substitution (`page-layout-$page-layout$` does the same verbatim insert). React escapes attribute values so injection is bounded to class-attr scope, but users can still produce arbitrary class lists. Same parity caveat applies as for `body-classes`: the wrapper passes the value through; theme CSS owns interpretation. Not validated against an enum.
- **List-form `body-classes`** (e.g. `body-classes: [foo, bar]`): `extractMetaString` returns `undefined` for `MetaList` → fallback to literal `fullcontent`. **Intentional divergence from Rust** (which would render `TemplateValue::List` via empty-string-join as `body class="foobar"` — one nonsensical class). Rust's pipeline never produces list-form body-classes today (`sidebar_render.rs:92` writes `format!()` string scalars; user-override path also stays string-shaped), so the list-form is an undocumented Rust quirk rather than a tested path. Falling back to a known-good sentinel (`fullcontent`) is more useful than empty-joining; when Rust adopts a proper list-of-classes contract (probably space-join, the CSS convention), q2-preview flips to `extractMetaStringList(meta['body-classes']).join(' ')` in the same plan that fixes Rust.
- **Whitespace-only `body-classes`** (e.g. `body-classes: " "`): the helper returns the whitespace string verbatim; `document.body.className = " "` produces a body with no classes (browser ignores whitespace-only class). Matches Rust template behavior.
- **Empty-string `body-classes`** (e.g. `body-classes: ""`): the helper returns `""`. Since `"" ?? 'fullcontent'` keeps the empty string (only `undefined` triggers the fallback), this produces a body with NO classes — distinct from "no override → fullcontent". **Resolved (Open design question §5, option A)**: empty string is the user's opt-out from `fullcontent` and matches Rust template behavior (`template.rs:177` `$if(body-classes)$$body-classes$$else$fullcontent$endif$` emits the empty string verbatim under Pandoc's truthy semantics). Locked by a vitest case in §Test plan ("body-classes: '' → document.body.className === ''").
- **Empty-string title-block fields** (`title: ""`, `subtitle: ""`, `author: ""`, `date: ""`, `abstract: ""`): each is treated **as falsy** — its element is suppressed exactly as if the key were missing. This mirrors Pandoc's `$if(x)$` semantics (which is what every `if (x ? <node> : null)` in `PreviewTitleBlock.tsx` is encoding). The behavior is **locked individually per field** in `PreviewTitleBlock.test.tsx`'s "Pandoc-falsy semantics" block (Phase 7.4), so a future switch from `if (x)` to `if (x !== undefined)` breaks the build instead of silently emitting empty `<p class="subtitle"></p>` elements. **Note the asymmetry with `body-classes`**: body-classes treats `""` as opt-out (preserves the empty value), title-block fields treat `""` as missing (suppress the optional element). Both match the Rust template — `body-classes` renders via `$body-classes$` (substitution: `""` → `""`); title-block fields gate on `$if(x)$` (truthy check: `""` is falsy in Pandoc).

## Design decisions

- **Option B (read `ast.meta` in JS), not Option A (Rust-side typed field).** The HTML pipeline reads `page-layout` directly from `ast.meta` at template-render time (`template.rs:415`); q2-preview matches that pattern. Existing precedent: `ReactRenderer.tsx:211` (format detection) and `ReactAstSlideRenderer.tsx:220-221` (slide title/author) already read `ast.meta` directly. Adds zero Rust / WASM-bridge plumbing for v1. **The Option A pattern is reserved for transform-computed values** (sidebar body-classes, future stage outputs) — when those land, `RenderResponse` grows a typed field and q2-preview's resolver layers it before the `ast.meta` read.
- **Body-class application via `useEffect`, not module-scope imperative.** `applyTheme` at `entry.tsx:116-131` lives at module scope because it has to fire before any React mount (the parent posts theme + AST from sibling `useEffect`s on the same `iframeReady` transition; if theme posts first the message would be dropped). Body-classes have no such race — they read from the AST that's already mounted, so the React commit fires before any user can observe a class mismatch. Keeps document-level concerns inside `PreviewDocument` rather than spreading across `entry.tsx` + the wrapper component.
- **Wrapper structure mirrors `template.rs` byte-for-byte.** Same element + class list + nesting. The justification is the same as Plan 2C's "Visual + structural parity target": Bootstrap-flavored theme CSS targets the wrapper's selectors, and any divergence forces a per-format CSS fork. The cost of strict mirroring is low — three classes + an id + an inner `<main>` — and the payoff is "load Quarto's compiled theme CSS and it just works".
- **`minimal` / `theme: none` / `theme: pandoc` skip the wrapper.** Mirrors `is_minimal_html()` at `template.rs:501`. The minimal HTML template at `template.rs:80-111` omits `#quarto-content` and `<main>` entirely; q2-preview's PreviewDocument matches by returning the bare Fragment. If a user opts into `minimal: true`, they expect no chrome — q2-preview shouldn't add any.
- **`extractMetaString` lifted to `framework/meta.ts`, alongside the plain-text walks in `framework/plainText.ts`.** The slide renderer's private `extractMetaString` (and q2-preview's pre-2D plan to add another) collapse to a single framework helper. The walks were previously in `q2-preview/utils.tsx` (q2-preview-only by accident — they have no format-specific behavior); they move to `framework/plainText.ts` so `framework/meta.ts` can call them without crossing format boundaries. Phase 6.0 captures the migration; Phases 6.1+ consume the helpers directly. See §"Open design questions §2" for why this beat the original `hub-client/src/utils/meta.ts` placement.
- **Cleanup on `useEffect` unmount.** Test re-mounts (vitest, Playwright) need a clean body class slate; restoring the previous `document.body.className` on unmount keeps the suite hermetic. Production iframe unmounts only on document switch — same restore is benign there.

## Open design questions — all RESOLVED

The points below surfaced when the plan was cross-checked against the actual sources on 2026-05-10. Each is recorded with the chosen resolution so a reader can reconstruct *why* the plan looks the way it does without re-deriving the trade-off.

### §1. Iframe tab title — RESOLVED: bundle into 2D (option A)

**Background.** A previous draft of this plan asserted that `entry.tsx` already sets the iframe `<title>` from the AST. Verified 2026-05-10: false. The iframe title is the static literal at `q2-preview.html:6`. The iframe's tab title is normally not user-visible (the parent's tab title wins; that's set by `Editor.tsx:348-350` from project metadata) but IS visible in screen readers, in DevTools, and if the iframe is ever popped out / loaded directly.

**Resolution:** Phase 6.2a adds a 3-LOC `useEffect` inside `PreviewDocument` that reads `extractMetaString(meta.pagetitle) ?? extractMetaString(meta.title) ?? null` and writes `document.title`, restoring the previous value on unmount. Bundled into 2D rather than deferred to a separate plan; the cost is too small to justify a follow-up cycle.

### §2. `meta.ts` location — RESOLVED: `framework/meta.ts` (option D, derived during structural review)

**Background.** `extractMetaString`'s `MetaInlines` / `MetaBlocks` branches need `inlinesToPlainText` / `blocksToPlainText`. Those walks today live at `q2-preview/utils.tsx:69` / `:134` even though they are pure Pandoc-AST utilities with no q2-preview-specific behavior. `hub-client/src/utils/` does not currently reach into `components/render/` — every existing file in `utils/` imports only React or stdlib types.

The originally-listed options ((A) co-locate in `q2-preview/utils.tsx`, (B) lift types out of framework, (C) duplicate the walks into `meta.ts`) all left some asymmetry on the table. A fourth option emerged from the structural review on 2026-05-10:

**Resolution: lift the plain-text walks AND the meta helpers into `framework/`**. They are framework-tier concerns (Pandoc-AST shape, no format opinions); putting them next to the types they walk eliminates the layering question entirely. q2-preview's `utils.tsx` slims to format-specific helpers (asset URL lookup, slot factories, theorem-label formatting). Bonus: q2-debug, q2-preview, and the slide renderer all import from the same place; the duplicate `meta.format` checks at `ReactRenderer.tsx:211` and `getQ2Format.ts` collapse to one helper call. Captured as Phase 6.0 in the checklist above.

### §3. Slide-renderer regression test — RESOLVED: option A (add the test, in 6.0d)

The `extractMetaString` lift is no longer just a "lift" — it's the deliberate consolidation in Phase 6.0d. The vitest case (`MetaInlines [Str("Hello"), Space, Emph([Str("world")])]` → `"Hello world"`) lives next to `ReactAstSlideRenderer.tsx` and locks the new walk's behavior. The behavior change is documented in the commit message for 6.0d; the test prevents silent regression on future edits to the framework walks.

### §4. `__title_block__` test target — RESOLVED: option A (move under `./custom/`)

`PreviewTitleBlock` lives at `q2-preview/custom/PreviewTitleBlock.tsx` (parallel to `Custom.Fallback`). `registry.ts:34`'s `__title_block__: Custom.PreviewTitleBlock` mirrors the existing `__fallback__: Custom.Fallback` line. Test wiring extends the existing "every expected CustomNode component is exported from ./custom" check (`registry.test.ts:80-93`) to include `'PreviewTitleBlock'`. No new test category; pattern-consistent with how `Fallback` is wired today. Captured in Phases 7.2 and 7.3.

### §5. Empty-string `body-classes` — RESOLVED: option A (opt-out)

`body-classes: ""` produces `document.body.className = ''`, matching the Rust template at `template.rs:177` (Pandoc's `$if$` treats empty string as truthy and emits it verbatim). Locked by a vitest case in §Test plan.

### §6. `__title_block__` prop shape — RESOLVED: `AstProps` (parallel to `Ast`)

**Background.** Verified 2026-05-10 against `dispatchers.tsx:38-93`, `framework/dispatch.tsx:340-429`, and `Ast.tsx:84-88`: every non-synthetic registry entry today receives `NodeArgs<…>` (Pandoc tags via `<Block>` / `<Inline>`; CustomNode `type_name`s via `<CustomBlock>` / `<CustomInline>`). The one existing synthetic key `__fallback__` also receives `NodeArgs<…>` because the CustomBlock/CustomInline dispatchers forward their own `args`. Only `Ast` differs — it receives `AstProps` (`{ ast, onNavigateToDocument, setAst }`).

An earlier draft of Plan 2D proposed mounting `<TitleBlock meta={meta} />` — a third synthetic prop shape. That would have introduced a heterogeneous synthetic-key convention with no compile-time check (`FormatRegistry` types every entry as `(props: any) => ReactNode`).

**Resolution: mount `__title_block__` with `AstProps`** — the same shape registered under the `Ast` key. The title block operates on document-level state (`ast.meta`), not on a node in the AST; treating it parallel to `Ast` is honest about the design and collapses three potential synthetic-key shapes to two:
- `Ast` and `__title_block__` → document-level via `AstProps`.
- `__fallback__` → node-level via `NodeArgs<CustomBlockNode | CustomInlineNode>`.

The built-in `PreviewTitleBlock` reads `ast.meta` and ignores `setAst` / `onNavigateToDocument`. A user override that wants editable titles can call `setAst`; one that wants click-to-navigate on the title can call `onNavigateToDocument`. Captured in Phase 7.2 and Phase 7.3.

### §7. `FormatRegistry` type tightening for synthetic keys — RESOLVED: typed optional entries

**Background.** `FormatRegistry` at `framework/types.ts:163` is `Record<string, (props: any) => React.ReactNode> & { Ast, Block, Inline }`. The intersection types only the three reserved framework keys; everything else (Pandoc tags, CustomNode `type_name`s, `__fallback__`, future `__title_block__`) falls through the `Record<string, (props: any) => ReactNode>` index signature and is unchecked. A user TSX file registering `__title_block__: ({ meta }) => …` vs `__title_block__: ({ ast }) => …` would be silently accepted at compile time; only a runtime render reveals the mismatch.

**Resolution: declare typed optional entries for the two known synthetic keys** alongside the existing reserved keys:

```ts
export type FormatRegistry = Record<string, (props: any) => React.ReactNode> & {
    Ast: AstComponent;
    Block: DispatcherComponent;
    Inline: DispatcherComponent;
    __fallback__?: (args: NodeArgs<CustomBlockNode | CustomInlineNode>) => React.ReactNode;
    __title_block__?: AstComponent;
};
```

Optional (`?`) because not every format must register them — the runtime `??` fallback in dispatchers / PreviewDocument covers the missing case. Pandoc tag keys and CustomNode `type_name` keys keep their loose `(props: any)` typing via the index signature, preserving the existing namespace flexibility (closes the synthetic-key hole without closing the user-tag hole, which would require a much larger type-system change). Lands in `framework/types.ts:163` alongside the rest of the registry contract; captured in Phase 7.3.

### §8. Extension hook for `__title_block__` — RESOLVED: composition via `__Q2_PREVIEW_RENDERER__` (option b)

**Background.** With §6/§7 in place, a user override of `__title_block__` **fully replaces** the built-in. There's no slot/wrap hook. Adding a single field (DOI, license, download button) means re-implementing the entire `<header id="title-block-header">` subtree and tracking Rust template structural changes by hand.

**Resolution: expose `PreviewTitleBlock` on `window.__Q2_PREVIEW_RENDERER__`** so user overrides can compose it instead of replacing it. Same exposure mechanism as Phase 6.0c.1 (which adds five framework helpers); `PreviewTitleBlock` joins as the user-composable building block for the title-block chrome.

```tsx
const { PreviewTitleBlock, extractMetaString } = window.__Q2_PREVIEW_RENDERER__;

export const __title_block__ = ({ ast, setAst, onNavigateToDocument }) => (
    <>
        <PreviewTitleBlock ast={ast} setAst={setAst} onNavigateToDocument={onNavigateToDocument} />
        <div className="doi">DOI: {extractMetaString(ast.meta.doi)}</div>
    </>
);
```

Rejected alternatives:
- **Slot keys** (`__title_block_before__`, `__title_block_after__`, …) proliferate registry surface and encode opinions about extension points that Rust has no analogue for.
- **In-component plugin hooks** (`<PreviewTitleBlock onAfterAuthor={…} />`) lock in extension surface harder to evolve than plain React composition.
- **Higher-order component wrapping** (`withExtensions(PreviewTitleBlock)`) requires the user to learn an extension API instead of React's native composition.

The `__Q2_PREVIEW_RENDERER__` exposure approach is the React-composition idiom; adds one line to an already-exposed global; gives the user full flexibility (compose before, after, around, or replace entirely). Captured as Phase 7.3.1 in the checklist; locked by a "composing the default" vitest case in Phase 7.4.

## Multi-plan contracts

### Consumed: Plan 2B / 2C (PreviewDocument + registry both shipped)

`PreviewDocument.tsx` ships in Plan 2B as the registry's `Ast` entry; Plan 2C extends `previewRegistry` with the unified Pandoc-tag / CustomNode dispatcher pattern, the `__fallback__` synthetic key, and the namespace-disjoint policy locked at `registry.test.ts`. Both have landed on `feature/q2-preview` (the q2-preview-work branch is current as of 2026-05-10).

Plan 2D extends `PreviewDocument.tsx` with the wrapper + minimal-mode title synthesis (Phase 6) and adds `PreviewTitleBlock` registered under `'__title_block__'` (Phase 7). The synthetic-key precedent is already established by 2C's `__fallback__`. The PreviewDocument prop shape stays at `{ ast, setAst, onNavigateToDocument }` (matching the framework `AstProps` at `framework/types.ts:155-159`); 2D does NOT migrate to `NodeArgs<PandocAST>`.

### Provided: foundation for sidebar / navbar / footer plans

When the future sidebar plan lands (separate from Plan 2E, which is the q2-slides migration), the `<div id="quarto-content">` wrapper is the slot the sidebar `<nav>` and `<div id="quarto-margin-sidebar">` siblings live alongside `<main class="content">`. The wrapper structure assumed by 2D is exactly the structure those follow-up plans will extend, so they add elements without restructuring the wrapper.

When the typed-field-on-`RenderResponse` pattern is used for transform-computed body-classes (the future sidebar plan, or later), q2-preview's resolver layers the typed field above the `ast.meta` read:

```ts
// Future shape (sidebar plan):
const bodyClasses =
    props.bodyClassesOverride ??                            // sidebar plan: typed field on RenderResponse
    extractMetaString(meta['body-classes']) ??              // Plan 2D: user override
    'fullcontent';                                          // Plan 2D: literal default
```

Mirrors `template.rs:419-429` precedence.

### Soft activation dependencies

None for 2D. The wrapper's structure is fully determined by `ast.meta` reads, and `ast.meta` is already plumbed end-to-end (Plan 2A's WASM bridge, Plan 2B's iframe entry, no changes needed).

## Test plan

### Test-tier conventions

Same tiers as Plans 2B / 2C: vitest unit / vitest integration / smoke-all WASM / Playwright e2e. The project-context coverage rule does **not** apply to 2D — the body container is computed entirely from `ast.meta`. Project-level cascade values (e.g. `body-classes` set in `_quarto.yml`) are merged into per-doc `ast.meta` upstream by the Rust pipeline's `MetadataMergeStage` (long-shipped; `crates/quarto-core/src/stage/stages/metadata_merge.rs`), so by the time q2-preview's `PreviewDocument` reads `ast.meta`, the project and single-doc paths are equivalent. Single-doc fixtures are sufficient.

### Vitest unit tests (`framework/meta.test.ts` and `framework/plainText.test.ts`)

NEW files at `hub-client/src/components/render/framework/meta.test.ts` and `framework/plainText.test.ts`:

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
- Cleanup: mount → unmount → assert `document.body.className` is restored to its pre-mount value.

### Vitest snapshot tests (`q2-preview/custom/PreviewTitleBlock.test.tsx`) — Phase 7

NEW file at `hub-client/src/components/render/q2-preview/custom/PreviewTitleBlock.test.tsx`:

- No title (meta empty) → `render` returns `null`. No `<header>` in the DOM.
- Title only → `<header id="title-block-header" class="quarto-title-block default">` with `<h1 class="title">` inside `<div class="quarto-title">`. Asserts no `<p class="subtitle">`, no `<div class="quarto-title-meta">`, no `<div class="abstract">`.
- Title + subtitle → adds `<p class="subtitle">`.
- Title + author (single MetaString) → exactly one `<div class="quarto-title-meta-author">` inside `<div class="quarto-title-meta">`.
- Title + author (MetaList of two) → exactly one `<div class="quarto-title-meta-author">` whose `.quarto-title-meta-contents` text is the empty-string-joined names (matches Rust).
- Title + author + date → `<div class="quarto-title-meta-date">` rendered inside the meta wrapper, with heading "Published".
- Title + date but NO author → date is NOT rendered (locks the Rust quirk; explicit regression test for the deliberate divergence).
- Title + abstract → `<div class="abstract">` with `<div class="abstract-title">Abstract</div>` and the abstract text.
- Title with inline emphasis (MetaInlines `[Str("Hello"), Space, Emph([Str("World")])]`) → `<h1 class="title">Hello World</h1>` (emphasis stripped, matches Rust).
- User override via `__title_block__` registry key — **full replacement** → stub component (receives `AstProps`, ignores them: `() => <div data-testid="custom-title">x</div>`) is mounted in place of `PreviewTitleBlock`, and the built-in `<header id="title-block-header">` is NOT in the DOM.
- User override via `__title_block__` registry key — **composing the default** → stub that calls `window.__Q2_PREVIEW_RENDERER__.PreviewTitleBlock` and emits a sibling. Asserts BOTH the built-in `<header id="title-block-header">` AND `data-testid="extra"` are present. Locks the §8 composition idiom: the `__Q2_PREVIEW_RENDERER__.PreviewTitleBlock` exposure is load-bearing for user extensions.
- **Pandoc-falsy semantics for every optional field** — empty string is treated identically to missing-key on the title-block side, mirroring Pandoc template `$if(x)$`. Each case is its own regression test so a future refactor that switches `if (x)` to `if (x !== undefined)` (or similar) breaks the build:
  - Empty-string title (`title: ""` parses as `MetaString("")`) → renders `null`, no `<header>` element.
  - Title set + empty-string subtitle → `<p class="subtitle">` is NOT rendered.
  - Title set + empty-string author → `<div class="quarto-title-meta">` is NOT rendered (matches Rust template's `$if(author)$` gate; an empty author string is treated as no author).
  - Title + author + empty-string date → date sub-block is NOT rendered (separate from the Rust quirk above; this case verifies that empty-string date is also suppressed regardless of whether author is set).
  - Title + empty-string abstract → `<div class="abstract">` is NOT rendered.
  - Title + author = MetaList of `["Alice", ""]` → exactly one `<div class="quarto-title-meta-author">` whose `.quarto-title-meta-contents` text is `"Alice"`. Empty-string entries are **kept** in the list (`extractMetaStringList` filters only `undefined`, and `extractMetaString("")` returns `""` which is defined), then empty-string-joined per `Array.prototype.join('')` — the empty entries contribute nothing to the joined string. Matches Rust's `TemplateValue::List` empty-string-join exactly: `["Alice", ""]` → `"Alice"`.

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
- **`q2-preview/body-classes-override.qmd`**: `body-classes: custom-cls`. Positives: `['body.custom-cls']`. Negatives: `['body.fullcontent']` (locks the override-replaces-default precedence).
- **`q2-preview/body-classes-full-layout-combo.qmd`**: `body-classes: custom-cls` AND `page-layout: full` together. Positives: `['div.page-layout-full', 'body.custom-cls']`. Negatives: `['body.fullcontent', 'div.page-layout-article']`. Locks the two-knobs-together case so a future refactor that conflates the body-classes useEffect with the page-layout className doesn't slip past per-knob fixtures.
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
- **Minimal-mode section structure divergence (known, minor)**. In Rust minimal mode the synthetic title Header is added BEFORE the section-structure transform runs, so it ends up wrapped in a `<section level1>` along with following content. In q2-preview the title-block transform is excluded entirely; the React-side synthesis emits `<h1>{title}</h1>` AFTER section-structure has run, so the `<h1>` is a sibling of (not nested inside) the body's section wrappers. Visual difference is normally invisible — minimal-mode CSS doesn't depend on the section nesting — but worth flagging if a regression points at minimal-mode title positioning. Mitigation: the smoke fixture asserts the `<h1>` is present without pinning section nesting.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `hub-client/src/components/render/framework/plainText.ts` (NEW — moved from `q2-preview/utils.tsx`) | ~110 |
| `hub-client/src/components/render/framework/meta.ts` (NEW — `extractMetaString` + `extractMetaBool` + `extractMetaStringList`) | ~50 |
| `framework/index.ts` re-export updates (`meta`, `plainText`, `customNode` bookkeeping fix) | ~5 |
| `framework/types.ts:163` `FormatRegistry` tightening — typed optional `__fallback__` and `__title_block__` entries (Phase 7.3) | ~6 |
| `q2-preview/utils.tsx` slim-down (remove the moved walks) + import updates in `Image.tsx` and `Note.tsx` | ~15 |
| `ReactAstSlideRenderer.tsx` migration to `framework/meta` (replaces private `extractMetaString`; behavior change for inline-markup titles, regression test added) | ~15 |
| `ReactRenderer.tsx:211` + `getQ2Format.ts` consolidation onto `extractMetaString(meta.format)` | ~10 |
| `q2-preview/PreviewDocument.tsx` extension (wrapper + useEffect + title-block mount) | ~32 |
| `q2-preview/PreviewTitleBlock.tsx` (NEW — title block, `AstProps` shape) | ~70 |
| `q2-preview/entry.tsx` — `__Q2_PREVIEW_RENDERER__` exposure (5 framework helpers via 6.0c.1; `PreviewTitleBlock` via 7.3.1) | ~7 |
| Slide-renderer import update (`ReactAstSlideRenderer.tsx:350` lifted to `framework/meta`) | ~3 |
| `framework/meta.test.ts` (NEW) — unit tests for all three helpers | ~70 |
| `q2-preview/PreviewDocument.test.tsx` extension — wrapper snapshot tests | ~80 |
| `q2-preview/custom/PreviewTitleBlock.test.tsx` (NEW) — title-block snapshot tests (incl. composition case) | ~95 |
| Smoke-all q2-preview fixtures (4 body-container + 2 body-classes + 5 title-block, all single-doc) | ~110 |
| **Total** | **~525** |

Still comfortable for a focused session. About double the original 2D scope; the title-block work is small but multiplied by the number of conditional branches. **Phase 7 depends on Phase 6**: `<PreviewTitleBlock>` mounts inside Phase 6's `<main class="content">`, and the "skip the wrapper" logic (minimal / theme: none / theme: pandoc) is what makes the title block also disappear in those modes. Phase 6 must land first.

**Sub-ordering**:
1. **Phase 6.0** — `framework/plainText.ts` extraction, `framework/meta.ts` creation, `framework/index.ts` updates, slide-renderer migration, `meta.format` consolidation, unit tests. Six small commits per the 6.0a–f checklist; each leaves the tree green.
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
- `extractMetaString` already exists in the slide renderer (`ReactAstSlideRenderer.tsx`, top-level under `components/render/`, NOT inside `q2-debug/`); the lift relocates it to `framework/meta.ts`. Once it's at `framework/`, future format renderers (e.g. the q2-slides migration tracked in Plan 2E, docx/PDF preview hypothetically) get the helper for free without re-importing from a sibling format.
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

- `hub-client/src/components/render/framework/meta.ts` (NEW) — `extractMetaString`, `extractMetaBool`, `extractMetaStringList`.
- `hub-client/src/components/render/framework/meta.test.ts` (NEW) — unit tests for all three helpers.
- `hub-client/src/components/render/framework/plainText.ts` (NEW; moved verbatim from `q2-preview/utils.tsx:69-170`) — `inlinesToPlainText`, `blocksToPlainText`.
- `hub-client/src/components/render/framework/plainText.test.ts` (NEW) — unit tests for the walks.
- `hub-client/src/components/render/framework/index.ts` — re-exports for `meta`, `plainText`, `customNode` (bookkeeping fix).
- `hub-client/src/components/render/q2-preview/utils.tsx` — DELETE the `inlinesToPlainText` / `blocksToPlainText` definitions and their internal `inlineText` / `blockText` helpers; the file now contains only format-specific helpers.
- `hub-client/src/components/render/q2-preview/inlines/Image.tsx:4` — import path updates from `../utils` to `../../framework`.
- `hub-client/src/components/render/q2-preview/inlines/Note.tsx:5` — import path updates from `../utils` to `../../framework`.
- `hub-client/src/components/render/ReactRenderer.tsx:211` — replace inline `MetaString`-only check with `extractMetaString(ast?.meta?.format) === 'revealjs'`.
- `hub-client/src/components/render/getQ2Format.ts` — replace body with `extractMetaString(ast?.meta?.format)` + the existing `q2-` / `revealjs` filter.
- `hub-client/src/components/render/q2-preview/PreviewDocument.tsx` — extend with body-container wrapper + useEffect; mount `<PreviewTitleBlock>` inside `<main>`.
- `hub-client/src/components/render/q2-preview/PreviewDocument.test.tsx` (extend if exists, NEW otherwise) — wrapper snapshot tests.
- `hub-client/src/components/render/q2-preview/custom/PreviewTitleBlock.tsx` (NEW) — title-block component (Phase 7); lives in `custom/` next to every other CustomNode-component for q2-preview.
- `hub-client/src/components/render/q2-preview/custom/PreviewTitleBlock.test.tsx` (NEW) — title-block snapshot tests (Phase 7).
- `hub-client/src/components/render/q2-preview/custom/index.ts` — add `export { PreviewTitleBlock } from './PreviewTitleBlock';` next to the existing CustomNode exports.
- `hub-client/src/components/render/ReactAstSlideRenderer.tsx:350` — remove the local `extractMetaString` definition; update `:220-221` to use `import { extractMetaString } from './framework'`. Behavior change for inline-markup titles is locked by a regression test.
- `crates/quarto/tests/smoke-all/q2-preview/body-container-{default,full-layout,minimal,minimal-title}.qmd` (NEW) — body-container smoke fixtures (Phase 6).
- `crates/quarto/tests/smoke-all/q2-preview/body-classes-{override,full-layout-combo}.qmd` (NEW) — body-classes smoke fixtures (Phase 6).
- `crates/quarto/tests/smoke-all/q2-preview/title-block-{default,full,no-title,multi-author,date-no-author}.qmd` (NEW) — title-block smoke fixtures (Phase 7).
- `hub-client/src/components/render/q2-preview/registry.ts` — add `__title_block__: Custom.PreviewTitleBlock` next to `__fallback__: Custom.Fallback` (line 34). Also add a one-line doc-comment above the synthetic-key block clarifying that synthetic keys (`__fallback__`, `__title_block__`) carry component-specific prop shapes (NodeArgs vs `{ meta }`), distinct from the per-tag `Block`/`Inline` dispatcher contract — closes the user-override-DX gap raised in the §Code review addendum (item 1).
- `hub-client/src/components/render/q2-preview/entry.tsx` — extend `window.__Q2_PREVIEW_RENDERER__` (currently `entry.tsx:56-64`) with `extractMetaString`, `extractMetaBool`, `extractMetaStringList`, `inlinesToPlainText`, `blocksToPlainText` (Phase 6.0c.1) — so user TSX overrides of `__title_block__` can coerce `meta` values without re-implementing the framework walks. Phase 7.3.1 adds `PreviewTitleBlock` to the same global so user overrides can compose the built-in instead of fully replacing it.
- `hub-client/src/components/render/framework/types.ts:163` — extend `FormatRegistry` with typed optional entries for the two known synthetic keys: `__fallback__?: (args: NodeArgs<CustomBlockNode | CustomInlineNode>) => React.ReactNode` and `__title_block__?: AstComponent` (Phase 7.3). Closes the synthetic-key prop-shape hole; user TSX overrides get compile-time prop-shape checking on both synthetic keys. Pandoc-tag / CustomNode-`type_name` keys keep their loose `(props: any)` typing via the index signature unchanged.
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

## End-to-end verification record (Phase 8.1, 2026-05-10)

**Invocation:**

```bash
# From hub-client/
npx playwright test smoke-all \
  -g "q2-preview/(body-container|body-classes|title-block)" \
  --reporter=line
```

**Result:** `11 passed (10.2s)`. All 11 Plan 2D smoke fixtures green:

- `body-classes-full-layout-combo.qmd`
- `body-classes-override.qmd`
- `body-container-default.qmd`
- `body-container-full-layout.qmd`
- `body-container-minimal.qmd`
- `body-container-minimal-title.qmd`
- `title-block-date-no-author.qmd`
- `title-block-default.qmd`
- `title-block-full.qmd`
- `title-block-multi-author.qmd`
- `title-block-no-title.qmd`

**Observed iframe DOM samples (from Playwright `runAssertions`):**

- `body-container-default.qmd` → `<div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-article"><main class="content" id="quarto-document-content">…</main></div>` with `<body class="fullcontent">`.
- `title-block-multi-author.qmd` → exactly one `<div class="quarto-title-meta-author">` whose contents element renders `AliceBob` (empty-string-joined, matching Rust quirk).
- `title-block-date-no-author.qmd` → `<header id="title-block-header">` with `<h1 class="title">`, NO `div.quarto-title-meta` (date alone is suppressed per replicated Rust quirk).

**Pre-existing failures NOT introduced by 2D** (observed in the same run; unrelated to body-container / title-block work):

- `q2-preview/image-with-attrs.qmd` — fixture has a malformed 3-array `ensureHtmlElements` spec and a self-contradictory negative selector (`img[width="400"]` must NOT match, but the markdown `{width=400}` produces that attribute). The runner only consumes the first two arrays, so the third (`['img[alt="alt"]']`) is silently dropped. Either the fixture was authored against an older renderer that didn't propagate the `width` attribute, or the `parseTwoArraySpec` schema changed under it.
- `quarto-test/callout-note.qmd`, `extensions/lipsum-override/test.qmd`, `highlighting/03-user-grammar/03-user-grammar-toml.qmd`, `metadata/theme-inheritance/root-doc.qmd`, `themes/theme-project-scss-relpath/chapters/subdir-relpath.qmd`, `theme-subdir-e2e.spec.ts` — html-format flakes/preexisting.

**Process notes:**

- Initial run hit a parse error on `title-block-multi-author.qmd` (line 23 col 6, "Out of scope: Multi-author rendering UX"). Root cause: bd-1qk5 (post-codespan apostrophe trips Q-2-7). The fixture body had `\`AliceBob\`) — matches Rust's \`TemplateValue::List\`` — backtick→`)`→apostrophe-after-codespan combo that the parser still chokes on. Rewrote the body to plain ASCII with no backticks/apostrophes; fixture green on re-run.
- `body-container-minimal.qmd` failed once with a "Peer connection failed" sync flake; passed on retry without any content change. Re-stating the smoke fixture cleanly (no special characters) reduces parser-vs-flake ambiguity.

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
  3. **Title-block transform exclusion (the user's hint)** — confirmed `"title-block"` is in `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1052`). In full HTML mode the transform short-circuits anyway (template emits the chrome) so q2-preview is aligned. In **minimal mode** the Rust transform prepends a synthetic `<h1>` to `ast.blocks` (`transforms/title_block.rs:54-110`); without React-side replication, q2-preview's minimal mode silently drops the title. Phase 6.2 now re-implements the minimal-mode branch in `PreviewDocument.tsx`'s minimal branch, with its own vitest cases and a `body-container-minimal-title.qmd` smoke fixture.
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

- **2026-05-10 (cross-check pass)**: re-verified every line-number citation and behavioral claim against the actual sources before implementation. Edits:
  - **Iframe `<title>` claim corrected**: previous draft said `entry.tsx` "already sets the iframe's `<title>` from the AST". Verified false — iframe `<title>` is the static literal at `q2-preview.html:6`; only `Editor.tsx:348-350` writes a tab title (parent window only, sourced from project filename + description, NOT doc frontmatter). Reclassified from "out of scope, already works" to **Open design question §1** (bundle into 2D vs follow-up).
  - **`is_minimal_html()` citation fixed**: definition lives at `format.rs:306-318`, not `template.rs:501` (501 is a call site). Function-level wording in §Checklist 6.2 and the Phase 6.2 source comment updated.
  - **`title_block.rs` line range corrected**: actual span is 54-110 (impl + transform fn) plus helpers at 113-141, not "42-95".
  - **`applyTheme` citation fixed**: spans `entry.tsx:120-135`, not "116-131" (close, not load-bearing).
  - **`meta.ts` location flagged as Open design question §2**: `hub-client/src/utils/` does not currently import from `components/render/`, but `inlinesToPlainText`/`blocksToPlainText` (the helpers `extractMetaString` depends on for the `MetaInlines`/`MetaBlocks` branches) live at `q2-preview/utils.tsx:69`/`:134` and depend on `BlockNode`/`InlineNode` from `components/render/framework/`. Putting `meta.ts` in `utils/` would invert that layering for the first time. Three resolution options analyzed; recommended path is to put `meta.ts` next to `q2-preview/utils.tsx`.
  - **Slide-renderer behavior change called out**: lifting `extractMetaString` upgrades the `MetaInlines` walk from `Str`/`Space`-only to a full `inlinesToPlainText` walk, so slide titles containing `Emph`/`Strong`/`Code`/`Link` start rendering text where they previously rendered `''`. Strict improvement; Phase 6.0d now requires a regression-test case to lock the new behavior. **Open design question §3** captures the tradeoff.
  - **`registry.test.ts` extension target flagged as Open design question §4**: existing tests check `Custom.*` exports vs Pandoc tags, NOT synthetic registry keys. Either move `PreviewTitleBlock` under `./custom` (matches how `Fallback` is wired and reuses the existing test) or add a new test that asserts `previewRegistry['__title_block__']` directly.
  - **Empty-string `body-classes` flagged as Open design question §5**: `body-classes: ""` produces a body with no classes (matches Rust template behavior). Recommended (A) on parity grounds; alternate (B) is `||`-vs-`??` if "no override" semantics are wanted.
  - **`mergedPreviewRegistry` synthetic-key passthrough verified**: `customRegistry.ts:14-20` does no key filtering, so `__title_block__` flows through user TSX overrides exactly as the plan claims.
  - **`format: revealjs` detection verified**: existing precedent at `ReactRenderer.tsx:211` only checks `MetaString`; the plan's `extractMetaString(meta.format) === 'revealjs'` newly matches `MetaInlines` as well. **NOTE — superseded by the 2026-05-10 pre-implementation review pass below**: this was originally framed as a "strict superset" but is actually a deliberate behavior change for multi-child `MetaInlines` (e.g. `MetaInlines [Str("re"), Space, Str("vealjs")]`). Benign in practice because realistic format values are single-token. See the pre-implementation review pass entry for the corrected framing; Phase 6.0e's checklist text is authoritative.

- **2026-05-10 (structural-review pass + design-question resolutions)**: stepped back from the meta.ts location question to audit `components/render/`'s overall layering against what 2pre established. Findings: (a) `inlinesToPlainText` / `blocksToPlainText` ended up in `q2-preview/utils.tsx` opportunistically — they are pure Pandoc-AST walks with no q2-preview-specific behavior; (b) the slide renderer at `ReactAstSlideRenderer.tsx` is a "ghost format" (885 LOC, top-level, has private `extractMetaString` + per-tag `switch` rendering), structurally a third format that 2pre intentionally deferred migrating; (c) `meta.format` extraction is duplicated three times (`getQ2Format.ts`, `ReactRenderer.tsx:211`, `ReactAstSlideRenderer.tsx:350`) and Plan 2D would have added a fourth; (d) `framework/customNode.ts` is in framework/ but never re-exported from `framework/index.ts`. Resolutions:
  - **Phase 6.0 added**: framework extraction as the prep step for 2D. `framework/plainText.ts` (moved walks) + `framework/meta.ts` (new) + `framework/index.ts` re-exports including `customNode` bookkeeping fix + slide-renderer migration to the framework helpers + consolidation of the three `meta.format` checks. Six small commits per the 6.0a–f checklist; each leaves the tree green.
  - **§1 (tab title) RESOLVED option A**: 3-LOC `useEffect` in `PreviewDocument` writes `document.title` from `meta.pagetitle ?? meta.title`. Captured as Phase 6.2a. The earlier "already done by `entry.tsx`" claim was wrong (verified 2026-05-10).
  - **§2 (`meta.ts` location) RESOLVED option D — framework-tier**: previously deadlocked between three options (`utils/`, `q2-preview/utils.tsx`, duplication), all of which left some asymmetry. Lifting both the meta helpers AND the plain-text walks into `framework/` resolves the question by putting these utilities next to the types they walk. Bonus: the slide renderer and the duplicate `meta.format` checks all become consumers of the framework helpers, eliminating the leakage that motivated the question in the first place.
  - **§3 (slide-renderer regression test) RESOLVED option A**: vitest case in Phase 6.0d locks the behavior change for inline-markup titles.
  - **§4 (`__title_block__` test target) RESOLVED option A**: `PreviewTitleBlock` lives at `q2-preview/custom/PreviewTitleBlock.tsx` (mirrors `Custom.Fallback` wiring); registered as `__title_block__: Custom.PreviewTitleBlock`; existing `'every expected CustomNode component is exported from ./custom'` test extends to include `'PreviewTitleBlock'`.
  - **§5 (empty-string `body-classes`) RESOLVED option A**: empty-string is opt-out; matches Rust template behavior; locked by vitest.
  - **Plan 2E preview**: q2-slides migration (the next structural cleanup) gets its own plan. Outline discussed before commitment to writing.

- **2026-05-10 (final review pass — editorial cleanup + dead-code removal)**: read 2D end-to-end after design-question resolution. Editorial fixes:
  - **Stale "q2-debug" attribution corrected** in seven places. The slide renderer (`ReactAstSlideRenderer.tsx`) lives at the top level of `components/render/`, NOT inside `q2-debug/`; earlier drafts of 2D conflated "the format that today routes via `ReactRenderer.tsx`'s slide branch" with q2-debug. Doc-comments, prose, and revision history now consistently say "the slide renderer" (or `ReactAstSlideRenderer.tsx` by file) where appropriate.
  - **§"Provided" Plan 2E reference**: Plan 2E is now the q2-slides migration (per the 2E sketch). The sidebar plan is a separate unnumbered future plan; references updated.
  - **§Scope source listings**: import paths in the `framework/meta.ts` and `PreviewDocument.tsx` example listings were stale (`from '../../../utils/meta'`, `from './PreviewTitleBlock'`). Updated to match the resolved file locations: `from '../framework'` and `import * as Custom from './custom'`.
  - **Stale §Open-design-question-§2 layering caveat removed** from §Scope's `framework/meta.ts` description (the question is resolved).
  - **`transforms/title_block.rs:42-95`** in the inline source comment updated to `:54-110` with helpers at `:113-141` (matches the verified line range).
  - **§Test plan §Test-tier conventions**: tightened the meta-merge claim to cite the specific Rust stage (`MetadataMergeStage`) so the "single-doc fixtures are sufficient" rationale is grounded in a long-shipped pipeline stage rather than the hand-wavy "orchestrator."

  Dead-code removal:
  - The defensive `isRevealjs` branch in `PreviewDocument.tsx`'s example source has been removed. `format: revealjs` documents route to the slide renderers (`SlideAst` / `RevealjsSlideAst`) via `ReactRenderer.tsx`'s format-dispatch and never reach `Q2PreviewIframe` / `PreviewDocument`. A defensive skip would be dead code by design — confusing for reviewers who would ask "when does this fire?". The §Out-of-scope entry now explains why no revealjs-specific branch is needed (and what the one-line fix would be if upstream routing ever changed). §Design-decisions, §Test-plan, and §Risk-areas entries that referenced the dead branch were also dropped. All `format` reads were removed from the body-container source — `format`, `theme`, `minimal` reduces to `theme`, `minimal`.

- **2026-05-10 (pre-implementation review pass)**: ambiguities surfaced during a final cross-check before starting implementation. Edits:
  - **`PreviewTitleBlock.tsx` location locked to `q2-preview/custom/`** (parallel to every other CustomNode component — `Callout`, `Theorem`, `Proof`, `FloatRefTarget`, `Equation`, `CrossrefResolvedRef`, `Fallback`, all in `custom/`). Three stale references in §Scope header, §Test plan filename, and §References list updated to match.
  - **§Scope source-listing imports fixed** to match the resolved `framework/` location for meta helpers and the `q2-preview/custom/` location for the component itself: `from '../../framework'` (was `from '../../../utils/meta'` for `extractMeta*`; was `from '../framework/types'` for `PandocAST`). Earlier revision-history claim that these were updated turned out to be incorrect — the source listing was missed.
  - **`meta.format` consolidation reframed from "strict superset" to "deliberate behavior change, benign in practice"**: today's `getQ2Format.ts:15` reads only the first child of `MetaInlines`; today's `ReactRenderer.tsx:211` matches `MetaString` only. The new `extractMetaString`-based path walks the full inlines list. For single-token format values (the realistic case) old and new agree; for multi-child MetaInlines they diverge. The consolidation is still the right call (eliminates three duplicate code paths) but the framing was wrong.
  - **Synthetic-key registration test added** to Phase 7.3. Two assertions: (1) extends the existing "every expected CustomNode component is exported from ./custom" check to include `PreviewTitleBlock`, and (2) adds a NEW test asserting `previewRegistry['__title_block__'] === Custom.PreviewTitleBlock`. The new test also covers `__fallback__` (closes a pre-existing gap — the `__fallback__` registration line is exercised by behavior tests but never directly asserted).
  - **Phase 6.2a iframe `<title>` no-title path resolved**: only write `document.title` when an AST title resolves; on no-title, the static `q2-preview Renderer (Sandboxed)` from `q2-preview.html:6` stays in place. Cleanup restores the pre-mount snapshot; first-mount and subsequent-mount cleanup behave identically. Tests added for: title set, pagetitle wins over title, empty meta (sentinel preserved), empty-string title (Pandoc-falsy), cleanup-on-unmount.
  - **Pandoc-falsy semantics locked per-field** for every optional title-block element (subtitle, author, date, abstract). Each gets its own regression test in Phase 7.4 so a future refactor can't silently emit empty `<p class="subtitle"></p>` etc. §Defensive variants now explicitly contrasts the empty-string handling between `body-classes` (opt-out, preserves the empty value) and title-block fields (treated as missing, suppress the optional element) — both match Rust under Pandoc-template semantics (`$body-classes$` substitutes the empty string verbatim; `$if(x)$` treats `""` as falsy).
  - **i18n research recorded inline** (verified against `/Users/gordon/src/quarto-cli/`): TS Quarto data model (`_language[-<locale>].yml` keys + `authors.lua:852-906` filter + `$labels.*$` template variables), active-locale resolution chain, and Rust q2's missing `LanguageResolveStage`. Decision: defer i18n from 2D; ship hardcoded English literals matching Rust's current `template.rs`. When a future Rust-side stage exposes `meta.labels.*`, q2-preview's `PreviewTitleBlock` flips its three literals to `extractMetaString(meta.labels?.<key>) ?? '<English fallback>'` in a single commit. Sources cited in §"Out of scope: i18n" so the research isn't redone.
  - **Items NOT changed**: title-block transform exclusion is permanent (we have determined we are incompatible with the Rust transform and re-implement it React-side; revisiting is not worth research now); Code review addendum stays as-is.
  - **User-override DX for synthetic keys (addendum item 1) partly mitigated**: applied option (a) — added a doc-comment above `PreviewTitleBlockArgs` in the §Scope source listing explaining that `__title_block__` overrides receive `{ meta }` (not the dispatcher's `NodeArgs<…>` shape used by per-tag entries), and that TS can't catch a wrong-shape override because `FormatRegistry` types entries as `(props: any) => ReactNode`. Also added a `registry.ts` doc-comment note in the §References list. **New Phase 6.0c.1**: extend `window.__Q2_PREVIEW_RENDERER__` (the explicit user-facing renderer surface global at `entry.tsx:56-64`) with the framework helpers `extractMetaString`, `extractMetaBool`, `extractMetaStringList`, `inlinesToPlainText`, `blocksToPlainText`. Without this, the doc-comment that points user overrides at `extractMetaString` would be a lie — the helpers are framework-private until we expose them. The global already exposes `renderChildren`, `renderNode`, `renderSlot`, `Node`, `Block`, `Inline`, `previewRegistry`; meta helpers are a natural extension since they're now framework-tier (Phase 6.0).

- **2026-05-10 (synthetic-key prop-shape resolution + cleanup pass)**: pre-implementation cross-check raised the prop-shape mismatch and several wavering items. Edits:
  - **§6 / §7 / §8 added to Open design questions** (all RESOLVED): `__title_block__` mounts with `AstProps` (parallel to `Ast`, not a third novel shape); `FormatRegistry` at `framework/types.ts:163` extended with typed optional entries for both `__fallback__` and `__title_block__` so user TSX overrides get compile-time prop-shape checking; `PreviewTitleBlock` exposed on `window.__Q2_PREVIEW_RENDERER__` (Phase 7.3.1) so user overrides can compose the built-in instead of fully replacing it.
  - **Phase 7.2 / 7.3 / 7.4 rewritten** to use `AstProps`. Mount in `PreviewDocument.tsx` is now `<TitleBlock ast={ast} setAst={setAst} onNavigateToDocument={onNavigateToDocument} />`. Built-in `PreviewTitleBlock` accepts `AstProps`, reads `ast.meta`, ignores `setAst`/`onNavigateToDocument` (user overrides can use them). Phase 7.4 gets a second user-override test case covering composition via the exposed `PreviewTitleBlock`. Phase 7.4 "nested inside the author meta wrapper" wording corrected to "sibling of `<div class="quarto-title-meta-author">`, both inside `<div class="quarto-title-meta">`" — the source code was always correct; only the test prose was ambiguous.
  - **Phase 6.4 fixture rename + new combo fixture**: `body-container-override.qmd` renamed to `body-classes-override.qmd` (more specific, doesn't overlap with theme/CSS terminology); new fixture `body-classes-full-layout-combo.qmd` exercises `body-classes: custom-cls` + `page-layout: full` together to catch regressions where the two flow paths interact.
  - **Phase 6.0c clarified**: re-exports from `framework/index.ts` are **flat** (`export * from './meta'`), not namespaced — the source listings later in the plan import named symbols directly from `'../framework'`.
  - **Phase 6.0c.1 ordering tightened**: explicitly lands after 6.0a + 6.0b (helpers must exist before being exposed); pairs with 6.0c in the same commit. `PreviewTitleBlock` itself is added to the same global later in Phase 7.3.1 (it doesn't exist yet at 6.0c.1 time).
  - **§Defensive variants extended**: multi-token `page-layout` value flows verbatim into the class attribute (matches Rust parity; user-class-list responsibility); list-form `body-classes` intentionally falls back to `fullcontent` rather than empty-string-joining via `extractMetaStringList` (Rust's pipeline never produces list-form body-classes today; `sidebar_render.rs:92` and template body-classes code path are string-shaped; falling back to a known-good sentinel is more useful than mirroring Rust's broken-but-untested behavior).
  - **§Out of scope addition**: re-enabling the Rust `title-block` transform for minimal mode only is the cleaner long-term fix for the React-side `<h1>` placement divergence from Rust's section-structure nesting. 2D ships the React-side synthesis (Phase 6.2) as the pragmatic v1; the follow-up un-excludes `"title-block"` from `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1052`) and reconfigures it to short-circuit in full-template mode while running only in minimal mode.
  - **MetaList `["Alice", ""]` test prose trimmed** — was dual-direction discussion; now states the decision and rationale cleanly. Behavior is unchanged: empty entries are kept in the list and contribute nothing to the empty-string-join.
  - **§Code review addendum items 1 and 7 marked RESOLVED in 2D** with cross-references to Open design questions §6/§7. Retained for historical context.
  - **Revision-history stale "strict superset" claim** for the `meta.format` consolidation marked as superseded by the later "deliberate behavior change, benign in practice" correction.

## Code review addendum (out of scope for 2D)

Items raised during the final review pass that are **not** addressed by 2D and **not** required for it to land. Captured here so they aren't lost; each is fully optional, and pursuing any of them is its own follow-up. Plan 2D is a focused framework refactor in service of body container + title block; broader hub-client cleanups belong elsewhere.

1. **Synthetic-key prop shape — RESOLVED in 2D (Open design questions §6/§7).** The earlier draft proposed `{ meta }` as a third synthetic-key prop shape; resolved to `AstProps` (parallel to `Ast`), and `FormatRegistry` is tightened at `framework/types.ts:163` with typed optional entries for both `__fallback__` and `__title_block__`. User TSX overrides now get compile-time prop-shape checking on both synthetic keys. Item retained here for historical context — no longer an open follow-up.

2. **`renderChildren({ node: ast as any })` cast is hand-waved.** The example comment claims "renderChildren is typed for `BlockNode | InlineNode` but special-cases `PandocAST`". Worth verifying once that `framework/dispatch.tsx`'s renderChildren actually handles the `PandocAST` shape rather than relying on the `as any` cast to silently coerce. If it doesn't, the cast is hiding a bug.

3. **`extractMetaBool` accepts `MetaString("true"|"false")` unconditionally.** The fallback is targeted at YAML's quoted-boolean form (`minimal: "true"`). A future caller reading some unrelated field that happens to contain the literal `"true"` or `"false"` would coerce to boolean unexpectedly. Worth a one-line doc-comment caveat.

4. **`q2-preview/utils.tsx`'s private `blockText` helper.** Phase 6.0a moves `inlinesToPlainText`/`blocksToPlainText` to `framework/plainText.ts`. The internal `blockText` helper used by `blocksToPlainText` is part of that function's body and must move *with* it. The current 6.0a wording is correct but doesn't explicitly call this out; a careful implementer will figure it out, but a one-line note would prevent the file from being left half-migrated.

5. **`framework/customNode.ts` now has two import paths.** Phase 6.0c adds the barrel re-export but leaves deep-import consumers (`q2-preview/entry.tsx:36`'s `from '../framework/customNode'`) working. Drift risk is small but real. Migrating the existing deep-importer in the same commit (one-line additional change) would unify the import path; deferring leaves dual paths permanently.

6. **Plan 2D Phase 6.0 is independently landable.** 6.0 is structural prep with no dependency on 6.1+ / Phase 7. Plan 2E also depends on 6.0 only. If Plan 2E's writer wants to start before 2D's body-container work is done, 6.0 can land first as its own commit-stack and 6.1+ / 2E proceed independently afterwards. Worth noting in the dependency graph if the team wants that flexibility.

7. **Synthetic-key prop-shape convention — RESOLVED in 2D (Open design questions §6/§7).** The mount now uses `<TitleBlock ast={ast} setAst={setAst} onNavigateToDocument={onNavigateToDocument} />` (`AstProps`, parallel to the `Ast` key, not a third novel shape). Two synthetic keys, two prop shapes (`__fallback__` → `NodeArgs`; `__title_block__` → `AstProps`); both declared at the type level in `FormatRegistry`. The convention is "synthetic keys CAN have different shapes, but each shape is declared and TS-checked". Item retained here for historical context — no longer an open follow-up.

8. **CSS-class collision between user `body-classes` value and theme CSS.** A user can write `body-classes: container-fluid` and clash with Bootstrap's class. Same as Rust template — user's responsibility.

9. **Multi-author concatenation `AliceBob` matches Rust** but renders as a single token without separation. Theme CSS targeting `.quarto-title-meta-author` could add pseudo-element separators, but neither side does today. Worth flagging in §Out of scope as "if theme CSS adds separators via `::after`, we get those for free; we don't add separators ourselves" — but only if there's an existing thread on multi-author UX worth pinning to.

None of these are blockers; none are in scope for 2D's body-container + title-block work.
