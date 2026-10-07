# `format: typst` / `format: pdf` / `output-ext` parity (Q1, native, wasm)

**Branch:** `fix/typst-pdf-format-parity` (from `feature/pandoc-wasm` @ 3dfa5b296; **rebase onto the tip first**: H5's post-STOP wire-ups, including the R9 book menu, landed at `0ae5119bc` and change the hub-client sites below; see "After H5's wire-ups")

## Goal

Q1 feature parity for the typst-family formats on both Q2 runtimes.

## Q1 behaviour (reference; `quarto-cli` `src/format/typst/format-typst.ts`, `src/command/render/output-typst.ts`)

- `format: typst` → pandoc `-t typst`, then `typst compile` → `<stem>.pdf`. `typstFormat()` calls
  `createFormat("Typst", "pdf", …)`, which sets `render.output-ext: pdf`.
- `output-ext` is a free string. The compile recipe is chosen only when
  `to == typst && output-ext == pdf` (`useTypstPdfOutputRecipe`, `output-typst.ts:203`). Any other
  value (conventionally `typ`) means "stop after pandoc": output is `<stem>.<ext>`.
- `keep-typ` keeps the intermediate `.typ` when the PDF is also produced; no effect with `output-ext: typ`.
- `format: pdf` is LaTeX (`pdf-engine`), not typst.

## Current state (surveyed 2026-10-03)

| | native | wasm |
|---|---|---|
| `format: typst` | compiles → `.pdf` (`TypstCompileStage`) ✔ | no compile: `typst` key is a `.typ` download row. Since H5's R4 wire-up (`0ae5119bc`) the menu offers it, so `classifyPreviewMode` gives a `format: typst` document the click-only "Download Typst source" pane (before: DOM preview) ✘ |
| `format: pdf` | parses, no pipeline (latex epic owns it) | built as typst + `RequestPost::CompileTypst` (`pandoc_request/render.rs:92-100`) — wrongly aliased ✘ |
| `output-ext` | schema only, ignored ✘ | ignored ✘ |
| `keep-typ` | read from metadata in `typst_compile.rs:274` ✔ | n/a (wasm has no intermediate to keep) |

## Decisions (Gordon, 2026-10-03)

1. `format: pdf` means nothing in wasm and never will; on native it will mean LaTeX (separate epic, under
   review). In wasm it is an **error** whose message points the user at `format: typst`.
2. `output-ext` follows Q1 semantics. LaTeX support for it is the latex epic's job.
3. In wasm, `format: typst` previews as PDF and downloads PDF; the typst-source download is selected by a
   non-`pdf` `output-ext`, and the file gets that extension literally (`output-ext: typst` → `.typst`), as in Q1.

## Design

Semantics, shared by both runtimes: for typst, effective ext = `output-ext` from the format's options,
default `pdf`. `pdf` → pandoc + compile. Anything else → pandoc output only, written as `<stem>.<ext>`.

### Shared: one `output-ext` lookup (in `quarto-core`)

A pure function finding `output-ext` for a resolved format key. Precedence, highest first: document
`format.<key>.output-ext`; document top-level `output-ext`; project `format.<key>`; project top-level
(Q1 builds a base format from top-level metadata and merges each format's options over it,
`render-contexts.ts:119`; project side can reuse `resolve_format_config`). Used by native `render_to_file`
and wasm `resolve_formats`, so both runtimes support all four layers with one implementation. Pick one YAML
parser for the document side (`formats.rs` uses `yaml_rust2`, `format.rs` uses `serde_yaml`; both already
compile for wasm).

Known gap, shared with format-key resolution today: directory `_metadata.yml`, included metadata and `-M`
flags are not seen by this pre-pipeline lookup (Q1 merges them). Document it; do not solve it here.

Semantics, verified against Q1 by running it (`format: typst` + `output-ext: foo` → `a.foo` containing typst
source, nothing compiled): `pdf` (the default) → pandoc + `typst compile`; **any other value, taken
literally** → pandoc output only, written as `<stem>.<value>`. `typ` has no special status.

### Native (resolve once, before the pipeline)

`render_document_to_file` (`render_to_file.rs:~285-317`) already resolves the format key before the pipeline,
because it decides output path and extension. Resolve `output-ext` in the same step, then:
- override `render_format.output_extension` and pass the extension into `determine_output_paths` (it
  currently re-derives it from the format name);
- `doc_info.with_output(&output_path)` then carries it into `ctx.output_path()`.

**(Corrected during implementation: `PandocWriteStage` did need a change — with `output-ext: foo` it wrote `<stem>.typ`, not `<stem>.foo`, so it now writes the intermediate only when `output_extension == "pdf"`; `TypstCompileStage` skips explicitly on non-`pdf`.)** Originally: **No stage changes expected:** `pandoc_write.rs:993` (`.with_extension("typ")`) is a no-op when the output is
already `.typ`-named, and `TypstCompileStage` already returns early when `typ_input == pdf_output`
(`typst_compile.rs:~99`, today used by tests passing an explicit `.typ` output). With `output-ext: foo` the
intermediate is still `.typ`, so confirm the early return still triggers (`typ_input != pdf_output`) and
make the stage skip explicitly on non-`pdf` instead of relying on path equality.

Other sites to update: `project/pass2_renderer.rs:1453` (resolver anchor path only), and
`quarto-test/src/runner.rs:480,622` (expected-extension computation; otherwise `output-ext` fixtures look
for `.pdf`). Check the project-render path (orchestrator → `render_document_to_file`) picks the override up.
Rejected: building `Format` after metadata merge (~40 caller sites), re-reading metadata at each site
(drift), compile-stage-only read (earlier path computations would still say `.pdf`).

Leave `format: pdf` alone on native (fails cleanly today: `Format 'pdf' is not yet supported`; latex epic).

### Wasm

- `resolve_formats` (`pandoc_request/formats.rs`) uses the shared lookup:
  - `typst`, value `pdf`/absent → resolved key `pdf` (the compile chain; existing PDF preview/download).
  - `typst`, any other value → key `typst` (typst source download) **with that value as the file extension**
    (Q1 literal: `output-ext: typst` → `doc.typst`). `ResolvedFormat` gains an optional `extension`; the
    hub-client download state uses it instead of the table row's `typ` for the file name
    (`downloadController.ts`: the two `sanitizeDownloadName(nameFrom, format.extension)` sites, one for the plain
    download and one for the PDF chain; `nameFrom` is the book's output path for a whole-book download, else the
    document path). Mime stays `text/plain`. See "After H5's wire-ups" for the two paths that do not go through
    the menu row.
  - document-level `pdf` → class `neither`, with the "try `format: typst`" message.
- `render_pandoc_request` keeps treating the `pdf` *input key* as typst+compile (the chain's internal key);
  a document's own `format: pdf` never reaches it (error at resolution).
- hub-client: pdf-specific `neither` copy in `strings.ts`; `selectMenuFormats` comment/embed note stays
  accurate. The menu's "Download as" for a typst-source row keeps the table's `typ` unless the document
  carries its own `output-ext`.

## After H5's wire-ups (landed `0ae5119bc`, 2026-10-03)

Read before rebasing; the plan above was written before these landed.

- **Textual conflicts.** `downloadController.ts` (the two file-name sites, a new `buildRequest` sixth argument,
  `StartOptions.scope`/`captureDocIds`, `done.book`), `downloadService.ts` (`MENU_FORMATS` is now every table row,
  with a rewritten doc comment; `wasmDeps` forwards `scope`/`capturesByPath`/`onProgress`), `useDownloadAs.ts`,
  `DownloadAsControl.tsx`. Nothing in `PreviewRouter.tsx`, `getQ2Format.ts`, `strings.ts`\'s `neither` copy or Rust
  changed, so the classifier and Rust tasks rebase cleanly.
- **`ResolvedFormat.extension` must reach three consumers, not one:** (1) the menu's `DownloadFormat` for a
  `typst` row; (2) `DownloadModePane` in `PreviewRouter.tsx`, which builds its format from `formatByKey(formatKey)`
  (the table row, so the click-only button would name the file `.typ` whatever `output-ext` says); (3) the
  whole-book download: `output-ext: typst` on a book chapter's "Download book as Typst source" is named from
  `basename(request.output_path)` for the stem but `format.extension` for the extension, so without the override
  it is `.typ` while Rust wrote `.typst`. Add a wasm test for (3) beside `downloadBook.wasm.test.ts`.
- **Book menu.** `BOOK_FORMATS = ['typst','pdf','epub']` in `DownloadAsControl.tsx` is keyed on the *menu* key.
  A `format: typst` book chapter resolves to key `pdf` under this plan, which is a whole-book PDF by default
  (`scope: 'auto'`); with a non-`pdf` `output-ext` the key is `typst`, so "Download book as Typst source" is
  offered and must produce one book-wide typst source file named with that extension. Check that the Rust book path honours
  `output-ext` (it should, since it shares the request builder; not tested).
- **Resolver `book` field is independent** of resolved keys (`bookInfoFrom` reads only `book`), so the new
  `extension` field is additive there.
- **Still true:** the menu and resolution accept the `pdf` key; a document's own `format: pdf` reaches the download
  path only through `formatByKey`, which the "error at resolution" rule catches first; the PDF preview pane's
  controller stays chapter-alone (`createPdfPreviewController` defaults to scope `'chapter'`).
- **Tests that pin the current table:** `downloadService.test.ts` asserts `MENU_FORMATS` equals all five rows and
  that the embed keeps only docx, pptx, epub; hiding or renaming a row breaks it.

## Tasks

- [x] Native: confirm what `format: pdf` actually does today — `q2 render` (main build, 2026-10-01) fails cleanly with
      `Error: Format 'pdf' is not yet supported.` (plus a backtrace dump, which is noise); `format: typst` → `.pdf` as expected.
      Nothing to change on native for `pdf`.
- [x] Shared: `output-ext` lookup function in `quarto-core` + unit tests (4 precedence layers, non-string values)
- [x] Native: resolve in `render_document_to_file`; override `output_extension`; explicit skip in `TypstCompileStage`;
      `pass2_renderer`, `quarto-test` runner; integration tests (`pandoc_typst_compile.rs`: `pdf`, `typ`, `foo`)
- [x] Wasm: `resolve_formats` uses the shared lookup; `ResolvedFormat.extension`; unit tests in `formats.rs`
- [x] Wasm: document `format: pdf` → `neither` + message; `strings.ts`; vitest for `classifyPreviewMode`
- [x] Wasm: `format: typst` doc previews as PDF; `output-ext: <other>` doc downloads `<stem>.<other>` (hub-client
      download state uses `ResolvedFormat.extension`); vitest + wasm test for the file name
- [x] Docs: note `output-ext` for typst in user docs; note `format: pdf` status
- [x] Phase-boundary `cargo nextest run --workspace`: 15750 passed, 202 skipped (2026-10-03); hub-client vitest 1510 passed, wasm suite 236 passed / 124 skipped

## Deferred: rename the wasm `pdf` artifact key (after R9 and H10)

Not part of this branch's first pass. The wasm `pdf` string means two things (a document's `format:` value;
an internal "typst compiled to PDF" artifact/menu key). Renaming the artifact key (e.g. `typst-pdf`) makes a
document's `format: pdf` fall into `neither` with no special case, and stops a document from naming the
artifact key. It touches the same download/preview code that R9 (whole-book download,
`pandoc-wasm/r9-whole-book`) and H10 (warm executor, `pandoc-wasm/h10a-warm-executor`) are changing, so it
**lands after both**, as its own commit. (R9 landed 2026-10-03 and H10a is closed; only H10b remains.)

Until then, the wasm work above uses an explicit rule: *the `pdf` key is valid from the menu and from
resolution of a `typst` document, never from a document's own `format:`.* Keep that rule in one place
(`resolve_formats`) so the rename can delete it.

**The implementer must re-survey before renaming.** My count (2026-10-03, pre-R9/H10) was ~13 production
sites plus ~25 test references:
- R9's UI additions (landed `0ae5119bc`, after this list was made): `DownloadAsControl.tsx` (`BOOK_FORMATS`
  holds the string `pdf`), `downloadController.ts` (`format.key === 'pdf'` also gates the font-listing prelude,
  which now runs before the capture fetch and the scoped `buildRequest` call), and `pdf` fixtures in
  `downloadController.test.ts` and `DownloadAsControl.integration.test.tsx` (the `PDF2` format). No persisted
  state holds the string.
