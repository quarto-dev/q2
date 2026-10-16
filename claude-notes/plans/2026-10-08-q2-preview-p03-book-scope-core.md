# Plan 3: explicit book scope: quarto-core and the CLI

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E11); the execution model and landing procedure are there.

> **Superseded in part by [`2026-10-15-format-resolution-epic.md`](2026-10-15-format-resolution-epic.md):** book scope is now the `book-scope` metadata key (a render overlay carries it), `--book-scope`, the `scope` request fields and the `Option<BookScope>` plumbing described below are gone, and `Page` is `none`. This plan's text and Handoff log are history and are left as written.

## Execution

- **Lane:** A (`workspace-1`), binding: one agent per plan, in this order: Plan 3, Plan 5, Plan 10, Plan 13. This plan is number 1 of 4 in the lane.
- **Waits on:** nothing. Nothing inside this plan waits on another plan.
- **Start gate:** none; start immediately.
- **Branch:** `q2-preview/p03-book-scope-core`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** Plan 5 (wasm and hub-client scope), Plan 9 (routes resolve scope and call `render_chapter_alone`), Plan 10 (book downloads).
- **Next in lane:** Plan 5 (`2026-10-08-q2-preview-p05-book-scope-clients`); the next agent starts only when its own start gate passes.

## Goal

Book scope (render the whole book, or one chapter as a standalone document) becomes an explicit option instead of being inferred. This plan builds it in `quarto-core` and exposes it on `q2 render`; Plan 5 does the wasm option and hub-client; Plans 9 and 10 use it in the routes.

## Where scope is smuggled today (the audit)

- `orchestrator.rs:1030-1062`: scope is decided by format x `RenderMode`. typst and epub always go to `run_book_single_file_merge`, even for a one-file render, so `q2 render ch1.qmd --to typst` renders the WHOLE book (ran); html renders one page, pinned by `book_project_type::single_chapter_subset_render_keeps_one_file_render_set` (which uses `Format::html()`, so it does not pin typst/epub). The comment at `:1035-1041` says the per-document typst template invocation is "unverified".
- `quarto/src/commands/render.rs:1180-1252`: naming a file on the CLI silently means `RenderMode::Subset`.
- `pandoc_request/render.rs:25-35,424-455`: `BookScope::{Auto,Chapter}`. `Auto` means whole book only if the project is a book, the format is typst/epub and the page is a chapter, and otherwise degrades silently to the page alone (docx/pptx too). `book_membership` is a private fn (`:357`); `book_chapter_files` is public and pass-free (`project/book/render_item.rs:105`). `book_resolution` (`pandoc_request/formats.rs:280-310`) re-derives membership from sidecar keys for the menu.
- Chapter-alone is built from `RenderMode::ActivePage` plus `extra_render_files` plus `seed_missing_book_title_metadata` (`render.rs:470-520`; `project/book/project_type.rs:127-135`), as side effects of the request path. `render_to_file` is per document and never runs `pre_render`.
- The wasm option, the hub-client download code and the native routes also decide scope implicitly (Plans 5, 9, 10).
- Today `q2 render other.qmd --to typst`, where `other.qmd` is in a book project but not a chapter, renders the whole book to `_book/<name>.pdf` without the page's text (ran).

## Decisions

