---
title: 'EXR-4: the engine execution record format (nbformat 4.5 with quarto_ metadata)'
date: 2026-10-28
description: 'A target-neutral library that defines, writes, reads, coalesces and hashes engine execution records as ipynb files, with Positron-compatible matching keys and the freeze groundwork, and no change to any pipeline.'
status: complete
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md)
**Depends on:** the epic's decision record; EXR-2 Phase 3 (units, their typed id slot, the scanner and its fixtures; EXR-2 Phases 4 and 5 are not needed).
Soft: EXR-1 (it reuses the nbformat reader `stored_result_for_cell`, which EXR-1
moves; the record type does not otherwise need the presenter).
**Blocks:** EXR-5 and EXR-7 (from Phase 4), EXR-11 and EXR-13a (from Phase 5), EXR-16 (from Phase 5). Per the dependency table in the epic, it also blocks EXR-6, EXR-8, EXR-9, EXR-12, EXR-13b, EXR-14, EXR-15.
**Lane:** B (`workspace-5`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 3-4 (hash vectors and the size measurement are the long poles).

**Merge gates (2026-10-31, Gordon).** Merge into the integration branch at the end of Phase 4
(types, hashes, ids, builder, writer) with `status: in-progress`: EXR-5 and EXR-7 need Phases 0
to 4. Merge again at the end of Phase 5 (reader, `place`, Positron import): the reader half of
EXR-13, EXR-14, EXR-16, 9C and 11C need it. Phase 6 (the freeze checklist) gates nothing and
comes last, with `status: complete`. Start when EXR-2 Phase 3 is ticked on the integration
branch. Phase 0 still comes first in order; its first task (the label-line hash mechanism)
is on EXR-5\'s path, and the size measurement may slip behind Phase 1 if it blocks.

> **For implementers:** TDD. This plan is a library plus one trait method
> (`ExecutionEngine::hash_definition`, with its Jupyter and knitr overrides) and the id
> function that fills the scanner\'s typed id slot; it must not otherwise touch the
> render pipeline, `EngineCapture`, or any engine.

## Goal

Define the record that every engine produces, in one place, with the properties
the epic requires: matchable by Positron's content hash, hashable for a future
freeze, deterministic on disk, self-contained, and readable without an engine.

## Background (verified 2026-10-28 unless marked)

**What exists to replace.** `quarto_trace::EngineCapture` (`crates/quarto-trace/src/lib.rs:187-220`)
holds `engine_name`, `input_qmd`, an opaque `result` JSON and `files`
(`CaptureFile`, doc-relative forward-slash keys, base64 payloads). The capture
array is gzipped by `gzip_captures` and read by `parse_capture_gz`
(`pandoc_request/captures.rs:18`, which also accepts a single object). The result
fields that must survive into a record are the `ExecuteResult` fields in
`engine/context.rs:291-381`: `markdown`, `supporting_files`, `filters`, `includes`,
`needs_postprocess`, `html_dependencies`, `metadata`, `pandoc`, `resource_files`,
`preserve`, `warnings`. Size: only a 10 MB gzipped warning exists
(`CAPTURE_DOC_WARN_BYTES`, `engine/capture_files.rs:206`); real-engine size is
unmeasured (bd-sr73, open P2).

**Positron's cache** (`external-sources/positron/src/vs/workbench/contrib/positronQuarto/`).

- Writer `browser/quartoOutputCacheService.ts:777-793`: `nbformat: 4`,
  `nbformat_minor: 5`, notebook metadata `quarto_source` (the document URI),
  `quarto_cache_version = CACHE_VERSION` (`:35`, value 1) and `last_updated` (a
  timestamp, so the bytes change on every write); per cell `metadata.quarto_cell_id`,
  `quarto_content_hash` (`:777-778`); `source` as a line array with a trailing
  newline on every line but the last (`:782`). The cell type (`IpynbCell`, `:40-50`)
  has no top-level `id`, which nbformat 5.11.1 accepts at minor 5 with a warning
  (verified, below). The file path is a hash of the document URI
  (`:208-212`) in per-user global storage, which q2 cannot reproduce.
- Outputs (verified 2026-10-29, reading `:880-990`): stream, error and rich outputs.
  Every rich output, `display_data` included, is written as `execute_result` with all
  mimes in `data`. Every payload is held as a string, so a JSON mime
  (`application/json`, anything `+json`) is written as a string although nbformat
  stores JSON mimes as objects; the reader `JSON.stringify`s any value that is not a
  string. `metadata.quarto_webview_metadata` carries Positron's webview state. Nothing
  was run against a real Positron round trip.
- Reader `:223-227,255-266`: requires `nbformat` and `metadata.quarto_cache_version`
  to be numbers (and a `cells` array: inferred, not read); an invalid structure or
  a version other than 1 **deletes the file**. Hence the two-version-key rule below.
- Content hash `browser/quartoDocumentModel.ts:22`: `StringSHA1` over the block
  content, hex digest truncated to 16 characters. The block content is the lines
  strictly between the fences joined with `\n` (`common/quartoParser.ts:173`), from
  a split on `\r?\n` (`:113`), so `#|` option lines are included. Line endings are
  normalized by that split and rejoin: a CRLF document and its LF twin hash alike in
  Positron, so q2\'s `quarto_content_hash` must split on `\r?\n` and rejoin with `\n`
  to match (the rule; a Phase 2 test). `StringSHA1` encodes UTF-8 (verified 2026-10-30,
  `external-sources/positron/src/vs/base/common/hash.ts:220-233`): a surrogate pair
  combines to one code point and a lone surrogate becomes U+FFFD (`:190-205`). The
  test vectors in Phase 0 were computed with node `crypto` over UTF-8; running them
  against Positron's own implementation is spike S1 in EXR-16.
- Cell id `quartoDocumentModel.ts:32-40`: `{index}-{first 8 hash chars}-{label or unlabeled}`;
  unstable across edits, used only as a duplicate tiebreak. Positron\'s label is the
  first option inside the fence braces, so `#| label: fig-x` is `unlabeled` there and
  `{python.marimo}` gives the label `.marimo`; its parser matches only column-0,
  exactly-three-backtick fences with a `\w+` language and has no nesting rule
  (`common/quartoParser.ts:~18-30`), so indices differ from q2\'s on documents with
  blockquote, list or four-backtick cells and a `{python}` line inside an unbraced display
  fence is a phantom cell there (decision 39).
- Restore is content-hash-first; stale cells are skipped silently.
- Positron rewrites the file from fixed fields, so extra metadata is dropped on its
  next write. Its records are therefore display-trust only.

**Reusable code.** `stored_result_for_cell` and `mime_bundle` (`engine/jupyter/stored.rs:229,300`)
already read nbformat outputs into the presenter's types.

**Where the hash inputs come from today** (verified 2026-10-29).
`compute_input_qmd` (`crates/quarto-core/src/engine/preview_record.rs:209-250`) runs the
pre-engine pipeline (includes expanded, project and document metadata merged) and then
the nested-cell mask. Preview staleness is a byte compare of those bytes
(`quarto-preview/src/capture_driver.rs:248-283`); the preview cache key is their SHA-256
(`quarto-preview/src/cache.rs:48`); and `provide-hub -M` rewrites the recorded copy
overlay-free (`quarto-hub-provider/src/execute.rs:340-352`, F8-D9).

**The nbformat validator** (verified by a reviewer, nbformat 5.11.1 with Python
`nbformat.validate`; Phase 0 turns each into a test): a minor-5 cell with `id`
validates; without `id` it validates with `MissingIDFieldWarning` ("will become a hard
error"); a minor-4 notebook with `id` fails; a 64-character id is valid and 65 fails;
`4390f67858f3f1c7-2` is valid; duplicate ids validate with a `DuplicateCellId` warning
and get renamed (so the `-2`, `-3` suffixes matter); the custom mime
`text/x-quarto-markdown` and extra `quarto_*` metadata validate; an `execute_result`
without `execution_count` **fails**, so the builder must always emit it. Decision 25
is correct.

**Which keys engines read** (verified by a reviewer; Phase 0 records it as facts with
tests). Jupyter (Rust) reads only `error` (`engine/jupyter/text_execute.rs:392`; the stored-replay path reads it at `stored.rs:199`) and the
visibility keys `include`, `echo`, `output`, `warning` from `ctx.execute_scope`
(`presenter/visibility.rs`, `CellVisibility::resolve`, called at `text_execute.rs:393`; EXR-1 moved it there); it reads no `eval`, `execute-dir` or `params`. knitr reads `fig-width`,
`fig-height`, `fig-asp`, `fig-dpi`, `fig-format`, `df-print`, `eval`, `warning`,
`error`, `include`, `enabled`, `debug`, `echo`, `output` and `cache`
(`engine/knitr/format.rs:343-385`) plus `ctx.format` (`engine/knitr/mod.rs:326`). The TS
engines (Julia, marimo, extensions) were not audited (UNVERIFIED; EXR-8 and EXR-9 own
their declarations). `execute-dir` and `params` over-invalidate harmlessly, because no
Rust engine reads them from metadata, and `fig-width` and `fig-height` over-invalidate
Jupyter cells harmlessly; both are accepted.

**Not verified:** how `preserve` placeholders merge across engines per cell (the
design below avoids the question by never merging); what `RenderOverlay` does to
project-level versus document-level metadata, which decides which overlay keys count as
execution-relevant (EXR-15 checks it first); that Positron's in-memory JSON-mime payload
is JSON text (inferred from its types); whether `body_source_for`
(`crossref/codeblock_shorthand.rs:669-697`) falls back to the whole block after a
mid-body strip (reviewer-reported; the first task of Phase 0); and that q1's `freezeInputHash` offers no field list to
copy (it is an md5 of the whole LF-normalized input file,
`external-sources/quarto-cli/src/command/render/freeze.ts:312-316`, verified 2026-10-30).

## Design decisions (from the epic and the 2026-10-29 review; do not re-litigate)

- Location in code: a target-neutral module `crates/quarto-core/src/record/`
  (`quarto-hub` does not depend on `quarto-core`, so the hub only ever sees bytes).
  Extract to a leaf crate only if a second consumer needs it.
- Deterministic serialization; no volatile fields; no absolute paths.
- Two systems, two jobs. Positron's `quarto_content_hash` is for **matching**. The q2
  execution-input hash is what **record-level staleness** consumes (EXR-15: for records it
  replaces the whole-input byte compare; legacy payloads keep the byte compare) and is the
  freeze groundwork. It is never used for matching, and nothing in this epic skips
  execution because of it (the preview cache stays keyed by the SHA-256 of the input
  bytes until freeze; EXR-15).
- The hash is computed from the **units and merged metadata of the masked pre-engine
  pipeline output** (includes expanded, project and document metadata merged), never from
  raw file bytes. A prose-only edit therefore leaves it unchanged; a code edit, an
  included file's code edit, or an execution-relevant metadata change moves it.
- **The rule is the engine's declaration; core owns the algorithm** (epic decision 35).
  Core hashing code names no engine. Each engine declares a `HashDefinition` as data
  (below); an engine that declares nothing gets empty exclusion lists, so everything
  counts (over-invalidates, never shows stale output). A rule that cannot be declared as
  data is a finding to report, not code to add in core.
- **Matching** (epic decision 34): `quarto_exec_hash` first, `quarto_content_hash` as
  the fallback and the only key for Positron records. The record stores both per cell;
  EXR-14 implements the matching.
  **Duplicates** (2026-10-31 review): when several document units carry the same hash H,
  the k-th document occurrence of H takes the k-th record cell with hash H, in record
  order; the index inside an id is never consulted.
- **Overlay.** The hash is overlay-free: it is computed on the overlay-free source
  project, as F8-D9 does for `input_qmd` today, so a viewer with no `-M` overlay sees a
  record produced under one as fresh. The record also stores the overlay's
  execution-relevant keys as informational provenance (`quarto_overlay`, below), never
  used for invalidation. Known cost: a record produced under a materially different
  overlay (for example `params`) looks fresh.
- Provenance is recorded and never used for invalidation.
- Figures inline as base64; supporting files in a notebook-level map; a sidecar
  field reserved, not emitted.
- One file per document; rendered units carry their output format.
- **Language tokens** follow EXR-2: `[A-Za-z][A-Za-z0-9_.-]*`, compared case-insensitively,
  authority is pampa. `python.marimo` (no space) is one language token (Positron reads
  `python`); `{python .marimo}` is language `python` plus first class `marimo`.
  `quarto_lang` holds the full token. The language is in neither the content hash nor
  the cell id.
- **Label handling** (epic decision 36). `quarto_content_hash` hashes the author's source
  text between the fences, including an authored `#| label:` line and caption lines, to
  match Positron. Crossref sugaring strips the label and `<reftype>-cap` lines from the
  engine-visible CodeBlock text and wraps the cell in a Div
  (`crossref/codeblock_shorthand.rs:182-237`, `strip_consumed_lines` at `:237`), so the
  record builder cannot hash the text it is given. The engine-execution stage obtains each
  unit\'s author text from the original text range (or the pre-sugaring AST; choosing the
  mechanism is the first task of Phase 0) and builds one value, the **author-text
  mapping**, which has a single type: `AuthorTexts`, a `Vec<AuthorText>` aligned
  index-for-index with the stage\'s unit list, so it is total (one entry for every unit,
  never a gap). `AuthorText { text, content_hash }`: `text` is the lines strictly between
  the fences, split on `\r?\n` and rejoined with `\n`, with the `#|` lines (label and
  caption lines included) exactly as the author wrote them; for a span it is the
  expression; for a unit that crossref sugaring did not touch it equals the scanned text.
  The stage computes `content_hash` (the Positron hash of `text`) and uses it for the id.
  The unit\'s exec hash is computed from the same `text`, partitioned into options and
  canonical code, by `hash_unit` under the owning engine\'s `HashDefinition`; the
  definition differs per engine, so the exec hash cannot be fixed in the stage before the
  engines are known. The id\'s label segment is the value of the first `#| label:` line in
  `text`, or `unlabeled` (Positron reads the first option inside the fence braces instead,
  so the segments differ for labeled cells, which the epic already accepts because
  matching uses the hash). `source_info` is kept (`:247`, `:268`), but
  `body_source_for` (`:669-697`) locates the body with `find(&cb.text)` and may fall
  back to the whole block after a mid-body strip, and `label_reinject::inject` re-adds
  only the label line, only for knitr (`engine_execution.rs:526`), so neither is the
  mechanism. A non-crossref label stays in the text. `quarto_exec_hash` uses the
  canonical text plus the options.
