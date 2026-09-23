# Plan: Book project foundations (book-projects P1)

**Date:** 2026-09-21
**Epic:** [`2026-09-21-book-projects-epic.md`](2026-09-21-book-projects-epic.md)
**Design (authoritative):** [`../designs/book-projects-architecture.md`](../designs/book-projects-architecture.md) §3, §4
**Q1 reference:** `src/project/types/book/{book,book-config,book-shared,book-types,book-constants}.ts`

## Overview

Give `ProjectKind::Book` a real `ProjectType` implementation. This phase is deliberately numbering-free: it builds the chapter list, the config translation, and the sidebar reuse — all the scaffolding both render modes (P2's single-file merge, P4's multi-file HTML) will build on. No chapter-number injection happens here (that's P4-only, per the design doc's finding that single-file merge needs none at all). **Note: the "new `render_documents` trait hook" this phase originally built is dropped, resolved by a seventeenth-pass research round — see the Decision below.** Book's Pass-2 dispatch is a concrete special case on `ProjectPipeline<RenderToFileRenderer>`, not a `ProjectType` trait method; this phase's scope shrinks by exactly that one item.

## Decisions

- `BookProjectType` reuses everything website-shaped (sidebar, navbar, footer generation; `lib_dir`) exactly as Q1's `bookProjectType` delegates to `websiteProjectType` — not a parallel implementation. **Precision note (review pass): sidebar/navbar/footer generation are `ProjectKind`-agnostic pipeline transforms driven by config, not methods on a `WebsiteProjectType` instance** (`WebsiteProjectType::pre_render` is itself a no-op default) — so "delegates to WebsiteProjectType" means "feeds the same config shape those transforms already consume," not a literal call into a `WebsiteProjectType` value. `lib_dir()` *is* a real trait method, and does return `"site_libs"` the same way `WebsiteProjectType`'s own override does.
- **`post_render()` override, required, not optional — found missing by a review pass, and under-scoped as first written, corrected by an implementability review.** `orchestrator.rs`'s `ProjectType` doc comment requires `post_render` to flush project-scoped artifacts whenever `lib_dir()` is non-empty; since `BookProjectType::lib_dir()` returns `"site_libs"` (previous bullet), omitting a `post_render()` override would silently drop those artifacts. `crate::artifact_flush::flush_project_artifacts(...)` is confirmed to be a real, standalone, reusable free function (called from `WebsiteProjectType::post_render` at `orchestrator.rs:527`) — but **`WebsiteProjectType::post_render`'s real body (`orchestrator.rs:517-580`) bundles that flush together with favicon/navbar-logo/footer-image copying, `sitemap.xml`, `robots.txt`, alias-redirect stubs, listing-placeholder substitution, and RSS-feed finalization — seven concerns, not one.** Given this epic's own framing ("Q1 renders books by having `bookProjectType` inherit `websiteProjectType`"), `BookProjectType::post_render` needs to call the same set `WebsiteProjectType::post_render` calls, not just the artifact flush in isolation — an implementer who stops at "reuse the flush" would silently drop favicon/sitemap/robots.txt/alias-redirects for every book project, a real Q1-parity regression with no test catching it. Extract/reuse whichever of those seven helpers make sense for a book project (almost certainly all of them — a book is a website-shaped output for HTML purposes) rather than hand-picking just the flush.
- `book.chapters`/`appendices`/`references` → `website.sidebar.contents` translation ports `bookChaptersToSidebarItems`/`chapterToSidebarItem` (book-config.ts) as idiomatic Rust — same idea (part → section, chapters → contents), not transliterated.
- `BookRenderItem` (index/chapter/appendix/part/**references**, depth, optional file, optional number) is the single source of truth for render order and numbering assignment — ported from `book-types.ts`/`bookRenderItems`. Appendices get their own fresh 1-based number sequence. **Presentation as a letter happens independently, more than once, in more than one place — this is not "P2/P4's job, not this phase."** P1 itself computes and bakes the letter form into sidebar nav-item text (see the sidebar-decoration Decision below — that's config-translation-time work, squarely this phase's job). P4 separately computes a letter for page-title decoration at render time. P2's single-file Typst path doesn't compute a letter in Q2 code at all — Typst's own compiler assigns it natively, in document order, once the extension's show-rule switches numbering into appendix mode. **These should be provably equal, not just hopefully equal — but the invariant has two parts, not one, corrected by a ground-truth-verification pass.** P1's sequence order and the merged document's actual heading order are both derived from the same `BookRenderItem` list, so an implementation bug that fails to preserve that order somewhere in P2's merge is one way these two letters can disagree. **A prior draft here also claimed "Q1's model has no way to mark one appendix chapter `.unnumbered` independent of the others" — that's wrong**: Q1's `findChapters`/`isNumberedChapter` mechanism (`book-config.ts`) applies the identical per-chapter `.unnumbered` check to appendix items as to ordinary chapters, so an individual appendix chapter genuinely can be unnumbered on its own. P1 needs a test for that case (sidebar shows no letter, consumes no slot); see design doc §4's refined "cross-artifact risk" note for the second half this creates — a seventeenth-pass research round found `orange-book`'s Typst extension likely does *not* skip an `.unnumbered` appendix heading the same way (Typst's native heading counter keeps incrementing regardless of display suppression), probably a real, inherited Q1 bug rather than a Q2 gap — not confirmed by an actual compile yet, P2/P3's already-planned fixture is where that gets settled. P2/P3 should test both halves of the invariant directly rather than assume either holds.
- **`book.references`** (`kBookReferences`) is a real, distinct Q1 config key (`book-config.ts:419-424`) for a designated references page, positioned after the main chapters and before appendices. Give it its own `BookRenderItemKind::References` and track *which* render item it is explicitly — this is a deliberate improvement over Q1's own mechanism, which locates the references page later (in `book-bibliography.ts`'s `bookBibliographyPostRender`, P6's territory) by scanning the project's render list for the first file whose `inputTargetIndex(...).markdown.containsRefs` flag is set — a structural check, not a filename convention (corrected by a review pass: an earlier draft here claimed Q1 matches against a literal `"references.html"` substring, which doesn't appear anywhere in Q1's source). Q2's explicit `BookRenderItemKind::References` is still a real improvement — it's known at config-translation time rather than inferred after the fact — the fix is only to the cited reason, not the decision.
- **`outputDir: "_book"`** — `BookProjectType`'s own output directory (`book.ts:115`), distinct from `WebsiteProjectType`'s `_site`. Don't let `lib_dir`'s delegation to website imply the output directory delegates too; it doesn't, in Q1 or here.
- **Book projects render only to formats with book support** (`projectFormatsOnly: true` / `isSupportedFormat: (format) => !!format.extensions?.book`, `book.ts:149-153`) — this is the general mechanism P3's DOCX/PPTX diagnostic is one instance of, not a one-off special case; implement it as the general rule (`BookProjectType::is_supported_format`) so any future non-book-aware format target gets the same clear diagnostic, not just the two formats this epic happens to test.
- **No `ProjectType::render_documents` trait method — resolved by a seventeenth-pass research round in favor of a smaller, concrete mechanism (design doc §3).** The original sketch wasn't `dyn`-safe (`Pass2Renderer::render_batch`'s generic, associated-type-returning signature can't be wrapped by a `Box<dyn ProjectType>` method), and research found the genericity was never needed: `BookProjectType`'s Pass-2 override is only ever reachable through the concrete `ProjectPipeline<'a, RenderToFileRenderer<'a>>`. **This phase's actual scope is unchanged in spirit but smaller in surface**: no new trait method, no change to `ProjectType`'s definition or `project_type_for`'s other call sites. P2/P4/P5 build their book-specific dispatch as a new `run_with_book_support()` method directly on the existing concrete `impl<'a> ProjectPipeline<'a, RenderToFileRenderer<'a>>` block, branching on `self.project_type.kind() == ProjectKind::Book` and falling through to the existing `run()` otherwise.
- `project_type_for` (`orchestrator.rs`) gains a real `ProjectKind::Book => Box::new(BookProjectType)` arm, replacing the current fallback to `DefaultProjectType`.
- **Sidebar nav-item chapter-number decoration is a config-translation-time concern, not a render-time hook.** Q1 has a genuine per-project-type extension point for this (`ProjectType.navItemText`, called generically by `website-navigation.ts` at render time — confirmed a real, pre-existing Q1 mechanism, not something book.ts bolted on ad hoc) because Q1's sidebar renderer builds nav text as it renders. Q2's chapter numbers are static and fully known once P1's `BookRenderItem` list exists — well before any chapter renders — so bake the decorated text (`<span class="chapter-number">N</span>&nbsp; <span class="chapter-title">Title</span>`, port of `numberChapterHtmlNav`) directly into each sidebar item's `text`/html field during the `book.chapters` → `website.sidebar.contents` translation this phase already builds. This avoids adding a new render-time hook to `WebsiteProjectType`'s sidebar renderer at all — **confirmed free, not hedged** (design doc §4, resolved in an earlier review pass): `NavigationItem.text`/`SidebarEntry` are already typed as `ConfigValue`, which can hold real Pandoc inlines, and `sidebar_to_html_with_options`'s renderer is a genuine Pandoc-inline-to-HTML walker that round-trips an attr-preserving `<span>` verbatim — no new field or mechanism needed.
- **Config-level book features, all real and previously missing from this epic's scope until a review pass caught the gap** — none of these are render-mode-specific, so they belong here, not in P2/P3/P4:
  - **Special-date resolution**: `book.date: today`/`last-modified` resolves once against the project directory, port of `book.ts`'s `bookPreRender` (`isSpecialDate`/`parseSpecialDate`).
  - **Project-render-list additions**: a book's render list is *restricted* to exactly what `BookRenderItem` names (unlike a website project's default-render-everything-found behavior) — so a file referenced from a `page-footer` region, or a `404.*` page sitting in the project directory, would never render at all without an explicit addition, port of `bookProjectConfig`'s footer-file walk and `ext404` check.
  - **Download/sharing sidebar tools**: PDF/EPUB/DOCX download buttons (for whichever single-file formats `book.downloads` names) and a social-share menu (`book.sharing`: LinkedIn/Facebook/Twitter), port of `downloadTools`/`sharingTools`. Verify whether `WebsiteProjectType` already computes an equivalent "source code" repo-link tool generically (both `book-config.ts` and website config read the same `website.repo-url` key) before porting that one piece specifically — it may already be free.
  - **Book-wide format defaults**: `number-sections: true` and `crossref.chapters: true`, applied uniformly to every book render regardless of format or render mode — confirmed by reading `book.ts`'s `formatExtras` directly: this pair is set unconditionally at the top of the function, *before* the per-format (`isHtmlOutput`/`isLatexOutput`/`isTypstOutput`) branches. Previously stated only in P2 (single-file merge); belongs here since it applies equally to P4's multi-file HTML. P2 and P4 both just inherit this rather than each setting it independently — see design doc §4/§6.

## Checklist

### Tests first
- [x] Unit test: a `book.chapters` list with nested `part:` entries produces the correct `BookRenderItem` sequence (index first, then chapters in order, parts as depth-tagged dividers) — mirrors `book-config.ts`'s `bookRenderItems` test shape.
- [x] Unit test: `book.appendices` numbers as a fresh sequence starting at 1, positioned after the main chapters in the render item list.
- [x] Unit test: `book.references` produces a `BookRenderItem` explicitly identifiable as the references item (not located later by matching an output filename convention) — the type/field P6 will read.
- [x] Unit test: a book target format without book support (e.g. a plain non-book-aware format) produces a clear diagnostic naming the unsupported combination, via the general `is_supported_format` mechanism P3 also relies on for docx/pptx.
- [x] Unit test: a chapter file that doesn't exist on disk produces a clear diagnostic (port of Q1's "Book contents file(s) do not exist" check), not a panic.
- [x] Unit test: a book with no index page produces a clear diagnostic (port of Q1's "Book contents must include a home page" check).
- [x] **Unit test, added by a ground-truth-verification pass that corrected a false premise about Q1's model**: an individual `book.appendices` chapter marked `.unnumbered` (in its own front matter or first heading) gets `number: None` on its `BookRenderItem`, consumes no slot in the appendix letter sequence, and its translated sidebar entry carries plain, undecorated text (no letter) — mirroring the identical, already-tested behavior for an unnumbered ordinary chapter. This is the case design doc §4's "cross-artifact risk" note depends on P1 handling correctly.
- [x] Unit test: a `part:` entry nested under another `part:` entry in `book.chapters`/`book.appendices` config produces a clear diagnostic, not a silent flatten/misparse or a panic — matches Q1's own type definition, which doesn't support nested parts either, but Q2's YAML-config parsing needs to actively reject the shape rather than silently accept and mis-render it.
- [x] Integration test: `q2 preview` on a single chapter file that belongs to a book project renders without error or crash through the ordinary per-document preview path — a smoke test confirming `BookProjectType`'s new config/sidebar shapes don't collide with the preview renderer, which never calls `run_with_book_support`'s book orchestration at all (see design doc §11's preview Known Limitation).
- [x] Unit test: `book.*` config keys translate into the equivalent `website.sidebar.contents` shape the existing sidebar generator already consumes — assert on the generated sidebar structure, not rendered HTML.
- [x] Unit test: a numbered chapter's translated sidebar item carries the chapter-number-decorated text (`<span class="chapter-number">2</span>...`); an appendix chapter's carries the letter form; an unnumbered chapter's carries plain undecorated text.
- [x] Integration test: rendering a minimal non-book project through `run_with_book_support()` (its `else` branch, falling through to the existing `run()`) produces byte-identical output to calling `run()` directly — regression guard that adding this method changed nothing for `Default`/`Website`.
- [x] Unit test: `book.date: today` resolves to a concrete date once, project-wide, not per-chapter.
- [x] Integration test: a `page-footer` region referencing `extra.qmd` (not listed in `book.chapters`) causes `extra.qmd` to render anyway; same for a `404.qmd` present in the project directory.
- [x] Unit test: `book.downloads: [pdf, epub]` produces the expected sidebar download-tool entries; `book.sharing: [twitter]` produces the expected share-tool entry.
- [x] Unit test: `number-sections: true` and `crossref.chapters: true` are applied to a book render's format metadata regardless of target format — assert for both an HTML target and a non-HTML target, since this default is format-agnostic (P2/P4 both inherit it rather than each setting it).
- [x] Unit test: a book project's render output lands under `_book/`, not `_site/` — added by a review pass; the `outputDir: "_book"` Decision above was bolded but had no checklist item exercising it.

### Implementation
- [x] `BookRenderItem`/`BookRenderItemKind` types (`crates/quarto-core/src/project/book/render_item.rs` or similar).
- [x] Chapter-list-building function, port of `bookRenderItems`, including the missing-file and no-index diagnostics.
- [x] Config translation function, port of `bookChaptersToSidebarItems`/`chapterToSidebarItem`, feeding the existing sidebar builder.
- [x] `BookProjectType` struct + `ProjectType` impl: `kind()`, `lib_dir()` (delegates to website's `"site_libs"`), `post_render()` (calls the same helpers `WebsiteProjectType::post_render` does — see the Decision above, not just the artifact flush), `pre_render()` (chapter-list build + sidebar translation + special-date resolution + footer/404 render-list additions + download/sharing tools + book-wide format defaults + delegate to website's pre_render). **`output_dir()` is not a `ProjectType` trait method — corrected by an implementability review**: `ProjectType` (`orchestrator.rs:330-456`) has no such method; the real mechanism is the free function `default_output_dir(dir: &Path, config: Option<&ProjectConfig>) -> PathBuf` in `project/mod.rs:72`, matched on `config.project_kind` (its own doc comment already anticipates this: "Phase-1 book / manuscript land in default... Their real defaults will be set when those project kinds are implemented"). Add `Some(ProjectKind::Book) => dir.join("_book"),` to that match instead.
- [x] Confirm `run_inner()` (`orchestrator.rs:1066`) is real and reusable as `run_with_book_support()`'s non-book fallthrough — no trait changes, no changes to `project_type_for` or `ProjectType`.
- [x] **Found by an eighteenth-pass review: `run()` (`orchestrator.rs:1059-1072`) is not pure delegation to `run_inner()` — it unconditionally calls `self.project.registry.shutdown_all()` (best-effort, reaps TS-engine Deno subprocesses) after `run_inner()` returns, success or failure.** `run_with_book_support()` must give both its book and non-book branches this same teardown — wrap the `if`/`else` in one shared `shutdown_all()` call after it (see design doc §3's corrected code sketch), not just call `self.run()` for the non-book branch and leave the book branch to invent its own. Add a regression test: a book render (success or induced failure) still calls `shutdown_all()` exactly once, matching a non-book render's existing behavior.
- [x] `project_type_for` dispatch arm for `ProjectKind::Book`.
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; phase-boundary `cargo nextest run --workspace` with delta reported. **Done 2026-09-23: clippy clean; quarto-core 4850/4850; workspace 14430 passed / 200 skipped / 0 failed** (run #4, after three attributed fixture repairs — see below).

## Details

At the end of this phase, a book project should render **exactly like a website** (each chapter as an independent HTML page, correct sidebar, no book-specific numbering or title decoration yet) — a deliberately small, verifiable milestone before P2/P4 add anything render-mode-specific. This is the right place to confirm end-to-end (per this repo's standing rule): `cargo run --bin q2 -- render <fixture-book-dir>`, inspect the rendered sidebar HTML for the translated chapter/part structure.

## Status (2026-09-23)

Implementation complete; all gates green (workspace run #4: 14430 passed, 200 skipped). Deviations/discoveries during execution:

- **Q-5-18 warning removed for Book** (`project_kind_diagnostics` in
  `project/mod.rs`): the "`book` projects are not yet implemented"
  warning is now stale — it fires only for Manuscript. The
  `project_type_parsing.rs` test was updated to pin the new contract.
- **Heading-only chapters have no sidebar title**: Q2's
  `DocumentProfile.title` is `None` for a chapter whose title comes
  only from a first-level heading (no front-matter `title:`), so the
  sidebar falls back to the bare filename — *identical* to website
  behavior for such pages (verified: the website enrichment shows the
  same fallback). Not book-specific; not fixed here.
- **`toc_title_context::book_project_uses_the_document_term` fixture
  was an invalid book**: `type: book` with no `book.chapters` now
  errors with Q-5-32 — exactly as Q1 throws "Book contents must
  include a home page" for an empty list (verified against
  `book-config.ts` `bookRenderItems`: `indexPos === -1` throw). The
  test fixture gained a one-entry `chapters` list.
- **Preview smoke test uses the real preview path**:
  `ProjectPipeline<RenderToHtmlRenderer>` + `RenderMode::ActivePage`,
  the same code path the WASM `render_page_in_project` entry point
  drives (native harness from `render_page_in_project.rs`), not the
  disk renderer.
- **Teardown-parity regression test needed an observability hook**:
  `EngineRegistry::shutdown_all_call_count()` (an `AtomicUsize`
  counter incremented in `shutdown_all`) — `run()` and
  `run_with_book_support()` both call it exactly once, success or
  failure.
- **Pre-existing branch breakage** (HEAD did not compile:
  `footnotes_resolve.rs` written against a newer quarto-source-map
  API than the pinned 0.2.0, plus the `hephaestus-render` bucket
  classification failure) was fixed upstream on
  `feature/book-projects` by P0's `a46ff9620`; picked up by rebase
  (linear history — no merge commits on this epic's branches).

- **Phase-boundary workspace redness, attributed from full-phase
  context** (neither failure was P1's):
  - `conditional_content_cli::hidden_float_does_not_consume_a_crossref_number`
    — fallout from P0's `9e71d2c32` (crossref caption prefixes now use a
    literal U+00A0 NBSP, matching Q1's `titlePrefix`); the CLI-level test
    still expected `&nbsp;`/ASCII-space. Assertion updated to the literal
    NBSP (verified with `xxd`: `c2 a0` in the output).
  - `render_pandoc_formats_e2e::e2e_docx_without_pandoc_on_path` —
    pre-existing on `feature/book-projects` (verified by stashing all P1
    changes and re-running: still red). The Lua-filter vendoring
    (`9b03c319c`) moved pandoc discovery ahead of the engine layer, so
    the not-found diagnostic is `Q-20-1` ("Pandoc Not Found"), not the
    `Q-18-1` ("Engine Returned an Unreadable Result") the test was
    written against. Expectation updated; `Q-20-1` matches the test's
    stated intent better.
  - `smoke_all::smoke_all` — three more sub-tests red from the same
    `9e71d2c32` NBSP change, this time in smoke-all fixture specs
    (`markdown/crossref-caption-prefix-forms.qmd`,
    `localization/lang-es-crossref.qmd`,
    `localization/language-inline-override.qmd`): the
    `ensureFileRegexMatches` patterns still expected `Figure 1:` /
    `Tabla 1:` / `Tabella 1:` with an ASCII space. Patterns updated to
    `Figure 1:` etc. (YAML double-quoted escape → literal NBSP in
    the regex); the YAML escape survives quarto-yaml's parser, verified
    by the now-green `smoke_all` run.

### End-to-end verification (recorded per repo rule)

- Invocation: `cargo run --bin q2 -- render /tmp/booktest` with a
  minimal book fixture (`_quarto.yml` with `project: {type: book}`,
  `book: {title: "My Book", chapters: [index.qmd, ch1.qmd]}`, both
  chapters with front-matter titles).
- Observed (inspected `_book/index.html` directly):
  `Rendered 2 of 2 files to /private/tmp/booktest/_book` — output
  under `_book/`, no `_site/`, no Q-5-18 warning; the rendered
  sidebar contains
  `<a href="index.html" ...><span class="menu-text"><span class="chapter-number">1</span>  <span class="chapter-title">Home</span></span></a>`
  and likewise `2 / First` for `ch1.html` — per-chapter HTML, correct
  sidebar, numbering decoration baked into the sidebar text only.
