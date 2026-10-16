# CLI format resolution vs. the metadata-merge layers (and what Q1 does)

Date: 2026-10-09. Found while hand-testing `q2 render` in
`crates/quarto/tests/smoke-all/typst/orange-book` (workspace-7, branch
`feature/q2-preview-hub-parity`). Research only; no code changed.

## 1. The symptom

In a book project whose `_quarto.yml` says `format: typst`:

- `q2 render index.qmd` (also `q2 render`, `q2 render chapter1.qmd`,
  `--book-scope book|chapter`) fails:
  `Error [Q-21-3]: typst compile ... expected content, found array`
  at `@preview/orange-book:0.7.1/lib.typ:584:48` (`author`), and prints
  `Info [Q-5-42] Book scope does not apply to this format`.
- `q2 render index.qmd --to typst` works.
- `q2 preview` chapter scope works.

## 2. Cause

The format is chosen twice, differently:

1. **CLI, early.** `render_once` (`crates/quarto/src/commands/render.rs:926`):
   `--to`, else the *input file's own* front-matter `format:`
   (`detect_single_input_format` → `format_key_from_frontmatter`), else
   `"html"`. The project is not consulted; the project is not even
   discovered yet. Result here: `html`.
2. **Per document, late.** `render_to_file.rs:340-364` calls
   `format::resolve_format_key(project_format, document_format, --to, default)`
   (`format.rs:730`), which prefer-merges `format:` as project → document →
   `--to`. Result here: `typst`. This is why the typst compile still runs.

The book dispatch in `ProjectPipeline::run_with_book_support`
(`project/orchestrator.rs:1043`) and `run_book_scoped` use only the early
`self.format`. `format_consolidates_book(html)` is false, so the book falls
through to `run_inner()`, which compiles each chapter as a standalone typst
document with no book-level metadata. orange-book's `book()` then has no
`author` (default `()`), hence the typst error. The `Q-5-42` notice is emitted
at `orchestrator.rs:1068` for the same reason (explicit scope, format judged
non-consolidating).

## 3. What `MetadataMergeStage` does (and does not) do

`stage/stages/metadata_merge.rs`. It receives an already-chosen
`ctx.format` and merges settings, lowest to highest precedence:

1. extension `contributes.metadata` (non-`project` keys)
2. project config (`ctx.project.config.metadata`)
3. extension format metadata (needs the full `target_format`)
4. `_metadata.yml` chain, root to leaf, via
   `directory_metadata_for_document` (skipped for single-file projects)
5. document front matter
6. runtime metadata

Each layer is first flattened with `resolve_format_config(layer, base_format)`
(`quarto-config/src/format.rs:72`): every top-level key except `format` is
kept, the `format.<base>` block is lifted over them, and the `format` key is
dropped. At the end the stage deletes `format` from the merged result and
writes `ctx.format.target_format` (`metadata_merge.rs:480`).

Consequences:

- The stage never *selects* a format. Format selection is not a duplicated
  piece of it; there is nothing to extract.
- Flattening needs the answer (`base_format`), so the stage cannot be run to
  find the format. Pass 1 (`pass_one_dispatch(..., &self.format, ...)`) is
  format-parameterised too.
- What the stage and a resolver *can* share is layer collection and ordering:
  which layers exist, in what order, which files count. `directory_metadata_for_document`
  already returns the `_metadata.yml` layers as unflattened `ConfigValue`s.

## 4. What Quarto 1 does

`src/command/render/render-contexts.ts`, `resolveFormats` (~line 395), plus
`formatKeys` in `src/config/metadata.ts:140`. Selection is by **key sets**,
not by merging `format:` values:

- Keys come from `formatKeys(metadata)`: the scalar, or the map's keys,
  dropping entries whose value is `null` or `false`.
- Directory `_metadata.yml` keys are treated as input keys.
- Selection:
  1. if the project type has `projectFormatsOnly` (**book** does,
     `project/types/book/book.ts:149`): use the **project's keys only**;
     input and directory `format:` are ignored;
  2. else if input keys ∪ directory keys is non-empty: use those and ignore
     the project's;
  3. else use the project's keys.
- A per-file `formats` filter (`file.formats`) can narrow the list.
- `--to` (`resolveFormatsFromMetadata`): absent → render all selected keys
  (several outputs); `default` → the first; otherwise the named formats are
  added to the list even if no layer declared them; `all` → the list.
- With no keys at all the fallback is `pandoc.to || pandoc.writer || "html"`,
  read from the fully merged metadata (including `metadata-files` includes and
  `--metadata` flags).
- Per-format *settings* are then merged project < directory < input, over the
  writer defaults and extension format metadata. That is the part that
  corresponds to `MetadataMergeStage`.

## 5. Where q2 differs

| | Q1 | q2 `resolve_format_key` / CLI |
|---|---|---|
| Book project | project keys only | CLI: file front matter or `html`; per-doc: prefer-merge incl. document |
| Directory `_metadata.yml` `format:` | input-level override of project | not read at all |
| Several keys | renders all | first key only ("single-format-per-render for now") |
| `null`/`false` format entries | skipped | not skipped |
| `--to` not declared anywhere | allowed, appended | allowed |
| `metadata-files` includes | merged for the fallback | not read |
| Project consulted by CLI | yes | no (only per doc, late) |

## 6. Implications for a fix

- For the reported bug, a **book** needs only the project's `format:` keys
  (Q1 `projectFormatsOnly`). `_metadata.yml` does not matter for books,
  which narrows the minimum fix considerably.
- For non-book projects, `_metadata.yml` format keys do matter (Q1 treats
  them like front matter). A resolver should take (project keys, directory
  keys, input keys, `projectFormatsOnly`, `--to`) and apply the Q1 rule
  above, rather than prefer-merge strings.
- Sharing with the stage is limited to layer collection: a function in
  `quarto-core` that returns the ordered, unflattened layers for
  (project, document path), used by the stage (which then flattens) and by
  the resolver (which reads only `format:` keys). `render_to_file`\'s inline
  resolution and the CLI's early guess should both call the resolver.
- The CLI must discover the project before choosing the format. Today it
  chooses first.
- Open questions: (a) keep "first key only" or adopt Q1's multi-output
  semantics (out of scope for the book bug); (b) whether to read
  `metadata-files` for the fallback; (c) the preview server's format
  selection (`quarto-preview/src/render_helper.rs`) takes an explicit key
  from the request, so it does not hit this, but any new resolver should be
  checked against it.
- Regression test: render the orange-book fixture through the CLI with no
  `--to`. The existing smoke fixture is gated on `knitr` and likely always
  passes `--to`, which is why this went unnoticed.

