# Plan: the import pipeline in Rust (document import P3)

**Date:** 2026-10-03
**Epic:** [`2026-10-03-document-import-epic.md`](2026-10-03-document-import-epic.md) (I1, I3, I4, I6, I9, I12, I13, I16, I17, I19, I20; builds interfaces 2, 3 and 4)
**Depends on:** P2 landed (the writer fixes; T4's oracle and T7's qmd output depend on them). P1 **stage 1** landed (T1-T4: the fixtures with `argv.json` and `corrupt-docx`, the T3 answers, the Rust request fields and `job_id`) before T2. P1 **T7 passed** (its Handoff log says so) before T6, because a T7 STOP would replace collection with data URIs and change T6's input. T1 can start before either. **Unblocks:** P4.
**Branch:** `import/p3-import-pipeline` from `feature/hub-import`.

## Why

Rust owns everything about an import except running pandoc and handling image bytes (I9): the format table, the pandoc request, reading pandoc's JSON, the transforms that turn Word's track changes and comments into q2 editorial marks, the media plan and link rewriting, the qmd, and every user-facing diagnostic. This plan builds that as a native-testable module plus four thin wasm exports.

## Where things go

- **New module `crates/quarto-core/src/import/`:**
  - `formats.rs`: the format table;
  - `request.rs`: `prepare_import`'s request builder, reusing `crate::pandoc_request` types, constants and `job_id` computation;
  - `transforms.rs`: track changes, comments, pptx format, list-style detection;
  - `media.rs`: the media plan and image-target rewrite;
  - `report.rs`: diagnostics, including stderr warnings and failure classification;
  - `mod.rs`: `finish_import`, the pure function.

  Nothing in it touches the VFS or the clock (I20).
