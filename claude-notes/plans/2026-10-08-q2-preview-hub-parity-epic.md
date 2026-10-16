# Epic: `q2 preview` reaches quarto-hub parity for typst/PDF, downloads and import

**Date:** 2026-10-08
**Integration branch:** `feature/q2-preview-hub-parity`, checked out in `.worktrees/workspace-7`. Cut from `feature/pandoc-wasm-2`; that branch has since merged, so the integration branch is rebased onto `main` before development starts (done by Gordon, outside the plans). Not pushed; the whole epic lands as one PR.
**Status:** all thirteen plans implemented and landed (2026-10-08). The plans were split on 2026-10-08 for parallel development by up to three workers; Plan 13 closed the epic.

## Why this epic exists

The pandoc-wasm epic gave hub-client "Download as" and a PDF preview, all in the browser. Its design D7 kept pandoc.wasm out of the `q2 preview` embed and gave it a thin native route (H4b: `POST /api/preview/render`, docx/pptx/epub, no captures, no authorship, no books, no typst); the document-import epic then declared the embed out of scope. Together these leave `q2 preview` (both frontends) behind hub-client. Gordon's scope decision (2026-10-08): improvements to `--ui editor` and `--share` that bring them to parity with quarto-hub, and all aspects of `q2 preview` parity with quarto-hub in typst, are in scope. Design principle (E7): all runtimes have the same functionality, and rules live once in `quarto-core`.

## Gap table

| Capability | hub-client | `--ui editor` embed | `--ui viewer` SPA | Closed by |
|---|---|---|---|---|
| PDF preview of `format: typst` | yes (pdf.js) | no | no ("no live preview" overlay); `--static` opens the raw `.pdf` with no reload | Plans 4, 9, 11 (viewer SPA), 8 (`--static`), 12 (embed) |
| Download as PDF | yes | no | none | Plans 9, 12 |
| Download typst source (`.typ`, `output-ext`) | yes | no | none | Plans 9, 12 (end to end: 13) |
| Book scope: "Download book as X" and "This chapter only" (explicit option, E11; superseded in part by the format-resolution epic: now the `book-scope` metadata key) | yes (R9; implicit `auto`) | no (`bookInfoFor` gated off) | none | Plans 3, 5, 10, 13 |
| Code-cell outputs and images in downloads | yes (captures) | no (`ExecutionPolicy::None`; docx/epub from the route lose images outside the project, Plan 1 1.2.4) | none | Plans 2, 6, 9 |
| odt download | yes | no (`NATIVE_FORMATS` = docx, pptx, epub) | none | Plans 9, 12 |
| Word comment/track-change authors | yes | no ("unknown") | none | Plans 9, 12 |
| Unexecuted-cells notice | yes | no | none | Plans 6, 9, 11, 12 |
| Import (docx/odt/rtf/epub/pptx to qmd) | yes | hidden | none | Plans 1, 7, 13 |

Not a gap: remote images in typst (native fetches them; the wasm path fails until its prefetch lands).

**After the epic (Plan 13.8).** Every row above is closed in the embed and, where E5 says so, in the viewer SPA and `--static`; the Plan 13 end-to-end specs (`q2-preview-spa/e2e/embed-books.spec.ts`, `embed-import.spec.ts`) cover books, the `.typ` download on a chapter, import and `--join`. Remaining, documented differences: (a) hub and `q2 preview` PDFs are not byte-identical (E4); (b) a whole-book download fails when a chapter `embed`s a cell from another chapter, because the route never executes code and a recorded capture is not spliced into an embed (reported by Plan 10, not reproduced in Plan 13; chapter-alone works); (c) a book download renders other chapters from disk, runs no pre/post-render scripts and ignores the book's `output-ext`; (d) hub-client stores imported images only up to 10 MB, as in the browser. Plan 13 also fixed a bug the plans did not predict: the format resolver's `book.chapter` flag was false for the project-relative paths the hub client sends, so the book menu entries never appeared (now canonicalized in `quarto-core`).

## Decisions

- **E1 One mechanism for the embed: native routes.** They never launch kernels (`ExecutionPolicy::None` plus the AST capture splice). The download route grows into a general one (format key from the same Rust table, `attributionJson`, `scope`); `POST /api/preview/pdf` stays the supersede-aware inline variant; both share one render helper and one response contract (`X-Q2-Diagnostics`, `X-Q2-Stats`, structured failure diagnostics). Plan 9.
  Note: the route's `scope` field was replaced by a generic `metadata` object; see [`2026-10-15-format-resolution-epic.md`](2026-10-15-format-resolution-epic.md) (FR7, F4).