- **Ids** (epic decisions 13, 38 and 39). Cell ids use Positron's format,
  `{index}-{hash8}-{label|unlabeled}`, with the same content hash; the label segment and
  the index can differ from Positron's, and matching uses the hash. Span ids use their own
  counter over all spans, owned or not, `{n}-{hash8}-inline`, so cell indices are the
  same whatever spans surround them; matching never uses `n` (EXR-14 keys spans by
  `(lang, code, occurrence)`). The cell index counts every braced non-raw fence in
  document order, owned or not (EXR-2 returns every braced cell, and each engine\'s view
  marks the ones it owns; raw `{=format}` blocks are excluded, as in Positron\'s
  `RAW_START_REGEX`). The span counter runs over the spans the scanner claims (epic
  decision 41: prose and the document\'s own front matter; never any fence or HTML comment), in document order, so a span in the document\'s own front
  matter, which comes first, takes the lowest numbers, while a span that arrives through
  project or `_metadata.yml` metadata is not claimed and takes none. A front-matter span
  is recorded like any other span, with `quarto_location: metadata`, and is never
  spliceable (EXR-14). The stage scans
  units once, on the text entering the engine stage, and fills each unit's typed id slot
  (EXR-2's `Unit.id`) with this plan's id function; every engine then receives the same
  list and records carry the document-wide index, so coalescing needs no renumbering.
  Where q2 and Positron differ (nesting, the label rule, phantom cells), EXR-16 S1
  records the divergence as a finding.
- **Why q2's record is its own file and not Positron's cache.** (1) Positron's writer
  rebuilds the file from fixed fields, so q2 metadata would vanish on its next write.
  (2) Positron deletes a cache file with a missing or different `quarto_cache_version`
  (`:255-266`), so q2 could not evolve a shared schema. (3) Its path is a SHA-1 of the VS
  Code URI in per-user storage; q2 wants a project-local path that copies to `_freeze/`.
  (4) The content differs: includes, filters, post-process flags, HTML dependencies,
  supporting files, per-engine results and inline units, and knitr and marimo store
  rendered markdown, which Positron has no concept of. (5) Determinism: Positron writes
  `last_updated`. (6) Positron records have no provenance or execution-input hash, so
  they are display-trust only.
- **Execution options are flat cell metadata keys** (decided with Gordon, 2026-10-29).
  A record cell\'s metadata is deliberately isomorphic to Quarto executable-block
  metadata, so `fig-width: 7` in a `#|` line is `"fig-width": 7` on the cell, not an
  entry in a `quarto_options` map. Nothing outside q2 reads these keys from the file:
  Positron\'s cache holds only id, hash and label, and Ark takes `fig-width` and
  `fig-height` from the execute request
  (`external-sources/ark/crates/amalthea/src/wire/execute_request.rs:49-54`); the file
  serves q2 itself (freeze, single-cell re-execution).
- `quarto_cache_version: 1` stays in the record only while the schema subset is really
  Positron-compatible; keep it for now. If the schemas diverge, drop it rather than bump.
- The Positron importer normalizes at read, so
  records only ever hold standard nbformat (EXR-16 specifies the details).

## Schema reference (normative for this epic)

Notebook: `nbformat: 4`, `nbformat_minor: 5`, `metadata`, `cells`.

Notebook `metadata` (flat keys, all optional unless noted):

- `quarto_record_version` (required): q2's schema version, starts at 1.
- `quarto_cache_version: 1`: emitted **only while the schema subset really is
  Positron-compatible**; any incompatible change drops it rather than bumping it.
  **The two version keys, in one place.** `quarto_record_version` is q2's schema version and
  the only one q2 reads and gates on. `quarto_cache_version` is *defensive only*: it has no
  consumer in q2's own flow, because q2 never writes into Positron's cache directory. It is
  emitted (always `1`, from `Record::CACHE_VERSION`) so that a record dropped there by hand
  passes Positron's loader, which deletes a file whose value is missing or other than 1 (and
  then consumes a valid one; see EXR-16 Background). If Positron bumps its version, q2 drops
  the key instead of following. Tests: `positron_conformance::q2_only_ever_emits_cache_version_one`
  and `a_q2_record_passes_positrons_loader_checks_only_with_cache_version_one`.
- `quarto_source`: project-relative document path, forward slashes.
- `quarto_engines`: ordered list, one entry per engine that ran:
  `name`, `provenance` (engine version, kernelspec name and language, executor
  version, platform; **informational only**), `can_freeze`, `hash_definition` (the
  engine's full `HashDefinition` declaration, not only `{id, version}`, so a consumer
  with no engine loaded can recompute and compare), `document_hash` (that engine's
  document-level execution-input hash, computed with its own definition over its
  document-level inputs and the ordered hashes of its units), `owned_languages` (the
  languages the engine owned for this document, so a consumer with no engine loaded can
  tell which units belong to it; added in review, not an epic decision), and its folded
  result:
  `includes`, `filters`, `preserve`, `needs_postprocess`, `html_dependencies`,
  `pandoc`, `metadata`, `resource_files`, `warnings`. Each engine's result is kept
  separate and applied in order at consumption (as the stage does today), so no
  cross-engine merge is needed.
- `quarto_files`: map from doc-relative forward-slash path to `{data, size}`
  (`data` base64): the supporting files that today ride in `CaptureFile`s.
- `quarto_execution_hash`: a hash over the ordered per-engine `document_hash` values of
  `quarto_engines` (each of which covers that engine's document-level inputs plus its
  ordered unit hashes, below). This is the value record-level staleness compares
  (EXR-15).
- `quarto_overlay`: optional, informational only. The execution-relevant keys an
  overlay (`provide-hub -M`) contributed: the post-merge difference between the overlay
  and overlay-free metadata, restricted to the keys that count under the engines\'
  declarations (not the raw `-M` strings, so the bytes do not depend on how the
  flag was spelled). Absent when no overlay was applied. It never enters any hash and
  is never used for invalidation.

Cell (`cell_type: code`, `execution_count: null`, `source` as a line array, `outputs`):

- Positron keys: `quarto_cell_id`, `quarto_content_hash`, optional `quarto_label`.
- `source` is the author text as a line array: the lines between the fences with the `#|`
  option lines (label and caption lines included) exactly as the author wrote them, which
  is what Positron stores and what `quarto_content_hash` covers (decided in the
  2026-10-31 review; EXR-16\'s importer reads the same lines). For an inline unit it is the
  expression.
- q2 keys: `quarto_kind` (`cell` or `inline`), `quarto_lang` (the full language token as
  written, for example `python.marimo`; every comparison of it is case-insensitive),
  `quarto_engine`, `quarto_kernel` (per cell, so mixed kernels never need a notebook-level
  one; provenance only, in no hash), optional `quarto_location` (`metadata` for a span in
  the document\'s own front matter, absent for every body unit), `quarto_exec_hash` (the
  unit\'s execution-input hash), optional `quarto_hash_rule` (the name of the hash profile
  the unit was hashed under; absent means the default), and the execution-relevant
  options as **flat keys under their Quarto names** (`fig-width`, `fig-height`, and any
  other execution-relevant option; there is no options map). The set is the cell's
  options minus the `presentation_cell_options` of the profile the unit names in
  `quarto_hash_rule` (the default profile when it names none), so it can never disagree
  with the hash.
- Every `execute_result` output carries `execution_count` (null when unknown): the
  nbformat validator rejects one without it.
- Inline units are code cells with `quarto_kind: inline`, the expression as
  `source` and one value output. Their `quarto_content_hash` is domain-separated
  (hash input `inline:<lang>\n<code>`, with `<lang>` lowercased, so `{Python}` and
  `{python}` hash alike) so Positron can never match a span to a real cell with identical
  text; their `quarto_cell_id` is `{n}-{hash8}-inline`, where `n` counts all spans the
  scanner claims, owned or not (cells keep their own index; see the design decisions).
- Top-level cell `id` (decided, epic decision 25): emitted at nbformat minor 5, a
  stable hash-derived id (the 16-hex content hash, with `-2`, `-3` appended to later
  duplicates; 1 to 64 characters of letters, digits, `-`, `_`), never the
  index-bearing `quarto_cell_id`. Positron writes minor 5 without it and ignores the
  field when it reads. The Phase 0 `nbformat.validate` test pins the schema (verified: valid with `id`, a warning without).

### Differences from Positron's cache

| Area | Positron | q2 record |
|---|---|---|
| Location | VS Code global storage, `<sha1(URI)>.ipynb` | `.quarto/execution-records/<doc path>.ipynb`, project-relative |
| Notebook metadata | `quarto_source` (URI), `quarto_cache_version`, `last_updated` | `quarto_record_version`, `quarto_cache_version` (while compatible), `quarto_source` (project-relative path), `quarto_engines`, `quarto_files`, `quarto_execution_hash`, `quarto_overlay`; no `last_updated` |
| Cell metadata | id, content hash, label | the same three, plus `quarto_kind`, `quarto_lang`, `quarto_engine`, `quarto_kernel`, `quarto_exec_hash`, `quarto_location` (front-matter spans only), options |
| Units | fenced cells only | also inline expression spans |
| Outputs | stream, error, every rich output as `execute_result`, JSON mimes as strings | standard nbformat, `display_data` kept, JSON mimes as objects; knitr and marimo store one `display_data` with `text/x-quarto-markdown` |
| Supporting files | none | notebook-level `quarto_files` map |
| Bytes | `last_updated` changes every write | deterministic |
| Writer | Positron rebuilds the file from fixed fields | q2 owns the file |

Outputs: standard nbformat (`stream`, `execute_result`, `display_data`, `error`).
knitr and marimo units store one `display_data` output with mime
`text/x-quarto-markdown` (finished markdown) and metadata `quarto_format` (the
output format it was produced for). Image data inline as base64 strings.

Figures: image bytes are stored inline as base64 strings in the outputs, so a record is one
self-contained file. `quarto_files_ref` is reserved for a future sidecar: the reader parses and
ignores it and the writer never emits it. Moving figures out of the file would be a schema change
(a `quarto_record_version` bump), not an extension.

Size decision (EXR-15 Phase D, bd-sr73, 2026-11-01): figures stay inline. Measured on a real
kernel (the `size-measurement.qmd` fixture, one matplotlib PNG and a three-row data frame):
the legacy capture gzips to 24,417 bytes and the record to 23,099 (record JSON 32,381 bytes
against 41,434 for the capture, because the record omits `input_qmd`). The figure is about 89%
of the gzipped payload (29,072 base64 bytes of PNG, estimated from the EXR-4 Phase 0 table).
Stored as a hub automerge document the record takes 23,522 bytes (the binary doc adds about
420 bytes of metadata), and the filesystem store holds exactly that much per document with no
extra overhead. Roughly 23 KB per figure of this kind puts the 10 MB warning at about 430
figures in one document. Sidecar threshold, if one is ever needed: a record whose gzipped
payload passes 1 MB (about 40 figures), where one execution already costs more than the
orphan budget below; nothing in the fixtures comes near it, so no sidecar design is scheduled.

Serialization: struct-ordered fields, `BTreeMap` for maps, one-space indent with
sorted keys and a trailing newline (the nbformat writer convention), LF endings,
no `last_updated`.

## Execution-input hash definitions (definition version 1)

Core owns the algorithm; each engine declares its rule as data (epic decision 35).
The declaration type, defined here in the record library:

```text
HashDefinition {
  id: String,                           // "jupyter", "knitr", "julia", "marimo", ...
  version: u32,
  granularity: Unit | Document,         // defined here; EXR-7 reuses it
  presentation_cell_options: Vec<String>,   // the default profile: cell options excluded from the unit exec hash
  presentation_execute_keys: Vec<String>,   // the default profile: `execute:` keys excluded from the document hash
  document_keys: Vec<String>,           // extra top-level metadata keys that count
  profiles: Map<String, HashProfile>,   // optional named alternatives to the default profile
}

HashProfile {
  presentation_cell_options: Vec<String>,
  presentation_execute_keys: Vec<String>,
}
```

`HashDefinition` and `HashProfile` serialize with camelCase field names
(`presentationCellOptions`, `presentationExecuteKeys`, `documentKeys`) and `Granularity`
as the lowercase strings `unit` and `document`, in the notebook\'s
`quarto_engines[].hash_definition` and on the engine wire alike. This plan defines
`Granularity`; EXR-7 reuses it.

A unit may name a profile in its cell metadata, `quarto_hash_rule` (absent means the
default profile, the top-level lists). This lets one engine's record hold units with
different rules (the knitr follow-on epic: structured units leave presentation options
out of the hash, fallback units holding baked markdown count every option). Engines in
this epic declare no profiles. A unit naming a profile that the stored declaration lacks
cannot be recomputed, so it reads as stale.

**The algorithm (core, engine-agnostic).** SHA-256 over length-prefixed fields (each field
is its UTF-8 bytes preceded by their length as an 8-byte big-endian integer), line endings
normalized to LF (as Q1\'s `freezeInputHash` does), the first two fields being the
declaration\'s `id` and `version` (decimal) so a change never silently matches; the digest
is stored as 64 lowercase hex characters. An option value is encoded as canonical JSON of
the parsed YAML value, exactly as q2\'s YAML parser yields it (compact, object keys
sorted), so `7`, `7.0` and `"7"` hash differently; an options set is one canonical JSON
object. Inputs are read from the units and merged metadata of the masked pre-engine
pipeline output and are overlay-free (design decisions above).

- **Unit hash** (`quarto_exec_hash`), fields in order: `id`, `version`, the tag `unit`,
  kind (`cell` or `inline`), language (lowercased), the code after option partition, and
  the cell\'s options minus the `presentation_cell_options` of the unit\'s profile (the
  default profile unless `quarto_hash_rule` names another), as one canonical JSON object.
  The kernel is not an input (2026-10-31 review): it is chosen at execution time, which a
  viewer with no engine loaded cannot reproduce, so `quarto_kernel` is provenance only.
  Golden vectors, pinned by a Phase 2 test (a change to the encoding bumps the
  definition\'s `version`): `id` `jupyter`, `version` 1, kind `cell`, language `python`,
  code `x = 1`, options `{"fig-width":7}` gives
  `d8cae62dd1cc0cf4301084561ca4bee22fd98c2ab1315ad53ff149043616f07a`; kind `inline`,
  language `python`, code `1 + 1`, options `{}` gives
  `7c77518b4cf801cf8cf3cfbaf1024e285b8e28366efd0c4043672b11c7080deb`.
- **Document hash**, one per engine, computed with that engine\'s own declaration over: the
  merged `engine` key, `execute-dir`, `params`, every top-level key in `document_keys`,
  the `execute:` map minus the `presentation_execute_keys` common to every profile that a
  unit of that engine\'s record uses (the strictest rule; the default profile when the
  record has no units), and the ordered unit hashes of that engine\'s units, for every
  engine whatever its granularity (2026-10-31 review: a code edit must move the document
  hash, and EXR-15\'s staleness test depends on it). `granularity: Document` differs only
  in that the engine\'s units are not individually matchable: its cells are judged
  together, by this hash. A unit belongs to the engine whose stored `owned_languages`
  contains its language, so a consumer with no engine loaded can group the units. Unknown
  keys count, so the exclusion lists are the only way a key stops counting. Project-level
  `execute:` is covered because the hash reads merged metadata. Core always excludes
  `record` (the opt-in of epic decision 24) at document level, for every engine, so
  toggling it never changes any hash.
- **`quarto_execution_hash`** is a hash over the ordered per-engine document hashes. The
  record stores each engine's full declaration and document hash in its
  `quarto_engines` entry, so a consumer with no engine loaded (WASM, hub) can recompute
  and compare.
- **Default.** An engine that declares nothing gets empty exclusion lists and no extra
  `document_keys`: everything counts.

**Declarations.**

- **Jupyter** (Rust engine, `ExecutionEngine::hash_definition` override in the Jupyter
  engine module): `granularity: Unit`; `presentation_cell_options` is the closed
  allowlist of display-only options (`echo`, `output`, `warning`, `include`, the code-fold
  options, captions and layout display options), and the list lives in the Jupyter module,
  not in core; `presentation_execute_keys` the same display-only keys for the `execute:`
  map; `document_keys: ["jupyter"]`.
- **knitr** (override in the knitr engine module): `granularity: Unit`, EMPTY exclusion
  lists, because presentation is baked into its rendered markdown, so every chunk option
  counts; `document_keys: ["knitr"]`. This plan owns the override and its tests; EXR-11 cites it and adds only
  `knitr_unit_hash_covers_all_options` (the resolved-options digest stays provenance). This plan also pins, in the knitr override test, `knitr_hash_ignores_the_record_key` (toggling the `record` key does not change the hash, a case EXR-11 dropped).
- **Julia** and **marimo** (TS engines, supplied through `recordInfo.hashDefinition` in
  EXR-7's wire, by EXR-8 and EXR-9 in the forks). UNVERIFIED whether a flat declaration is
  enough. Julia follows the runner's own source hash (code cells, markdown containing
  inline code, execution-relevant frontmatter, owned languages;
  `external-sources/QuartoNotebookRunner.jl/src/parsing.jl:170-266`). marimo is reactive
  across the document, so it uses `granularity: Document`, with a hash over all owned
  cells plus `eval`, `external-env`, `pyproject`, the bare-sql flag and the mode
  (`external-sources/quarto-marimo/src/engine/index.ts:159-175`). The marimo
  declaration is the single source and EXR-9 must say the same thing as this plan. A rule
  that cannot be declared as data is a finding to report, not code to add.

**Accepted over-invalidation and risks** (these stay as stated). `execute-dir` and
`params` count for every engine although no Rust engine reads them from metadata;
`fig-width` and `fig-height` count for Jupyter cells. Both over-invalidate harmlessly.
Outside the hash, by design: `.env` and `project_env`, data files, and format-specific
defaults such as docx and pptx figure sizes merged after `ast.meta`
(`engine_execution.rs:965-1006`).

## Out of scope

- Any engine or pipeline change beyond the `hash_definition` trait method, its Jupyter and
  knitr overrides and the id function; the hub transport (EXR-13); matching (EXR-14) and the
  record-level staleness policy that consumes `quarto_execution_hash` (EXR-15); freeze
  behavior; writing records anywhere.

## Phased checklist

Every phase exit gate is exactly `cargo clippy -p quarto-core --all-targets -- -D warnings`
plus `cargo nextest run -p quarto-core` (Phase 6 adds the WASM compile).

### Phase 0: design checks and spikes

- [x] FIRST: design the label-line hash mechanism (epic decision 36). Read
  `crossref/codeblock_shorthand.rs:182-237` and `:669-697`: confirm or refute that
  `body_source_for` falls back to the whole block after a mid-body strip (a test with a
  `fig-cap` line in the middle of the body). Choose where the stage obtains the author's
  original text range per unit (the original text range, or the pre-sugaring AST),
  write the choice and its reasons into this plan (the mapping\'s type, `AuthorTexts`, is
  fixed in the design decisions, so this task only chooses how the stage fills it). Tests first: a labeled
  `fig-` cell with a `fig-cap` line gives the author-text hash of the raw lines, and a
  non-crossref label stays in the text.
  **Findings (verified 2026-10-31).**
  (1) The `body_source_for` fallback is real for the sugared block, not inside the sugaring
  pass: `try_desugar_code_block` calls it on the unstripped block (`:142`), but the stripped
  block keeps the original `source_info` and its text is no longer contiguous once a line is
  removed from between two kept options, so the span becomes the whole block. Stripping only
  leading lines leaves a contiguous tail and the span stays correct. Pinned by
  `body_source_for_falls_back_to_whole_block_after_mid_body_strip`.
  (2) The serializer's source map cannot recover the author text. The stage scans
  `serialize_ast_to_qmd` output of the already-sugared AST (`pre_engine_sugaring` runs first),
  and mapping a unit's offsets back through the returned `SourceInfo` gave offsets that are
  not the block's (serialized 24, 36 and 78 mapped to original 25, 37 and 79 while the block
  starts at 7, inside the label line). The map tracks the wrapper, not positions inside
  rewritten text, so neither the original range nor the source map is a mechanism.
  **Chosen mechanism: a pre-sugaring snapshot.** `pre_engine_sugaring` records, before
  `desugar_blocks` rewrites a block, the pair (author text, sugared text) for every block the
  pass rewrites, in document order, on the stage context. The engine stage fills `AuthorTexts`
  by walking the scanned units with a monotonic cursor over that list: a cell unit takes the
  next recorded author text whose sugared text equals the unit's code (container prefixes
  removed); a unit with no match is untouched by sugaring, so its author text is its scanned
  text, as the design decisions state. The cursor makes duplicate identical cells align by
  order. Open check for the implementation task: a sugared cell inside a container, and a
  second engine iteration that re-serializes (the stage scans once, so the snapshot is
  consumed once).
  **Scope (Gordon, 2026-10-31): EXR-5 owns the pipeline change; EXR-4 owns the pure part.**
  `pre_engine_sugaring` (EXR-5) records one `SugaredBlock { author_text, sugared_text }` for
  **every** braced executable block, in document order, not only the rewritten ones: if the
  snapshot listed only rewritten blocks, an untouched cell whose text equals a rewritten cell's
  stripped text would take that cell's entry. EXR-4 ships `SugaredBlock` and
  `author_texts(units, snapshot) -> AuthorTexts` (`record/author_text.rs`), pure and tested against
  the real sugaring pass. EXR-5 must call it and must record untouched blocks too.
  Done (2026-10-31): the mechanism, its tests (`record/author_text.rs`: a sugared cell hashes the
  author's raw lines with label and caption included, checked against the real sugaring pass; a
  non-crossref label stays in the text; identical untouched and sugared cells align by order) and the
  Phase 2 label tests (with `exec_hash.rs`: moving the label line changes the content hash and not the
  exec hash). The contract is also written into EXR-5's plan, Phase 2, "Unit ids and author texts in
  the stage".