## 7. Survey of format-deciding sites (2026-10-14)

Scope: every place that decides, or silently assumes, which output format a document (or a whole project render) uses. Research only; nothing was run, so "affected" is from reading the code, not from reproduction.

**Which tree.** The strand's line numbers (render.rs:926, orchestrator.rs:1043, book scope, Q-5-41/39) belong to `feature/q2-preview-hub-parity` as checked out in `workspace-7` (HEAD 9b0933641, clean). The task's worktree (`workspace-3`, branch `braid/bd-7jjzqeys-consistent-format-resolution`, 332e5b8e0) is 24 commits behind it and has no `format_consolidates_book`, `resolve_book_scope`, `run_book_scoped`, `Q-5-41/39` or `--book-scope`. Unless marked `[W3]`, all file:line below were read in `workspace-7`. `crates/quarto-core/src/format.rs` is byte-identical in the two trees. The fix branch needs to be rebased onto the parity branch (or the survey re-checked) before implementing the orchestrator parts.

Abbreviations: **FM** = the input's own front matter; **proj keys** = `format:` in `_quarto.yml` (`project.config.metadata`); **dir keys** = `format:` in `_metadata.yml` chain; **req key** = an explicit format key supplied by the caller/request; **early** = the CLI's pre-discovery guess (`render.rs:928`).

Naming trap: there are now **four independent front-matter `format:` parsers**, with different edge cases (see section b, item 6): `format_key_from_frontmatter` / `format_keys_from_frontmatter` (format.rs:610/629, serde_yaml), `frontmatter_format_keys` (pandoc_request/formats.rs:181, yaml_rust2), `detect_format_from_content` (wasm lib.rs:775, yaml_rust2), and `output_ext::document_ext` (output_ext.rs:35, serde_yaml, reads `format.<key>.output-ext`).

### Table

