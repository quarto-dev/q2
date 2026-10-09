---
title: 'Epic: import documents into quarto-hub through pandoc.wasm (docx first)'
date: 2026-10-03
---

**Date:** 2026-10-03
**Status:** Planned; nothing started. Reviewed 2026-10-03 (three parallel reviewers, findings spot-checked); decisions from that review are folded into I3-I5, I10-I13, I16, I17, I19-I22 and the interfaces. Implementability review 2026-10-03 (three reviewers walked every task as its implementer): P1 lands in two stages, comment replies are grouped by equal range, P2's mechanism is pinned (AST pre-pass plus the line-start flag), and drop routing is tested through extracted functions plus Playwright. Angle review 2026-10-03 (handoffs, verification, tree and P6): P6's placement, comment ids, default comment author and Lua-quirk choices settled with Gordon; fixtures named by directory; `expected.qmd` and the TS response types pinned; an import wall-time STOP in P1 T8. Rebased 2026-10-03 onto the squashed `feature/pandoc-wasm` at `3dfa5b296` (on `main` `b19cb4464`); every citation re-checked against that tree: H10 is now H10a/H10b (I10 and the overlap paragraph rewritten), main's `b6b1817e1` already widened `format_supports_attribution` (P6 T5 changed with Gordon), the new Playwright `firefox` projects noted and line numbers fixed in P1, P4, P5 and P6. P0, P2 and P3 needed no change. Each touched plan's Handoff log has the details.
**Integration branch:** `feature/hub-import`, cut from `feature/pandoc-wasm` (the pandoc.wasm epics), which is assumed to merge into `main` first. Lives in workspace-1 (`.worktrees/workspace-1`).
**Builds on:** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) and its two epics ([host](2026-10-01-pandoc-host-epic.md), [request](2026-10-01-pandoc-request-epic.md)): the host package, loader, runner and the ownership rule.
**Evidence:** [`../research/2026-10-02-document-import-spike/`](../research/2026-10-02-document-import-spike/): probe inputs, the nested-comment probe test and its results on two branches. Every "verified" claim below was run on 2026-10-02/03, mostly with native pandoc 3.11 and `pampa` built from this branch.

## The problem

quarto-hub renders documents. With pandoc running in the browser it can also **import** them. Import runs only pandoc's front end (`pandoc -f <fmt> -t json`); pampa reads that JSON (pandoc's JSON is compatible with pampa's by design) and writes **qmd** with its qmd writer; the hub adds the qmd and its extracted images to the project.