- [x] Record the audit of which metadata keys engines read as facts with tests: Jupyter
  reads only `error` (`text_execute.rs:392`) and the visibility keys `include`, `echo`,
  `output`, `warning` from `ctx.execute_scope` (`presenter/visibility.rs`, `CellVisibility::resolve`, called at `text_execute.rs:393`), and no `eval`, `execute-dir`
  or `params`; knitr reads `fig-width`, `fig-height`, `fig-asp`, `fig-dpi`, `fig-format`,
  `df-print`, `eval`, `warning`, `error`, `include`, `enabled`, `debug`, `echo`,
  `output`, `cache` (`knitr/format.rs:343-385`) plus `ctx.format` (`knitr/mod.rs:326`).
  Compare with the declarations in the hash section and name the accepted
  over-invalidations (`execute-dir`, `params` for all engines; `fig-width` and
  `fig-height` for Jupyter). The TS engines are not audited (UNVERIFIED): leave that as a
  task in EXR-8 and EXR-9.
  Done (2026-10-31; the line references above were refreshed, they had drifted after EXR-1 moved
  the visibility code). Pinned by `exr4_audit_jupyter_visibility_ignores_every_other_key`
  (`presenter/visibility.rs`: the four visibility keys move `CellVisibility`, sixteen others do not)
  and `exr4_audit_knitr_reads_exactly_these_execute_keys` (`knitr/format.rs`: the fifteen keys
  change the config, seven others do not). `error` for Jupyter is read at `text_execute.rs:392`
  and has no kernel-free test here; its behavior (cell and document level) is covered by
  `tests/integration/engine_error_policy.rs`.