| Site (file:line) | What it does | Inputs consulted | Affected? |
|---|---|---|---|
| **CLI native** | | | |
| `crates/quarto/src/commands/render.rs:928` (`render_once`) | Picks `format_str`: `--to`, else `detect_single_input_format`, else `"html"`. Runs before `classify_inputs` (963) and before any project discovery. Result feeds `Format` (930), the refusal gate (955), `ProjectPipeline::new` (1097/1289) and `RenderReport.format_str` (1334; stored, not read elsewhere in `crates/quarto/src`). | `--to`; FM of a single `.qmd`/`.md` input. Never proj keys, never dir keys. Directory, multi-input and no-input renders always get `html`. | **Yes** (known site 1; root cause). |
| `render.rs:955-961` | "Format not yet supported" refusal for non-native, non-pandoc-hybrid formats, keyed on `format_str`. | early guess only | **Yes**: a project with `format: pdf` (LaTeX) is not refused up front, it reaches the pipeline as html and is re-resolved per doc. A typst project is not refused either, which is why it "works" with `--to`. |
| `render.rs:942-947`, `1954-1967` (`multi_format_warning_diagnostics`, Q-20-8) | Warns when a single input's FM `format:` declares several keys but one is rendered. | FM of a single input; `--to` suppresses | **Partly**: never warns for a multi-key project or directory `format:`; silent first-key truncation there. |
| `render.rs:1942` `detect_single_input_format` + `1917` `read_single_input_content` | Reads a single `.qmd`/`.md` file and calls `format_key_from_frontmatter`. Returns `None` for directories and multiple inputs. | FM only | **Yes**; two callers: `render.rs:928`, `preview_static.rs:306`. |
| `render.rs:1097-1105` (`render_single_doc`), `1289-1297` (`render_project`) | `ProjectPipeline::new(project_type, format, format_str, ...)` with the early guess as `self.format`/`self.format_str`; `.with_format_override(args.to)` carries only `--to`. | early guess, `--to` | **Yes**: everything keyed on `self.format` downstream (section "Project/book dispatch") sees `html` for a project-only `format: typst`. |
| `crates/quarto/src/commands/preview_static.rs:306` (`run`), `85-100` + `103` (`check_servable_format`) | "Front-matter format decided before any render" for the servable-format gate (html/revealjs/typst only). For a directory or a project file, `detect_single_input_format` sees the path argument, not the project. | `--to`; FM of the path argument | **Yes** (known site 2): gate never sees project `format: docx/pptx/epub`, so those are not refused; and typst is not recognised. `--to` help text `main.rs:357` ("Defaults to the document's `format:`, else `html`") is wrong for projects. |
| `preview_static.rs:360-375` (boot render), `487` + `669` (`start_render`, watcher re-render), `159-178` (`render_args_for`) | Every render, boot and watcher, goes through `render_once` with `RenderArgs.to = args.to`. Boot input is the project root (a directory), so early = html. Watcher `Action::Subset(paths)` passes the changed files: a single changed chapter reads its own FM; several changed files get `html`. | `args.to` only | **Yes**, and the watcher is a **new variant** (b1): editing one chapter of a project-typst book re-renders it as a standalone per-chapter typst doc (or as html), never as the book. |
| `crates/quarto/src/commands/get_config.rs:79` + `main.rs:560` (`--to`, `default_value = "html"`) | Builds `Format` from `--to` and runs `merge_document_metadata` (ParseDocument + MetadataMerge only). | `--to` (defaults to html); project is discovered (68) and used only for metadata layers | **Partly** (b8): `format.<fmt>.*` flattening uses html unless the caller passes `--to`; a typst-project doc reports html-flattened config. |
| `crates/quarto/src/commands/publish.rs:282` | Hard-coded `Format::from_format_string("html")`; `ProjectPipeline` with **no** `with_format_override`; then `run_with_book_support`. | nothing | **Partly / suspicion** (b7): publish is meant to be html, but nothing forces it, so project-level non-html `format:` is re-resolved per doc inside an html-shaped book dispatch. |
| `crates/quarto-test/src/runner.rs:460-474` (`render_project_document`) | Whole-project fixture render: requested format, else proj `format:` key, else typst for a Book, else html. Passes the result as both `Format` and `with_format_override`. | spec format; proj keys; project kind | **No**: this is the only caller that already resolves from the project before building the pipeline. Differs from CLI in the "Book defaults to typst" fallback (Q1 and `render_once` default a book to html). |
| `runner.rs:658` -> `render_to_file.rs:252` (`render_to_file`) | Single-doc fixture render; passes the explicit format as `format_override`. | req key | **No** (explicit by construction). |
| **quarto-core** | | | |
| `crates/quarto-core/src/format.rs:610` `format_key_from_frontmatter` | serde_yaml peek: scalar, or first key of a map. Requires the document to start with `---\n` (BOM tolerated) and a `\n---` or `\n...` terminator. Does not skip `null`/`false` map entries (Q1 does). | FM | n/a (building block). Callers: render.rs:1944, render_to_file.rs:357, multi_file_html.rs:184. |
| `format.rs:629` `format_keys_from_frontmatter` | All keys of FM `format:`. | FM | n/a. One caller (Q-20-8, above). |
| `format.rs:730` `resolve_format_key` | Prefer-merge of `format:` strings, precedence proj < FM < `--to`; returns `default` if none. First key only. | proj key (first), FM key (first), `--to`, default | **Partly**: correct for non-book "doc over project", but (i) document beats project even in a **book** (Q1 `projectFormatsOnly`: project keys only), (ii) dir keys never read, (iii) only the first key. Callers: render_to_file.rs:358, multi_file_html.rs:185. |
| `crates/quarto-core/src/render_to_file.rs:349-365` (`render_document_to_file`) | Per-document resolution: `resolve_format_key(project_format, document_format, format_override, format)`. Result drives pipeline profile, output path and extension. The `format` param (the early guess) is only the last-resort default. | proj key, FM key, `format_override` (`--to` on CLI; the explicit key for `render_to_file`, preview `RealRender` at render_helper.rs:204-212, and `render_chapter_alone`) | **No for the typst/revealjs decision** (this is why typst still compiles with the wrong early guess); **yes for consistency**, since it disagrees with `self.format` used by dispatch, Pass 1 and post-render. |
| `render_to_file.rs:398` (`format_from_name`), `800` (`determine_output_paths_with_ext`), `877` (`typst_output_ext`) | Consume the already-resolved key. `typst_output_ext` calls `resolve_output_ext(content, project.config.metadata, target_format)`. | resolved key; FM + proj `output-ext` | **No** (consumers), except `output-ext` has the dir-keys gap (b5). |
| `crates/quarto-core/src/output_ext.rs:35-80` (`resolve_output_ext`) | `output-ext` for typst: FM `format.<key>.output-ext`, FM top level, proj (flattened for the key). Header comment already records the gap: "directory `_metadata.yml`, included metadata and `-M` flags are not seen". | FM, proj | **Partly** (same gap as `resolve_format_key`; dir keys). Also used by `resolve_declared` (formats.rs:245) which decides `typst` vs `typst-pdf`. |
| `crates/quarto-core/src/pandoc_request/formats.rs:181` `frontmatter_format_keys`, `209` `resolve_formats`, `269` `resolve_document_formats` | The Rust-owned classifier used by the browser: FM keys win, else proj keys, else `html`; classifies each (`preview`/`download`/`neither`) and maps `typst` to `typst-pdf` by `output-ext`. `resolve_document_formats` does `ProjectContext::discover(path)`, so it **is** project-aware. For a Book it attaches `book` (chapter membership). | FM keys (all), proj keys (all), proj `output-ext`; not dir keys; not `projectFormatsOnly` | **Partly**: correct for the reported symptom (project-only typst resolves to `typst-pdf`); wrong for Q1 book semantics and dir keys; reads the buffer from the VFS, not the editor buffer (b9). Sole caller: wasm `resolve_pandoc_formats` (lib.rs:2120). Note `book_resolution` here uses `book_membership` (parity-branch change). |
| `crates/quarto-core/src/pandoc_request/render.rs:141` | Builds `Format` from the request key (`typst-pdf` -> `typst` + compile). | req key | **No** (explicit). |
| `crates/quarto-core/src/book_download.rs:229-236` | `ProjectPipeline::new(format, format_str)` from `BookDownloadParams.format`; no override needed (uses `run_book_download`). | req key | **No** (explicit). |
| `orchestrator.rs:2028`/`2031` -> `pass1_profile_with_cache` (2595), `format_id` at **2617**, live Pass 1 at 2718-2730 | Pass 1 builds a `RenderContext` with `self.format` (early guess) for every document: metadata merge (`format.<early>.*` flattening) and the cache key (`format_id = format.target_format`). | early guess | **Yes / suspicion** (b3): a project-typst book gets its Pass-1 profiles computed and cached under `html`, while Pass 2 renders typst. Unverified how much downstream data depends on it. |
| `orchestrator.rs:1370` (book multi-file), `1731` (`run_inner`) | `post_render` (sitemap, robots, alias redirects, feeds) gated on `self.format.identifier.is_html_based()`. | early guess | **Yes / suspicion** (b4): with early html and a project that really renders docx/typst, the website `post_render` runs against non-HTML outputs. Unverified whether it hard-fails. |
| `orchestrator.rs:1609` | `missing_project_css_diagnostics(self.project, self.format.identifier.as_str(), ...)`. | early guess | **Yes** (minor): css warnings keyed on the wrong format. |
| **Project/book dispatch** | | | |
| `orchestrator.rs:1031` (Q-5-33 gate) | `FormatIdentifier::try_from(self.format_str)` and `is_supported_format`. | early guess | **Yes** (b2): a book whose `_quarto.yml` says `format: docx/pptx/revealjs` is not refused (early html passes) and then renders per doc. |
| `orchestrator.rs:1053` (`format_consolidates_book(&self.format)` -> `run_book_scoped`), `1055` (`Html` & `Full` -> `run_book_multi_file_html`), else `run_inner` | The book dispatch. | early guess | **Yes** (root cause): early html + Full goes to `run_book_multi_file_html` (1055); early html + Subset goes to `run_inner`. Neither does the typst book merge. |
| `orchestrator.rs:1070-1080` (Q-5-42), `scope.rs:84` (`format_consolidates_book`), `scope.rs:~116-170` (`resolve_book_scope`, Q-5-41/39), called at `orchestrator.rs:1092-1113` (`run_book_scoped`) | Scope resolution and the "scope ignored" notice keyed on `self.format` / the passed `&Format`. | early guess (CLI) or explicit key (preview, wasm request) | **Yes for CLI** (Q-5-42 reported on the fixture); **No** for preview routes and `render_pandoc_request`, which pass the explicit key (render_helper.rs:429, pandoc_request/render.rs:353). |
| `orchestrator.rs:1109/1118` -> `single_file_render.rs` (`render_book_single_file`, `Format` param at lines 112/250), `run_book_download` (1141-1160), `chapter.rs:92-103` (`prepare_page_in_book`, `render_chapter_alone_summary` with `with_format_override(Some(format_str))`) | Consume the `&Format` handed in; chapter-alone forces its key as override. | passed `Format` | **No** (given a correct format they are right; they inherit the early guess only on the CLI full/subset path). |
| `crates/quarto-core/src/project/book/multi_file_html.rs:134-139`, `182-192`, `525` | Per chapter: `resolve_format_key(project_format, chapter FM, format_override, format_str)`; non-native chapters delegate to `render_document_to_file`; non-item project files go through `render_document_to_file` (525). | proj key, chapter FM key, `--to`, early guess as default | **Partly**: self-consistent with `render_to_file`, but (i) reached with early html for a project-typst book, (ii) honours a chapter's own `format:` in a book (Q1 does not). |
| **Preview server (quarto-preview)** | | | |
| `crates/quarto-preview/src/render_helper.rs:204-212` (`RealRender::render_document`, `Some(format)` override), `288` (`NativeBookDownload`), `414-429` (`render_blocking`: key split by `render_format_and_target`, then `resolve_book_scope`) | Native render/download with the key from the request. | req key only | **No** (by construction; never consults project or FM for the format). |
| `render_download.rs:58-90` (`download_format`, `RenderJob.format_key = info.key`), `pdf_preview.rs:200` (hard-coded `TYPST_PDF_KEY`) | Validates a client-named key against the table; the PDF route always asks for `typst-pdf`. | req key (client) | **No** server-side; correctness depends on how the client derived the key (TS section). |
| `crates/quarto/src/commands/preview.rs` (non-static `q2 preview`) | No Rust format decision found (`grep format` is empty apart from `format!`). Boot is the hub plus embedded SPA; there is no eager native render and no native watcher re-render in this path; rendering happens in the browser via wasm. | none | **No** Rust site. All of its format behaviour is in the wasm and TS rows below. |
| `crates/quarto-preview/src/config.rs:374` | `StageContext::new(runtime, Format::html(), ...)` only to parse+expand includes for the watch set. | hard-coded | hard-coded, not a decision. |
| **wasm entries (`crates/wasm-quarto-hub-client/src/lib.rs`)** | | | |
| `lib.rs:775` `detect_format_from_content` | FM first key (yaml_rust2; `trim_start`, `---` prefix, `\n---` terminator), else `"html"`. No project, no VFS. | FM only | **Yes** (known site 3): the decision point for six callers below. |
| `lib.rs:858`/`870-871` `parse_qmd_to_ast(_with_attribution)` | Bare `/input.qmd`, `ProjectConfig::default()` (`create_wasm_project_context`, 718), format from `detect_format_from_content`. The router parses `props.content` through it. | FM only (no path, no project) | **Yes** (known site 3): `meta.format` is `html` for a project-typst chapter. |
| `lib.rs:1076` `render_qmd` (calls `render_single_doc_to_response`, 2150, with a **discovered** project), `1168` `render_qmd_content` (synthetic project) | Single-doc HTML render; format from `detect_format_from_content` (2175-2185). The discovered `project` is passed for metadata but never for the format. | FM only | **Yes** whenever a real project exists (`render_qmd`); n/a for `render_qmd_content`. |
| `lib.rs:1127`/`1141` `render_printable` | `coerce_format_for_print(detect_format_from_content(FM))` passed as `format_override` (2175: override beats everything). | FM only; override beats project | **Yes** (b-new): printing a chapter of a project-`format: revealjs` site renders html, not the deck; the override also masks project metadata flattening for the real format. Gate: `DocumentTopBar` shows the button only when router's `currentFormat` is q2-preview/q2-slides/revealjs. |
| `lib.rs:1271-1330` `render_page_in_project_with_attribution` (single-file branch at 1324), `1385-1425` `render_page_for_preview` (single-file branch at 1421) | Entry points of the live preview. Single-file branch: `render_single_doc_to_response` (above). Multi-file branch: `render_project_active_page_to_response`. | FM only | **Yes** (see next row). |
| `lib.rs:2369-2375` `render_project_active_page_to_response` (via 1330+, 1421+) | Reads the active file, `detect_format_from_content`, optionally `map_format_for_preview` (742: html->q2-preview, revealjs->q2-slides), builds `Format`, and runs the whole `ProjectPipeline` (`pipeline.run()`, **not** `run_with_book_support`) with it for Pass 1 and Pass 2. Reports it as `RenderResponse.format` (2470+). | FM of the active file only; project ignored | **Yes**: a project-level `format: revealjs` previews as q2-preview, and project `typst/docx` previews as the html page. Also feeds `meta.format`/`ast_json` into the SPA and `PreviewRoot.tsx:1494` (`isSlides`). |
| `lib.rs:2120` `resolve_pandoc_formats` | Wrapper over `resolve_document_formats` (project-aware). | FM keys, proj keys | **Partly** (see quarto-core row). |
| `lib.rs:1626` `render_pandoc_request` (calls `pandoc_request/render.rs:116`) | Format is the caller's key; book scope via `resolve_book_scope(format)`. | req key | **No** (explicit; origin of key is the menu or `classifyPreviewMode`). |
| `lib.rs:742` `map_format_for_preview`, `764` `coerce_format_for_print` | Pseudo-format mapping, applied after detection. | key | n/a: must be applied **after** whatever resolver replaces detection. Duplicated in TS (`getQ2Format`, `pipelineKind.ts`) and `format.rs:592` (`lua_format_for`). |
| `lib.rs:3138-3305` `lsp_*` (via `quarto-lsp-core/src/analysis.rs:66`, `project/book/static_analyzer.rs:164`) | Diagnostics/outline analysis with hard-coded `html` and a minimal single-file project. | hard-coded | hard-coded, not a decision, but format-dependent diagnostics ignore the real format (b10). |
| **hub-client / q2-preview-spa TS** | | | |
| `hub-client/src/components/render/PreviewRouter.tsx:143-151` | Parses the buffer with `parseQmdToAst(props.content)` (no path) and calls `classifyPreviewMode(ast, path, deps)` with `resolve = resolvePandocFormats`. | AST `meta.format` (FM only), then resolver | **Yes** (known site 3). |
| `hub-client/src/components/render/getQ2Format.ts:31-41` `getQ2Format`, `83-107` `classifyPreviewMode` | `getQ2Format` maps `meta.format` `html` -> `q2-preview`, `q2-*`/`revealjs` -> itself, else `null`. `classifyPreviewMode` returns `react` when `getQ2Format` is non-null; the project-aware `resolve` is consulted only for `null`. | `meta.format` first, resolver second | **Yes** (known site 3): the html default short-circuits before the resolver, so PDF mode, download mode and "neither" are unreachable for any project-only format (typst, docx, pptx, epub, latex). |
| `hub-client/src/components/render/ReactPreview.tsx:261-285` (`renderPageInProjectWithAttribution(..., preferPreviewFormat=true)`), `329` (`parseQmdToAstWithAttribution`) | Renders with the router's format, but the wasm **re-detects from FM** and the flag only re-applies `html->q2-preview` / `revealjs->q2-slides` (comment at 261-270 says so). | router format ignored by wasm | **Yes (b-new)**: fixing `classifyPreviewMode` alone cannot make a project-level `format: revealjs` render as slides; the wasm render path must take the decision too. |
| `hub-client/src/components/render/Preview.tsx:115` (`renderToHtml` -> `render_page_in_project`) | Full-DOM renderer, same wasm FM detection. | FM only | **Yes** (same as above). |
| `hub-client/src/components/Editor.tsx:445` (`handleFormatChange`), `456`, `1908`, `1912`; `DocumentTopBar.tsx:78-85`; `hub-client/src/services/printableDocument.ts:93` | `currentFormat` (from router `onFormatChange`) gates slide thumbnails (`q2-slides`), preview editing (`q2-preview`/`q2-debug`), and "open printable". | router decision | **Yes** (inherited): same project-level `format: revealjs` problem. |
| `Editor.tsx:1670` (`previewMode.mode === 'neither'` -> `disabledReason`), `useDownloadAs.ts:55`/`70`/`76`, `pandoc/bookInfo.ts:18-19`, `PreviewRouter.tsx:219-226` (`DownloadModePane`) | Download menu: `bookInfoFor` and `withOutputExt` read `resolvePandocFormats(path)`; the pane/`neither` state comes from `classifyPreviewMode`. | resolver (menu); classifier (mode) | **Partly**: the menu and book info are project-aware and correct; the *mode* inherits the short-circuit above. |
| `hub-client/src/components/render/PdfPreviewPane.tsx` (gating via `classifyPreviewMode` `pdf` branch, `pdfPreviewAvailable` at `pandoc/downloadService.ts:168`) | PDF pane shown only for `mode: 'pdf'`. File internals not read. | classifier | **Yes** (inherited) - unverified beyond the call chain. |
| `q2-preview-spa/src/pdfPage.ts:7-14` (`isTypstPdf`) -> `PreviewApp.tsx:617-621` (`pageKind`) | PDF vs html page kind straight from `resolvePandocFormats(path).formats[0]` (`typst-pdf`/`download`). | resolver | **No** for the PDF decision (project-aware; project-only typst works). |
| `PreviewApp.tsx:1202` (`renderPageForPreview`), `1257` + `285` (`noLivePreviewMessage(result.format)`) | HTML/AST render for `pageKind === 'html'`; format from wasm FM detection. | FM only | **Partly**: project-level `format: revealjs/docx` previews as q2-preview html; project-typst is intercepted earlier by `pageKind`. |
| `ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx:1494` (`isSlides` from `meta.format`), `hub-client/src/components/render/getQ2Format.ts` pseudo-format table, `ts-packages/preview-runtime/src/pipelineKind.ts` | Consume the wasm's decision. | wasm output | n/a (consumers). |
| **Other** | | | |
| `crates/quarto/src/main.rs:357` (help text) | Documents the wrong default for `preview --static --to`. | n/a | Docs only. |
| `resolve_format` (`render.rs:1909`) | `Format::from_format_string` wrapper. | key | n/a. |