- **P3-D1 The option.** `quarto_core::BookScope { Book, Chapter }`; `Auto` is removed. The option is `Option<BookScope>` at every boundary (the CLI flag, the wasm request option, the route field): `None` means the user did not choose, and behaves as `Book` wherever the format can consolidate a book. The option applies only to formats that consolidate a book into one file (typst, pdf, epub). html, docx and pptx have no whole-book output, so an explicit `Some(_)` is ignored with an info diagnostic; `None` is silent. `q2 render ch1.qmd --to html` stays per page, and `q2 render ch1.qmd --to typst` keeps rendering the whole book.
- **P3-D2 The resolver.** `resolve_book_scope(project, path, scope: Option<BookScope>, format)` returns `Book` / `Chapter` / `Page`. A page that is not a member of a book project resolves to `Page` when the request was `None` or `Some(Book)`: it is rendered standalone and the result carries a visible notice (an info diagnostic; allocate the code in the error catalog), never an error and never a silent downgrade. `Some(Chapter)` on a non-member is `Page` with no notice (the user asked for that page alone). A non-book project, or a non-consolidating format, resolves to `Page` without a notice. `ResolvedBookScope` stays as the response echo and gains `Page`. This changes today's CLI behaviour for a non-member page (whole book to standalone page); the change is intentional and goes in the changelog.
- **P3-D3 One chapter-alone entry.** `render_chapter_alone(project: &ProjectContext, abs_path: &Path, format: &Format, options: &RenderToFileOptions) -> Result<RenderToFileResult, QuartoError>` in `quarto-core`, built on `ProjectPipeline`, `RenderMode::ActivePage`, `extra_render_files` and title seeding (the browser's mechanism). It takes the options struct, so the output directory, `source_override` and cancellation that exist today, and the `typst_target`, captures, attribution and materialize directory that Plans 2, 6 and 9 add, reach it without a signature change. The CLI calls it for `Chapter`; Plan 9's routes call it for a resolved `Chapter`.
- **P3-D4 Wasm compatibility shim.** The wasm option still says `'auto' | 'chapter'` until Plan 5; `parse_request_options` (`wasm-quarto-hub-client/src/lib.rs:~1771`) maps `'auto'` to `None` and `'chapter'` to `Some(Chapter)`. The only behaviour change for existing wasm requests is the notice for a non-member page of a book on typst/epub (previously a silent downgrade); everything else, including docx and pptx, is unchanged and gets no new diagnostic because `None` is silent. Plan 5 removes `'auto'`.

## Tasks

Per task: `cargo clippy -p <crate> --all-targets -- -D warnings` plus `cargo nextest run -p <crate>` for each touched crate (quarto-core, quarto, wasm-quarto-hub-client). Workspace nextest at the end of the plan. Integration tests go in `crates/<crate>/tests/integration/<name>.rs` plus `pub mod` in `main.rs`.

- [x] 3.1 Baseline `cargo nextest run --workspace` on the integration tip. **Baseline: 15990 run, 15990 passed, 202 skipped.**
- [x] 3.2 Move `BookScope` into `quarto-core` (out of `pandoc_request/render.rs`), remove `Auto`, and make the option `Option<BookScope>` (P3-D1); move `book_membership` next to `book_chapter_files`, make it public, and have `book_resolution` call it (E7; the existing test `the_resolver_and_the_exports_auto_scope_agree_for_every_file_of_the_book` guards this). Update the wasm `parse_request_options` shim (P3-D4) and `ResolvedBookScope`\'s exhaustive match at `wasm-quarto-hub-client/src/lib.rs:~1561` in the same change, and every `BookScope::Auto` literal (about 25 sites in 8 integration files under `quarto-core/tests/integration/`: `pandoc_request_exports.rs`, `pandoc_request_captures.rs`, `pandoc_svg_rasterize.rs`, `pandoc_request_extension_filters.rs`, `pandoc_request_projects.rs`, `pandoc_request_books.rs`, `pandoc_request_typst.rs`, `pandoc_remote_images.rs`), so the workspace builds after this one task.
- [x] 3.3 `resolve_book_scope` per P3-D2; `render_pandoc_request` calls it instead of `Auto`, and reports its resolved scope plus the notice; the explicit-scope info diagnostic for html/docx/pptx is emitted only for `Some(_)`.
- [x] 3.4 `render_chapter_alone` (P3-D3).
- [x] 3.5 The CLI: `q2 render --book-scope book|chapter` (`render.rs`), absent means `None`; `--book-scope chapter` requires exactly one file argument (zero or several is an error: "`--book-scope chapter` needs exactly one file"); `--book-scope book` with a file renders the whole book for typst/epub; ignored with an info diagnostic for html, docx, pptx. `ProjectPipeline` gains a scope field and builder method (default `None`; `publish.rs:295` and the `quarto-test` runner keep the default); the orchestrator dispatch at `:1030-1062` reads the explicit scope (through `resolve_book_scope`) instead of inferring from format and `RenderMode`, and calls `render_chapter_alone` for `Chapter`. Help text and user docs.
- [x] 3.6 Spike (was 1.1.A), after 3.4 and 3.5: diff `q2 render ch1.qmd --to typst --book-scope chapter` and the same for epub against the browser-style chapter-alone request on a small book (compare the `.typ` or the pre-writer AST, not a compiled PDF against a request); record what native lacks (book title metadata, `extra_render_files`, the per-document typst template invocation the comment at `orchestrator.rs:1035-1041` calls unverified) and fix it in `render_chapter_alone` before moving on.
- [x] 3.7 Tests. Pinned and unchanged: `single_chapter_subset_render_keeps_one_file_render_set`; `:488` (`docx_and_pptx_on_a_chapter_stay_chapter_only_without_a_warning`) passes unchanged because `None` is silent; the parity tests at `pandoc_request_books.rs:306,370`. Rewritten for P3-D2: `pandoc_request_books.rs:536-550` and `:566-585` (non-member `scratch.qmd` and `drafts/idea.qmd` now resolve to `Page` with the notice for `None`/`Some(Book)`). New: CLI default typst renders the whole book (today's behaviour); `--book-scope chapter` output matches the browser request path; `q2 render other.qmd --to typst` (non-member) renders standalone with the notice; `Some(Chapter)` on a non-member renders standalone with no notice; `--book-scope` with html/docx/pptx reports the info diagnostic only when passed; `--book-scope chapter` with no file or two files is an error; epub chapter alone.
- [x] 3.8 `cargo build -p quarto`; note the option and the non-member change in the changelog; plan end: workspace nextest against 3.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- `render_pandoc_request` is shared with the wasm host: a change in resolved-scope reporting must not alter output bytes for existing requests (the parity tests at `pandoc_request_books.rs:306,370` guard it).
- The native chapter-alone typst path has never been exercised through the CLI; 3.6 is where that is found out.

## Handoff log

**Headline:** `BookScope`, `resolve_book_scope` and `render_chapter_alone` are in `quarto-core`; `q2 render --book-scope book|chapter` works; the browser request path resolves scope through the same resolver. Landed as `caecb012b` (3.2), `2eae5f047` (3.3-3.7, hashes after the rebase onto Plan 1) and the changelog commit after it.

**Nextest.** Baseline (3.1, integration tip before Plan 1): 15990 run, 15990 passed, 202 skipped. Final (rebased on Plan 1, workspace): 16013 run, 16013 passed (1 leaky: `comrak-to-pandoc block::tests::test_multiple_paragraphs`, untouched), 202 skipped. Delta +23 = 11 tests from Plan 1 (rebase) + 12 of mine (8 in `quarto` `book_scope_cli_e2e`, 4 in `quarto-core` `pandoc_request_books`: chapter-alone typst parity, chapter-alone epub parity, chapter scope on a non-member, explicit scope for docx/pptx). No test was removed; two were rewritten (see below).

**Interfaces as they ended up** (later plans consume these):

- `quarto_core::BookScope { Book, Chapter }` (also at `pandoc_request::render::BookScope`); no `Auto`. The option is `Option<BookScope>` everywhere (`PandocRequestInput.scope`, `ProjectPipeline::with_book_scope`, `RenderArgs.book_scope`).
- `quarto_core::project::book::{BookMembership, book_membership, ResolvedBookScope, BookScopeResolution, resolve_book_scope, format_consolidates_book, scope_ignored_notice}` in `project/book/scope.rs`. `ResolvedBookScope { Book, Chapter, Page }` moved here and is re-exported from `pandoc_request::render`.
  `resolve_book_scope(project: &ProjectContext, active: &Path, scope: Option<BookScope>, format: &Format, runtime: &dyn SystemRuntime) -> Result<BookScopeResolution, QuartoError>`. `BookScopeResolution { scope: ResolvedBookScope, membership: Option<BookMembership>, diagnostics: Vec<DiagnosticMessage> }`. `active` must be absolute/canonical. `Err` only for a broken chapter list when the render would consolidate the book. The caller delivers `diagnostics` (info `Q-5-41` non-member notice, `Q-5-42` ignored option).
- `quarto_core::render_chapter_alone(project: &ProjectContext, abs_path: &Path, format: &Format, options: &RenderToFileOptions, runtime: Arc<dyn SystemRuntime>) -> Result<RenderToFileResult>` (native only). Also `project::book::prepare_page_in_book(&mut ProjectContext, active, &Format)` (pub(crate)), the shared "page of a book" prep the browser request now calls too, and `pub(crate) render_chapter_alone_summary` returning the whole `ProjectRenderSummary` (the orchestrator uses it).
- CLI: `q2 render --book-scope book|chapter`; `chapter` needs exactly one file (a project directory, zero or several files error with "`--book-scope chapter` needs exactly one file"). Orchestrator dispatch `run_book_scoped` (typst/epub): resolves against the single named file; no or several files render the whole book as before.
- Error catalog: `Q-5-41` (page not part of the book, info), `Q-5-42` (scope ignored for this format, info), with docs pages and `docs/_quarto.yml` entries; `docs/guides/projects/book.qmd` has a "Rendering one chapter" section; `hub-client/changelog.md` has an entry (`test:wasm` run, green).

**Deviations from the plan, and why.**

1. `render_chapter_alone` and `resolve_book_scope` take a `runtime` parameter the plan's signatures omitted (canonicalization, chapter-list reads).
2. Non-consolidating formats in a book (html, docx, pptx): the resolver returns `Chapter` for a member and `Page` for a non-member, not `Page` for both. The plan's "resolves to `Page`" would have changed the echoed `stats.book.scope` of the pinned test `docx_and_pptx_on_a_chapter_stay_chapter_only_without_a_warning`, which the plan says must pass unchanged. No notice either way (except `Q-5-42` for an explicit `Some(_)`).
3. `Q-5-42` for `Some(_)` on a non-consolidating format is emitted by the resolver (browser path, book projects only) and by `run_with_book_support` (CLI, any project kind). A single document outside a project in the browser (`is_single_file`) gets none (the hub sends a scope only for consolidating formats).
4. In `pandoc_request_projects.rs` the test helper `render()` sent `Some(Chapter)` for every format; it now sends it only for typst, as hub-client will, so docx tests stay silent.
5. The two rewritten tests (`a_page_in_the_project_but_not_in_the_book_renders_alone`, `a_page_excluded_from_the_render_list_renders_alone`) now expect `Page` plus `Q-5-41` for typst/epub/typst-pdf (and `Page` without a notice for docx).
6. The CLI "default typst renders the whole book" test uses epub (same dispatch, no network); typst compiles need the orange-book download. The typst chapter-alone CLI test uses `output-ext: typ`, which skips the compile.
7. The wasm crate (excluded from the workspace) was verified with `cargo check --lib --target wasm32-unknown-unknown` using the C-toolchain env in `dev-docs/wasm.md`, not clippy or `npm run build:all`.

**Spike result (3.6).** Native chapter-alone and the browser request, replayed through pandoc, produce the same `.typ` (`one.qmd`, `sub/two.qmd`, `titleonly.qmd`, `app.qmd` of the parity book) and the same EPUB entries (`titleonly.qmd`). Nothing in `render_chapter_alone` needed fixing: book title/author/date, the part/appendix context, numbering and the typst template invocation already match. One known difference, pinned in `chapter_alone_typst_request_equals_native`: a link to another chapter is `#link("../one.pdf")` natively and `#link("../one.qmd")` in the browser request (native rewrites sibling `.qmd` hrefs to their output; the request pipeline does not). Not a Plan 3 change; whoever owns link parity (Plan 5 or later) should decide which is right for a standalone chapter.

**What the next plans must know.**

- Plan 5: the wasm shim still maps `'auto'` to `None` and `'chapter'` to `Some(Chapter)` (`parse_request_options`); replace it with `'book' | 'chapter'`. `stats.book.scope` can now be `"page"` (non-member): the TS types in `downloadController.ts`, `ts-packages/preview-runtime/src/wasm-quarto-hub-client.d.ts` and the wasm tests still say `'book' | 'chapter'`; the client only tests `=== 'book'`, so nothing breaks, but the stale-click notice should key on the `Q-5-41` diagnostic, which the request outcome now carries first in its diagnostics.
- Plans 9 and 10: call `resolve_book_scope` then `render_chapter_alone` for `Chapter`/`Page`; deliver `resolution.diagnostics`. `render_chapter_alone` sets `format_override` from `format.identifier`, so a format key like `typst-pdf` is not understood there; pass the base `Format`.
- Whole-book typst with a non-`pdf` `output-ext` still goes through the merge path, which ignores it and compiles (`Q-21-3` on a `.typ` request); that is Plan 6/10 territory, unchanged here.
- `book_resolution` (the browser's menu resolver) now takes `chapter` from `book_membership`; its `chapters` list still uses sidecar keys. The test `the_resolver_and_the_exports_auto_scope_agree_for_every_file_of_the_book` passes.