- [x] Tests first, then a script, for the nbformat validator facts (Python
  `nbformat.validate`, nbformat 5.11.1): a minor-5 cell with `id` validates; without `id`
  it validates with `MissingIDFieldWarning`; a minor-4 notebook with `id` fails; a
  64-character id is valid and 65 fails; `4390f67858f3f1c7-2` is valid; duplicate ids
  validate with `DuplicateCellId` and get renamed; `text/x-quarto-markdown` and extra
  `quarto_*` metadata validate; an `execute_result` without `execution_count` fails.
  Check in a sample record that passes with no warning.
  Done (2026-10-31): `uv run scripts/exr-record/nbformat_facts.py` (a PEP 723 script, so `uv`
  builds its own cached environment; no project venv) checks every fact above plus the sample
  `crates/quarto-core/tests/fixtures/records/sample.ipynb`; with file arguments it validates
  them and fails on any error or warning (the Phase 4 golden gate).
- [x] Positron test vectors: check in the `quarto_content_hash` vectors (SHA-1 over
  UTF-8, first 16 hex characters, computed with node `crypto`): `x <- 1` gives
  `4390f67858f3f1c7`; `café` gives `f424452a9673918c`; `😀` gives `9c533688a979a858`;
  `#| label: fig-é\nx <- 'é😀'` gives `774a7e618b133ed6` (all four recomputed and
  agreeing with Positron's hash on 2026-10-31). Add an empty cell and a lone-surrogate
  vector (Positron maps a lone surrogate to U+FFFD, `hash.ts:190-205`, so the expected
  value is the hash of the replacement character). Add the CRLF vector: a CRLF input
  hashes the same as its LF twin, because Positron splits on `\r?\n` and rejoins with `\n`
  (`quartoParser.ts:113,173`). Running the same vectors against Positron's own
  `StringSHA1` is spike S1 in EXR-16, and the two results must agree.
  Done (2026-10-31): `crates/quarto-core/tests/fixtures/records/content-hash-vectors.json` holds eight
  vectors (the four above, empty cell, lone surrogate stored as U+FFFD because a Rust string cannot
  hold one, CRLF, trailing newline); `node scripts/exr-record/content_hash_vectors.mjs` recomputes and
  verifies them (`--write` regenerates). They come from node `crypto`, not from Positron's own
  `StringSHA1`; Phase 2's Rust `content_hash` reads this fixture.
- [x] bd-sr73 size measurement: record a real Jupyter document (one matplotlib
  figure, one small table); report gzipped size of today's capture versus an ipynb with
  inline base64; write the table into this plan. Decision thresholds: stay inline below
  the 10 MB warning for the fixture; else Phase 5 designs the sidecar.
  **Result (2026-10-31): stay inline; Phase 5 needs no sidecar design.** Fixture
  `crates/quarto-core/tests/fixtures/records/size-measurement.qmd` (one matplotlib line plot, one
  three-row pandas table), rendered with `q2 0.36.0-nightly.20261027` and `trace: true` against a
  throwaway `uv` environment (ipykernel, matplotlib, pandas; the render needs a `jupyter` on PATH
  and `JUPYTER_PATH` pointing at the kernelspec). `uv run scripts/exr-record/size_measure.py DOC
  TRACE FIGURES_DIR` rebuilds today's `EngineCapture` (the trace holds no file payloads, so the
  `{path, contents_base64}` files are rebuilt as `collect_capture_files` emits them, from the
  engine's own `doc_files/figure-html/` only, not the render's CSS and JS) and compares it with
  the same cells run through a real kernel into an ipynb:

  | item | bytes |
  |---|---:|
  | figure files in capture (raw PNG) | 21804 |
  | figure files in capture (base64) | 29072 |
  | capture `result.markdown` | 6150 |
  | capture `input_qmd` | 5311 |
  | capture JSON (array, minified) | 41452 |
  | capture gzipped | 24511 |
  | ipynb with inline base64 | 32097 |
  | ipynb gzipped | 22689 |
  | ratio ipynb.gz / capture.gz | 0.93 |

  Both are about 0.2% of the 10 MB warning and a quarter of bd-sr73's 100 KB CI-fixture budget. The
  ipynb is a lower bound for a record: the record adds source lines, hashes and the engine entry
  (a few KB), and a PNG compresses only by its base64 overhead, so the figure count is what scales
  the size (roughly 20 KB gzipped per figure of this kind; 10 MB is about 450 of them). Note
  `input_qmd` is 5.3 KB for a 0.4 KB document because it carries the merged `quarto.language`
  terms; the record does not store it.