Hard-coded, not a decision: `Format::from_format_string("html")` in `crates/perf-harness/src/bin/{parse_qmd_to_ast,parse_corpus,vfs_flush}.rs`, `quarto-core/src/book_download.rs` tests, `pipeline.rs` tests, `quarto-lsp-core/src/analysis.rs:66`, `project/book/static_analyzer.rs:164`, `quarto-preview/src/config.rs:374`, `quarto-core/src/engine/preview_record.rs:150,219` (`q2-preview` for recorded previews).

Not found (checked, no format decision): `crates/quarto/src/commands/{check,list,convert,pandoc,hub,provide_hub,install,add,remove,inline_css,docs_llms,lsp,mcp}.rs`, `crates/quarto-publish/src`, `crates/quarto-hub*`, `crates/quarto-lsp/src` (the Format import lives only in `quarto-lsp-core`), `quarto-preview` static-mode server (serves by output path), `ProjectContext::discover` (no format selection). `directory_metadata_for_document` is called only from `MetadataMergeStage` (metadata_merge.rs:318), never for format selection.

### (a) Sites that already behave correctly, and why

1. **`render_to_file.rs:349-365` per-document resolution.** Project is the discovered one and the project key is read; why a project-only `format: typst` still compiles on the CLI. (Correct for typst; wrong only for Q1's book rule and dir keys.)
2. **`render_to_file()` (252), `RealRender` (render_helper.rs:204-212), `render_chapter_alone_summary` (chapter.rs:103), `quarto-test` runner (runner.rs:494-503, 658).** The caller's key is forced as the override, so no inference happens.
3. **Native preview routes (render_helper.rs:288/414-429, render_download.rs:58-90, pdf_preview.rs:200) and `render_pandoc_request` (pandoc_request/render.rs:116-141, wasm lib.rs:1626).** Format is an explicit key from the request; the server never infers it. This confirms the research note's open question (c): the preview server does not hit this bug by itself. Correctness depends on the client deriving the key from a project-aware source.
4. **`q2-preview-spa` `isTypstPdf` (pdfPage.ts) and the hub-client download menu (`useDownloadAs`, `bookInfoFor`).** Both read `resolvePandocFormats(path)`, which runs `ProjectContext::discover`, so project-only typst resolves to `typst-pdf` there.
5. **`quarto-test` `render_project_document` (runner.rs:460-474).** The one existing implementation of "resolve from the project, then build the pipeline", incl. forcing the same value as `with_format_override`. A template for the CLI fix, except for its Book->typst default.
6. **Non-static `q2 preview` (`preview.rs`).** No Rust-side decision: boot is hub + SPA, rendering happens in the browser.

### (b) New bugs and suspicions beyond the three known

1. **Watcher re-render in `preview --static` (preview_static.rs:487, 669, 159-178).** Every re-render goes through `render_once` with the early guess; a single changed chapter reads only its own FM (usually none -> html), so after the first edit a project-typst book is re-rendered chapter-by-chapter without book metadata. Must be covered by the same fix and by a watcher test, not only the boot render. (Confirmed by reading; not run.)
2. **Q-5-33 bypass (orchestrator.rs:1031).** A book whose project `format:` is `docx`/`pptx`/`revealjs`/`pdf` is not refused, because the gate looks at the early guess (`html`).
3. **Pass 1 under the wrong format (orchestrator.rs:2028, 2595-2617, 2718-2730).** Pass-1 profiles and their cache key use the early format, Pass 2 uses the per-doc format. Unknown impact; at minimum a project-typst book populates the Pass-1 cache under `html`.
4. **`post_render` gating (orchestrator.rs:1370, 1731) and css diagnostics (1609)** use `self.format`. With an html early guess on a project that renders docx or typst, website post-render hooks may run on non-HTML outputs (the comment at ~1726 warns one of them hard-fails). Unverified.
5. **`_metadata.yml` `format:` is read by no resolver** (`resolve_format_key`, `resolve_formats`, `resolve_output_ext`, multi_file_html). Q1 treats it as input keys. `output_ext.rs`\'s header already admits the gap. (Note: for books it would be ignored anyway under `projectFormatsOnly`.)
6. **Four front-matter parsers with different edge cases.** `format_key_from_frontmatter` (serde_yaml; `---\n` required at byte 0 after BOM; `\n---` or `\n...`); `frontmatter_format_keys` and `detect_format_from_content` (yaml_rust2; leading whitespace allowed; `\n---` only; `...` terminator not accepted); none skips `null`/`false` map entries (Q1 does); `format: ""` and non-string scalars fall to different defaults. A document can be classified differently by the CLI, the resolver and the wasm detector.
7. **`publish.rs:282` hard-codes html without forcing it** (no `with_format_override`). Either force html explicitly (then say so) or resolve from the project; today project-level non-html `format:` is re-resolved per doc inside an html-shaped book dispatch.
8. **`get-config --to` defaults to html (main.rs:560).** A doc in a typst project reports html-flattened config unless `--to typst` is passed.
9. **Resolver reads the VFS copy, not the editor buffer.** `resolve_document_formats` reads `path` through the runtime (formats.rs:273-277), while `parse_qmd_to_ast` takes `props.content`. During typing, FM edits are visible to one and not the other until the VFS sync lands (`PreviewRouter.tsx` calls both in one effect). A shared resolver needs a `content` override.
10. **Router fix alone is insufficient (ReactPreview.tsx:261-285, PreviewApp.tsx:1202, lib.rs:2369-2375).** The wasm render re-detects the format from the file's FM, so any decision the router takes from the resolver is not what the render uses. Same for `render_printable` (override from FM beats project, lib.rs:1141).
11. **Book semantics differ across layers.** Q1 `projectFormatsOnly`: a book ignores chapter and directory `format:`. q2: `resolve_format_key`, `resolve_formats`, `multi_file_html` all let chapter FM beat the project; `quarto-test` defaults a Book to typst while `render_once` defaults to html. A decision (adopt Q1 for `ProjectKind::Book`, or document the divergence) is needed before the shared resolver is written.
12. **`lua_format_for` (format.rs:592), `map_format_for_preview`/`coerce_format_for_print` (lib.rs:742/764) and `getQ2Format.ts` / `pipelineKind.ts`** encode the pseudo-format table in three places; the resolver will have to say who owns it.
13. **LSP/static analyzer hard-code html with a minimal project** (`quarto-lsp-core/src/analysis.rs:66`, `static_analyzer.rs:164`); format-specific diagnostics (typst-only keys, `format.typst.*`) are never produced in the editor. Low priority; flag only.

### (c) What API a shared resolver needs so every caller is covered

Callers to serve, with what each can supply: `render_once` (cli `--to`, inputs: none / dir / files, project discoverable, native FS); `preview --static` boot and watcher (same, plus changed-file subsets); `render_to_file` / `multi_file_html` (project already discovered, one document, `--to`/explicit key); orchestrator dispatch, Pass 1, post-render (project-wide, no single document); wasm entries (VFS runtime, path, **editor buffer text**, no `--to`; need the preview pseudo-format mapping); hub-client / SPA (via wasm; want `formats[0]` + class); `get-config`, `publish` (project-wide or single doc, optionally forced).

Proposed shape, in `quarto-core` (native + wasm, `&dyn SystemRuntime`, no I/O beyond the runtime):

1. **Layer collection shared with `MetadataMergeStage`.** `format_layers_for_document(project, doc_path, runtime, doc_content: Option<&str>) -> FormatLayers { project: Option<ConfigValue>, directory: Vec<(PathBuf, ConfigValue)>, input: Option<ConfigValue> }`, built from the same ordered, unflattened layers the stage already gets (`directory_metadata_for_document`, project config, FM parsed once with the real YAML parser). Replaces all four FM parsers.
2. **Pure selection, Q1 rule.** `select_formats(FormatInputs { project_keys, directory_keys, input_keys, project_formats_only: bool, cli_to: Option<&str>, default: &str }) -> FormatSelection { keys: Vec<String>, primary: String, source: Cli | Input | Directory | Project | Default, extra_cli_to: bool }`. Keys are the scalar or the map keys with `null`/`false` dropped (Q1 `formatKeys`). `project_formats_only = project.project_kind() == Book`. `--to` handling per Q1 (`default` -> first, otherwise appended even if undeclared); for now callers take `primary` ("single format per render"), but `keys` is kept so Q-20-8 can warn for project and directory declarations too.
3. **Two entry points over it.**
   - `resolve_document_format(project: &ProjectContext, doc_path, doc_content: Option<&str>, cli_to, runtime) -> FormatSelection` for per-document callers (`render_document_to_file`, `multi_file_html`, wasm, `get-config`).
   - `resolve_project_format(project: &ProjectContext, cli_to) -> FormatSelection` for **document-less** callers: CLI before dispatch (`render_once` must discover the project first and call this, or the per-document one when exactly one file is named), `preview --static` (boot and every watcher action), `publish`, and the orchestrator's `self.format`.
4. **Single place for the classes and pseudo-formats.** Keep `ResolvedFormats`/`FormatClass` (formats.rs) but build them from the `FormatSelection` (so `resolve_pandoc_formats` stays and gains dir keys and the book rule). Move `map_format_for_preview` / `coerce_format_for_print` / `lua_format_for` into `quarto-core::format` and expose a `preview_format_for(selection)` so wasm and TS stop duplicating the table. `resolve_declared`\'s `typst` -> `typst-pdf` stays the classifier's job.
5. **wasm surface.** Make the Rust side the decider for the browser: `render_page_in_project_with_attribution`, `render_page_for_preview`, `render_printable`, `render_qmd` and `parse_qmd_to_ast` take (or compute from `path` + buffer) the resolved format, replace `detect_format_from_content`, and report it in `RenderResponse.format`; `parse_qmd_to_ast` needs a `path` (or a project-aware variant) so `meta.format` is correct. Then `classifyPreviewMode` can ignore `meta.format` and use `resolvePandocFormats(path, content)` + the preview mapping, which also repairs `PreviewRouter` / `Editor` `currentFormat`, the PDF pane, and the download-mode and "neither" states in one change.
6. **Orchestrator.** `ProjectPipeline::new` should receive the resolved project-level format (not the early guess); per-document resolution inside `render_document_to_file` / `multi_file_html` calls (3) with `project_formats_only` so a book is decided once. `format_consolidates_book`, Q-5-33, Q-5-42, Pass 1 and post-render gating then see the same value as Pass 2.
7. **Diagnostics.** Return the unknown/invalid-key error and the "several keys, one rendered" data from the selection (replace `format_keys_from_frontmatter` + `multi_format_diagnostics` inputs).
8. **Tests the API must make easy.** Pure table tests on `select_formats` (project-only, dir-only, FM-only, book with chapter FM, `null`/`false` keys, `--to` undeclared); CLI: orange-book with no `--to` for `render`, `render index.qmd`, `--book-scope`, `preview --static` boot **and** a watcher single-chapter re-render; wasm/TS: a front-matter-less chapter of a `format: typst` book classifies as `pdf`, and a project-level `format: revealjs` page previews as slides.

### Unverified / not read

- No code was executed; every "affected" call is inferred from reading. Section (b) items 3 and 4 in particular were not traced to a concrete failure.
- Q1 behavior is taken from section 4 of this note, not re-verified against the Q1 source.
- Not read: `hub-client/src/components/render/PdfPreviewPane.tsx` internals, `hub-client/src/pandoc/downloadService.ts` (beyond the `pdfPreviewAvailable` location), `quarto-preview` `locate`/route wiring beyond the three handlers cited, `scope.rs` lines 116-170 line numbers are approximate (read, not pinned).
- `workspace-3` (the task's checkout) was read for `format.rs`, `formats.rs`, `render.rs`, `render_to_file.rs`, `orchestrator.rs` and shows the same early-guess/`resolve_format_key` structure, without the book-scope and `book_membership` pieces; its line numbers differ (e.g. `render.rs:925`, `render_to_file.rs:301/302`, `orchestrator.rs:1043`-region without scope). Re-pin after rebasing.

## 8. Decision: book format rule is scope-dependent (2026-10-14, Gordon)

Q1's `projectFormatsOnly` is adopted only for **book scope**. Chapter scope is
new in q2 (Q1 could not render or preview a single chapter of a book), so Q1
has no rule to match there.

- **Book scope** (whole-book render or preview): the format comes from the
  project's `format:` keys only. Chapter and `_metadata.yml` `format:` are
  ignored. This matches Q1.
- **Chapter scope** (one chapter rendered or previewed alone): the document
  wins, i.e. today's prefer-merge: project < `_metadata.yml` < chapter front
  matter < `--to`.
- `--to` wins in both.
- Non-book projects are unchanged (input and directory keys override the
  project, per Q1).

Consequences for the resolver (§7 (c)):

- `select_formats` needs a scope input (`Book | Chapter`), not just
  `project_formats_only: bool`. Equivalent: `project_formats_only` is true
  exactly when the project is a book and the effective scope is Book.
- Ordering is a sequence, not a cycle: `resolve_book_scope` depends only on
  the *project's* format (`format_consolidates_book(project format)`),
  never on a chapter's. So: project keys -> project format -> scope
  (explicit flag, membership, consolidates?) -> final format (Book: project
  format; Chapter/Page: re-resolve with the document's keys). Without an
  explicit chapter scope, a member of a consolidating book resolves to Book.
- A chapter-scope render whose chapter format differs from the project's may
  leave the book dispatch (`format_consolidates_book`, Q-5-42) judging a
  different format than the document renders to. Define which format the
  dispatch sees in that case, and test it.
- The multi-chapter whole-book merge never consults a chapter's `format:`
  (verified in `run_book_scoped`); that already matches Book scope.
- Sites that currently let chapter front matter win regardless of scope
  (`resolve_format_key` callers, `multi_file_html`, wasm `resolve_formats`)
  must take the scope so the whole-book paths stop doing so.
- Undefined and to be settled before coding: (1) which format the
  chapter-alone render and the Q-5-42 / dispatch decision see (project's vs
  the chapter's resolved one); (2) a non-member page (Page scope, Q-5-41):
  presumed document-wins; (3) multiple `format:` keys (first only, as today).

### Outcome of F5a (native `q2 render`, 2026-10-16)

Fixed among §7's native CLI and dispatch sites:

- `render.rs` `render_once` no longer guesses a format before the project is
  known. `render_single_doc` and `render_project` call one function,
  `resolve_render_formats`, where the project is final (after pre-render
  scripts): `resolve_render_scope`, `resolve_project_format` and
  `resolve_document_format` per named file. It owns the "not yet supported"
  refusal (every format to be used, before any render) and Q-20-8 (fed by the
  resolver's key list, so a project-level multi-key `format:` warns too).
- `render_once`, `render_single_doc` and `render_project` no longer call
  `detect_single_input_format` or the `format_key(s)_from_frontmatter`
  helpers (they stay for `preview_static.rs`; F5b). `multi_format_warning_diagnostics`
  is deleted.
- The Q-5-33 gate judges the book's resolved format and only when the requested
  scope is the book; its message names `--to` or the project's `format:`.
- The dispatch (`run_with_book_support`): several named files with a chapter
  request either collapse to the whole-book html render (every chapter named,
  all in the project's html) or each render on their own through one head, one
  Pass 2 per partition and one tail (`run_book_named_separately`).
  `classify_inputs` no longer collapses a Book project.
- `render_chapter_alone_summary` takes the resolved key (`target_format`), so an
  extension format stays itself, and the project format for Pass 1
  (`ProjectPipeline::with_project_format`); it no longer shuts the engines down
  (the caller does, once).
- Named non-member pages join the book's render list as extras, so an html book
  renders them (it rendered nothing before).

Still open: html chapter and non-member renders leave cross-chapter references
unresolved (`bd-html-chapter-xref-szfxmvyg`); the wasm and preview sites are F6
and F5b.

### Outcome of F5b (`q2 preview --static` and the test runner, 2026-10-16)

- `preview_static.rs` no longer peeks at front matter. The servable-format gate
  (`check_servable`) runs once, before any render, on the resolved format:
  `classify_inputs`, then `resolve_project_format` for a project root (a file
  inside a project is mapped to the root first) or `resolve_document_format`
  for a standalone file, with `--to`, `-M` and `--profile` applied. A resolver
  error (malformed `-M`, discovery failure) leaves the gate open so the render
  reports it. `detect_single_input_format`, `read_single_input_content` and
  `format_key(s)_from_frontmatter` are deleted.
- Boot and every re-render clone one base `RenderArgs` (`--to`, `-M`,
  `--profile`). `rerender_action` turns a `Subset` into `Full` for a
  consolidating book (typst); html books and typst websites keep the
  per-document re-render. `LastRender` carries the fact from `RenderReport`.
- `--static` takes `-M` and `--profile`; the four config readers in
  `quarto-preview/src/config.rs` take the profile selection (the live viewer
  passes `None` until F8).
- A render that wrote nothing shows its diagnostics at `/` (`ReloadHub` holds
  the failure the server renders), instead of the page listing.
- `quarto-test`\'s project runner resolves the format with the resolver (`html`
  fallback, no book-means-typst default) and overrides the format only for the
  spec's requested format.

Still open: `--to q2-preview` and its siblings are accepted as before
(`bd-pseudo-format-to-meaning-vxhyni3t`); the live viewer's flags are F8.

## 9. Where this went (2026-10-15)

§7's survey and §8's rule are implemented by the epic
`claude-notes/plans/2026-10-15-format-resolution-epic.md` (plans F1-F7). The
decisions made after §8 (book scope as ordinary `book-scope` metadata supplied by a
runtime overlay, `none` instead of `page`, the render-intent rule for whole-project
renders, the runtime layer never selecting a format, a narrowed Q-5-42, `-M` wired,
the Pass 1 key hashing runtime metadata) are FR1-FR17 there.

## 10. Outcome (F7, 2026-10-16): every §7 row, and what was not adopted

Plans: F1 overlay and `-M`; F2 layer collector; F3a resolver; F3b/F3c scope and
`none`; F4 wire; F5a native render; F5b `--static` and the runner; F6 browser;
F8 live viewer flags. Each row of §7's table, by the plan that fixed it:

| §7 row | Outcome |
|---|---|
| `render_once` early guess; refusal gate; per-doc ctor (`render_single_doc`, `render_project`) | F5a: `resolve_render_formats` after the project is final; the gate judges each format to be used |
| Q-20-8 (`multi_format_warning_diagnostics`) | F5a: fed by the resolver's key list; also fires for project-level multi-key `format:`; reworded (FR18) |
| `detect_single_input_format`, `read_single_input_content` | F5a stopped using them; F5b deleted them with `format_key(s)_from_frontmatter` |
| `preview_static.rs` gate, boot render, watcher (b1) | F5b: one resolver-based gate, base `RenderArgs`, `rerender_action` (whole book for typst) |
| `get-config` `--to` default html (b8) | Not fixed: strand filed in F7 |
| `publish.rs:282` hard-coded html (b7) | Not fixed: strand filed in F7 |
| `quarto-test` runner (Book defaults to typst) | F5b: resolver, `html` fallback, override only for the spec's format |
| `format_key_from_frontmatter`, `format_keys_from_frontmatter`, `resolve_format_key`, `resolve_output_ext` | F2/F3a: one layer collector and resolver; `_metadata.yml` keys and `-M` now seen (b5); the old peeks deleted in F5b |
| `render_document_to_file` per-doc resolution, `multi_file_html` | F3a: the resolver, with the book rule (FR4) (b11) |
| `resolve_formats` / `resolve_document_formats` (wasm classifier) | F6: content override, dir keys, key and class in the result (b9) |
| Orchestrator Q-5-33 (b2), dispatch, Q-5-41/39, Pass 1 (b3), `post_render` and css gating (b4) | F3b/F5a: dispatch format and project format held separately (FR11); Q-5-42 narrowed (FR10) |
| `chapter.rs` chapter-alone | F5a: passes the resolved key (`target_format`) |
| Preview server routes, `render_pandoc_request`, `book_download` | Unchanged where explicit (§7a); F4 changed `scope` to the generic `metadata` wire; F8 gave the routes the session's flags |
| `preview.rs` (live viewer) | F8: `--to`, `-M`, `--profile`; refused with `--ui editor`, `--join`, `--no-project` |
| wasm `detect_format_from_content`, `parse_qmd_to_ast`, `render_qmd`, `render_printable`, `render_page_*`, `render_project_active_page_to_response` (b9, b10) | F6: `detect_format_from_content` deleted; entries call the resolver with the editor buffer; `parse_qmd_to_ast` stays front-matter-only on purpose |
| hub-client `PreviewRouter`, `getQ2Format`, `ReactPreview`, `Preview`, `Editor`, download menu, `PdfPreviewPane` | F6: `classifyPreviewMode` reads the Rust-resolved key and class; `getQ2Format` deleted |
| `q2-preview-spa` `isTypstPdf`, `PreviewApp` | F6 and F8: reads `key`/`class`, takes the viewer's flags from `/api/preview/config` |
| `map_format_for_preview`, `coerce_format_for_print`, `lua_format_for`, `pipelineKind.ts` (b12) | Left: all still live; a strand was filed in F7 |
| LSP and static-analyzer html hard-codes (b13) | Left: strand filed in F7 |
| `main.rs` help text | F5b corrected it; F8 re-worded it for the viewer |

Answers recorded in the Handoff logs: F2 2.7 (the book's `pre_render` mutates
the project layer in place and touches neither `format:` nor `book-scope`;
`seed_missing_book_title_metadata` likewise only book title keys, so the
collector needed no change); F5a 5a.3 (a)-(j) (Q-5-33 judges the resolved
format, only for a book-scope request; `q2 render sub/` renders its files; the
html chapter limit is pinned by two tests; a named non-member renders in every
format; the extension-format chapter case, `acm-typst` as a chapter, was never
tested because no typst-based extension fixture was built, so it is read, not
run).

**Not adopted from Quarto 1** (decided in the epic; the research above is the
record):

- `-M to:x` and `pandoc.to` as a fallback format when no layer declares one.
- `metadata-files` includes as a layer.
- The per-file `formats` filter.
- `--to all` and `--to default` (render every declared format).
- Q1's `-M format:x` being inert: q2's runtime layer is the top layer, so a
  scalar `format` selects (FR2).
- `projectFormatsOnly` for every render in a book: q2 applies it only to a
  whole-book (book-scope) render; a chapter render honors its own `format:`
  (FR4).
- Rendering a whole book from one named chapter: q2 renders the chapter, and
  `-M book-scope:book` asks for the book (FR3).
- Writing a named non-member's output beside its source: q2 keeps `_book/`
  (FR19).

Open: the meaning of the preview pseudo-formats as `--to` or `-M format:`
values (`bd-pseudo-format-to-meaning-vxhyni3t`).