- **Wasm exports** in `crates/wasm-quarto-hub-client/src/lib.rs`, next to `render_pandoc_request` and `get_pandoc_formats` (`lib.rs:1614`, `:1798`): `get_import_formats`, `prepare_import`, `finish_import`, `classify_import_failure`, exactly as interface 2 pins them. Add TS declarations to `ts-packages/preview-runtime/src/wasm-quarto-hub-client.d.ts`, where the other pandoc exports are declared (`hub-client/src/types/wasm-quarto-hub-client.d.ts` declares none of them; leave it alone). Add TS wrappers in `ts-packages/preview-runtime/src/wasmRenderer.ts` next to `getPandocFormats`, in the same synchronous `getWasm()` style that parses the JSON. Their names are pinned so they don't collide with P4's service functions: `getImportFormatTable()`, `prepareImport()`, `finishImport()` and `classifyImportFailure()`. Export them from the package index, so P4 imports them from `@quarto/preview-runtime` as `downloadService.ts` does. Callers must have awaited `initWasm()` first.
- **Diagnostic codes** Q-24-1 … Q-24-14 in `crates/quarto-error-catalog/error_catalog.json`, subsystem `import` (interface 3). P3 may append Q-24-15 and up if T4 needs them (e.g. for multi-paragraph comment text, per P1 T3 (d)); it never renumbers. First re-check that no branch has claimed Q-24 since 2026-10-03: `git log --all -S'"Q-24-1"' -- crates/quarto-error-catalog` (without the path filter it matches the epic's own commit).
- **Tests:** `crates/quarto-core/tests/integration/import_*.rs` (registered in `main.rs`), reading P1's `crates/quarto-core/tests/fixtures/import-recordings/`. Fixtures are named by their **directory name** (`basic-docx`, `track-changes-docx`, `emf-docx`, `basic-pptx`, …; P1 T2 lists them).
  - Expected qmd outputs depend on `target_qmd_path` (links are relative to it), so every fixture uses `target_qmd_path = "<directory name>.qmd"` at the project root (e.g. `emf-docx.qmd`). Record that convention in the fixtures README; P4 T6 uses the same.
  - The expected qmd lives at `import-recordings/<directory name>/expected.qmd`. P1's capture leaves it alone (P1 T2: not deleted, not in `manifest.json`). For `track-changes-docx` and `highlights-docx` it is written by hand from the I4 shapes (T4); for the others it is the implementation's output, reviewed by eye once, so it catches drift only. P4 T6 reads the same files.
  - `import_support.rs` (a test helper module in the same `integration` binary) builds each fixture's `media_manifest_json` from P1's `manifest.json`: every file under `media/` becomes `{ pandoc_path: "/__q2_share__/import/media/" + <relative path>, status: "stored", sha256, ext }`. For `emf-docx`, the manifest P4 T6 also produces (converter stubbed to fail): the EMF and WMF are `stored` with `ext` `emf`/`wmf`, the original bytes' sha256 and `conversion_failed: true`, so its `expected.qmd` links `<sha12>.emf`. T6 tests the converted (`png`) variant separately.

## Checklist

### Tasks

- [ ] **T0 Baseline.** Workspace nextest pass/skip counts in the Handoff log. Read P1's Handoff log section on docx reader edge cases (T3 there) before T4, and confirm P1 T7 has passed before T6.
- [ ] **T1 Format table and `get_import_formats`.** One entry per I1 format: `id` (pandoc reader name), `label`, `extensions`, `mime_types`, and whether `--track-changes=all` applies (docx only). MIME types:
  - docx: `application/vnd.openxmlformats-officedocument.wordprocessingml.document`
  - odt: `application/vnd.oasis.opendocument.text`
  - rtf: `application/rtf`, `text/rtf`
  - epub: `application/epub+zip`
  - pptx: `application/vnd.openxmlformats-officedocument.presentationml.presentation`

  `max_source_bytes` = 26214400 (I19; P1's T8 may have lowered it, so use P1's number). Extension matching is case-insensitive. Unit tests.
- [ ] **T2 `prepare_import`.** Validate the extension (Q-24-1), then the size (Q-24-2). An empty `sha256_hex` means validation only: return without a request (interface 2). Otherwise build the request exactly as interface 2 pins it, every field of the schema set:
  - `host_inputs` and `collect_dirs`;
  - `writer: "json"`, `stage_name: "import"`, `post: "none"`, `files: []`, `resource_refs: []`;
  - the empty share tree and its version;
  - the remaining fields: writer requests set them inline in `PandocWriteStage::prepare()` (`stage/stages/pandoc_write.rs:1247-1296`); there is no shared helper to call. Pinned values: `share_root` = `constants().share_root`; `share_tree_path` = `{share_root}/pandoc-share`; `expected_pandoc_wasm_sha256` = `constants().wasm_sha256`; `env` = `{}` (the writer allowlist's three keys, `QUARTO_SHARE_PATH`, `QUARTO_FILTER_PARAMS` and `QUARTO_FILTER_DEPENDENCY_FILE`, are all filter keys and don't apply); `dirs` = `[]` plus whatever P1 T7 found necessary (P1's import golden records it); `kind` = `RequestKind::Pandoc` (the only variant);
  - the empty share tree's version: hex SHA-256 over zero bytes, from a new function (`share::share_tree_version()` is the real tree's);
  - `job_id` from `compute_job_id`, which P1 T4 already extended to cover `host_inputs`.

  Tests:
  - the built request validates against `pandoc-request.schema.json`, using the `jsonschema` validator the way `crates/quarto-core/tests/integration/pandoc_request_contract.rs` does. Built with the import golden's file name, size and sha256, it equals P1's import golden exactly, `job_id` included;
  - per format, the argv equals P1's recorded `argv.json`, so the builder and the recordings can't drift.

  The golden's `dirs` is provisional until P1 T7. If T7 changes it after P3 has landed, P1 T7 updates the golden and `import/request.rs` in the same commit (P1 T7 says so).
- [ ] **T3 Lenient read.** In `finish_import`: `pampa::readers::json::read_completing_source_info(.., By::unknown())`, then `transform_divs`, mirroring the CLI (`crates/pampa/src/main.rs:284-300`). A read error is Q-24-12. Test on every P1 fixture: read succeeds, and the qmd written without any transform re-reads.
- [ ] **T4 Track-change and comment transform (I3, I4, I13).** A pure `Pandoc -> (Pandoc, counts)` walk over every inline list, including those nested in containers, table cells, notes and captions, and the metadata's inlines:
  - Class names are module constants in `import/transforms.rs`; P6 T1 later moves them, and the I4 shape, into a shared `editorial_marks.rs` used by both directions, so keep them in one place.
  - `Span .insertion {author,date}` → `Span .quarto-insert {author,date}`; `.deletion` → `.quarto-delete`. Keep `author` and `date`; drop any other pandoc attribute. The mark class goes first (P2 T4's shorthand rule).
  - `Span .mark` (a Word highlight) → `Span .quarto-highlight` (I3), other attributes kept.
  - `paragraph-insertion` / `paragraph-deletion` (shape from P1 T3c) → removed; counted for Q-24-6.
  - Comments: pair `comment-start` / `comment-end` by `id`. The comment span is `Span .quarto-edit-comment {author,date} [comment-start's content]`; the Word `id` is dropped.
    - If both markers are in the **same inline list**, start before end: replace the slice `start..=end` with `Span ("",[],[]) [range inlines…, comment span]`. Leading `Space`/`SoftBreak` inlines of the range go in front of the wrapper, not inside it (I4: the reader trims a leading space inside `[…]`, verified); trailing ones stay inside, since the comment span is last.
    - With an empty range, put the comment span alone at the start's position, with no wrapper (I4).
    - Process properly nested pairs innermost first, so an inner pair wraps before its outer pair. Between pairs that are not nested, process in document order of their start markers, so overlapping ranges give one deterministic result (I20).
    - **Equal ranges (replies).** pandoc 3.11 gives a reply as a second `comment-start` directly after the parent's, with its `comment-end` **nested inside** the parent's `comment-end` span (epic Findings, verified). Comments whose starts are adjacent (nothing but other starts between them) and whose ends are adjacent or nested in this way share **one** wrapper. Its trailing run holds their comment spans in start order: the first is the parent, the rest its replies (I4). They are one group, not overlapping pairs.
    - `comment-end` markers are found at any depth, including inside another `comment-end`'s content. Removing a marker splices its content back in its place, so a nested end is never lost.
    - Any pair whose markers are in different inline lists, including the second of two overlapping ranges after the first has wrapped, falls back to I13: remove both markers and append the comment span to the top-level inline list of the innermost `Para`/`Plain`/`Header` containing the start, or, if there is none (caption, LineBlock, definition term), to a new `Para` right after the innermost block containing the start. Count it for Q-24-5.
    - Comment markers in metadata inlines are removed and their comments dropped, counted for Q-24-5.
    - An unmatched start becomes a point comment at its position; an unmatched end is removed. Count both for Q-24-14.
  - **Resolved comments** are not distinguishable in pandoc 3.11 (it doesn't read `commentsExtended.xml`), so there is no resolved-comment rule: they import as ordinary comments. **Multi-paragraph comment text:** apply P1 T3 (d). **STOP** if P1 T3 found a reply shape other than the equal-range group above.

  **Walkers.** `ast_walk::for_each_inline_mut` (`crates/quarto-core/src/ast_walk.rs`) visits one inline at a time in pre-order: it gives no access to the containing `Vec<Inline>`, doesn't walk metadata, and skips `Cite` content. Add a list-level walker that hands each `&mut Vec<Inline>` to a callback, covering blocks, containers, table cells, notes, captions and `Cite`. Add a `ConfigValue` walker for metadata (`PandocInlines` / `PandocBlocks` inside maps and arrays; the nearest precedent is `pandoc_filters/meta_coerce.rs`).

  **Sizing:** several days, in three commits, each gated on its own:
  - **T4a:** the walkers, the class renames, paragraph marks;
  - **T4b:** same-list pairing, the equal-range groups, the leading-space hoist, empty ranges;
  - **T4c:** the I13 fallback, metadata, orphans.

  Oracle (the same idea as P2's): the transformed AST, written with `qmd::write` and re-read, equals the transformed AST, compared as P2's quarto-core tests do (`pampa::writers::native::write` output of both ASTs; pampa's `remove_location_fields` lives in a pampa test module quarto-core can't reach). A comment inside a multi-block note re-reads as a note only after `FootnotesTransform`, so that case runs it first, using P2 T2's quarto-core harness. And for each fixture, the qmd matches its `expected.qmd` (see "Where things go"), written by hand from the I4 shapes. Cases:
  - P1's `track-changes-docx` fixture;
  - P1's `highlights-docx` fixture;
  - P1's `comments-edge-docx` fixture (P1 T3's cases (a)-(f), real pandoc JSON);
  - plus hand-built JSON, for shapes the fixtures lack: nested ranges; overlapping ranges; a range starting inside `Emph`; a range crossing paragraphs; a range starting with a `Space` (the I4 hoist); a comment with a reply (the equal-range shape, nested `comment-end`); a comment with two replies; a wrapper followed by `Str "(see)"` (P2 T1's escape); a comment in a table cell; a comment in a footnote; a comment in a figure caption (the new-`Para` fallback); a tracked change in the metadata title; an empty range; orphans.
- [ ] **T5 Other transforms.**
  - pptx: set meta `format: revealjs` when `format` is absent (I6).
  - Detect ordered lists whose style is not `Decimal`/`DefaultStyle`, or whose delimiter is `TwoParens`; count them for Q-24-7 (I17). The writer keeps the start number and writes `.` or `)` (`write_orderedlist`, `qmd.rs:793-844`), so only the style is lost.

  Tests with the `basic-pptx` and `writer-bugs-docx` fixtures.
- [ ] **T6 Media plan and link rewrite (I12, I16).** Input: the parsed `media_manifest_json` (interface 2) and `target_qmd_path`.
  - Paths are in the epic's pinned form (project-relative, `/`-separated, no leading slash). For `stored` entries: `project_path = <dir(target)>/<stem>_media/<sha256[..12]>.<ext>`. Two entries with the same sha256 and ext map to one path, listed once.
  - Rewrite every `Image` target equal to an entry's `pandoc_path` to `<stem>_media/<name>`: the media folder is always the qmd's sibling, so no general relative-path computation is needed. Percent-encode each segment (I12: the qmd reader rejects spaces in link targets with Q-2-33). Use the `percent-encoding` crate: today only `quarto-preview` declares it (`crates/quarto-preview/Cargo.toml:48`), so add `percent-encoding = "2"` to quarto-core's `Cargo.toml` (pure Rust; check the wasm32 build of `wasm-quarto-hub-client` still compiles). Use an explicit set that covers space, `(`, `)`, `[`, `]`, `#`, `?`, `%`, controls and non-ASCII. `pandoc_request/resources.rs:57`'s `percent_decode` is private and decodes only.
  - For `skipped` entries (including those P4 built from host `collect-limit` warnings): rewrite to `<stem>_media/<basename of pandoc_path>`, encoded the same way (deliberately broken, I16), and emit Q-24-8 naming the file.
  - `conversion_failed` → Q-24-9; `converted_from` → Q-24-10 (one info per batch, with count).
  - A stored `ext` outside the browser-displayable set (png, jpg/jpeg, gif, webp, avif, svg, bmp, ico) → Q-24-11, except for a file that already has Q-24-9.
  - An `Image` target under the extract dir with no manifest entry → Q-24-12 (internal mismatch), fatal: `success: false`, no qmd (interface 2).
  - Targets that aren't extract paths (external URLs) are left alone.

  `media_plan` lists `{pandoc_path, project_path}` for stored entries in manifest order. Tests:
  - the `images-docx` fixture: the twice-used image yields one file and two links; and `basic-odt` (verified: odt extracts the same image used twice as two files, `Pictures/0.png` and `1.png`, which the sha256 dedupe merges);
  - a target at the project root and one in a subfolder: `project_path` includes the folder, the link doesn't;
  - pandoc paths with spaces and non-ASCII names, and each format's extract layout under `--extract-media=/__q2_share__/import/media` (verified: docx `media/media/rId9.png`, odt `media/Pictures/0.png`, epub `media/media/file0.png`, rtf `media/<hash>.png` with no subfolder; paths relative to `/__q2_share__/import/`);
  - a target stem with a space (`report 2.qmd`): the written qmd re-reads with no Q-2-33 and the Image target decodes to the planned path;
  - a skipped entry; a converted entry; a failed conversion of an EMF (Q-24-9 only, no Q-24-11); a duplicate.
- [ ] **T7 `finish_import` and the reports.** Compose T3 → T4 → T5 → T6 → `qmd::write`. Turn stderr `[WARNING]` lines into Q-24-4, one per warning, with continuation lines joined; drop other stderr noise. Reuse the ANSI stripping and `[WARNING]` detection of `classify_pandoc_stderr` (`crates/quarto-core/src/pandoc_filters/diagnostics.rs:59`, which emits Q-11-1 and the shim codes Q-20-5/6/7 and doesn't join continuation lines) by factoring out a shared helper that returns the raw warning texts; each caller picks its own codes. Don't copy it. A `qmd::write` error is Q-24-12. `classify_import_failure(kind: &str, status: Option<i32>, stderr: &str)`:
  - `pandoc-exit` → Q-24-3, with pandoc's message as the problem text;
  - `no-output` → Q-24-3 with a hint that pandoc produced nothing;
  - `oom`, `crash`, `timeout` → Q-24-13 with a kind-specific hint;
  - `invalid-request`, `superseded` → Q-24-12.

  The classification is tested against real stderr with P1's `corrupt-docx` fixture (verified: exit 63, `couldn't unpack docx container: …`, no `[WARNING]` prefix). Determinism test (I20): `finish_import` twice on the same inputs gives byte-identical output.
- [ ] **T8 Wasm exports.** The four exports, thin wrappers over the module, returning interface-2 JSON. `finish_import` takes the JSON as `&str`; no image bytes cross (I9). `prepare_import`'s `size` crosses as `f64` (a `u64` would be a `BigInt` in JS; see the `source_date_epoch` note near `lib.rs:1600`). The crate is a `cdylib` with no native tests of its exports, so the Rust logic is tested in quarto-core and the exports only by the smoke test. Add the `.d.ts` declarations and the `wasmRenderer.ts` wrappers and index exports (see "Where things go"). The `.d.ts` also declares the raw response types, snake_case exactly as interface 2 returns them: `ImportFormatTable` (`{ formats: { id; label; extensions; mime_types }[]; max_source_bytes }`), `PrepareImportResponse` (`request` typed as `PandocRequestWire`, `share_tree`, `source_path`, `format`, `success`, `diagnostics`), `FinishImportResponse` and `ClassifyImportFailureResponse`; the wrappers return these unmapped, and P4 maps them to its camelCase types. Smoke test: `hub-client/src/services/importPipeline.wasm.test.ts`, on the pattern of `pandocRequest.wasm.test.ts` but without its `describe.skipIf(!pandocWasmAvailable())` (`:451`, `:647`): it needs only the Rust wasm, so a missing pandoc asset must not skip it. It calls `get_import_formats` and runs `finish_import` on one fixture's recorded JSON plus a hand-built manifest. Gate: `npm run build:wasm` (the checked-out wasm predates P3), then `npm run test:wasm`, with the new tests reported as run, not skipped.
- [ ] **T9 Catalog.** Catalog entries for every Q-24 code used, in the house style of the Q-20 entries: `subsystem`, `title`, `message_template`, `docs_url`, `since_version` (the catalog has no hints field). Hints and the dynamic parts ("with count", "names the file") are built at emission into the `RustDiagnostic`. Every code also needs a docs page and a sidebar entry, or `cargo xtask lint` fails (`crates/xtask/src/lint/error_docs.rs`, `error_docs_sidebar.rs`): one `docs/errors/import/Q-24-<n>.qmd` per code, modelled on `docs/errors/citeproc/Q-23-1.qmd`, and an `import` section in `docs/_quarto.yml` (like `citeproc`'s at `:411`), codes in ascending order. Gate: `cargo xtask lint` plus any catalog consistency test. T8 and T9 are each their own commit.

### Verification

- [ ] `cargo clippy -p quarto-core --all-targets -- -D warnings` and `-p wasm-quarto-hub-client`; `cargo nextest run -p quarto-core`; the hub-client wasm suite for T8.
- [ ] Every P1 fixture except `corrupt-docx` (which has no `pandoc.json`) runs through `finish_import` natively with no Q-24-12, and its qmd equals its `expected.qmd` (reviewed by eye once, diffed on later changes).
- [ ] Workspace nextest at the plan boundary, delta against T0 accounted for.

### Close-out

- [ ] Checklist reconciled, committed; rebased and fast-forwarded into `feature/hub-import`; epic Progress ticked.
- [ ] Any Q-24 codes appended beyond Q-24-14 are recorded in the Handoff log for the orchestrator to add to the epic's interface-3 table (epic conventions: shared files).

## Handoff log

Append-only.

- 2026-10-03: plan written. Not started.
- 2026-10-03: revised after the implementability review (epic status line): equal-range reply groups and nested comment-end; no resolved rule; T4 split and walkers; T2 values pinned; percent-encoding set; catalog docs pages and lint; wrapper names.
- 2026-10-03: revised after the angle review: fixtures named by directory; `expected.qmd` location and the `import_support.rs` manifest helper; emf expected output uses the failed-conversion manifest; T4 oracle comparison named and `comments-edge-docx` used; `percent-encoding` added to quarto-core; extract layouts per format; raw response types in the `.d.ts`; smoke test without `skipIf`; golden `dirs` follow-up.
