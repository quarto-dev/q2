---
title: 'Epic: execution records in use (preview, hub, and records from other tools)'
date: 2026-11-02
description: 'Follow-up to the execution-records epic. The RIU-0 and RIU-0b spikes proved records through the real q2 preview and hub, Julia, and node-level provenance; the remaining plans make records a total substitute for the preview capture cache, flip the writer default per surface, add record sources that do not execute (starting with the Positron cache), and end with the Positron command demo.'
status: ready-for-review  # RIU-0 and RIU-0b done 2026-11-03; third, fourth and fifth reviews answered 2026-11-05; rebased onto the knitr stack and re-checked by spikes 2026-11-05; nothing in RIU-1 to RIU-6 is implemented
---

**Branch:** `feature/records-in-use`, the third branch of a stack: `feature/execution-records-epic`, then
`feature/knitr-structured-records`, then this one (worktree `workspace-4`). It was rebased onto the knitr
branch on 2026-11-05; see [the stack-rebase findings](../research/2026-11-05-riu-stack-rebase-spikes-findings.md)
for what that changed, what was run, and four contradictions with the stack's plans, which Gordon answered on
2026-11-06 (see "Decisions recorded 2026-11-06").
**Extends:** [the execution-records epic](2026-10-28-execution-records-epic.md). That epic
built the record, the presenter and the readers; this one makes them the thing preview and
hub actually use.
**Reviews:** [first](../research/2026-11-03-riu-plans-review.md), [second](../research/2026-11-03-riu-plans-review-2.md), [third](../research/2026-11-04-riu-plans-review-3.md), [fourth](../research/2026-11-05-riu-plans-review-4.md) [fifth](../research/2026-11-05-riu-plans-review-5.md), and the [stack-rebase spikes](../plans/2026-11-05-riu-stack-rebase-spikes.md) with their [findings](../research/2026-11-05-riu-stack-rebase-spikes-findings.md).
**Spikes after the second review:** [RIU-0c findings](../research/2026-11-03-riu-0c-findings.md) (viewer editing, stamp shapes, a failing cell, a type-check of the plans' signatures).
**Findings:** [RIU-0 findings](../research/2026-11-03-riu-0-spikes-findings.md) (S1 to S7, B1 to B13,
U3 to U20) and [RIU-0b findings](../research/2026-11-03-riu-0b-findings.md) (J1 to J8, P1 to P8,
U21 to U29). Section names in parentheses below point there.
**Parallel to:** [the knitr execution-records epic](2026-10-30-knitr-execution-records-epic.md)
(integration branch `feature/knitr-execution-records-epic`; the live work was on branch
`knr-2-presenter` in `workspace-3` as of 2026-11-03). See "Coordination with the knitr epic".
**Origin:** a session of 2026-11-02 that began as a Positron demo request and found that the
records were built and unit tested but had never been run through the real `q2 preview`. RIU-0
ran them; the demo moved to the end of this epic.
**Terminology:** as in the parent epic. A **record source** is anything that can supply a
record for a document: an engine run, the preview cache, a file under
`.quarto/execution-records/`, a directory of Positron-format cache files, or the Positron
cache.

## Goal

Make the ipynb record the only thing preview and hub need, and let it come from somewhere
other than an engine run:

```text
record source --> record --> capture doc / cache --> splice (preview, hub) --> page
(engine | cache | .quarto file | Positron cache | directory)
```

- Preview and hub work from records with the writer default on. The legacy capture is required only for a
  document that uses an engine that builds no record (stored whole in the legacy format, with a diagnostic),
  for data already stored in that format, and for `provide-hub` until its flip; EXR-17 deletes it once its
  trigger holds, which for extension engines may be a long way off.
- A document gets a record from the first source in its list that has one. With the default lists a
  preview always ends at the engine, so a document with code cells gets a record; if the list omits
  `engine`, or the policy is `preview.engine: off`, the engine is never run and a document gets a
  record only if another source holds one (nothing otherwise, and the page says so). The spliced
  output is the same whichever source supplied it. `execute: {record: true}` controls only whether a
  copy is also written under `.quarto/execution-records/`; it never decides whether a record exists.
- A user can point preview or provide-hub at records that were not made by q2, starting
  with the Positron inline-output cache, and no cell or engine code runs (the document's own pipeline still runs, as in
  any preview; see "Never execute" in settled decision 5).
- The output spliced from a record is marked on the AST nodes with where it came from, through the
  existing provenance system, whether or not any viewer reads it yet.
- It ends with the Positron command demo that started this: three commands in Gordon fork of
  Positron that exercise both directions, on a Python and a Julia document, with the preview shown in
  Positron's preview pane.

## What is known (verified by RIU-0 and RIU-0b on 2026-11-03)

**Records and the real binary**
- **Records work through the real binary with the writer gate on.** In the real `q2 preview` and
  through a local hub with `provide-hub` and a freshly built hub client, every output type
  shows (text, figure, HTML table, error with traceback, inline span), prose edits keep every
  output, and a code edit drops only the edited cell. The preview then offers Re-execute; the hub
  client offers only Clear results. Knitr, Python plus knitr, Julia and marimo behave the same with
  the gate on or off. (S1 B2 to B6, B10, B12)
- **The EXR-15 mixed-document worry is real only for an engine that builds no record.** A document with
  a Python cell and a knitr chunk (both engines build records) shows both outputs with the gate on, and the
  record carries the right engine per cell (S1 B10, re-run after KNR-6 on 2026-11-05). A document with a
  Python cell and a cell from an extension engine that builds no record **loses that engine's output** with the
  gate on and shows it with the gate off. The cause is in `encode_capture_payload`: a record with any engine
  is written alone and the legacy captures, which hold the record-less engine's output, are dropped. A document
  of only the record-less engine is fine either way, because its record describes nothing and falls back to
  the legacy array. (Stack-rebase spike S5.) The writer stores such a document whole in the
  legacy format and says so with a diagnostic (settled 2026-11-06, option A; RIU-1).
- **No released version contains the record reader.** The reader and the EXR-14 splice are on the
  epic branches only; `v0.39.0` (the latest tag) has no `CapturePayload::Record`. For the hosted
  hub the question is the deployed `q2_ref`, not a release number; the last recorded deploy is `ce01489c4` (2026-09-25), before the reader (see "Direction: flipping `provide-hub`"). (S3)
- **The writer gate is off by default.** `RECORD_WRITING_DEFAULT = false`
  (`capture_files.rs:259`); `QUARTO_ENGINE_RECORD_WRITE=1` turns it on. It decides only the
  form of the hub document payload; its only two production callers are `capture_driver.rs:405` and
  `execute.rs:361`, which hand it to `encode_capture_payload` as a boolean. It does **not**
  cover the local cache: `<key>.record.bin` is written whatever the gate says. `q2 render` does
  not consult it. (S1 B1)
- **Old readers fail loudly.** An older `q2` pointed at a store holding a record document shows
  the whole page as "Render Error: Failed to parse capture: JSON parse failed". (S3)

**Stranding map (S3).** Only these can hand an older reader a record document: `provide-hub` to
a remote hub whose client is older, and a persisted `--data-dir` carried across a downgrade.
A preview publishes only to its own embedded store (its hub is built with no peers,
`quarto-preview/src/lib.rs:694`), so there is no "preview to a remote hub" case. The default local
preview and a `--share` session cannot be stranded by version: writer and reader are one binary,
the store lives in a temp directory, and a guest on a different binary tunnels the host UI. (The
two-binary share check was not run; it rests on how the asset manifest works.)

**Records are a substitute for the capture cache with small changes (S2)**
- A record-only cache entry (no legacy `<key>.bin`) works end to end through the preview and a
  restart. S2 found five places (`record_capture_run_cached`, `record_one`,
  `perform_re_execute`, `run_and_store`, and the `engines=` log line) and one test assertion
  (`cached_wrapper_stores_and_returns_the_record`); the plan review added a sixth, the
  `encode_capture_payload` engines test, so RIU-2 changes six places. The survey expected many failing tests; they
  use the passthrough engine, which builds no record, so they were unaffected.
- A record-only entry is meaningful only with the gate on: `encode_capture_payload` writes an
  empty legacy list when the gate is off and there are no captures.
- An unreadable record with no legacy file is a miss that re-runs the engine (a corrupt or
  future-version record survived in S1 B11 only because the legacy file was still there).
- The cache key is `sha256(compute_input_qmd)` (`cache.rs:55`), so any edit misses a cold
  cache. In a live session the hub path-keyed `CaptureRef` carries reuse across a prose edit,
  and a persisted `--data-dir` keeps the ref, so reuse across a restart works once defect D1 is
  fixed (it is, `f607ad7d2`). A path-keyed pointer in the cache is needed only if the cache
  moves out of `data_dir`; it does not. (S1 B3, B7; S2b)
- The cache lives in `<data_dir>/captures`, a `TempDir` deleted on exit unless `--data-dir` is
  given (`crates/quarto/src/commands/preview.rs:248-255`; the CLI passes `cache_dir: None`).
- `record_eager_captures` skips a path that already has a capture ref; it now recomputes staleness
  for that path. (S1 B7)
- Nothing in the WASM and hub-client read path changes for a record-only capture doc: the payload
  is dispatched by shape. (S1 B12, S2)

**Execution control (S1 B8)**
- `preview.engine` is `manual`, `auto` or `off`. With `off` and an existing ref, records show, no
  kernel starts and an edited cell loses its output silently. With `off` and no ref, the eager
  driver does nothing, so nothing shows. `POST /api/preview/re-execute` is **not** gated by
  `off`: it returns 202, starts a kernel and writes a capture. The automatic re-execute under `auto`
  (`trigger_auto_re_execute`) reaches `claim_and_spawn` without going through that handler, so it is
  ungated too (second plan review). The `off` guard exists twice, in the
  eager driver (`capture_driver.rs:67`) and in the watcher hook (`:262`).

- **A failing cell** (RIU-0c spike 3): without `error: true` a cell that raises makes the whole engine
  run fail (`record_capture_run` returns an error, for Python and for Julia); with `error: true` the run
  succeeds and the record holds an `error` output for that cell.

**Providers (S3)**
- The consent gate is one call, `self.consent.review(...)` at `execute.rs:313` in
  `run_and_store`, after materialize and the review file and before any engine runs. An import
  path that does not call `run_and_store` never reaches it.
- `write_capture_doc` exists in `capture_driver.rs:396-441` and `execute.rs` (`:530`); encoding
  and the payload document are shared (each caller updates its own sidecar; `set_capture` is not part of the seam). A single "publish a `CaptureRun`" function
  is the natural seam.
- `.quarto/execution-records/` is write-only: `q2 render` writes it for `record: true`, and
  nothing reads it. The write is `write_record_file` (`engine_execution.rs:1120`, sole caller
  `:892`), propagated with `?`, so an unwritable `.quarto/execution-records/` aborts the render
  with no HTML. `.quarto` being a regular file fails the render earlier with "Failed to create
  .quarto directory", with or without `record: true`. (S1 B9, B11)

**Positron cache as a record source (S4)**
- **A record with no engines, empty execution hashes and the real content hash splices in the
  unchanged viewer**, on a real Positron cache (an R document, Ark R): 0 kernels, stream, table,
  image, error and result all show, an edited cell loses only its own output, and the three
  content hashes equal q2 own. Conversion signature (the production function also stores a count of
  cache cells no unit paired with in the record `extra`, because a record built from the current units
  can never show an orphaned cache cell by itself; RIU-3 "Unclaimed cache cells"):

  ```rust
  pub fn record_from_positron(
      import: &PositronImport,
      units: &[Unit],           // ids filled
      authors: &AuthorTexts,
      source: Option<String>,   // project-relative document path
      owner: &dyn Fn(&str) -> String,
  ) -> Record
  ```
- The language gate and the occurrence count belong at conversion, with the caller supplying the
  owner: the count is per content hash across all units and precedes the gate. The conversion
  and `import_positron_cache` build for WASM through `npm run build:wasm`. The spike took two
  shortcuts the real path cannot: it hashed a raw scan instead of the sugared author text, and it
  hard-coded the owner; RIU-3 uses the splice path's inputs and the engine registry's resolution.
- Provenance at the record level is `quarto_provenance: "positron"` in the record `extra` and each
  cell `extra`. It does not map onto engine provenance: a Positron record has no engine entry, and
  an invented one would claim languages and break the content-hash-only match. A future freeze reader
  sees an empty engine list and the marker. (RIU-0b, P and Answer 5)
- An engineless record needs a predicate: `Record::describes_something()` (engines or cells
  non-empty; it does not exist in the tree yet, only on a spike branch) replaces "has engines" in
  `encode_capture_payload` (`capture_files.rs:303`) and the cache hit and write filters. **RIU-2 owns
  both the predicate and the encoder change.** Without them the hub doc is written as an empty legacy
  list, and with the gate off an imported record has no legacy form at all, so the publish seam refuses
  (decision 9).
- `record_is_stale` returns not-stale with no engines and `rehash_record` stamps a hash of an
  empty list; neither may be called on an imported record. But `recompute_staleness` reaches
  `record_is_stale` for any record document (`capture_driver.rs:324`), so RIU-3 adds a guard there that
  fires when the record carries `quarto_provenance` **and** has no engines. Staleness of an imported
  record comes from one function (orphaned cells: the stored unclaimed count, and `Matching::orphaned`
  from `match_units`; never `UnitStaleness::Stale`, which means an uncovered document unit).
- `DisplayOnlyOutputs` must never gain a `From` or `Into` to a record cell
  (`imported_outputs_convert_to_nothing_a_reuse_decision_takes`); building a cell by hand from
  `outputs()` is allowed.
- Under real q2, an `{r}` chunk is owned by knitr, so a cache of R cells run by Ark gets **no**
  outputs; S4 showed them only by forcing a Jupyter owner. Whether R through Ark should route to
  Jupyter decides whether Positron imports help R documents at all (settled: not in this epic; KNR-5's gate widening, merged, covers Ark R caches, decision 11).

**Julia (RIU-0b spike J)**
- A Julia cell is owned by the **`julia` engine**, q2's own Julia engine (a QuartoNotebookRunner
  server), never by Jupyter: `jupyter: julia-1.11` and `engine: jupyter` both still resolve to
  `julia`, and the `julia-1.11` kernelspec is unused. (J1, J2)
- So the S7 export rule (owned by `jupyter`) skips every Julia cell, and the Positron import gate
  (owner `jupyter` only) gives a Julia unit nothing. Admitting the owner `julia` in both fixes it,
  proven on a patched scratch build (4 of 4 cells export and import). The harness passed the owners as
  string literals; the registry's own resolution was not called, so that `julia` is the owner comes from
  the J1 record (`owned_languages`, cell `quarto_engine`), and RIU-3 tests it with the real registry.
  (J4, J6)
- Julia outputs are strings with several MIME types per bundle; a plot is PDF, PNG, SVG and HTML in
  one bundle with no `text/plain`. Pushed into an open Julia document in the Positron dev build,
  text, vector and HTML outputs matched by content hash, displayed and persisted (Gordon, J7), and a
  plot showed **twice with base64 text**, because Positron renders every item of a bundle (Gordon,
  J8). The exporter therefore selects one image representation. (J5, J8)
- Not tested: Positron producing a Julia cache itself, and a Julia cell with option lines.

**Node-level provenance (RIU-0b spike P)**
- Spliced output **can** be identified after the reparse (the file slot of the reparsed text plus the
  unit's text range) and stamped by a post-walk: `Generated` with `from` empty and
  `data = {"source", "unit"}`. The wire format, a prose-edit round trip through `apply_node_edit`, the
  tiling auditor's CI-gated finding kinds and the structural hash all hold. The incremental writer
  never receives the spliced AST. 12 spike tests, and the full `quarto-core` and `quarto-preview`
  suites (7606 tests) pass on the spike branch. (P1 to P8)
- Two constraints it found: a stamped `CodeBlock` loses its copy-button decoration (the key needs an
  `Original`), so `CodeBlock` is never stamped; and an inline value merges with adjacent prose into one
  `Str`, so a `Str` is stamped on overlap. The review added `Header`: a `Generated` header keeps its id
  and is never re-derived (`heading_id.rs:56`), so it is not stamped either (decision 14).
- **The shapes** (RIU-0c spike 2, real splice on a stub Jupyter engine): a cell in a div, callout,
  tabset, list item or blockquote, a labeled and captioned cell, a cell with no outputs and a stored
  markdown cell are all stamped correctly; an inline value stamps only its `Str`, except that a value
  alone in a table cell stamps the cell's `Plain`. Stamping changed no rendered HTML or diagnostic text
  in any shape, including heading ids and the Q-15-1 duplicate-id diagnostic (only its raw location
  differs). Two things the stamp does not cover: the copy-button scaffold `Div` and `RawBlock` around each
  `CodeBlock` keep a reparse-slot range, and an expanded cell that shifts the block count lets reconcile
  pair the author's next Div with a spliced `.cell` Div, which then carries author bytes unstamped.
- **Echoed code is not editable today**: its source info resolves into the reparse slot, never the
  author's file, and the atomic gate only silences `setLocalAst`, which `CodeBlock` never calls. An
  atomic cell Div therefore breaks nothing that works. (RIU-0c spike 1)
- `quarto-source-map` is an **external crate** (crates.io 0.3.1) that already defines an atomic
  `By::jupyter_output()` kind nothing here uses. Decision 14 reuses it; a new first-party kind would need a release of that crate,
  or be non-atomic and built in `quarto-core`. The TypeScript `ATOMIC_KINDS` set lacks `jupyter-output`
  and `citeproc`; the provenance contract's catalog is out of date. (P2)

**Positron side (S5 to S7, J7, J8)**
- **Push into an open document is one method** on `QuartoOutputContribution` (match each cell by
  content hash with `findCellByContentHash`, clear its outputs, add the new ones to the view
  zone, `saveOutput` to persist) plus a parse helper on the cache service. Seen in the dev
  build: outputs appear live, persist across a close and reopen, survive a dirty document, and a
  cell whose code changed is skipped. No reload plumbing and no `_cachedOutputsLoaded` change
  is needed. (S5)
- **Cache location.** Observed on macOS: release `~/Library/Application Support/Positron/User/globalStorage/quarto-inline-outputs/`,
  dev build `.../code-oss-dev/User/globalStorage/quarto-inline-outputs/`. Read from
  `userDataPath.ts`, not run: Linux uses the **config** directory (`$XDG_CONFIG_HOME` or
  `~/.config`), not the data directory; Windows uses `%APPDATA%`; `POSITRON_PORTABLE`,
  `POSITRON_APPDATA` and `--user-data-dir` override. A file is `sha1(document URI string)[0:16]`
  plus `.ipynb`, checked on two plain macOS `file://` paths; VS Code's `URI.toString()` lowercases a
  Windows drive letter and encodes `:` and non-ASCII its own way, unverified. Every file carries
  `metadata.quarto_source`, which Positron writes but does not read. (S5; RIU-3 Phase C)
- **Running q2 from Positron.** An extension runs `child_process.execFile` in the extension host
  and hands the result to the workbench through an internal `_` command. `q2` is **not** found by
  name when Positron starts without the shell PATH; an absolute path in a setting works, and
  the `q2` on PATH can be an old nightly, so the command must treat a usage error as "q2 too old". (S6)
- **What Positron displays.** A real q2 record of Jupyter Python cells (stream, PNG figure, HTML
  table, error with traceback and colors, labels, a duplicate-hash pair) is displayed correctly,
  and the record loads with **no transform**. The export is a filter: skip inline units and any
  unit that is not a plain-language cell owned by `jupyter` or `julia`, skip a whole duplicate-hash
  group if one member is skipped, keep the three cell keys, always emit `outputs`, stringify
  non-string data, and send one representation per image. Untested: array and JSON-valued outputs,
  where the transform does change bytes, and the representation rule in the window. (S7, J8)

## Settled decisions

Made with Gordon on 2026-11-02 and 2026-11-03, with the per-surface refinements and the
design choices the spikes and the plan check left to settle.

1. **Records substitute for the preview capture cache.** A record is built whenever a
   consumer asks (preview and hub already do: `want_record = record_requested ||
   ctx.observer.wants_execution_record()`), and the preview cache holds it. `record: true`
   decides only whether it is also written to `.quarto/execution-records/`. It never decides
   whether a record is available.
2. **Flip the writer default, per surface.** `RECORD_WRITING_DEFAULT` becomes on, and the
   flip is the cutover from legacy captures to records in the cache and the hub store. What the runbook
   protects is that a record is never handed to a reader that cannot parse it, so the rule is stated per
   writer and reader pair (the S3 stranding table), not per release. The **local preview** flips with the
   first release that carries the reader, with RIU-2 landing first, because its writer and reader are one
   binary and its store is a temp directory; the one exception is a kept `--data-dir` opened by an older
   binary after a downgrade, which strands, and the release notes say so. **`provide-hub`** flips only after
   the hosted hub runs a `q2_ref` that contains the reader. A plain `q2 render` keeps writing nothing under
   `.quarto/` unless `record: true` is set. RIU-1 rewrites the runbook rule to match, and the parent epic's
   "readers ship one release before writers" points here (decisions of 2026-11-06).
3. **The existing reuse is unchanged, and one bounded kind is new.** Preview and hub already reuse a
   recorded result (a cache hit skips the engine; a hub viewer splices a provided capture), and that stays.
   What this epic adds is a second kind of record provider, one that does not execute, and with it a new
   kind of reuse: a `q2` or directory record can skip the engine in a preview. It is bounded four ways. The
   record is chosen only if it passes the staleness check at choice (RIU-4). Reuse stays whole-document or
   nothing (parent decision 1). `provide-hub` defaults to `engine` and reuses a file only when the user
   names one. `q2 render` reads no record. The preview's default list `cache,q2,engine` includes `q2` on
   purpose (decided 2026-11-06). This changes the parent's reading of decision 8, "nothing skips execution
   on a hit", which was written when a record file was write-only.
4. **Record sources are an ordered list, per document, first wins, no merging.**
   `--records cache,q2,positron,./some/dir,engine` on `q2 preview` and `q2 provide-hub`,
   default `cache,q2,engine` for `q2 preview` and `engine` for `q2 provide-hub` (the provider's
   Decision #4 is a fresh, uncached run, `execute.rs:332-335`, and it has no cache directory, so
   `cache` there is a start-up error; a user opts into more with `--records q2,engine`, which with the gate off,
   the default until the `provide-hub` flip, is refused at the publish seam and falls through to the engine). `q2` is the
   optional `.quarto` copy. One `CaptureRef` still points at one record. The list is read once at
   start, not again when a file changes: a file change recomputes staleness only, so a Positron cache
   written after the preview started is not seen until restart. The option is a comma list or a
   repeated flag with no custom parser: a known name is that source, an entry that looks like a path
   (a separator, a leading `.` or `~`, or absolute) is a directory of Positron-format cache files read
   by the same importer as `positron`, and anything else is an error that lists the valid names. (Revised
   2026-11-03: the first design was a `dir:` prefix with a parser that let it take the rest of its
   value; a bare path replaced it.) A source supplies a record only if it yields one that describes
   something and, for a source that matches by unit, at least one unit matches; otherwise the next
   source is tried. The name and the defaults are settled.
5. **"Never execute" is `preview.engine: off` or an absent `engine` in `--records`, not a new
   flag.** The record-source lookup runs outside the eager driver's `off` guard
   (`capture_driver.rs:67`), so with `off` the sources are consulted and the engine is never run; the
   watcher hook keeps its guard (it has no lookup, only a staleness compare). Every re-execute path
   (the `POST /api/preview/re-execute` handler, open today, S1 B8, and the automatic re-execute under
   `auto`, which bypasses the handler) goes through `claim_and_spawn`, which refuses when the policy is
   `off` **or** `engine` is not in the resolved source list. Re-execute runs the engine only: it does not
   consult the other sources, stores the result and points the `CaptureRef` at it. RIU-2 owns the
   guard (in `claim_and_spawn`, with the `ReExecuteConfig` that carries the policy and the list) and the
   engine-only meaning; RIU-4 owns the validation and the tests that need the flag. "Never execute" means no
   cell or engine code, that is, no code from a code cell or inline expression and no engine process
   (kernel, `knitr`, `julia`). Everything the pre-engine pipeline does still runs, as in any preview and as
   in the provider's consent review: Lua filters, shortcodes, includes, and (once, at start-up) project
   pre-render scripts. A guest cannot trigger the scripts. (Decided 2026-11-03; extended after the second plan review; guard moved to RIU-2
   and the scripts clarified 2026-11-05.)
6. **A Positron record is display-only.** It is built by hand with no engines and empty
   execution hashes, carries `quarto_provenance: "positron"`, uses only content hashes and the
   language gate (run at conversion, owner from the engine registry), and in this epic drives no
   automatic reuse or freeze decision (q2 never decides on its own that a run can be skipped because a
   Positron record exists; the user opts in by naming the source). It is display-only here, not
   barred from a later render use: see "Direction: render from a Positron record". The parent decision 14 and EXR-16 decision 7 stand. `recompute_staleness`
   never reaches the engine-hash comparison (`record_is_stale`) for an imported record: a record
   carrying `quarto_provenance` **and** no engines skips that comparison before any source is read
   (RIU-3 Phase B), but an orphaned cache cell still flags the page stale (decision 5 of
   "Decisions recorded 2026-11-05"; an imported record never asserts freshness, it may report
   staleness). Requiring the empty
   engine list keeps a hand-edited `.quarto` or directory file that has engines, and happens to carry
   the key, under the normal document-level staleness check.
7. **The Positron demo is last and is a spike in the fork.** Spike branches stay in Gordon
   fork `gordonwoodhull/positron`; nothing is sent to anyone. Commands 2 and 3 refuse an untitled or
   unsaved document ("Save the document first"); a dirty document exports from disk and the push
   skips changed cells with a report.
8. **The record cache stays where the capture cache is.** `<data_dir>/captures/`, a temp
   directory deleted on exit unless `--data-dir` is given. No new location, no gitignore, no
   eviction, and **no path-keyed pointer** (S2b: it is needed only if the cache moves). The
   record file keeps today's name and form, `<key>.record.bin`, gzipped (the rename to plain
   `<key>.ipynb` was dropped: nothing needs it, and `.quarto/execution-records/` is already plain
   `.ipynb`); `.quarto/execution-records/` and the `q2` source remain the deliberate way to keep
   records across sessions.
9. **The gate does not cover the local cache.** It decides the form of the hub document payload.
   The cache stores what a run has (a `.bin` when there are captures, the record file when the record
   describes something) and never reads the gate. A record with no legacy form (an imported one has no
   captures) cannot be published as a legacy payload, which would be an empty page; the publish seam
   (RIU-2) **refuses** with a message naming `QUARTO_ENGINE_RECORD_WRITE` when the gate is off, and
   publishes a record document when it is on. So local preview shows imported output once the RIU-1
   flip makes the gate on, and `provide-hub` with a Positron source needs the variable until the hosted
   hub has the reader (decision 2). The runbook and `docs/` say so.
10. **A failed optional `record: true` copy does not fail a render.** RIU-1 makes the write fail
    soft with a warning diagnostic. This is the one choice S1 left open (it is opt-in, so failing
    loudly is defensible); settled 2026-11-05: fail soft stays.
11. **A persisted `--data-dir` is documented, not gated.** After the flip a downgrade can hand an
    older reader record documents (S3: full-page Render Error). The docs and runbook say so, and
    RIU-1 runs the downgrade once and records the recovery step before the docs state it. What a
    persisted store holds: `automerge/`, `captures/`, `hub.json`, `hub.lock`, `.gitignore`; secrets
    stay in memory for a preview.
12. **Julia is in scope** for RIU-5 and the RIU-6 demo. Spike J settled the specifics: the `julia`
    engine owns the cells, so RIU-5's keep rule and RIU-3's import gate admit both owners `jupyter`
    and `julia`, and RIU-5 selects one image representation per bundle. No conflict with this
    decision.
13. **`q2 record export-positron` defaults to `q2,engine`** and runs the document's code when no
    record exists, with no consent prompt (it is a command the user types on their own files,
    like `q2 render`). It prints one line to stderr before running code, and `--records q2`
    forbids execution. RIU-5 owns the subcommand and its exit codes. The subcommand takes
    `--source-uri` and builds no URI, so Windows and non-ASCII encoding stay VS Code's. The export
    sends Positron one representation per image: drop `application/pdf`, prefer PNG over SVG, drop a
    `text/html` that is only an `<img>` with a `data:image/` URI, and keep any other HTML (designed
    from the J8 window run, not yet run in the window).
14. **Provenance goes through the existing system, node by node.** Spliced record output carries a
    `Generated` `SourceInfo` with a first-party `By::` kind (the contract in
    `claude-notes/designs/provenance-contract.md`), whether or not any viewer reads it yet. Spike P
    proved the post-walk, RIU-3 Phase E builds it, for every record splice (engine-run and
    imported), with `data.source` of `engine` or `positron`. `CodeBlock` and `Header` are never stamped
    (a stamped code block loses its copy button; a `Generated` header is treated as having a deliberate
    id), and a stamped matched cell is the cell Div, its output Div and the other non-code blocks inside.
    The kind is the upstream atomic `jupyter-output`, decided 2026-11-03. The condition attached to it
    is resolved (RIU-0c spike 1): echoed code is not editable today and the atomic gate does not touch
    a code block, so the cell Div is stamped and atomic. The `Header` exemption is precautionary: spike
    2 saw no render difference when `Header` was stamped (settled 2026-11-05: keep it, decision 9).
15. **No "from Positron" banner.** The label "Showing executed output"
    (`PreviewStatusBar.tsx:204`) reads the sidecar `CaptureRef`, which carries no provenance, and no
    plan changes it. Provenance reaches the page only as the node stamp of decision 14. Decided
    2026-11-03. If a banner is ever wanted it needs a sidecar field set by the RIU-2 publish seam, a
    schema change in `quarto-hub` and the TypeScript `CaptureRef`, and a component edit, and then the
    `hub-client/` changelog and `npm run build:all` rules apply.

## Decisions for review

All of these are settled (Gordon, 2026-11-05); see "Decisions recorded 2026-11-05" below for the
answers and the plan each one lands in. The nine decisions of the first plan review (numbered 1 to 9
there, not the settled numbers above), the Julia and provenance answers, and the seven of the second
review are settled and written into the plans:
[the first review](../research/2026-11-03-riu-plans-review.md),
[the second](../research/2026-11-03-riu-plans-review-2.md) and
[the third](../research/2026-11-04-riu-plans-review-3.md).

- Hosted hub `q2_ref`: settled. The last recorded deploy predates the reader; the `provide-hub` flip
  is a deferred plan (see "Deferred plan: flipping `provide-hub`").
- R through Ark: settled, not in this epic.
- Fail-soft for a failed `record: true` copy: settled, fail soft (decision 10 stands).
- `Header` exemption: settled, keep it.
- Mis-paired expanded cells: settled, an interim rule (the edit affordance refuses a `.cell` Div) because the
  splice cannot name its own nodes (RIU-3 Phase E; decisions of 2026-11-05, item 7).
- What runs before consent in `provide-hub`: settled as no change. The pre-engine pipeline already runs
  before the consent gate today (`write_review_file` calls `compute_input_qmd`), so a non-engine source adds
  no pre-consent work (RIU-4).

### Hub deployment facts (read 2026-11-03 from `~/src/quarto-hub-deployment`)

- The hub binary (`quarto-hub`) and the hub-client are built together by one manual GitHub
  Action, "Build and deploy artifacts", from a `q2_ref` input (default `main`; a branch, tag or
  SHA), uploaded to S3, then deployed by hand with `quarto-hub-deploy` on the instance
  (`docs/operations.md`). So "the hub is on a reader" means the deployed `q2_ref` contains the
  reader commits. The numbered q2 release is irrelevant to the hub.
- Deploys are rarer than binary releases. The braid snapshot records deploys on 2026-09-12,
  09-14 (twice), 09-17 and 09-25 (`main-20260925-205359`); nothing later is recorded there,
  though the log may lag. The authoritative answer is `cat /etc/quarto-hub/version.json` on the
  instance, which was not run.
- There is a rollback (`quarto-hub-rollback`, maintenance mode required) that restores an earlier
  `versions/<ref>-<stamp>` from S3. A rollback to a build without the reader is the exact case
  that strands record documents already written.
- The record reader is on the epic branches, not on `main`, so the hosted hub cannot have it
  unless someone deployed one of those branches.

## Verification environment

Every plan's verify step assumes this, so it is stated once.

- **Use `target/debug/q2`** (or `cargo run --bin q2 --`), never the `q2` on PATH: that one is an old
  nightly (0.36.0-nightly.20261027) that cannot read records. Build with `cargo build --bin q2` in the
  worktree; after Rust changes that preview serves, run the full chain from `AGENTS.md`
  (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`).
- **Jupyter** is not importable on this machine by default. Make a venv in the session scratchpad,
  not the repo (`SCRATCH` below is any such scratch directory; the recipe RIU-0 S0 used; the install step was re-run during RIU-0b with extra
  packages, a Python render was not):

  ```bash
  uv venv "$SCRATCH/venv"
  uv pip install --python "$SCRATCH/venv/bin/python" jupyter-core ipykernel matplotlib pandas
  "$SCRATCH/venv/bin/python" -m ipykernel install --prefix "$SCRATCH/venv"
  export PATH="$SCRATCH/venv/bin:$PATH"
  ```
- **Julia** 1.12.7 (juliaup) is installed and q2 finds it as `julia`. Plots in a document's Julia
  project: `JULIA_PKG_OFFLINE=true julia --project=. -e 'import Pkg; Pkg.add("Plots")'` (the packages
  are in `~/.julia`). R (knitr) works; R through Ark is not reachable.
- **A browser:** Playwright with headless Chromium works from an agent session (`~/Library/Caches/ms-playwright`).
  Steps marked "needs a browser" in the plans use it on the preview iframe.
- **Gordon at the Positron window:** an agent cannot drive the Electron dev build
  (`/Users/gordon/src/positron-spike-s3`, `scripts/code.sh`). Steps in RIU-5 Phase C and all of RIU-6
  need him; mark them "not run" when he is not there, and never substitute the conformance mirror.
- **Fixtures:** `crates/quarto-preview/tests/fixtures/records-in-use/` holds `basic`, `echo`,
  `julia`, `knitr`, `marimo`, `mixed` and `project/`. There is no `dup.qmd` and no durable copy of the
  S4 Positron cache; RIU-5 and RIU-3 create theirs.

## Plans

| ID | Plan | Sessions (guesses) | Status |
|---|---|---|---|
| RIU-0 | [Spikes that settle the unknowns](2026-11-02-riu-00-spikes.md) | 6 to 7 | done 2026-11-03; [findings](../research/2026-11-03-riu-0-spikes-findings.md) |
| RIU-0b | [Julia and node-level provenance spikes](2026-11-03-riu-0b-julia-and-provenance-spikes.md) | 1 | done 2026-11-03; [findings](../research/2026-11-03-riu-0b-findings.md) |
| RIU-1 | [Enablement and the writer flip](2026-11-02-riu-01-enablement-and-writer-flip.md) | 1 | ready; per-surface flip |
| RIU-2 | [A provider seam; records replace the capture cache](2026-11-02-riu-02-provider-seam-records-replace-cache.md) | 4 to 5 | ready; the central piece (the record-only part is small, S2); also the source list, the walk and its outcomes, the refusal predicate and the resolver trait (in `quarto-core`), the cache split, the `describes_something` change, engine-only re-execute, the `ReExecuteConfig` that carries the gate, and the execution guard |
| RIU-3 | [The Positron cache as a record source](2026-11-02-riu-03-positron-cache-as-record-source.md) | 4 to 6 | ready; split: 3a conversion (with the unclaimed-cell count), locator and directory source, the provider publish method, the staleness function and a minimal `--records` flag; 3b the provenance stamp (spike P), the TypeScript side and the browser checks |
| RIU-4 | [CLI surface for record sources](2026-11-02-riu-04-cli-surface-for-record-sources.md) | 1.5 to 2 | ready; hardens the flag (bare paths, no prefix), implements the resolver for the provider and wires it into `execute_document`, builds the diagnostics from outcomes, final docs |
| RIU-5 | [Export for Positron](2026-11-02-riu-05-export-for-positron.md) | 1 to 2 | ready; the function and `q2 record export-positron`; Julia and the representation rule (spike J) |
| RIU-6 | [The Positron command demo](2026-11-02-riu-06-positron-command-demo.md) | 2 plus an early spike | ready; spike in the fork, mechanisms proven in S5 and S6; Phase 0 tests the viewer in Positron's preview pane and can run now; owns the closing reconciliation |

The six remaining plans total about 13.5 to 18 sessions (1, 4 to 5, 4 to 6, 1.5 to 2, 1 to 2, 2), an
estimate after the fifth review. With the spikes already spent (RIU-0 at 6 to 7, RIU-0b at 1) the epic is
about 21 to 26 sessions. RIU-2 and RIU-3 are the largest.

## Ordering

```text
RIU-1 (local preview) --\
                         +--> flip ships (RIU-2 first) --> RIU-3 --> RIU-4 --> RIU-5 --> RIU-6
RIU-2 -------------------/
RIU-1 (provide-hub flip) waits on the hosted hub q2_ref, independently of the rest
RIU-6 Phase 0 (the viewer in Positron's preview pane) needs none of the above and can run first
```

RIU-3 and RIU-6 need the **preview** flip for any real-browser check, because imported output reaches
the page only as a record document, which needs the writer gate on; until the flip ships, set
`QUARTO_ENGINE_RECORD_WRITE=1`. There is no cycle: the chain is RIU-2, RIU-3, RIU-4, RIU-5, RIU-6, with
RIU-1 independent in code.

RIU-4 comes before RIU-5 because RIU-5's subcommand uses RIU-4's `--records` parsing; RIU-4 does not
wait on RIU-5. RIU-5's export function needs none of RIU-2 to RIU-4 and can be built first. RIU-1 and
RIU-2 can overlap. RIU-2 must land before the flip ships in a release, because the flip makes the
cache hold records. The `provide-hub` flip is a separate, later change gated by the deployed hub.

## Decisions recorded 2026-11-05 (third and fourth plan reviews, answered by Gordon)

Each is a settled decision; the plan named in parentheses absorbs it before its phase starts. Items 1, 5, 6,
7, 8 and 10 were revised after the fourth review.

1. The record-source resolver lives in `quarto-core`. `quarto-core` owns the list type, the first-wins walk, the
   one refusal predicate the walk and the publish seam both call, and a per-source outcome record. A two-method
   trait takes `&SourceInputs` and covers only what depends on who is asking: `lookup` for `Cache` and `Q2`,
   and `run_engine` for `Engine`, which returns either a run for the caller to publish (the preview) or
   "already published" (the provider, whose engine step is `run_and_store`). The preview and the provider each
   implement it, and the provider's `execute_document` calls the walk whenever the list names a source other
   than `engine` (with the default list it calls `run_and_store` as today). `Positron` and `Dir` are shared
   code in `quarto-core`, with no trait method. RIU-2 defines the trait, the walk and the preview's
   implementation; RIU-4 implements it for the provider and wires it into `execute_document`.
2. A `.quarto` (`q2`) record is staleness-checked when it is chosen (RIU-4).
3. A record the publish seam refuses falls through to the next source with a warning naming
   `QUARTO_ENGINE_RECORD_WRITE`; the error stands if it was the last source (RIU-2, RIU-4).
4. Imported records are not written to the cache; the cache holds engine-run results only.
   Re-execute keeps its lookup and so runs the engine. Test: "Re-execute on a document with only an
   imported record runs the engine" (RIU-2, RIU-3).
5. **An imported record never asserts freshness, but it may report staleness.** The page is stale when at
   least one cached cell is orphaned: its output belongs to code that is no longer in the document (an
   edited or deleted cell). A unit the cache does not cover, or one the language gate refuses, is not stale.
   Because a record converted from the current document cannot show an orphan by itself, the conversion
   stores the number of cache cells no unit paired with in the record, and one function in `quarto-core`
   returns "stale" when that count is above zero or `match_units` leaves a record cell orphaned (it never
   reads `UnitStaleness::Stale`, which means an uncovered document unit). `record_one` uses it at publish (so
   a cache older than the document is flagged at boot) and `recompute_staleness` on each change, so both give
   the same answer. Staleness shows the Re-execute overlay in the viewer, and `auto` may re-run, but only if
   `engine` is in the `--records` list and the policy is not `off` (decision 5 above still refuses
   otherwise). A match proves nothing about the environment, so a matching record is displayed, not called
   fresh. This narrows decision 6: only the engine-hash comparison is skipped for an imported record
   (RIU-3 Phase B).
6. When no source supplies a record and `engine` is unavailable (absent from the list, or policy `off`), the
   preview emits an uncoded diagnostic through the page diagnostic sink listing each source's outcome
   (supplied, nothing, refused with the reason, unavailable), so a refusal is named and not mistaken for
   "nothing found". It fires only for a document with executable units, at boot, for a path with no ref.
   The default list and policy are unaffected. A record refused at the publish seam (decision 3) falls
   through with a page warning in the preview (RIU-2) and a stderr line in `provide-hub` (RIU-4). `--records` conflicts with
   `--join`, `--static` and `--no-project` (RIU-4).
7. A mis-paired expanded-cell Div is handled by an **interim rule, not a contract**. Read 2026-11-05: the
   splice cannot name its own nodes (the `ReconciliationPlan` holds only index alignments and the splice
   discards it), so the fallback applies: the viewer's edit affordance refuses any Div whose class includes
   `.cell`. First check which attributes the merged Div carries, so the rule catches it. Engines will
   eventually return provenance data for the nodes they produce, which is a much larger change out of
   scope here; when it lands the rule and its `interim_` tests are replaced (RIU-3 Phase E).
8. RIU-3 is split. 3a: import, locator, the walk's Positron and directory arms, the provider's publish path,
   the staleness rule, ending with a minimal `--records` flag (parse the list, set
   `PreviewConfig.record_sources`). 3b: the stamp, the TypeScript side, the browser checks of the editor
   UI and live share. The execution guard is RIU-2's. RIU-4 keeps start-up validation, the defaults, the
   `provide-hub` rules, the conflicts and the docs (RIU-2, RIU-3, RIU-4).
9. Keep the `Header` exemption from the stamp. Dropping it has no demonstrated benefit and a known
   risk to ids; revisit only if the splice reading under decision 7 shows an unmarked heading can
   capture a wrong author line (RIU-3 Phase E).
10. **Product and demo are separate.** The product's browser checks are the default viewer and
    `q2 preview --ui editor` with live share (`--share`, with `--allow-edit` where the check edits); the
    overlay and Re-execute behavior is the viewer's, and the editor UI shows a stale label and, in a
    preview, probably no Run (to be seen). A guest-triggered Re-execute on a share host with no `engine` in
    `--records` is refused (RIU-2 guard, RIU-4 share test). The demo is the default viewer opened in
    Positron's preview pane, like Positron's Quarto 1 preview, started by a command in the fork; an early
    spike (RIU-6 Phase 0) tests the viewer inside that pane. The two-binary share case is a product check
    (RIU-4).
11. R through Ark is not in this epic. Importing an Ark-written R cache rides on KNR-5's gate
    widening (merged in `5db29f6bc`); running R through Ark from q2 would be its own design. The acceptance cases of the browser
    checks and the demo are therefore Python and Julia (RIU-3); KNR-5's K-S2 cases are the R ones.
12. A failed optional `record: true` copy stays fail-soft (decision 10 stands). `q2 render --strict`
    turns the warning into a failure for anyone who needs that; not re-verified against the code.

## Decisions recorded 2026-11-06 (the four contradictions with the stack, answered by Gordon)

Found by the stack-rebase review ([findings](../research/2026-11-05-riu-stack-rebase-spikes-findings.md)).
Gordon agreed with each recommendation. The second was settled after spike S5 (option A, with a diagnostic).

1. **Flip timing is a rule per writer and reader pair.** Decision 2 stands, restated above. The runbook's
   "do not flip it in the same release that first ships the reader" was a stricter stand-in for "never hand a
   record to a reader that cannot parse it". RIU-1 rewrites the runbook rule with the S3 table, and the
   release notes name the kept `--data-dir` downgrade. The parent epic's decision 4 and EXR-13 point here.
2. **Mixed documents: a per-document rule for engines that build no record (option A).** No rule is needed for
   engines that build records. Spike S5 reproduced EXR-15's worry for an extension engine that builds none. The
   writer therefore stores a document whole in the legacy format when any capture's engine is not in the
   record, and a catalogued warning names the engines (RIU-1; the agreed text is there). Consequences, all
   recorded in the plans: the rule is a stopgap that needs the legacy captures as its fallback, so it holds until
   EXR-17 deletes the legacy path, and EXR-17's trigger now treats the diagnostic's engine list as the inventory
   of engines still keeping the arm alive; such a document keeps main's quirk of a Re-execute banner after any
   edit, prose included, which is not a regression; and a record never reaches the legacy splice, so a document
   whose engines all build records uses only the record splice. Not verified: what reuse, preview without
   running code, and export do for such a document, which is why the hint says "may not work". The rejected
   alternatives are a wire change that carries record-less captures inside the record, and accepting the loss.
3. **An engineless record is a clarification, not a contradiction.** EXR-15's rule is that a record describing
   nothing never replaces the legacy captures. RIU-2 keeps it: with no engines and no cells the record still
   falls back, and the stack's own test passes unchanged. The one new case is no engines with cells, which only
   RIU-3's importer produces; RIU-2 adds a test for it and EXR-15 gets a pointer.
4. **Reuse of a record file is stated plainly.** Decision 3 is rewritten above, and `q2` stays in the
   preview's default list.

## Direction: flipping `provide-hub` (deferred plan)

Not built in this epic. The hosted hub (quarto-hub.com) was last deployed 2026-09-25 from q2 `main` at
`ce01489c4` (the newest deploy the deployment repo records; the instance's
`/etc/quarto-hub/version.json` was not checked). The record reader, the Positron import and the splice
by unit hash landed on 2026-10-31 (`430641e8d`, `9de60b9fe`, `9078d7470`), after it. So the hosted hub
cannot read a record document, and the gate stays off for `provide-hub`.

The flip, once this epic's features are on `main` and a deploy is wanted:

1. Land the reader and the RIU work on `main` (they are on epic branches today) and tell anyone running
   `provide-hub` against quarto-hub.com before the flip. Redeploy the hosted hub from a q2 ref that contains the reader and the RIU work (the deployment
   repo's "Build and deploy artifacts" workflow with that `q2_ref`, then `quarto-hub-deploy` on the
   instance; Carlos's runbook, `docs/operations.md` "Updating the deployment").
2. Confirm the deployed version: `cat /etc/quarto-hub/version.json` shows a `main-YYYYMMDD-HHMMSS`
   later than the reader's landing. (`q2_sha` there is the ref name, not a commit; a known defect in
   that repo.)
3. Run `q2 provide-hub --records positron` (gate on) against it and check a viewer sees the output,
   including what a guest on an older client sees (S3 observed a full-page Render Error, not a fallback).
4. Then flip the `provide-hub` default and update the release runbook's "Engine-record writer gate"
   section per surface (preview flips with the reader; `provide-hub` flips after this redeploy). The
   runbook text also carries the rollback hazard (`quarto-hub-rollback` to a build without the reader
   strands record documents already written). Open question for that day: `--server` or
   `$QUARTO_HUB_SERVER` can aim `provide-hub` at any hub, whose version is not visible, so a default flip
   also changes behavior for self-hosted or older hubs.

Until then a `provide-hub` of a record with no legacy form is refused with the variable named
(decision 9).

## Direction: render from a Positron record

Not in this epic (preview only is enough for the demo), but the design must not rule it out. The use
case: the user has run every cell in Positron, and `q2 render` builds the document from that cache
without executing. Partial execution is not possible in Positron, so the rule is complete or refuse.

- What is reusable as is: the source list, the resolver in `quarto-core` (decision 1), the conversion
  to a record, and the content-hash match of each cell. None needs engine hashes.
- What render would add later: a `--records` flag on `q2 render`, and a completeness check (every
  executable unit has a matching imported output, else an error or a fall back to the engine).
- Keep out of the way: no resolver or conversion code in `quarto-preview`, and no rule that an
  imported record can never satisfy a document.
- Coordination: KNR-5's real-binary check (K-S2) is preview-only until this follow-up exists.

## Coordination with the knitr epic

- **KNR-5** owns widening the importer gate to knitr-owned `r` units and its
  `fallback: bool` argument. RIU-3 consumes it and adds no second gate.
- **Fallback units.** Until KNR-8 removes them, a knitr record may hold
  `text/x-quarto-markdown` units. RIU-5 must refuse them for Positron, and RIU-3 provenance
  has to survive them.
- **Records for R.** knitr fallback records are format-specific and a second render can
  overwrite the first. That affects `record: true` files, not the cache.
- **KNR-6** moves knitr consumers onto structured records. RIU-2 record-only cache entry is a
  prerequisite it can use; if the two collide on `capture_driver.rs`, the integration branches
  rebase, with no coupling in the plans.
- R through knitr is not in the first demo, and KNR-5's K-S2 cases (the Ark R document) stay KNR-5's; the
  RIU acceptance cases are Python and Julia.

## Exit criteria

- With the writer default on, `q2 preview` and hub work from records alone, verified through
  the real binary and a browser (state plainly if any step could not be browser-tested).
- A Positron cache supplies a document outputs to preview with no execution, for Python and Julia.
- Spliced record output is stamped, with the tests in RIU-3 Phase E green.
- The RIU-6 demo exists, or the notes say what blocked it.
- Plan checklists are reconciled with what landed; RIU-6 Phase E owns this step.

## Risks

- **The flip is a one-way door for shared hub stores** (parent decision 4). RIU-1 states what a
  rollback leaves behind before the flip, not after, and the `provide-hub` flip waits for the
  hosted hub.
- **A latent defect in a path that has only run in tests.** S1 found one (D1, fixed); the real
  binary and a browser stay the standard for every plan here.
- **Transforms keyed on an `Original` source range.** Spike P found one (code-block decoration) only
  because a parity test failed. Any new stamping or walk needs the full suites, not the unit tests alone.
- **Records in the cache do not outlive a session by default**, as legacy captures do not.
  A user who wants that passes `--data-dir`, or sets `record: true` and reads the copy back
  with the `q2` source.
- **Positron cache format can change** under us. The importer already rejects a
  `quarto_cache_version` other than 1, and the conformance fixtures from EXR-16 stay the check.
- **Positron-side work needs Gordon at the window**, and the dev build may be slow.
- **The `q2` found by a Positron command may be too old to read records.** RIU-6 treats a usage
  error as "too old".
- **Browser verification may not be possible in an agent session.** The plans say which steps
  need a human or a browser tool.

## References

- [RIU-0](2026-11-02-riu-00-spikes.md), [RIU-0b](2026-11-03-riu-0b-julia-and-provenance-spikes.md) and
  their findings
- [Execution-records epic](2026-10-28-execution-records-epic.md), plans EXR-13 to EXR-16
- [Knitr execution-records epic](2026-10-30-knitr-execution-records-epic.md), KNR-5, KNR-6
- `claude-notes/designs/provenance-contract.md`
- `claude-notes/research/2026-11-02-exr-16-s2b-s6-seed-and-demo-findings.md`,
  `claude-notes/research/2026-11-02-exr-16-s3-inline-exec-findings.md`
- `claude-notes/instructions/release-runbook.md` ("Engine-record writer gate")