The user sees an **Import** button next to "Download as". It opens a file picker filtered to the supported formats, then a placement dialog (folder and name, defaulting to the source's file name). Dropping a supported file on the file sidebar or the editor runs the same flow.

## Decisions

Settled with Gordon on 2026-10-02/03. Plans cite them by number.

- **I1 Formats:** docx, odt, rtf, epub, pptx. docx gets the emphasis: fidelity work, most fixtures and most tests. The others get one fixture each and whatever pandoc's reader gives. A Rust **format table** drives the picker's `accept` filter, drop interception and each format's argv.
- **I2 Faithful first.** v1 imports what pandoc's reader produces and fixes the qmd writer's content-corrupting bugs (I18). Reconstructing Quarto's own docx output (layout tables back into figures and callouts, crossrefs) is deferred to a follow-on (see Follow-ons).
- **I3 Track changes** are read with `--track-changes=all` and become q2 editorial marks with `author=` and `date=` kept: a pandoc `.insertion` span becomes `[++ …]`, a `.deletion` span becomes `[-- …]`. This is the in-band convention `DocumentProfile.comments` already reads (`claude-notes/designs/document-profile-contract.md`, the `comments` row: ISO 8601 UTC dates and display-name identity). Word highlights, which the docx reader emits as `Span .mark` (verified), become `[!! …]` highlights: the inverse of what `quarto-ooxml-editorial-marks` emits for `[!! ]`.
- **I4 A Word comment attaches to its parent element.** A comment whose range lies inside a single inline list becomes a plain span holding the range, followed by the comment and then its replies, in order:

  ```
  [a commented range [>> Please reword this.]{author="Ann" date="2026-09-01T10:00:00Z"}]
  ```

  The wrapper ends in a **trailing run** of one or more comment spans: the first is the range's comment, the rest are its replies. Leading `Space`/`SoftBreak` inlines of the range are moved out in front of the wrapper, because the qmd reader trims a leading space inside `[…]` (verified: `A[ range [>> c]]` re-reads as "Arange"). It is a plain span, not a `[!! ]` highlight. Elliot's `feature/span-comments` renders plain spans this way and deliberately excludes highlights (verified, see Findings). A comment with an empty range becomes a point comment `[>> …]{author= date=}` at the comment's position, with no wrapper span.
- **I5 Dropping a supported file anywhere in the hub window imports it.** A window-level drop handler catches drops the sidebar and editor don't handle (top bar, preview pane, image viewer, no file open, the overlay of an open dialog) and routes them like an editor drop: importable top-level files go to the import queue with the current file's folder (or `resolveDefaultDestination` when no file is open), and everything else goes through the existing upload path instead of letting the browser navigate to the file. The sidebar keeps its own folder targeting. Two exceptions: the "Add asset" dialog's own drop zone and file input still store any file as-is (I21), and files inside a dropped *folder* are stored as-is, keeping the folder upload's structure. Several importable files dropped together open one dialog each, queued. `.md`, `.html` and `.ipynb` keep their current upload behaviour; none of them is in I1.
- **I6 pptx** imports get `format: revealjs` in the front matter, unless the metadata already sets `format`.
- **I7 One qmd per import**, epub included. Splitting epub chapters into a book project is a follow-on.
- **I8 EMF and WMF images** (common in older Word files; browsers can't show them) **are converted in the browser to SVG and stored as SVG** (revised by `2026-10-05-metafile-svg-media.md`; it was PNG). pandoc.wasm has no `rsvg-convert`, so an SVG image in a docx/pptx download would become alt text with no picture (`../designs/pandoc-wasm-architecture.md:122`; Word check in `../research/2026-10-01-pandoc-wasm-evidence.md:174`); instead the docx/pptx request rasterizes each SVG image to a PNG at export (`RasterizeSvgImagesStage`, via a main-thread canvas hook), so the stored file stays vector and html/typst/epub keep it. If conversion fails, the original is stored anyway (it still works in docx and PDF downloads) and the report says so. Other formats browsers can't show (e.g. TIFF) are stored as-is and reported; v1 converts only EMF and WMF.
- **I9 Ownership** follows the pandoc.wasm design rule: Rust builds the request and owns paths, the format table, AST transforms, the qmd and every user-facing diagnostic. TS mounts, runs, converts image bytes and stores. Image bytes do not enter the Rust wasm at import: the main-thread Rust wasm grows about 4.5x the payload and never shrinks (`../research/2026-10-01-pandoc-wasm-evidence.md`, memory section), so only pandoc's JSON and metadata cross into it. One scoped exception, at docx/pptx *export* (not import): the rasterized PNG of each SVG image enters the request as compressed bytes, capped by the image-size limits; project SVGs are already in the VFS snapshot and the remote-image prefetch already brings remote bytes in.
- **I10 Own runner.** Import uses its own `PandocRunner` (`new PandocRunner({ loader: getPandoc().loader, … })`), sharing the app-wide `PandocLoader`. `PandocRunner` has one `current` slot and a new run supersedes the old (`hub-client/src/pandoc/pandocRunner.ts:172,199`), so with a shared runner an import would cancel a download and vice versa. Today the PDF preview also shares `getPandoc().runner` (`downloadService.ts:91`). H10b ([warm pool and preview](2026-10-02-pandoc-host-H10b-warm-pool-preview.md), Task 1b; not started) gives the preview a second, warm runner on the same loader (`getPreviewPandocRunner()`, a pool of up to two warm workers); Download, parity and the book menu keep `getPandoc().runner`. Import does not use the warm pool: H10a's warm executor ([warm executor](2026-10-02-pandoc-host-H10a-warm-executor.md)) translates only typst requests and runs anything else on a fresh instance, and the pool's latest-wins pending slot and per-document grace rule are built for preview edits, so an import queued there could replace a preview request or be replaced by one. Import's runner is a third runner on the shared loader.
- **I11 The placement dialog appears for every import,** button or drop. It proposes `<stem>.qmd` in the drop target's folder (the button uses `resolveDefaultDestination`), where `<stem>` is the source name without its extension, passed through `sanitizeFilename`. It never overwrites: if `<stem>.qmd` or `<stem>_media/` exists, the proposal moves to the first free `<stem> 2`, `<stem> 3`, … for both together (the `uniquePath` convention, `hub-client/src/utils/uniquePath.ts`), computed when the dialog reaches the head of the queue. A user-typed name that collides shows an error and disables Import (the `PlaceFileDialog` behaviour). Names may contain spaces; image links are percent-encoded (I12).
- **I12 Media** goes into the sibling folder `<stem>_media/`, named by content: `<first 12 hex of sha256>.<ext>` over the stored bytes. Image links are rewritten relative to the qmd and percent-encoded (a space becomes `%20`): pampa writes link targets verbatim and the qmd reader rejects a space in one (verified: `![x](foo 2_media/ab.png)` gives Q-2-33). Content names are stable across re-imports of an edited source (I20) and dedupe identical images (for converted EMF/WMF only because the converter renumbers rtf.js's page-lifetime clip/pattern ids, so the same drawing gives the same SVG bytes). After an import the new qmd opens, and an **import report** lists everything that was lost, converted, skipped or warned about.
- **I13 Comment ranges the span can't express** (the range crosses blocks, or its start and end sit at different inline nesting depths, e.g. the start inside emphasis) become a **block comment**: the comment span is appended to the top-level inline list of the innermost `Para`, `Plain` or `Header` holding the range's start; if there is none (a caption, a LineBlock, a definition term), a new `Para` holding the comment goes right after the innermost block containing the start. The report counts them.
- **I14 Elliot's span comments ship early, temporarily.** P0 forks `origin/feature/span-comments` and rebases it onto the integration line, so imported span comments render as bubbles while we work. **Those commits are removed before the epic's final PR** (Close-out); his branch lands through its own PR.
- **I15 EMF/WMF conversion uses rtf.js's EMFJS and WMFJS** renderers, serialized to SVG at 1x (root `width`/`height` = the metafile's size at 96 dpi).
- **I16 Images over 10 MB** (`FILE_SIZE_LIMITS.MAX_FILE_SIZE`, `hub-client/src/services/resourceService.ts:167`; for a converted EMF/WMF the size is the SVG's, and a bitmap-wrapping EMF can inflate ~1.33x as base64, in which case the original is kept, flagged `conversion_failed`) are skipped with a report entry. Their links point where the image would have been stored, so the gap is visible. They are not resized. This includes images the host drops for exceeding `collected_file_bytes` (25 MB; a small docx can hold a large uncompressed BMP or TIFF): the host's `collect-limit` warning carries the path and size, and P4 turns it into the same `skipped` manifest entry.
- **I17 List number styles are reported, not preserved.** qmd has no fancy list markers: `a.`, `(i)` and `A)` parse as text (verified; `(@)` is the one exception, it reads as an example list). The qmd writer keeps the start number and writes `.` or `)` (`write_orderedlist`, `qmd.rs:793-844`), so an ordered list whose style isn't `Decimal`/`DefaultStyle`, or whose delimiter is `TwoParens`, loses its style and keeps its start number, and is reported. Changing the grammar is out of scope.
- **I18 qmd writer fixes belong to this epic (P2).** Three content-corrupting bugs reachable from docx, plus the editorial-mark shorthand with attributes, which track changes depend on. Writer bugs docx can't reach (pipe-table footers, definition-list indentation) stay out.
- **I19 Source size cap: 25 MB** (Gordon asked for a recommendation; this is it, adjustable by P1's measurement). A source file over the cap is refused in the dialog before pandoc loads. Reasons: the browser process grew 14-16x the payload during a pandoc render (evidence doc), so 25 MB already means about 350-400 MB transient; the 300 MB host total was flagged as too generous for 4 GB devices; 25 MB matches the host's existing per-image limit (`image_bytes`); typical docx files are a few MB, and image-heavy ones are mostly under 25 MB. P1 measures memory for a 25 MB image-heavy docx, and for a 25 MB docx of compressible images that expands toward `collected_total_bytes`. The trigger is **Chromium** process RSS growth over the run above 1 GB: lower the cap. WebKit numbers are recorded for magnitude only, since its baselines are 5-7 GB and its growth isn't monotonic (evidence doc §12-13). The value lives in the Rust format table (interface 4), not in TS. `collected_total_bytes` stays 300 MB (decided 2026-10-03, review); the measurement is recorded so it can be revisited.
- **I20 Built for re-import.** Re-importing an edited source over an existing qmd (merging reviewers\' Word edits back) is **out of scope**, but v1 must make it easy later:
  - the pipeline (`prepare_import`, `finish_import`) is a pure, deterministic function of the source bytes, the pinned pandoc and the options: no timestamps, no random ids, stable media names (I12; the SVG converter normalizes rtf.js's counter ids to get this);
  - the transforms and the media plan are separable, so a future three-way merge can rerun them on a new source;
  - the qmd is created with its full import content as **one** Automerge change (the content passed as the new doc's initial value; today's `createFile` makes an empty doc and then a second change), so the import is a recoverable base in the document's history;
  - the I3/I4 shapes invert to Word: an attributed `quarto-insert` / `quarto-delete` span is a tracked change, a `quarto-highlight` span is a highlight, and a plain span ending in a trailing run of `quarto-edit-comment` spans is a commented range with its replies. P6's export transform maps them back to pandoc's `insertion` / `deletion` / `mark` / `comment-start` / `comment-end` spans (I23; Findings → "The export side"), and P6 T7 tests the round trip. Not everything inverts: a point comment, an I13 block comment and a comment at the end of a paragraph look the same, so an I13 comment comes back as a point comment.
- **I21 The "Add asset" dialog stays the way to store a file as-is.** P5 adds the I1 extensions, plus `emf` and `wmf` (stored when conversion fails, I8), to `BINARY_EXTENSIONS` and their MIME types to `inferMimeType` (`ts-packages/quarto-automerge-schema/src/index.ts:478`, `:599`), and to the mirrored `BINARY_EXTENSIONS` in `crates/quarto-hub/src/resource.rs:31`, which classifies files on disk. Today a stored `.docx` isn't in `BINARY_EXTENSIONS`, so clicking it probably opens an empty text editor (inferred from `Editor.tsx:988-993`, not run). Afterwards a click on it is a no-op, like other binary files without a viewer (pdf, fonts).
- **I22 Write order and failure:** the images are written first, then the qmd, which is the commit point. A crash or closed tab mid-import then leaves at worst unreferenced images in `<stem>_media/`, never a qmd with broken links. On any failure every file this import created is deleted (not a deduplicated hit on an existing identical file, and not a path another client has since replaced), and the report names the step, path and cause.
- **I23 Editorial marks render to Word and PowerPoint, built in.** The inverse of I3/I4: a Rust AST transform (P6), always on for docx and pptx, ports Gordon's `quarto-ooxml-editorial-marks` extension (both filters: the mark rewrite and attribution stamping) and adds the I4 commented-range shape, comment replies included. It runs in quarto-core's transform pipeline, before pandoc, so native renders and the hub's "Download as" both get it. Where it diverges from the extension (numeric comment ids, `author="unknown"` on an unattributed comment, insert/delete/highlight marks inside block bodies converted (comment marks in a block comment's body fold into its text), pptx block text joined with a space), P6 says so; decided with Gordon 2026-10-03. A project that still lists the extension renders the same, because the transform leaves it no marks to rewrite. Decided 2026-10-03.
- **Out of scope for v1:** the `q2 preview` embed. It ships no pandoc.wasm (pandoc-wasm design D7), so import there would need its own route through native pandoc. The Import button is hidden wherever "Download as" uses the native route.

## Findings that shape the plans

### The pipeline works natively, and Quarto's own docx comes back as layout tables
`pandoc 3.11 -f docx -t json --extract-media=DIR`, then `pampa -f json -t qmd`, on the four Quarto-produced docx fixtures in `crates/quarto-core/tests/fixtures/pandoc-recordings/recordings/{tables,images,crossrefs,callouts}-docx/reference/`, succeeded with no errors.
- Figures and captioned tables come back as `::: {.list-table}` with a "Figure 1: …" paragraph; callouts as one-column pipe tables holding an icon image and the title; crossref links point at ids that no longer exist (`[Figure 1](#fig-elephant)`). This is why I2 defers reconstruction.
- Image targets are pandoc's extract paths: `DIR/media/rId9.png` for docx (the docx-internal `media/` path under `DIR`). The import must choose the stored path and rewrite links (I12).

### pandoc.wasm host (`ts-packages/pandoc-host`, `hub-client/src/pandoc/`)
- The official pandoc 3.11 release asset (`resources/pandoc-wasm.json`). Native 3.11 lists all five I1 readers. **Nobody has run `--list-input-formats` on the wasm build.** It has no HTTP support, so remote images are never fetched.
- `execute()` returns **one** output file (read at `ts-packages/pandoc-host/src/execute.ts:179`, returned at :181); the virtual filesystem is thrown away, so `--extract-media` output is lost today. The research spike's patched host already walked the tree after a run (`claude-notes/research/2026-10-01-pandoc-wasm-spike/host-patched.js:147-230`).
- Inputs: stdin is always empty (`execute.ts:110`), so the source must be a mounted file. Files arrive inside the request (`files`, bytes) and are validated by `src/validate.ts` (absolute normalized paths, under the share root or project root, no double mounts). Limits are in `src/limits.ts`, mirrored from `resources/pandoc-wasm.json`; `src/request.test.ts:45` checks the drift (the `limits.ts` header comment's `constants.test.ts` does not exist).
- The request schema (`crates/quarto-core/schemas/pandoc-request.schema.json`, `additionalProperties: false`, `schema_version` const 1) requires writer-oriented fields: `writer`, `stage_name`, `json_path`, `typst_available_fonts`, and others. An import request fills them as described in interface 1.
- The worker protocol (`src/protocol.ts`) transfers the output buffer; `src/transfer.ts` has `prepareForPost`.

### JSON → qmd (pampa)
- **Bug:** the existing wasm export `ast_to_qmd` (`crates/wasm-quarto-hub-client/src/lib.rs:3341`) uses the strict `pampa::readers::json::read`, which requires a source-info `s` field on every node. Pandoc JSON has none, so it fails on the first node. The lenient reader is `read_completing_source_info(reader, By::unknown())` (`crates/pampa/src/readers/json.rs:1606`), as the CLI uses it (`crates/pampa/src/main.rs:284-290`, followed by `transform_divs`).
- The TS wrapper `writeQmd` (`ts-packages/preview-runtime/src/wasmRenderer.ts:901-928`) calls a non-existent `write_qmd` export and has no callers. `crates/wasm-qmd-parser`\'s `convert` uses the strict reader and `unwrap()`s. Neither is used by this epic.
- `pandoc-api-version` is not checked (`json.rs:1630`). Meta becomes YAML front matter: `MetaInlines` as qmd-inline strings, keys alphabetical.

### qmd writer bugs reachable from docx (verified; inputs in the research folder)
Probed with `losses.md` → pandoc JSON (directly, and via docx and `-f docx`) → `pampa -t qmd` → re-read, plus the hand-written `esc.json`:
- A paragraph whose text starts with `1. `, `- ` or `+ ` re-reads as a list (the `1.` case verified with `esc.json`; `losses.md`\'s `\1. not a list` reaches pandoc as a literal backslash, so P1 corrects that source). The same holds at the start of a list item or block quote's first line (`* 1. x` and `> + z` re-read as nested lists). Line starts after a soft break matter too: in `para\n- dash`, `para\n1) x`, `para\n---` and `para\n::: x` the second line becomes a list, a rule or a div (`para\n: colon` warns Q-2-54).
- A footnote's paragraphs are joined into one. A list inside a footnote is flattened into its text (docx path) or replaced with the literal `[complex block]` (`crates/pampa/src/writers/qmd.rs:2416`, markdown path). qmd has a native multi-block footnote syntax, the fenced definition `::: ^id … :::` (`crates/pampa/tests/roundtrip_tests/qmd-json-qmd/note_definition_fenced_block_*.qmd`); 4-space indented continuation is rejected (Q-2-35). The pampa reader keeps `[^id]` as a `quarto-note-reference` span plus a `NoteDefinitionFencedBlock`, and quarto-core's `FootnotesTransform` (`crates/quarto-core/src/transforms/footnotes.rs`) resolves them into `Note` for every format. `pampa -t native` alone refuses them (Q-3-11).
- Two adjacent lists merge on re-read. The docx reader emits two adjacent `OrderedList`s when Word restarts numbering (seen in `losses.md`\'s docx round trip). For bullet lists, alternating the marker (`*` then `-`) keeps them apart with no AST change (verified). For ordered lists there is no lossless separator: changing the delimiter changes the AST, and `<!-- -->` re-reads as `RawBlock (Format "html") "<!-- -->"` (verified).
- Editorial marks lose their shorthand when they carry attributes. `write_span` emits `[++ ]`, `[-- ]`, `[>> ]` or `[!! ]` only for a span whose single class is the mark and which has no id or attributes (`qmd.rs:2238-2250`), so `author=` turns `[++ x]{author="Ann"}` into `[x]{.quarto-insert author="Ann"}`. The reader accepts the attributed shorthand (`tests/roundtrip_tests/qmd-json-qmd/editorial_marks_with_attributes.qmd`), and the `Inline::Insert`/`Delete`/`EditComment` writers (`qmd.rs:2791-2850`) already emit it. Verified: `editorial-marks-roundtrip.qmd` in the research folder.
- Not reachable from docx: pipe-table footers (Word has none; the footer came back as a body row).
- **Correction (P2 execution, 2026-10-03):** multi-block definition-list indentation *is* reachable when the docx carries pandoc's "Definition" / "Definition Term" styles (pandoc-made docx; Word users who apply those styles): the writer emits `Term\n:   Definition para one.\n\n    Definition para two continues here.`, which the qmd reader rejects (Q-2-35, indented code block). P2 left it out of scope; it needs a follow-up strand or a wider I18. See P2's Handoff log.

### Track changes and comments from the docx reader (verified)
`pandoc track-changes-source.md -o tc2.docx; pandoc -f docx --track-changes=all -t native tc2.docx`:
- Insertions and deletions: `Span ("", ["insertion"|"deletion"], [("author",…),("date",…)]) [content]`.
- A comment is three **siblings** in an inline list: `Span ("", ["comment-start"], [("id","0"),("author",…),("date",…)]) [comment text]`, then the range's inlines, then `Span ("", ["comment-end"], [("id","0")]) []`. A range can start in one `Para` and end in the next.
- With `--track-changes=accept` (pandoc's default) all of these disappear.
- Paragraph-mark changes (verified in the implementability review by patching `<w:ins/>` / `<w:del/>` into a paragraph's `w:pPr/w:rPr`): an empty `Span ("", ["paragraph-insertion"|"paragraph-deletion"], [author, date]) []` as the **last** inline of that paragraph.
- Replies (verified the same way, with two comments over one range): the reply is a second `comment-start` directly after the parent's, and its `comment-end` is **nested inside** the parent's: `Span ("",["comment-end"],[("id","0")]) [Span ("",["comment-end"],[("id","1")]) []]`. pandoc 3.11 does not read `commentsExtended.xml`, so a reply isn't marked as one and resolved state is invisible. P3 T4 therefore groups comments over the same range (one wrapper, comment spans in start order: I4's parent-then-replies shape), and has no resolved-comment rule.
- Not yet checked: multi-paragraph comment text, and replies in a Word-saved file (P1 T3).

### Nested-comment rendering in q2-preview (verified)
`research/…/nested-comment-probe.integration.test.tsx.txt`, run with `vitest.integration.config.ts` in `ts-packages/preview-renderer`:

| comment placed | `feature/pandoc-wasm` | `origin/feature/span-comments` @ `d0004a327` |
|---|---|---|
| directly in a paragraph | bubble | bubble |
| in a plain span `[range [>> c]]` | **text inline, no bubble** | bubble; span gets `.q2-commented-span` |
| in a highlight `[!! range [>> c]]` | text inline | **text inline** (excluded by design) |
| in a `::: !!` div's paragraph | bubble | bubble |
| a Plain holding only a comment, inside a div | text inline | text inline |

The parser handles all of them. Without P0, I4's span comments are correct qmd but render as inline text.

### The export side already exists outside q2
Gordon's extension `quarto-ooxml-editorial-marks` (`github.com/gordonwoodhull/quarto-ooxml-editorial-marks`, local `~/src/quarto-ooxml-editorial-marks`) is the inverse of import. It turns `[++ ]`, `[-- ]`, `[!! ]` and `[>> ]` (inline and `:::` block forms, with `author=` / `date=`) into native Word tracked changes, highlights and comments. It does this by emitting the span classes pandoc's docx writer handles natively (`insertion`, `deletion`, `comment-start` / `comment-end`, bare `mark`), with no OOXML post-processing. Its README describes an inline comment only as the `[>> ]` mark itself and doesn't mention a parent span marking the commented range (I4), so a docx → qmd → docx round trip would likely lose comment ranges until the extension learns that shape. Not checked by running it. P6 ports it into q2 as a Rust transform and adds the I4 shape (I23). Two reasons to port rather than extend it: its docx filter runs at `post-quarto`, which for docx is inside the pandoc subprocess (`stage/stages/user_filters.rs:59-70`), after crossref and callout rendering have moved on; and its attribution stamping needs a second filter at `pre-quarto`, the only place with `quarto.attribution`.

### hub-client upload and drop (from code reading)
- `PlaceFileDialog` (`hub-client/src/components/PlaceFileDialog.tsx`) is the folder-and-name dialog, driven by a queue in `Editor.tsx:489-496` and rendered at `Editor.tsx:1889-1895`. Collisions show an error and disable confirm.
- Drops: `FileSidebar.tsx:540-603` (external files: `collectDroppedEntries`, `hub-client/src/utils/droppedEntries.ts:56`) and the Monaco DOM handlers (`Editor.tsx:1213-1295`). Both route to `Editor.handleDropFiles` (`Editor.tsx:1177-1208`) by extension only. The only other drop handler is `NewAssetDialog`\'s zone; there is no window-level handler, so a file dropped anywhere else (top bar, preview, image viewer, a dialog overlay) gets the browser default today.
- `createFile` (`ts-packages/quarto-sync-client/src/client.ts:1610`) does not check collisions and silently overwrites the index entry; it creates an empty doc and sets the text in a second change. `createBinaryFile` (`:1634`) returns `deduplicated: true` and writes nothing when the path already holds the same hash, or writes to `name-<hash8>.ext` when it holds different content, and returns the path it used. `deleteFile` (`:1692`) is keyed by path only. `createFolder` (`:1744`) writes a folder marker that `deleteFile` does not remove. No multi-file add is atomic.
- `handleSelectFile` (`Editor.tsx:985-1002`) reads content from `fileContents`, which a just-created file isn't in yet, and returns early in replay mode; `handleCreateTextFile` (`Editor.tsx:1116-1127`) is the pattern for opening a file just created.
- "Download as" is `DownloadAsControl` in `DocumentTopBar.tsx:138-147`; it is passed only for source files (`Editor.tsx:1594-1617`). Import must not have that restriction.

## Pinned interfaces

Plans build against these; changing one is a STOP-and-ask, not a plan-local edit. Field names are final; doc comments may grow.

### 1. Host: caller-supplied inputs and collected outputs (P1 builds, P3 emits, P4 uses)
Two optional fields join `PandocRequest` (schema, `types.ts`, `request.test.ts`\'s field list). They are additive, so `schema_version` stays 1:

```ts
/** Files whose bytes the caller supplies at run time instead of carrying them in the request. */
host_inputs?: { path: string; sha256: string; size: number }[];
/** Directories whose files, after a successful run, come back in `ExecuteSuccess.collected`. */
collect_dirs?: string[];
```

- `execute(request, shareTree, { …, inputs?: Record<string, Uint8Array> })`. The host mounts each `host_inputs` path from `inputs[path]` after checking size and sha256. A missing or mismatched input is `invalid-request` with a new host code `input-mismatch`.
- `validate.ts` (pure, synchronous) shape-checks both fields in `checkShape`. `host_inputs` paths obey the `files` rules: normalized, under the share or project root, not `/tmp`, and in the double-mount check. `collect_dirs` entries are normalized, under the share or project root, not `/tmp`, and not an ancestor of the share tree or of any mounted file. Declared `host_inputs[].size` counts toward `total_bytes`. The sha256/size check against real bytes happens in `execute()`.
- `ExecuteSuccess.collected: RequestFile[]`: every regular file under each `collect_dirs` entry, with absolute paths, sorted by path, empty if none. New limits `collected_file_bytes` (25 MB) and `collected_total_bytes` (300 MB) join `Limits` and `resources/pandoc-wasm.json`. Exceeding one is a host warning `collect-limit` that drops the offending file, not a failure; the warning carries the file's `path` (the existing `HostDiagnostic.path`) and its `size` (a new optional `HostDiagnostic.size`) so P4 can record it as skipped (I16).
- The worker protocol's `run` message gains `inputs`, transferred; `result` transfers the collected buffers too, using the same whole-buffer guard as `output` (`protocol.ts:46-47`) and `prepareForPost`-style dedupe.
- `job_id` covers `host_inputs` by sha256, like `files`, and only when the list is non-empty (the `post` pattern in `compute_job_id`), so writer requests keep their ids. P1 T4 makes this change, in the same file as the new fields.

### 2. Rust wasm exports (P3 builds, P4 calls)
All in `crates/wasm-quarto-hub-client/src/lib.rs`, returning JSON strings with a `success` flag and a `diagnostics` array in the `RustDiagnostic` shape (`ts-packages/pandoc-host/src/types.ts`).

```
get_import_formats() -> { formats: [{ id, label, extensions: [".docx"], mime_types: [..] }], max_source_bytes }

prepare_import(file_name, size, sha256_hex) ->
  { success, diagnostics, format?, request?, share_tree?, source_path? }

finish_import(json_text, stderr, target_qmd_path, media_manifest_json) ->
  { success, diagnostics, qmd?, media_plan?: [{ pandoc_path, project_path }] }

classify_import_failure(kind, status, stderr) -> { diagnostics }
```

- `prepare_import` checks the extension (Q-24-1, case-insensitive), then the size (Q-24-2), before it looks at `sha256_hex`. With `sha256_hex` empty it is a **validation-only** call: it returns `success` and diagnostics, but no request. P4 wraps it as `validateImportSource`, which P5's dialog calls on open to refuse a file before reading it; the UI never calls the export directly. With a hash, `size` must be the length of the bytes actually read, and it returns a request in `/__q2_share__/import/`:
  - argv: `pandoc -f <fmt> [--track-changes=all for docx] -t json --extract-media=/__q2_share__/import/media -o /__q2_share__/import/out.json /__q2_share__/import/source.<ext>`;
  - `host_inputs = [{ path: source_path, sha256, size }]`;
  - `collect_dirs = ["/__q2_share__/import/media"]`;
  - `writer: "json"`, `stage_name: "import"`, `json_path: source_path`, `post: "none"`, `typst_available_fonts: null`, `doc_dir` and `project_root` = `/__q2_share__/import`, `output_path` = the `-o` path;
  - `files: []`, `resource_refs: []`; `kind`, `env`, `dirs` and `expected_pandoc_wasm_sha256` set by the same code that sets them for writer requests (`crate::pandoc_request`; whether `dirs` needs `/tmp` is settled by P1 T7's real run);
  - an **empty share tree** with its own `share_tree_version` (SHA-256 over zero entries; no filters run).

  P1 confirms the paths satisfy `validate.ts` and adjusts the constants (not the shape) if they don't.
- `media_manifest_json` lists every collected file in collected order, then one `skipped` entry per `collect-limit` warning:

  ```
  [{ pandoc_path, status: "stored", sha256, ext, converted_from?, conversion_failed? }
   | { pandoc_path, status: "skipped", reason: "too-large", size }]
  ```

  `converted_from` is `"emf"` or `"wmf"`. When conversion fails the original is `stored` and the entry carries `conversion_failed: true` (I8).
- `target_qmd_path` and every `media_plan[].project_path` are project-relative, `/`-separated, with no leading slash (the `normalizeProjectPath` form).
- `finish_import` reads the JSON leniently, runs the import transforms (P3), builds `media_plan` (stored entries → `<target stem>_media/<sha12>.<ext>`; skipped entries get no plan entry and their links point at `<stem>_media/<pandoc basename>`), rewrites Image targets relative to the qmd and percent-encoded (I12), writes qmd with the P2 writer and turns pandoc's stderr `[WARNING]` lines into diagnostics. A Q-24-12 is fatal: `success: false`, no `qmd`, no `media_plan`.
- `classify_import_failure(kind, status, stderr)` turns a run failure into Q-24 diagnostics; `status` is `null` when pandoc didn't exit (crash, timeout). The full mapping of `RunFailureKind` to diagnostics and UI state is P4 T4's table: `pandoc-exit` and `no-output` → Q-24-3; `oom`, `crash`, `timeout` → Q-24-13; `invalid-request` (including `input-mismatch`) and `superseded` (imports are serialized, so it means a bug) → Q-24-12; `aborted` → cancelled, no report; `load-failed` and `worker-blocked` → `uiStateFor` (`pandocRunner.ts:30-115`), whose state P5's report view shows.

### 3. Import report and diagnostic codes (P3 owns the catalog; P4 and P5 display)
Subsystem **Q-24 `import`** (free on every branch as of 2026-10-03; re-check at P3 start with `git log --all -S'"Q-24-1"' -- crates/quarto-error-catalog`, since an unrestricted search matches this plan's own commit). Initial allocation. P3 may append codes after Q-24-14 if its transforms need them, and records them in this table; existing codes are never renumbered or repurposed.

| code | kind | meaning |
|---|---|---|
| Q-24-1 | error | unsupported file type |
| Q-24-2 | error | source over `max_source_bytes` |
| Q-24-3 | error | pandoc couldn't read the file (corrupt, encrypted, wrong type); from `classify_import_failure` |
| Q-24-4 | warning | pandoc reader warning (stderr `[WARNING]` passthrough) |
| Q-24-5 | info | comment couldn't wrap its range; attached to the block (I13), with count |
| Q-24-6 | info | paragraph-mark tracked change dropped, with count |
| Q-24-7 | warning | ordered list number style lost (I17), with count |
| Q-24-8 | warning | image skipped, over 10 MB (I16), names the file |
| Q-24-9 | warning | image conversion failed, original stored (I8); replaces Q-24-11 for that file |
| Q-24-10 | info | image converted from EMF/WMF |
| Q-24-11 | warning | image stored in a format browsers can't show (not raised for a file that already has Q-24-9) |
| Q-24-12 | error | internal import error: pandoc's JSON couldn't be read, an image target has no media manifest entry, or the host rejected the request; fatal |
| Q-24-13 | error | pandoc ran out of memory or time, or crashed, on this file; kind-specific hint |
| Q-24-14 | info | unmatched comment marker (a start without an end becomes a point comment; an end without a start is removed), with count |

TS-side import failures are diagnostics in the host style (`origin: 'host'`, kebab-case `code`): `import-read-failed` (P4: the source couldn't be read), `import-write-failed` and `import-cleanup-failed` (P5 storage). They are typed in `importService.ts` as `ImportHostCode`, alongside `HostDiagnosticCode`, not added to the host package. Since `HostDiagnostic.code` is the closed `HostDiagnosticCode` union, `importService.ts` also defines `ImportHostDiagnostic = Omit<HostDiagnostic, 'code'> & { code: ImportHostCode }` and `ImportDiagnostic = Diagnostic | ImportHostDiagnostic`, which the outcome and the report view use. A `HostDiagnostic` has only `message` (no title or problem), and the report view renders both shapes. The report shown to the user is `finish_import`\'s diagnostics plus host warnings plus any storage diagnostics.

### 4. Format table (P3 builds, P5 consumes)
`get_import_formats()` above is the single source for the picker's `accept` string, drop interception, labels and `max_source_bytes` (I19). TS keeps no copy of these lists.

## Plans and order

| Plan | Scope | Depends on |
|---|---|---|
| **P0** [span comments carry-over](2026-10-03-document-import-P0-span-comments.md) | Fork Elliot's branch onto the integration line (temporary, I14) | none |
| **P1** [pandoc.wasm as a reader](2026-10-03-document-import-P1-pandoc-reader.md) | Reader probe, interface 1 in the host, shared import fixtures, memory measurement (I19), docx edge-case recording. Lands in **two stages**: stage 1 = T1-T4, stage 2 = T5-T8 | none |
| **P2** [qmd writer fixes](2026-10-03-document-import-P2-qmd-writer.md) | Line-start escaping, multi-block footnotes, adjacent lists, editorial shorthand with attributes (I18) | none |
| **P3** [import pipeline in Rust](2026-10-03-document-import-P3-import-pipeline.md) | Format table, request builder, transforms, media plan, Q-24 codes, interface 2 and 4 exports and their TS wrappers | P2; P1 stage 1 (fixtures, `argv.json`, the Rust request fields, T3 answers) before P3 T2; P1 T7 passed before P3 T6 |
| **P4** [import service](2026-10-03-document-import-P4-import-service.md) | Runner, orchestration, EMF/WMF conversion, size policy | P1, P3 |
| **P5** [UI and storage](2026-10-03-document-import-P5-import-ui.md) | Button, picker, dialog, drop routing, collision-safe writes, cleanup, report view, e2e | P4 (T1-T4 can start on P4's stub); P0 only for a manual bubble check |
| **P6** [editorial marks export](2026-10-03-document-import-P6-editorial-marks-export.md) | Rust port of `quarto-ooxml-editorial-marks` (docx, pptx, attribution stamping) plus the I4 shape; round trip through import (I23) | P3 (shared vocabulary, `finish_import`); P1's fixtures. Runs alongside P4/P5 |

```
P0 ───────────────────────────────┐
P1 ──┬────────────────────► P4 ──► P5
P2 ──┴──► P3 ─────────────► P4
          P3 ─────────────► P6
```

P0, P1 and P2 start at once, each in its own worktree if run in parallel. P1 lands in two stages, each rebased and fast-forwarded into `feature/hub-import` with its own workspace nextest: **stage 1** is T1-T4 (reader probe, fixtures, edge cases, schema and types including `job_id`); **stage 2** is T5-T8 (host inputs, collection, real-wasm reader tests, memory). P3 starts when P2 and P1 stage 1 have landed; it can begin the format table (T1) earlier. P3 T6 (media plan) starts only once P1 T7 has passed, because a T7 STOP (no `--extract-media` output under wasm) would replace collection with data URIs and change T6's input. P4 needs all of P1 and P3. P6 needs P3 and can run alongside P4 and P5, in its own worktree. P5 needs P4; its button, dialog and drop routing can start against the service stub P4 commits first (P4 T0).

**H10a/H10b overlap.** The pandoc-host warm-preview plans, [H10a](2026-10-02-pandoc-host-H10a-warm-executor.md) (warm executor) and [H10b](2026-10-02-pandoc-host-H10b-warm-pool-preview.md) (warm pool and preview), neither started, edit files that P1 T4-T6 and P4 T1 also edit. H10a T1 factors validation, tree building and output reading out of `execute()` into shared functions, and H10a T4a adds `RunStats` fields in `types.ts`. H10b T1a changes `protocol.ts`\'s `createHandler`, `pandocRunner.ts` (`RunFailure.stats`, `RunOptions.docKey`, `RunnerConfig.now`) and the `pandocRunner.test.ts` fakes. H10b T1b adds `getPreviewPandocRunner()` next to the singleton in `pandocService.ts`, where P4 T1 adds `getImportRunner()`. Whichever lands second on `feature/pandoc-wasm` / `feature/hub-import` rebases onto the other. If H10a T1 has landed first, P1 T5-T6 put input mounting and collection in its shared functions, so the fresh path keeps one implementation; the warm path never sees an import request. P4 T1 builds its own runner directly and does not wait for H10b.

## Progress

Tick a plan when its Close-out is complete and it has landed on `feature/hub-import`.

- [x] P0 [span comments carry-over](2026-10-03-document-import-P0-span-comments.md)
- [x] P1 stage 1 (T1-T4) landed
- [x] P1 [pandoc.wasm as a reader](2026-10-03-document-import-P1-pandoc-reader.md)
- [x] P2 [qmd writer fixes](2026-10-03-document-import-P2-qmd-writer.md)
- [x] P3 [import pipeline in Rust](2026-10-03-document-import-P3-import-pipeline.md)
- [x] P4 [import service](2026-10-03-document-import-P4-import-service.md)
- [x] P5 [UI and storage](2026-10-03-document-import-P5-import-ui.md) (landed; its Verification still lists Firefox, which does not launch on the dev machine, and the manual dev-server check, both unticked in the plan)
- [x] P6 [editorial marks export](2026-10-03-document-import-P6-editorial-marks-export.md)

## Execution conventions (every plan)

- **One agent per plan.** It executes the plan's tasks itself and spawns no sub-agents or forks. Search and survey subagents, when a plan explicitly allows them, run on Sonnet.
- **Branches and history:** each plan works on a topic branch `import/p<N>-<slug>` cut from `feature/hub-import`. History stays **linear**: rebase onto `feature/hub-import`, never merge; land by fast-forwarding `feature/hub-import`. When `feature/pandoc-wasm` advances, `feature/hub-import` is rebased onto it (the orchestrator does this between plans, not mid-plan). When pandoc-wasm reaches `main`, rebase onto `main`.
- **Gates per task:** `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>` for Rust; for TS, the package's vitest suites (unit, `vitest.integration.config.ts`, and `vitest.wasm.config.ts` where the task touches wasm paths) plus `npm run typecheck` in that package. **Per plan boundary and before any push:** `cargo nextest run --workspace` (about 3 min), reporting pass/skip counts against the live baseline taken at plan start, and accounting for the delta. A red workspace run is attributed from the plan's task list, not by bisecting. A plan with no Rust changes (P0, possibly P4) still runs it at its boundary and reports "no Rust changes, counts unchanged".
- **Shared files:** the epic file (Progress ticks, the interface-3 table) is edited only by the orchestrator, after a plan lands, so parallel plans don't conflict on it. A plan that needs an epic edit records it in its Handoff log. `hub-client/changelog.md` is the one file parallel plans both append to (P5 T9, P6 T9): entries are append-only, and a rebase conflict there is resolved by keeping both.
- **Environment before any TS gate** (P0 T1, P1 T1 and T4 in stage 1, P1 stage 2, P3 T8, P4, P5, P6 T8): `npm install` at the repo root (without it, hub-client's typecheck fails on stale workspace links to `@quarto/pandoc-host`) (a stale `node_modules` lacks `@bjorn3/browser_wasi_shim`, which `pandoc-host` needs), `node scripts/fetch-pandoc-wasm.mjs --require` (also from the root), and from P3 T8 on `npm run build:wasm` in hub-client so the Rust wasm has the import exports. The `*.wasm.test.ts` suites skip silently when the assets are missing, so a wasm gate counts only if it reports the tests as **run**, not skipped.
- **Long commands** write to a log under the session scratchpad and are inspected with grep/tail, run once.
- **Integration tests** go in `crates/<crate>/tests/integration/<name>.rs`, registered in `main.rs` (`.claude/rules/integration-tests.md`); never a new top-level `tests/<name>.rs`.
- **Cross-platform:** paths through `Path`/`PathBuf` in Rust; no `\n` assumptions in snapshot comparisons (`.claude/rules/cross-platform.md`).
- **hub-client colors:** solid colors only (`.claude/rules/hub-client-theme.md`).
- **Out-of-plan bugs** noticed along the way go to braid (`braid` skill), never into a plan's checklist. Anything that serves a plan goes into that plan's checklist.
- **STOP points:** a pinned interface would have to change; a decision (I1-I23) turns out wrong; a verification fails in a way the plan didn't anticipate. Stop, record it in the Handoff log, ask Gordon.
- **Handoff log:** each plan's log is append-only, updated in the commit that ends each task and before any stop.
- **Changelog:** P5's user-visible hub-client changes, and P6's change to "Download as" docx, get `hub-client` changelog entries the way the pandoc-wasm plans did. P0 adds none (it is temporary).

## Close-out (epic)

- [x] Every plan ticked above (P5 with the caveat noted on its line).
- [ ] End-to-end check on a real hub build: a Word-authored docx with tracked changes, comments, a manually typed "1." paragraph, a multi-paragraph footnote, an EMF image and a restarted numbered list imports, opens and renders; the report lists what it should. P1 T2 asks Gordon for this file; if it hasn't arrived, run the check on P1's `track-changes-docx`, `writer-bugs-docx` and `emf-docx` fixtures and say so in the PR. Then "Download as" docx on the imported qmd, open it in Word, and check that the tracked changes, highlights and comments (with their ranges and replies) are back (I23).
- [ ] **Remove Elliot's three carried commits** (I14) from `feature/hub-import` before opening the final PR. This item is about his commits only, not P0's work in general. The branch was squashed and rebased onto `feature/pandoc-wasm` on 2026-10-04, so identify them by the `-x` trailer, not by SHA or author (Elliot has other commits on `main`, and "initial prototype" is a generic subject): `git log --format='%h %s' --grep='cherry picked from commit' <base>..HEAD`. The trailers name his original SHAs `42622bf23ad6183f79e270bbb229da2626af7738`, `67d87096eb86976e4cb83d3b978e631149eb5133` and `d0004a327ced4936b4816dc8ab70180c21545d92`; after the rebase they are the top three commits of the branch (above everything else, including the unrelated fixes squashed on 2026-10-05). P0's tests and handoff note were squashed into the single commit `be34067c7` ("Add Word-import span-comment tests"); those tests need Elliot's code, so when his commits go, decide in the same step whether that commit's tests go with them (they would fail without it). Intermediate commits below his three do not pass the P0 tests; that is intended. Drop non-interactively (`GIT_SEQUENCE_EDITOR` rewriting `pick` to `drop`, or the git-revise skill), then confirm `git diff <pre-removal tip>` touches only the files of the dropped commits. Re-run the hub-client suites and the workspace nextest after the removal.
  If `feature/span-comments` reaches `main` before the epic ends, drop P0's commits at the next rebase onto `main` instead, and resolve toward Elliot's landed version.
- [ ] Note in the final PR that imported span comments render as inline text until `feature/span-comments` lands, and link Elliot's branch.
- [x] Every plan file re-read and its checklist reconciled against what landed; this epic's checklist likewise; committed (2026-10-04: all seven plans have a LANDED entry; the only open plan boxes are P5's two Verification items).

## Follow-ons (not in this epic)

- Quarto-aware reconstruction: Quarto's layout tables back into figures and callouts, working crossrefs (I2).
- Re-import with a three-way merge against the import base (I20).
- Retire `quarto-ooxml-editorial-marks` (its own repo): point its README at q2's built-in support once P6 lands.
- Threaded Word replies on export: pandoc writes no `commentsExtended.xml`, so P6's replies appear in Word as separate comments on the same range.
- epub chapters into a book project (I7).
- Fancy list markers in the qmd grammar (I17).
- Import in the `q2 preview` embed through native pandoc.