- Rust: `pandoc_request/formats.rs` (table row + tests), `pandoc_request/render.rs` (`compile_typst` check).
- hub-client: `getQ2Format.ts`, `PreviewRouter.tsx`, `downloadService.ts` (`MENU_FORMATS`),
  `downloadController.ts` (`format.key === 'pdf'`), `PdfPreviewPane.tsx`, `DownloadAsControl.tsx`.
- Tests: `downloadService`, `getQ2Format`, `pandocRequest.wasm`, `downloadController`,
  `previewModeClassification.wasm`, `DownloadAsControl`, `PdfPreviewPane`.
- Unchecked: `ts-packages/quarto-api` / automerge-schema hits look like unrelated generic uses.
- Unchecked risk: any persisted state (e.g. a saved "last download format") holding the string `pdf` would
  silently stop matching. R9/H10 may add new sites; do not trust this list, grep again on the then-current tree.

- [x] Wasm: carry `ResolvedFormat.extension` to the own-format button (`DownloadModePane`) and the whole-book
      download as well as the menu (see "After H5's wire-ups"); wasm test for the book file name
- [x] Fresh survey of `'pdf'`/`"pdf"` key uses; rename the artifact key to `typst-pdf` (H10b is on the feature tip,
      nothing was blocking). Rust: `TYPST_PDF_KEY` in `pandoc_request/formats.rs` (table row, `resolve_declared`,
      `render.rs`); hub-client: `pandoc/formatKeys.ts` (`TYPST_PDF_KEY`) used by `getQ2Format`, `downloadService`,
      `downloadController`, `DownloadAsControl`, `PdfPreviewPane`, `test-hooks`; tests updated. No persisted state
      holds the key (grep of localStorage/IndexedDB uses found none). `format: pdf` now falls to `neither` with no
      special case; the one remaining guard is that a document naming `typst-pdf` itself is `neither`. `strings.ts`
      still special-cases a document's `format: pdf` (that is the declared LaTeX format, not the key).
      Gates: quarto-core clippy + nextest, hub-client tsc/vitest 1510/wasm 333 passed (28 skipped); workspace nextest
      15751 passed, 202 skipped (previous run 15750/202; +1 is the new `a_document_cannot_name_the_artifact_key`).

## Open questions

None blocking. (Resolved 2026-10-03: native extension resolved once pre-pipeline; wasm reads the shared
lookup incl. top-level `output-ext`; non-`pdf` values are literal on both runtimes, matching Q1.)