- [x] The knitr size case cannot be recorded before EXR-11 exists: use a hand-built
  record instead (a rendered-markdown `display_data` cell with a few figure files
  in `quarto_files`, sized like a typical knitr document), and note in the table that it
  is hand-built and is re-measured when EXR-11 lands.
  **Result (2026-10-31, HAND-BUILT, not a knitr run; re-measure when EXR-11 lands):**
  `uv run scripts/exr-record/size_knitr_handbuilt.py`. The shape and the sizes are my
  assumptions of a typical knitr document: 8 chunks of rendered markdown (one
  `text/x-quarto-markdown` `display_data` each) and 3 different matplotlib PNGs (identical copies
  would compress unrealistically) in the notebook-level `quarto_files` map as `{data, size}`.

  | item | bytes |
  |---|---:|
  | figure files (raw PNG, 3 figures) | 52723 |
  | rendered markdown, 8 chunks | 3367 |
  | hand-built record | 77710 |
  | hand-built record gzipped | 50312 |
  | comparable capture gzipped | 49978 |

  The record is within 1% of a capture of the same content, so the knitr case also stays inline,
  at about 50 KB gzipped, 0.5% of the 10 MB warning.
- Exit: the gate above.

### Phase 1: types and round trip

- [x] Tests first: serde round trip of every field in the schema reference;
  unknown `quarto_` keys survive a read and write (forward compatibility).