- **E2 Typst source download** uses the E8 option (Plans 6, 9, 10).
- **E3 Book downloads** render the whole project natively through the project pipeline (Plan 10).
- **E4 Output identity.** Hub PDFs (pinned wasm typst, bundled fonts) and `q2 preview` PDFs (system typst >= 0.8, system fonts, real packages) need not be byte-identical; document it, do not chase pixel parity.
- **E5 Viewer scope.** The viewer gets the PDF preview (Plan 11) and nothing else: no Download as, no Import.
- **E6 Import in the embed** is in scope (Plan 7); it is not blocked on `feature/hub-import`.
- **E7 Runtime consistency.** Where code can express "all runtimes behave the same", do it once in `quarto-core` and have both sides call it (one child runner, one scope resolver, one table of formats).
- **E8 Typst target.** `RenderToFileOptions.typst_target: Option<TypstTarget>` (`Pdf` | `Source`). `Pdf` forces the `pdf` extension regardless of `output-ext` (the browser's `typst-pdf` never refuses; no 422 natively); `Source` uses `output_ext.or("typ")` (a literal `output-ext: typst` is kept) and skips the compile; unset keeps `q2 render` unchanged. Plan 6.
- **E9 Preview directory.** Typst renders go to `<ProjectContext::discover(abs_path).dir>/.quarto/preview-out/<uuid>/` (typst `--root` is the discovered project dir); gitignored, swept at start (age cutoff), guard inside the blocking closure; read-only project gets a clear error, no fallback. Non-typst formats use an OS temp dir once Plan 2 fixes image resolution there. Plan 6.
- **E10 Captures resolved server-side only**, through the capture sidecar entry for the request's path. No capture ids on any request. Capture plot files are written into the request directory, never beside the source; typst and pandoc resolve them there (Plans 2, 6).
- **E11 Book scope is an explicit option, shared by every runtime.** `BookScope { Book, Chapter }` lives in `quarto-core` and is exposed, as `Option<BookScope>` (absent means the user did not choose, and behaves as `Book` wherever the format consolidates a book), everywhere scope used to be implied: `q2 render --book-scope book|chapter` (`chapter` needs exactly one file), the wasm request option (`'book' | 'chapter'`, replacing `'auto' | 'chapter'`), hub-client's existing Download as menu ("Download book as X" sends `book`, "This chapter only" and the plain entry of a consolidating format send `chapter`, other formats send nothing), and a `scope` field on the native routes. The preview SPA and PDF pane send `chapter` explicitly; `--static` passes `book`. The option exists only for formats that consolidate a book into one file (typst, pdf, epub). html, docx and pptx have no whole-book output, so the option is unavailable for them (not offered in the menu; an explicit value is ignored with an info diagnostic on the CLI and the routes, an absent one is silent), and `q2 render ch1.qmd --to html` stays per page. Book projects default to book scope wherever the format supports it, so `q2 render ch1.qmd --to typst` keeps rendering the whole book, as today; `--book-scope chapter` is the new capability. `resolve_book_scope` (Plan 3) turns (project, path, scope, format) into Book / Chapter / Page. A page that is not a member of the book is delivered as a standalone document with a visible notice (when `book` was asked for or nothing was chosen; asking for `chapter` on a non-member is just that page), never refused and never silently downgraded; Rust owns the notice, so a stale menu click in hub-client gets it too. This intentionally changes today's `q2 render other.qmd --to typst` for a non-member page (whole book to a standalone page). Chapter-alone is one core entry, `render_chapter_alone(project, path, format, &RenderToFileOptions)`, built on the browser's mechanism; the CLI and Plan 9's routes both call it. Book downloads run Pass 1, `pre_render` and the shared `render_book_single_file` against a cloned `ProjectContext` whose `output_dir` is the request directory, with no tail, so `_book/` and the render manifests are untouched (Plan 10); the active chapter's text comes through a `SystemRuntime` overlay wrapper; per-chapter captures follow the browser's `PartialKind::Request` model with a materialize directory.
  Superseded in part by [`2026-10-15-format-resolution-epic.md`](2026-10-15-format-resolution-epic.md): scope is the `book-scope` metadata key, supplied through a render overlay, and its default comes from what is rendered (a named chapter, or several, render each on their own; `--to` does not change the scope); `--book-scope`, the `scope` request fields and `Option<BookScope>` plumbing are gone.
- **E12 `--static`** has no render routes and never loads the SPA; it gets a Quarto-1-style pdf.js page (`viewer.html`, reload over the existing SSE channel) for typst output (Plan 8).
- **E13 Plan split for parallel work (2026-10-08).** Each plan waits only on whole other plans, never on a task inside itself, so a worker can start it when its gates are merged and finish without waiting. Task IDs are `<plan>.<n>` and restart in each plan; the old-to-new map is below.

## The plans

| # | Plan | Delivers | Waits on |
|---|---|---|---|
| 1 | [`p01-child-runner`](2026-10-08-q2-preview-p01-child-runner.md) | One child-process helper with deadline and cancel; public `resolve_and_gate_pandoc` | none |
| 2 | [`p02-core-hygiene`](2026-10-08-q2-preview-p02-core-hygiene.md) | Image fix for outputs outside the source tree; temp-dir leak fix | none |
| 3 | [`p03-book-scope-core`](2026-10-08-q2-preview-p03-book-scope-core.md) | `BookScope`, `resolve_book_scope`, `render_chapter_alone`, `q2 render --book-scope` | none |
| 4 | [`p04-pdfjs-packaging`](2026-10-08-q2-preview-p04-pdfjs-packaging.md) | pdf.js fetched, packaged, served at `/pdfjs/`; `ts-packages/pdf-viewer`; `.mjs` MIME | none |
| 5 | [`p05-book-scope-clients`](2026-10-08-q2-preview-p05-book-scope-clients.md) | Wasm option and hub-client types, explicit menu scope, stale-click notice | 3 |
| 6 | [`p06-typst-render-foundations`](2026-10-08-q2-preview-p06-typst-render-foundations.md) | `typst_target`, captures splice, unexecuted count, request directory | 2 |
| 7 | [`p07-embed-import`](2026-10-08-q2-preview-p07-embed-import.md) | Import route and native runner for the embed | 1 |
| 8 | [`p08-static-pdf-viewer`](2026-10-08-q2-preview-p08-static-pdf-viewer.md) | `--static` pdf.js viewer with reload | 4 |
| 9 | [`p09-preview-routes`](2026-10-08-q2-preview-p09-preview-routes.md) | Shared helper and contract, `/api/preview/pdf`, general download route | 1, 3, 6 |
| 10 | [`p10-book-downloads-server`](2026-10-08-q2-preview-p10-book-downloads-server.md) | Native book entry, overlay runtime, book branch of the route | 1, 3, 6, 9 |
| 11 | [`p11-spa-pdf-pane`](2026-10-08-q2-preview-p11-spa-pdf-pane.md) | PDF pane, Back button, e2e in the viewer SPA | 4, 9 |
| 12 | [`p12-embed-client`](2026-10-08-q2-preview-p12-embed-client.md) | Embed downloads, PDF preview, embed test harness | 4, 5, 9 |
| 13 | [`p13-embed-books-source-e2e-docs`](2026-10-08-q2-preview-p13-embed-books-source-e2e-docs.md) | Book scope in the embed, end-to-end checks, user docs, close-out | 7, 8, 10, 11, 12 |

Direct dependencies (an arrow means "must be merged before"):

```
1 -> 7, 9, 10          2 -> 6              3 -> 5, 9, 10
4 -> 8, 11, 12         5 -> 12             6 -> 9, 10
9 -> 10, 11, 12        7 -> 13             8 -> 13         10 -> 13
11 -> 13               12 -> 13
```

Critical path: 2 -> 6 -> 9 -> 10 -> 13 (and 9 -> 12 -> 13). Plans 1, 2, 3 and 4 start immediately.

## Execution model

- **Where.** The integration branch `feature/q2-preview-hub-parity` lives in `.worktrees/workspace-7` and nobody develops there: it holds a clean tree and only receives fast-forward merges. Three lanes, one per worktree (`.worktrees/workspace-1`, `workspace-2`, `workspace-3`), each run by one agent per plan, one plan at a time, with no orchestrator outside the plans.
- **Taking a plan.** Lanes are binding (see the lanes below and each plan's `Lane`, `Start gate` and `Next in lane` lines). An agent starts its plan only when the plan's start gate prints `ready`; otherwise it reports the missing plans and stops, and Gordon restarts the lane when they have landed. Each plan's agent reads this epic, its plan, and the Handoff log of every plan it waits on.
- **Starting.** In the lane's worktree, after the start gate passes: `git switch -c q2-preview/pNN-<slug> feature/q2-preview-hub-parity` (the previous plan's branch stays; switching away from it retires it). The worktree's own `CLAUDE.local.md` may describe older work; the epic and the plan govern. Record the workspace-nextest baseline in the plan's first task.
- **Landing (per plan, once; Plan 9's helper may land early if another lane is waiting).**
  1. Every task passes its gate: `cargo clippy -p <crate> --all-targets -- -D warnings` plus `cargo nextest run -p <crate>` for Rust; `tsc` plus `vitest` in the touched TS package.
  2. `git rebase feature/q2-preview-hub-parity` (linear history, no merge commits). After any conflict, sweep the whole tree for conflict markers (`git grep -nE '^(<<<<<<<|=======|>>>>>>>)'`) before committing.
  3. `cargo nextest run --workspace` once; report its pass/skip delta against the plan's baseline (the ledger entry for the tip you branched from, see "Workspace nextest ledger"), and account for it. This run covers exactly the tree that step 5 fast-forwards, so write its counts in the Handoff log as a line `Ledger: <landed tip sha>: <P> passed, <S> skipped`; if step 5 sends you back to step 2, rerun and update the line.
  4. Reconcile the plan's checklist against what landed (re-read it, verify each box) and commit it.
  5. `git -C ../workspace-7 merge --ff-only q2-preview/pNN-<slug>`. If the fast-forward fails because another plan landed first, go back to step 2 (rerun the crate gates for what the rebase pulled in, and the workspace run if the rebase touched the same crates).
  6. Do not delete the plan branch (later start gates test `branch --merged`). Report to Gordon: what landed, the nextest delta, and the Handoff log's headline; he ticks the epic checklist in workspace-7. Agents do not edit the epic, so no two lanes conflict on it.
- **Workspace nextest ledger.** A full workspace run takes about ten minutes and, started by three lanes at once, saturates the machine; so it is run once per distinct tip, not once per plan start. The ledger is the set of `Ledger: <sha>: <P> passed, <S> skipped` lines in the Handoff logs on the integration branch (`git grep -n "^Ledger:" feature/q2-preview-hub-parity -- claude-notes/plans`), plus this seed. A plan's baseline task looks up the entry for the tip it branches from (`git rev-parse feature/q2-preview-hub-parity`; if the tip is a fast-forward of an entry's sha by commits that only add plan notes, the entry still holds) and runs the workspace only if there is none. Plans 1, 2 and 3 ran their own baselines at the seed tip.
  - Seed: `Ledger: 135a42b69: 15990 passed, 202 skipped` (measured in workspace-2 on 2026-10-08, `--no-fail-fast`, 2 slow).
- **Never push.** Nothing is pushed until the whole epic is done and Gordon approves.
- **Hot files** that more than one plan touches, so expect rebase conflicts there: `render_to_file.rs` (2, 6, 9), `pipeline.rs` (1, 2, 6), `stage/context.rs` and the `RenderToFileOptions` struct (1, 2, 6, 9: each adds fields; Plan 1 waits on none of the others, so expect mechanical add-add conflicts), `render_download.rs` (9, 10), `orchestrator.rs` (3, 10), `single_file_render.rs` (10; Plan 3 only reaches it through the orchestrator), wasm `lib.rs` (3, 5), `downloadController.ts` / `downloadService.ts` / `nativeRender.ts` (5, 12, 13), `useDownloadAs.ts` (5, 13), and, with no dependency between them, `q2-preview-spa-e2e.yml` and its `paths:` filter (11, 12) plus `playwright.config.ts` (12): whichever of 11 and 12 lands second keeps both sets of steps and paths. The dependency order serialises the rest.
- **Lanes** (binding; each line is one worktree's order):
  - lane A (workspace-1): 3, 5, then 10 (after 9), then 13
  - lane B (workspace-2): 2, 6, 9, then 12 (after 4 and 5)
  - lane C (workspace-3): 1, 4, 7, 8, then 11 (after 9)

  Lanes A and C idle at Plans 10 and 11 until lane B lands Plan 9, and lane B waits at Plan 12 for lane C's Plan 4 and lane A's Plan 5; land Plans 1, 3, 4 and 9 promptly, since other lanes wait on them.

## Old-to-new task map (for readers of earlier notes and review records)

| Old | New |
|---|---|
| Old Plan 1 Phase 0: 0.2 characterisation, 0.3 leak, 0.5 `.mjs` | 6.2, 2.4, 4.2 |
| Old 1.1.A / 1.1.B / 1.1.D / 1.1.E | 3.6 / 2.2 / 6.9 / 6.3 |
| Old 1.1.C (static needs no SPA detection) | resolved; see Plan 8 |
| Old 1.2.1 `typst_target`; 1.2.2 captures; 1.2.3 unexecuted; 1.2.4 images; 1.2.5 timeouts; 1.2.6 build; 1.2.7 scope | 6.3; 6.4; 6.5; 2.3; 1.2-1.3; 6.6; 3.2-3.7 (clients: Plan 5) |
| Old 1.3 request directory | 6.7-6.9 |
| Old 1.4 server | 9.2-9.11 |
| Old Phase 2 (pdf.js, 2.1-2.4) | 4.3-4.6 |
| Old Phase 3 (SPA, 3.1-3.10) | 11.2-11.10 (docs: 13.7) |
| Old Phase 4 (static) | Plan 8 |
| Old Plan 2 Phase 0 | 9.1, 12.1 |
| Old Plan 2 Phase 1 (Rust) | 9.7-9.10 |
| Old Plan 2 Phase 2 (client downloads) | 12.2-12.4 |
| Old Plan 2 2.5 (scope in the client) | Plan 5 |
| Old Plan 2 Phase 3 (embed PDF) | 12.5-12.9 |
| Old Plan 2 Phase 4 (harness, CI, e2e, docs) | 12.10-12.13 |
| Old Plan 3 Phase 0 / Phase 1 | 10.1-10.3 / 10.4-10.9 |
| Old Plan 3 Phase 2 / Phase 3 | 13.2-13.4 / 13.5, 13.7 |
| Old Plan 4 Phases 0-2 | 7.1-7.14 |
| Old Plan 4 3.1 / 3.2 | 13.6 / 13.7 |

## Epic-level checklist

(Ticked by Gordon or the orchestrating session in workspace-7 after each plan lands.)

- [x] Plan 1 child runner
- [x] Plan 2 core hygiene
- [x] Plan 3 book scope: core and CLI
- [x] Plan 4 pdf.js packaging
- [x] Plan 5 book scope: clients
- [x] Plan 6 typst render foundations
- [x] Plan 7 embed import
- [x] Plan 8 static PDF viewer
- [x] Plan 9 preview routes
- [x] Plan 10 book downloads, server
- [x] Plan 11 SPA PDF pane
- [x] Plan 12 embed client
- [x] Plan 13 close-out
- [x] `--book-scope` documented for `q2 render`, the Download as menu and the wasm option, and noted in the changelogs; the default keeps today's CLI behaviour, so the pinned test `single_chapter_subset_render_keeps_one_file_render_set` is unchanged
- [x] User docs for `q2 preview` (Plan 13.7)
- [x] Reconcile every plan's checklist against what landed; update the gap table (Plan 13.8)

## Cross-cutting rules

- No braid strands for in-plan work; strands only for things found outside every plan (so far: `bd-j6qvbtbj`, the hub-client non-member page output).
- Per task: `cargo clippy -p <crate> --all-targets -- -D warnings` plus `cargo nextest run -p <crate>` (Rust); `tsc` plus `vitest` in the touched package (TS). Workspace nextest once per plan end (and at any phase boundary a plan names) and before landing, reporting its delta against the baseline taken at the start of the plan.
- New Rust integration tests go in `crates/<crate>/tests/integration/<name>.rs` plus `pub mod` in `main.rs`.
- Tests needing native `typst`/`pandoc` skip with a clear reason when absent. `test-suite.yml` installs typst; `q2-preview-spa-e2e.yml` installs neither and is extended in Plans 11 and 12; the embed's browser tier needs a workflow that builds `q2` with the embedded editor dist and installs pandoc and typst (Plan 12).
- Cross-platform: gate platform-specific test helpers as the repo rules say; never glob `$TMPDIR` (about 46k leaked directories).
- Never push without Gordon's approval.

## Final workspace nextest

`Ledger: ae2887867: 16134 passed, 202 skipped` (Plan 13, `--no-fail-fast`, 2 slow, 2 leaky, 562 s). Against the previous tip's entry (`ccd32719c: 16134 / 202`) the delta is 0 / 0, expected (Plan 13 adds no Rust test). Against the integration branch's first baseline (seed `135a42b69: 15990 / 202`) the epic added 144 passed, 0 skipped.

## Review record

- **Round 1 (Opus, blank slate):** a typst render into an output dir outside the project fails (`--root`), now E9; `ReplayEngine` fails on edited input and writes no files, so captures use the AST splice; `UnexecutedCellCountStage` is absent natively; capture files must not be written into the project; import argv needs a directory parameter and sandboxing; book renders must not overwrite `_book/`.
- **Round 2 (five sonnet agents plus spikes, Gordon's answers):** D12 works for typst (about 0.4 s); `--sandbox` is mandatory for import; the request directory must come from the discovered project; capture plots would be written beside the source; `.quarto/preview-out` needs gitignore, startup sweep and an in-closure guard; non-`pdf` `output-ext` parity (E8); the book path ignores `RenderToFileOptions` and `--output-dir`; one `X-Q2-Stats` and 409 contract; start gates corrected; odt images are lost under `--output-dir`.
- **Round 3 (two sonnet agents, Gordon's answers 2026-10-08):** the image loss comes from link rewriting relative to the output page, not the resource collector, and breaks capture plots (image fix first); once-per-process package extraction is declined; the browser's request-based book path already has scope, hooks, captures and chapter-alone, so the book download reuses it; the book path needs its own extension override and a no-tail entry; the merge tail needs pandoc as well as typst timeouts; `--static` never loads the SPA and gets a Q1-style viewer; the embed needs no pdf.js build step; import remaps the whole temp dir in all output and keeps the xtask builder; missing pandoc is `Q-20-*`.
- **Round 4 (two sonnet agents by layer, plus a book-scope audit and a hub non-member test, Gordon's answers 2026-10-08):** the image fix keeps the rewrite and mirrors the source tree instead of skipping it; book scope becomes an explicit option (E11) after the audit found it smuggled through format x `RenderMode`, `BookScope::Auto`, `extra_render_files`, the CLI file argument and the client's silent downgrade; the book download needs a cloned project with `output_dir` set to the request directory; `.quarto/` is not untouched (pass 1 and the crossref filter write there); one child-runner helper in `quarto-core` serves the render children and import; the static viewer needs an external reload client (its CSP blocks inline scripts), no `classify` change, no sidebar hash, and a locale alias; the embed PDF pane must pass `content`; import needs a diagnostics-only failure path; the Back counter lives in `history.state`; a non-member page is delivered standalone with a notice (the hub's current behaviour is degraded and tracked in `bd-j6qvbtbj`).
- **Round 5 (2026-10-08):** the four plans were split into thirteen for three parallel workers plus an integration worktree (E13); no content decisions changed.
- **Round 6 (2026-10-08, three sonnet agents by lane, each running reproductions, Gordon's answers 2026-10-08):** `--static` refused single-file typst (Plan 8 gains the gate task); a non-member page changes from whole-book to standalone on the CLI, intentionally; scope is `Option<BookScope>` so that docx, pptx and html get a diagnostic only when a scope was passed; `--book-scope chapter` needs exactly one file; the hub-client menu keeps its existing paired entries and the plain entry of a consolidating format sends `chapter`; pdf.js locale pruning applies to the preview binary only, with one shared alias function; typst images fail in website and book projects under the request directory (Plan 2 mirrors with `site_root = project.dir`); `typst_target: Source` also overrides an explicit `output-ext: pdf`; `render_chapter_alone` gets a signature that takes `RenderToFileOptions`, and Plan 9 calls it; the `BookDownload` and render seams are process-wide statics with stated types; the attribution option carries a JSON string; Plan 1 gets named types, the `RenderContext` plumbing and a `None` default deadline; Plan 13 waits on 8 and 11; Plan 5 lists every straggler.