- [x] `Record`, `RecordCell`, `EngineEntry` types in `record/`, plus `AuthorText` and
  `AuthorTexts` (design decisions), `Granularity`, and the builder\'s two types:
  `UnitResult { outputs, execution_count, kernel }` (the nbformat outputs of one unit as
  JSON values, the execution count the kernel reported, the kernel that ran it;
  `KernelExecuteResult`-shaped, so Jupyter converts from it and a TS engine fills it from
  its notebook\'s cells) and `EngineRecord { engine: EngineEntry, cells: Vec<RecordCell> }`
  (one engine\'s cells, in that engine\'s unit order, with its entry).
- [x] Test, then type: `HashDefinition` (fields as in the hash section) round trips through
  serde, including in `EngineEntry.hash_definition` and with a non-empty `profiles` map,
  and a golden JSON pins its field names (camelCase, with `Granularity` as `unit` or `document`; the file and the engine wire use the same form). A
  cell's optional `quarto_hash_rule` round trips and is absent when the unit uses the
  default profile.
  Done (2026-10-31), `crates/quarto-core/src/record/` (`types.rs`, `hash_definition.rs`).
  Implementation notes: `HashDefinition.profiles` is omitted from the JSON when empty and
  defaults on read, matching EXR-7's "optional `profiles` map"; a cell's options and any
  unknown `quarto_` keys share one flattened `extra` map (`CellMetadata::options()` filters out
  the `quarto_` keys), because serde allows a single flatten map per struct; `quarto_engine` is
  optional on read so a record that predates engine tags still loads (Phase 5 derives engines
  from ownership); the engine entry's fold fields are `serde_json::Value`s and `BTreeMap`s, not
  `ExecuteResult`\'s `HashMap`s, so the bytes stay deterministic.
- Exit: the gate above.

### Phase 2: hashes and ids

- [x] Positron `content_hash(lines)`: all Phase 0 vectors pass, including the CRLF
  normalization (split on `\r?\n`, rejoin with `\n`).
- [x] Id functions (tests first): `cell_id(index, hash, label)` gives
  `{index}-{hash8}-{label|unlabeled}` (the label is the first `#| label:` value in the
  author text) and `span_id(n, hash)` gives `{n}-{hash8}-inline`; the typed id slot of
  EXR-2\'s `Unit` is filled by one function, `fill_ids(units, &AuthorTexts)`, which returns
  an error naming both lengths when the mapping is not the same length as the unit list
  (no partly filled slot, no guess).
- [x] Id tests: inserting or removing a span before a cell leaves that cell's id
  unchanged; span ids count every claimed span, owned or not; a span in the document\'s
  own front matter takes span number 0 (the scanner's `span_index` is 0-based; EXR-18 agrees), shifts later spans by one and leaves every cell id
  unchanged, while a span that arrives through project metadata takes no number (not
  claimed, EXR-2); the cell index counts ceded and unowned braced fences but not raw
  `{=html}` blocks; `python.marimo` and `{python .marimo}` get the same id for the same
  text (language is not in the id); `quarto_lang` keeps the full token; `{Python}` and
  `{python}` spans of the same code get the same span hash; an id filled by the stage
  equals one filled again on the same list (idempotent).
  Done (2026-10-31), `record/ids.rs`: `content_hash` (SHA-1 via the `sha1` 0.11 crate that `pampa`
  already uses, so the lockfile gains one edge and no crate), `span_content_hash`, `cell_id`, `span_id`,
  `author_label`, `scanned_author_text` and `AuthorTexts::from_scanned` (for units sugaring did not
  touch), and `fill_ids`. The Rust `content_hash` reproduces every node-computed Phase 0 vector.
  Two readings of the plan to confirm: the label is the first `label:` option in the *leading run of
  option lines* of the author text, in the language's own option-comment syntax (`%%|` for mermaid,
  `//|` for js), not any `#| label:` line anywhere in the text, because a label after code is not an
  option; and span numbers are the scanner's 0-based `span_index`.
- [x] Hash algorithm in core (tests first): `hash_unit(def, unit)` is SHA-256 over
  length-prefixed, LF-normalized fields prefixed by `id` and `version`; equal inputs give
  equal hashes; changing the definition's `version` changes the hash.
- [x] Unit-hash exclusion tests: each field the definition counts changes the hash; an
  option listed in `presentation_cell_options` does not; an engine that declares nothing
  hashes everything (an `echo` change moves the hash).
- [x] Kernel, case and encoding tests: a unit\'s hash does not change when only its
  recorded `quarto_kernel` changes; both golden vectors of the algorithm section pass;
  `7`, `7.0` and `"7"` option values hash differently; option key order does not matter.
- [x] Profile tests, on a synthetic declaration (no engine in this epic declares one): a
  unit tagged with a profile that counts every option changes its hash on an `echo`
  edit while an untagged unit of the same engine does not; a unit naming a profile the
  stored declaration lacks is stale; the engine's document hash drops an `execute:` key
  only when every profile used in its record excludes it; recompute from the stored
  declaration with no engine loaded.
- [x] Document-hash tests: `engine`, `execute-dir`, `params`, each `document_keys` key
  and each `execute:` key not in `presentation_execute_keys` change the hash; an excluded
  key does not; toggling `record` changes no engine's hash; unknown keys count.
  Done (2026-10-31) for the five items above, `record/exec_hash.rs`: `hash_unit`, `hash_document`,
  `hash_record` (`quarto_execution_hash`), `execution_options` (the flat keys a cell records, by the same
  function that excludes them from the hash), `partition_author_text` and `yaml_to_json` (a tagged value
  such as `!expr 1 + 1` keeps its tag in the canonical JSON, because the tag changes its meaning),
  `recompute_cell_hash` (a cell plus its engine's stored declaration, no engine loaded), and
  `canonical_json`. Both golden vectors pass, and were first reproduced independently in Python. The
  tests also pin: moving a label line changes `quarto_content_hash` and not `quarto_exec_hash`; core
  ignores `record` even when an engine lists it in `document_keys`; the unit count and each unit hash
  are document-hash inputs. Still open in this phase and why: the mixed-engine and self-description
  items need the builder's `EngineEntry` (Phase 3) to be end-to-end, and the source-of-inputs, overlay
  and execution-relevant-option items need the pipeline-side inputs; the primitives they use are
  tested here.
- [x] `ExecutionEngine::hash_definition(&self) -> HashDefinition` on the trait with the
  conservative default (empty lists, `granularity: Unit`, `id` the engine name, version
  1). Test that a mock engine that overrides nothing hashes everything.
- [x] Jupyter override in the Jupyter engine module: the closed allowlist data lives
  there, and core has no engine-name match. Tests: toggling `execute: echo` does not
  change the Jupyter document hash; toggling `error` does; a cell with `fig-width: 7`
  and `echo: false` changes the unit hash only by `fig-width`.
- [x] knitr override in the knitr engine module: empty exclusion lists,
  `document_keys: ["knitr"]`. Tests: toggling `execute: echo` changes the knitr
  document hash and not the Jupyter one; a chunk option change moves the unit hash.
  Done (2026-10-31): the trait default is in `engine/traits.rs`; Jupyter's closed allowlist is
  `DISPLAY_ONLY_KEYS` in `engine/jupyter/mod.rs` (the four visibility keys, the `code-*` display
  options, captions, listing and layout keys; `error`, `eval`, `label`, `execute-dir`, `params`,
  `fig-width` and `fig-height` are deliberately absent and a test pins that); knitr's override is
  in `engine/knitr/mod.rs`, including `knitr_hash_ignores_the_record_key`. The allowlist contents
  are my reading of which options only change presentation: it is the one piece of this phase a
  reviewer should check, because a key wrongly listed there under-invalidates silently.
- [x] Mixed-engine document hash (tests first): each engine's document hash uses its own
  definition and is stored in its `quarto_engines` entry; `quarto_execution_hash` equals the
  hash over the ordered per-engine values and changes when the engine order changes.
  Done (2026-10-31), `record/coalesce.rs` tests `each_engines_document_hash_uses_its_own_definition`
  and `engine_order_is_execution_order_and_changes_the_record_hash`.
- [x] Self-description test: a record's stored declarations reproduce its
  `quarto_execution_hash` and its unit hashes with no engine loaded.
  Done (2026-10-31): `a_record_reproduces_its_hashes_from_its_stored_declarations_alone`, over a
  Jupyter plus knitr record.
- [x] Execution-relevant option keys (tests first): a cell with `fig-width: 7` and
  `echo: false` records `fig-width` as a flat key and no `echo` key under Jupyter; the
  recorded keys equal the options that move `quarto_exec_hash`; keys are emitted sorted
  by name and the bytes are identical across runs (a shuffled option order in the source
  gives the same record); a unit that names a non-default profile carries only that
  profile\'s execution-relevant options as flat keys (a synthetic declaration whose second
  profile excludes `echo` and `fig-cap`: the default-profile unit carries them, the
  named-profile unit does not, and each set equals the options that move that unit\'s
  hash).
  Done (2026-10-31): the Jupyter-shaped cases are in `record/builder.rs`
  (`execution_relevant_options_are_flat_keys_and_presentation_ones_are_not`,
  `recorded_keys_equal_the_options_that_move_the_exec_hash`,
  `option_order_in_the_source_changes_neither_keys_nor_exec_hash_and_bytes_are_stable`). Two
  readings: a shuffled option order changes the author text, so the Positron content hash and the
  `source` differ and only the flat keys and `quarto_exec_hash` are equal; and the named-profile case
  is tested on the primitives in `exec_hash.rs` (`recorded_option_keys_are_the_options_that_move_each_profiles_hash`)
  because the builder never names a profile. Byte-identical whole-record output waits on Phase 4\'s golden.
- Moved out of this plan (2026-10-31; both need pipeline-side inputs EXR-4 does not have, and
  the hash primitives they use are tested above):
  - Source-of-inputs tests (the hash is built from pre-engine units: an included file\'s code
    edit changes it, a prose-only edit does not, two byte-different spellings of the same metadata
    hash alike) are now in EXR-5, Phase 2, next to the stage\'s unit and author-text plumbing.
  - Overlay tests are now in EXR-15 Phase A (its first task already checks `RenderOverlay`); the
    two clauses this plan had beyond EXR-15\'s tests, `quarto_overlay` absent without an overlay
    and changing it changes no hash, were added there. This plan keeps only the `quarto_overlay`
    field in the schema and the types.
- [x] Label tests: a cell with an authored `#| label:` line hashes the same
  `quarto_content_hash` as Positron's hash over the raw lines (label line included),
  whatever position the label line has among the options, including a crossref-sugared
  `fig-` label with a `fig-cap` line (hashed from the author-text mapping, not the
  stripped text); `quarto_exec_hash` is built from canonical text and options, so moving
  the label line changes the content hash and not the exec hash.
- Exit: the gate above.

### Phase 3: coalescing

- [x] `RecordBuilder` (tests first; EXR-5 and EXR-7 call it, and EXR-8, EXR-9 and EXR-11
  reach it through EXR-7): `RecordBuilder::new(engine_name, &HashDefinition, &[Unit],
  &AuthorTexts)`, `push(unit_index, UnitResult)` for each unit the engine ran, and
  `finish(EngineEntryInput) -> EngineRecord`, where `EngineEntryInput` holds the
  provenance, `can_freeze`, owned languages and folded result fields of the
  `EngineEntry`. The builder takes `source`, `quarto_lang`, the id, `quarto_content_hash`
  and `quarto_location` from the stage\'s unit and its `AuthorTexts` entry, computes
  `quarto_exec_hash` with `hash_unit` under the given definition and the entry\'s
  `document_hash`, and takes only the outputs, execution count and kernel from the
  `UnitResult`, so code an engine trimmed or rewrote cannot change matching. It does no
  scan. Tests: cells come out in the engine\'s unit order with the stage\'s ids; a
  `UnitResult` produced from engine-side code that differs from the unit\'s still yields
  the unit\'s source and hashes; a unit the engine did not run gets no cell; the document
  hash covers the ordered unit hashes of that engine\'s units only.
  Done (2026-10-31), `record/builder.rs`. Three signature points differ from the sketch above, all
  forced by what the builder must compute: `new` returns `Result` (the unit list and `AuthorTexts` must
  have equal length: `BuildError::LengthMismatch`); `push` returns `Result` (out-of-range index, a unit
  pushed twice, an empty id slot meaning `fill_ids` did not run, or an invalid option block);
  `finish` returns `Result` (it calls `hash_document`). `EngineEntryInput` gains `document_metadata`,
  the merged top-level document metadata that `hash_document` reads; it is a hash input only and is
  not stored (the stored `metadata` stays the engine\'s folded result). The engine\'s "units" for its
  document hash are the units it pushed, in unit order whatever the push order, so a unit an engine
  never ran is not an input. The builder never names a profile, so `quarto_hash_rule` is absent until
  an engine can ask for one. The cell\'s nbformat `id` is the plain content hash here; uniqueness
  across the record is coalesce\'s job.
- [x] `coalesce(engine_records: &[EngineRecord], unit_order: &[Unit]) -> Record`:
  `unit_order` is the stage\'s single unit list (epic decision 38), so cells are ordered by
  that list with no renumbering and no use of source positions, each cell keeps its
  `quarto_engine`, and engines stay in execution order. Tests: two engines
  interleaved; an engine with no units; duplicate identical cells keep an
  occurrence index; a unit missing from one record is reported, not dropped.
  Done (2026-10-31), `record/coalesce.rs`. Returns `Result<Record, CoalesceError>`. "A unit missing
  from one record" is read as a cell whose `quarto_cell_id` is in no unit of the stage\'s list
  (`UnknownCell`, naming the engine); a unit no engine ran is legitimate (not owned) and absent. A unit
  recorded by two engines is `DuplicateCell`. Cells are matched to units by id, not position. The nbformat
  `id` is the content hash with `-2`, `-3` appended to later duplicates, computed from
  `quarto_content_hash` so coalescing a coalesced record gives the same ids. `quarto_source`,
  `quarto_files` and `quarto_overlay` are left for the caller to set (they come from the pipeline).
- Exit: the gate above.

### Phase 4: deterministic writer

- [x] Golden file test: a record serializes to checked-in bytes; serialize twice
  equals; shuffled input maps produce identical output; no absolute paths or
  timestamps (a scan test over the bytes).
- [x] Every `execute_result` the writer emits carries `execution_count`, and the golden
  validates with Python `nbformat.validate` and no warning.
  Done (2026-10-31), `record/writer.rs` and `tests/fixtures/records/golden.ipynb` (regenerate with
  `UPDATE_GOLDEN=1 cargo nextest run -p quarto-core -E 'test(record::writer)'`, review the diff, then
  run the tests again, since `include_str!` reads the file at compile time). `write_record(&Record) ->
  Result<String, serde_json::Error>`: nbformat convention (one-space indent, sorted keys, LF, UTF-8
  unescaped, one trailing newline). Keys are sorted explicitly because the workspace enables
  `serde_json`\'s `preserve_order`, so a `Value` object would otherwise carry the order an engine built
  it in into the bytes; a test builds the same outputs in two key orders and compares bytes. The writer
  also normalizes: a missing `execution_count` on an `execute_result` becomes `null`, and
  `quarto_files_ref` is never emitted. `uv run scripts/exr-record/nbformat_facts.py <golden>` reports
  `valid=True warnings=[]`. Nothing in CI runs that script, so the validation is a manual gate for now;
  wiring it is not in this plan.
- Exit: the gate above.

### Phase 5: reader and Positron import

- [x] Reader validation (nbformat 4, cells array, `quarto_record_version`
  present); a record without `quarto_engine` tags derives engines from ownership.
  Done (2026-10-31), `record/reader.rs`: `read_record`/`read_record_value` return `ReadError` for
  bad JSON, non-nbformat-4, no `cells` array, a missing or newer `quarto_record_version`, and an
  invalid structure; `Record::engine_of`/`cells_of` fall back to `owned_languages` (exact,
  case-insensitive match on the full language token) when a cell has no `quarto_engine` tag.
- [x] `import_positron_cache(bytes)`: returns outputs keyed by content hash,
  marked `provenance: positron`, display-only, normalized at read (a rich output is
  accepted as `execute_result` or `display_data`; string-valued JSON mimes are parsed
  into objects), and language-gated; the normalization rules, the gate and their tests
  are specified in EXR-16. Tests with a Positron-written file (version mismatch and
  invalid structure are rejected, not deleted).
  Done (2026-10-31), `record/positron_import.rs`: `import_positron_cache(&[u8]) -> Result<PositronImport,
  ImportError>`; `PositronImport::outputs_for(hash, occurrence, owner, lang)` applies the gate (owner is
  `jupyter`, language is `\w+`) and the occurrence rule, and returns `DisplayOnlyOutputs`, a type with no
  conversion to `UnitResult` or `RecordCell`. Rich outputs normalize to `display_data` (no execution
  count), `quarto_webview_metadata` is dropped, an invalid JSON string stays a string with a notice in
  `notices`. EXR-16 Phase A now says the same (reconciled 2026-11-01): a wrong `quarto_cache_version` is
  rejected (`ImportError::VersionMismatch`) and the importer never deletes the file.
- [x] `place(record, units, document_text, format) -> String`, the shared helper for
  rendered units: for each unit whose record cell holds one `display_data` with
  `text/x-quarto-markdown`, it substitutes that stored markdown at the unit, after
  checking `quarto_format` against `format` (a mismatch leaves the unit verbatim and is
  reported). EXR-4 owns it; EXR-9 (marimo) and EXR-11 (knitr) reuse it. Tests first: a
  marimo-shaped record (the existing shape is `marimo_shaped_capture_splices`,
  `crates/quarto-core/tests/integration/capture_splice_seam.rs:226`) places its markdown;
  a unit with no cell stays verbatim; a format mismatch is reported; a front-matter span
  (`quarto_location: metadata`) is never placed.
  Done (2026-10-31), `record/place.rs`: `place(&Record, &[Unit], &str, &str) -> Placed { text, notes }`. It
  returns a `Placed` rather than a bare `String` so a format mismatch can be reported
  (`PlaceNote::FormatMismatch`); callers use `.text`. Units match cells by `quarto_cell_id` (so an edited
  cell, whose hash changed, stays verbatim); a stored output with no `quarto_format` counts as a mismatch;
  a fenced cell keeps its trailing newline and its blockquote prefix and indent on continuation lines;
  edits apply back to front. The test shapes are in the module (marimo-shaped, no cell, mismatch,
  front matter, raw cell, several units, blockquote).
- [x] Reserved sidecar field: parse and ignore `quarto_files_ref`; writer never emits it.
  Done (2026-10-31): covered in `record/types.rs` and `record/writer.rs`, plus a reader-level test.
- Exit: the gate above.

### Phase 6: freeze-readiness checklist (epic decision 1), each a test

All in `record/freeze_readiness.rs` (test-only), done 2026-10-31 unless noted.

- [x] Prose-only edit keeps `quarto_execution_hash`; a code edit changes it. An inline
  expression edit changes it too.
- [x] Mutating provenance (engine, kernel, platform, `quarto_overlay`) changes no
  matching, staleness or reuse key (record hash, document hashes, cell ids, content and unit hashes).
- [x] Self-contained: every `ExecuteResult` fold field round trips.
- [x] Presenting needs no engine (proved with EXR-1's availability test on a record).
  `a_record_replays_without_probing_engine_availability` (`tests/integration/ipynb_stored_replay.rs`) writes a
  record with `write_record` and feeds it to the same setup as `replay_does_not_probe_engine_availability`
  (Jupyter registered, unavailable, counting checks): the stored output replays and no availability check
  runs. **Finding for EXR-13/14:** the notebook converter takes the fence language from
  `metadata.kernelspec.language` (Q1 behavior) and a record has none (it has per-cell `quarto_lang`), so a
  bare record converts to empty `{}` fences and replays nothing. The test supplies the kernelspec a consumer
  would derive from `quarto_lang`; code that presents records through this path must do the same.
- [x] Rendered units carry `quarto_format`; raw units carry none. Tested over the golden, which has both
  a raw (Jupyter) and a rendered (knitr) engine. Enforcing it when an engine pushes a unit belongs to
  EXR-9 and EXR-11.
- [x] Deterministic bytes (Phase 4 goldens), plus a build-twice check.
- [x] Figure story documented: inline now, sidecar field reserved (module docs and the schema
  reference above), and a test that inline image bytes round trip with no `quarto_files_ref` emitted.
- Exit: the gate above, and the WASM build compiles the module (recipe in EXR-1).
  Gate met (2026-10-31): clippy clean; `nextest -p quarto-core` 6771 passed, 1 failed (the known reveal.js
  vendoring test); the recipe `cargo check --target wasm32-unknown-unknown` from
  `crates/wasm-quarto-hub-client` exits 0, with `record` ungated in `lib.rs`. Dependents: 1408 passed, 2
  failed (`quarto-preview` `pdfjs_serving`, environmental, fixed by `npm install`).

## Risks and open questions

- **Hash definitions are the freeze-correctness surface.** Too narrow a
  definition under-invalidates silently; the default declaration excludes nothing, and an
  engine's exclusion list is a closed list of display-only keys.
- **Accepted: outside the hash.** `.env` and `project_env`, data files, and
  format-specific defaults merged after `ast.meta` (`engine_execution.rs:965-1006`).
- **The two version keys:** if q2 and Positron schemas diverge, drop
  `quarto_cache_version` rather than bump it.
- **Positron records have no execution order or engine tags**; reading them is
  display-only by construction.
- **Overlay-free hash:** a record produced under a materially different overlay looks
  fresh. Accepted; `quarto_overlay` keeps the data for a later warning.
- **Cell `id` at nbformat minor 5** is emitted (decision 25); duplicate hashes get the
  `-2`, `-3` suffixes, so the ids stay unique within a notebook.
- **`owned_languages` in `quarto_engines` is new.** It was added in review so that a
  viewer with no engine loaded can group units per engine when it recomputes document
  hashes; it is not an epic decision and needs Gordon\'s confirmation.
- **Positron consumes a hand-dropped record** (EXR-16): its cache lookup deletes a
  matching file; this plan\'s file never goes in Positron\'s cache directory.

## References

- `crates/quarto-trace/src/lib.rs:187`, `engine/context.rs:291`, `engine/capture_files.rs:206`
- the Positron files cited above; bd-sr73
- [EXR-1](2026-10-28-exr-01-presenter-extraction.md), [EXR-2](2026-10-28-exr-02-unit-scanner.md)
