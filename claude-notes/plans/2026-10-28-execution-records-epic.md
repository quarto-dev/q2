---
title: 'Epic: engine execution records as ipynb (and execution of inline expressions)'
date: 2026-10-28
description: 'Replaces q2''s three execution-result caching mechanisms with one ipynb-shaped engine execution record that every engine produces and one presenter consumes, delivers execution of inline expressions for Jupyter on the way, and subsumes the Jupyter owned-languages epic.'
status: in-progress  # JOL-1, JOL-2a, JOL-2b, JOL-4, JOL-5 and EXR-1 to EXR-16 and EXR-18 landed (JOL-3 deferred; EXR-17 trigger-gated); widget outputs and HTML dependencies are future development in the widget epic; see each plan for status
---

**Branch:** `feature/execution-records-epic`
**Subsumes (executes as part of this epic, stays separately reviewable):**
[the Jupyter owned-languages epic](2026-10-17-jupyter-owned-languages-epic.md) and
[Plan 4d](2026-07-06-plan4d-owned-languages.md).
**Origin:** the `ipynb-result-caching` design session (2026-10-27/28) and the review of
2026-10-29; the distilled handoff is `~/docs/claude/ipynb-result-caching/HANDOFF.md`.
**Terminology:** say "execution of inline expressions" (IEE), never "inline
execution" (that means something else in Positron). One engine's output for one
document run is an **engine execution record**; the merged one-per-document
file is the **coalesced record**.

## Goal

Make the record the interface between execution and presentation:

```text
units  -->  engine  -->  outputs per unit  -->  core presenter  -->  markdown
```

- A **unit** is a code cell or an inline expression span, in document order.
- Every engine (Jupyter, knitr, Julia, marimo) produces records. One presenter
  (Rust) turns a record into markdown, live and on replay, natively and in WASM.
- The record is an ipynb (nbformat 4.5), so single-cell execution, Positron
  interop and (later) freeze have a natural carrier.
- It replaces today's execution-result caching: preview capture and splice, the
  stored-ipynb replay engine, and the payload of the preview capture cache
  (`quarto-preview/src/cache.rs`, which stays keyed by a hash of the input qmd).
  Freeze does not exist yet. `ReplayEngine` and the trace file's `engine_capture`
  block are kept and also carry the record (settled decision 9).
- Match q1 *behavior*, not q1's transitory `quarto_ipynb` format.
- Execution of inline expressions for Jupyter falls out of the unit model and
  is delivered early (EXR-3).

## Settled decisions

All made with Gordon in the design sessions of 2026-10-27/28 and the reviews of
2026-10-29 and 2026-10-30.

1. **Freeze is out of scope.** We land groundwork only: the execution-input
   sequence is recorded and hashable; provenance is recorded but never used for
   invalidation; the record is self-contained (includes, supporting files,
   filters, `preserve`, `needs_postprocess`, HTML dependencies live in
   notebook-level metadata); presenting a record never needs the engine;
   rendered units are tagged with their output format; serialization is
   deterministic. Prefix reuse is dropped: reuse of a record is whole-document
   or nothing, because skipped cells cannot be re-created in kernel state.
   Freeze, when it comes, is a source-keyed, environment-independent consumer.
   It also reverses or amends `2026-05-27-multi-engine-execution.md:349`
   ("freeze reuses the trace, roughly `engine: replay`"); Carlos Scheidegger's
   2026-08-14 comments on bd-mx5x609r point at an ipynb-based format instead,
   and this epic picks that.
2. **Additive migration.** Presenter extraction, then the record format, then
   engines emit records alongside the unchanged live path with a shadow
   equivalence test, then a deliberate flip, then the consumers move. The
   Jupyter flip and the consumer moves are in this epic. Legacy removal waits
   for its trigger (EXR-17).
3. **Location and topology.** q2's record is its own file, never shared with
   Positron (the reasons are in EXR-4). Default location
   `.quarto/execution-records/<doc path without extension>.ipynb`
   (project-relative, forward slashes; standalone documents get a `.quarto/`
   beside the document), laid out so that a later `_freeze/` copy is a plain
   file copy. One file per document with no format in the path; rendered units
   carry the output format in their output metadata. Figures stay inline as
   base64 for now (sidecars deferred until bd-sr73 is measured); supporting
   files go in a notebook-level files map keyed doc-relative. Deterministic
   bytes (stable key order, no volatile fields, no absolute paths, LF). Two
   version keys: q2's `quarto_record_version`, and Positron's
   `quarto_cache_version: 1` only while the schema subset really is compatible.
   Matching metadata uses Positron's keys and content hash (`quarto_content_hash`,
   `quarto_cell_id`, `quarto_label`; the id format is shared, decision 13) with q2's
   execution-input hash (`quarto_exec_hash`) and `quarto_engine` as extra keys. Plain `q2 render` writes no record by
   default; preview, hub and the `execute: {record: true}` opt-in do (decision 24).
4. **Hub transition.** The capture payload is the same ipynb (gzipped). Same
   binary-doc shape and index `captures` sidecar; a new MIME
   (`application/x-engine-record+gzip`) with `meta.kind: "engine-record"`;
   Rust readers dispatch on the payload (`nbformat` present means a record).
   `classify.rs` must learn the new MIME or orphaned records leak. Immutable
   gzipped blob, not structured automerge (single writer, history bloat). One
   `CaptureRef` per path points at a legacy capture or a record, never both;
   old docs are never rewritten. **Readers ship before writers, per writer and reader pair:** a writer may flip once every reader of the store it writes can parse a record. For the local preview, whose writer and reader are one binary, that is the first release that carries the reader; for a hosted hub it is the deployed `q2_ref`. See the records-in-use epic, decision 2 (2026-11-06). The per-surface gate, its rollback and the stranding table are in [the release runbook](../instructions/release-runbook.md#engine-record-writer-gate-exr-13b-riu-1).
   Upgrade path kept open: content-addressed figure docs for single-cell runs.
5. **Engine API scope.** Julia and marimo are in. New wire fields are optional
   and versioned; the Quarto API is not published to npm (bd-ixz93hjp, retitled
   2026-10-28: the packages go to npm, not JSR) until Jupyter, Julia and knitr
   have exercised the contract. The Rust presenter is the long-term single
   presenter; Julia stays presented by the TS `toMarkdown` until a parity suite
   shows they match.
6. **knitr stores rendered chunk markdown, in this epic.** `text/x-quarto-markdown`
   as a `display_data` output plus files. Structured outputs (the old K3) are
   not part of this epic: the design here assumed they would require knitr to
   expose evaluated results before formatting, an upstream knitr contribution.
   A follow-on epic (branch `feature/knitr-execution-records-epic`, started
   2026-10-30) revisits this: it investigates capturing the evaluated pieces on
   the q2 side, presenting knitr from structured records in the Rust presenter,
   and making knitr a first-class Positron engine, with a per-chunk fallback to
   the markdown unit defined here. EXR-10, EXR-11 and EXR-12 land first, as
   written; the follow-on epic extends what they deliver.
   Consequences (for this epic): the knitr unit hash includes all chunk options
   (EXR-4 records each engine's full `hash_definition` declaration and prefixes the
   hash with its id and version, so the follow-on epic can change that rule and an
   older record never silently matches: it reads as stale); knitr records are tagged with their
   format; knitr records are not presentable by Positron (executor independence
   for R holds only when `{r}` runs through a Jupyter kernel, which the JOL epic
   permits). knitr is never flipped in this epic: its live path keeps returning
   markdown, and its records feed the consumers (EXR-13 to EXR-15) only.
7. **One epic, with the JOL epic folded in.** No B1/B2 split. The early,
   independently shippable slice is EXR-1 to EXR-3 plus JOL-1 and JOL-2a.
   Where the two epics disagree on sequencing, this one wins.
8. **Staleness uses the execution-input hash; the cache key does not.** For
   records, preview and hub staleness compares the execution-input hash (EXR-4),
   computed from the units of the masked pre-engine pipeline output (includes
   expanded, metadata merged; units scanned once in the stage, decision 38) under
   each engine's declared hash rule (decision 35), never from raw file bytes. Legacy payloads keep
   the byte compare (`capture_driver.rs:248-283`). The preview capture cache
   stays keyed by `sha256(input_qmd)` until freeze: a hit there skips
   execution, which is reuse. Staleness is advisory and drives only a
   user-edit-triggered re-execution in preview; nothing in this epic skips
   execution on a record's staleness. (The records-in-use epic later adds reuse of a record file, bounded by a
   staleness check at choice; its decision 3.) Intended, user-visible consequence: a prose-only edit no
   longer flips staleness or auto re-executes a record (it also fixes the second
   symptom of bd-n1vmlalw). The hash is overlay-free (`provide-hub -M`, as the
   F8-D9 rewrite does today); the record also stores the overlay's
   execution-relevant keys as informational provenance, never used for
   invalidation. Plans: EXR-4 (the hash and the provenance field), EXR-15
   (consumers).
9. **`ReplayEngine` and the trace stay.** The replay engine, `--replay`, and the
   trace file's `engine_capture` block (`claude-notes/instructions/replay-engine.md`)
   are kept; `EngineCapture` stays in `quarto-trace`. Trace recording stores the
   record through an explicit trace-only field, `engine_capture.record` (a new
   optional field on `EngineCapture`; EXR-5 keeps `serde(skip)` on
   `ExecuteResult.record` so legacy captures do not grow). After the Jupyter
   flip, replay returns the stored record and the Rust presenter runs again;
   engines still producing legacy markdown keep the markdown replay path; a
   Jupyter trace recorded before EXR-5 (no record field) fails with a "re-record
   this trace" error. The trace therefore snapshots what the engine produced, not what the
   user saw; no presented-markdown snapshot and no presenter-version field.
   Plans: EXR-5, EXR-6, EXR-17.
10. **Legacy captures degrade, on no schedule.** After legacy removal an old
    capture is read as "no capture" (cells as source, one logged notice); there
    is no converter, since a legacy capture holds one markdown blob and cannot
    become per-unit outputs. The window before removal is open and undated; the
    owner decides once the trigger in EXR-17 is met. The code is kept easy to
    delete: one `Legacy` arm of `CapturePayload` is the only caller of the legacy
    functions, `engine_cell_lang` moves out of `capture_splice.rs` early (EXR-2),
    new code never imports that module, legacy-only tests share a name prefix,
    and the arm's degrade behavior exists behind a flag from the start. Plans:
    EXR-2, EXR-13, EXR-14, EXR-15, EXR-17.
11. **Span-only documents are not routed by their spans.** A document with inline
    expressions and no executable cells still resolves to the markdown engine, as
    in q1: the resolver walks code blocks only. The existing route is an
    engine-selecting front matter key (`jupyter: python3`, `engine: jupyter`),
    which JOL-2a makes work for the text path (decision 42). Whether to change this is the
    question strand bd-my5fqvok, outside this epic. Consequently the resolver and
    JOL-1's present-in-render set never need the span scanner.
12. **Engine changes land on Gordon's forks; there are no external release
    gates.** The Julia engine (`gordonwoodhull/quarto-julia-engine`, branch
    `q2/node-port`, already a vendored subtree), QuartoNotebookRunner
    (`gordonwoodhull/QuartoNotebookRunner.jl`, branch
    `feat/quarto2-handled-languages`: upstream v0.18.6 plus the cede commit) and
    marimo (`gordonwoodhull/quarto-marimo`, branch `q2/node-port`) are all
    consumed from the forks by commit. Upstreaming is a follow-up that belongs to
    no plan. The Positron work (EXR-16) is a series of spikes in Gordon\'s fork of
    Positron (decision 14) and blocks nothing, and the upstream knitr idea stays
    out of scope. Plans: EXR-8, EXR-9.
13. **One language token, and ids in Positron's format.** The language token is
    `[A-Za-z][A-Za-z0-9_.-]*`, compared case-insensitively, with an optional
    attribute tail; pampa is the authority and `python.marimo` is one token while
    `{python .marimo}` is `python` plus a class (EXR-2). Cell ids use Positron's
    format, `{index}-{hash8}-{label|unlabeled}`, and the same content hash; the
    label segment and the index can differ, and matching uses the hash (decision
    39). The index counts every braced non-raw fence; span ids use their own
    counter over all spans, `{n}-{hash8}-inline` (EXR-4).
14. **Positron interop is import-and-conformance, never a shared file.** The
    importer is display-trust only, normalizes Positron outputs at read, and only
    imports into units whose owner is Jupyter and whose language token is a plain
    kernel language (Positron\'s cache records no kernel language, so the gate
    checks the q2 side). The Positron-facing items are spikes, not proposals:
    experiments in Gordon\'s fork `gordonwoodhull/positron` (created 2026-10-30)
    that test the shared fixtures, Positron reading q2 records and execution of
    inline expressions, and end in a demo if they go well. Nothing is sent to
    anyone (EXR-16).
15. **JOL-3 (several kernels per document) is deferred, not removed.** A
    multiplexing kernel such as SoS, selected with the `jupyter:` key (JOL-2a),
    covers mixed-language documents, so a q2-managed set of kernels is not needed
    now. The plan file and its design questions stay as the starting point for a
    reopen if SoS proves insufficient or another multiplexing need appears. The
    `kernel_for` seam (EXR-2) and the per-cell `quarto_kernel` record metadata stay.
    The SoS route is not yet verified through q2 (JOL-2a is not built; the earlier
    success used a shim kernelspec), and plain `{ggsql}`-style fences on SoS may need
    q2 to send SoS\'s `cell_kernel` in the execute request, an open item in JOL-2a
    and JOL-5. Plans: JOL-2a, JOL-3, JOL-5.
16. **QNR ownership stays a negative list.** QuartoNotebookRunner gets no positive
    `ownedLanguages` option and its `toplevel_language` is not generalized:
    inline-span evaluation starts honoring the existing `handledLanguages` list, the
    new `emitUnits` option stays, and the Julia engine keeps passing
    `handledLanguages`. QNR still runs only top-level `{julia}`, `{python}` and `{r}`
    cells (upstream and q1 behavior), so units QNR does not run, such as nested
    cells, produce a count mismatch: no record, a diagnostic, and the render
    succeeds. The positive `ownedLanguages` set stays on the TS wire for engines
    that need it, such as marimo. Plans: EXR-7, EXR-8.
17. **Value rules for inline expressions (EXR-3).** Statements and errors are
    fatal and `error: true` is ignored for spans, as in q1. `None` renders as the
    text `None`. An empty-string value keeps q1 parity: the span source stays in
    place (strand bd-30k2j00f asks whether it should render nothing instead). A
    span with no usable result at all, meaning no value or no text mime type, keeps
    its source and emits a diagnostic with file and line: never silent (q1 is
    silent) and never a failure. Plans: EXR-3, EXR-18.
18. **The hash and the ids are specified (EXR-4).** Each engine declares its hash
    rule as data (decision 35); the document-level execution-input hash reads the
    merged pre-engine metadata under that declaration: `engine`, the engine\'s
    declared top-level keys, `execute-dir`, `params`, and the `execute:` map minus
    the engine\'s declared presentation keys, so unknown keys count. The
    `quarto_content_hash` hashes the author\'s source text between the fences,
    including an authored `#| label:` line, to match Positron; the stage computes
    it from the original text range, because crossref sugaring strips label and
    caption lines from the engine-visible text (decision 36). `quarto_exec_hash`
    uses the canonical text plus the options. Positron\'s SHA-1 encodes UTF-8; the
    test vectors live in EXR-4 and spike S1 (EXR-16) runs them against Positron\'s
    own implementation. Execution options are flat cell keys. Plans: EXR-4, EXR-16.
19. **Wire and count-mismatch contract (EXR-7).** In v1 the wire units carry `id`,
    `kind`, `language`, `line`, `owned` and `inFrontMatter` (cells also carry `index`), with no byte or UTF-16
    offsets (engines use lines; the existing source map counts UTF-8 bytes while the
    TS side slices UTF-16). The extension documentation page waits until the
    contract has been exercised. While a record is additive, a count mismatch
    between the units core sent and the units an engine ran means no record plus a
    diagnostic, and the render succeeds, for Julia and marimo alike; each engine\'s
    flip task decides whether a mismatch may fail the render later. Plans: EXR-7,
    EXR-8, EXR-9.
20. **Julia details (EXR-8).** QNR cell expansions in record mode produce no
    record and a diagnostic until the Julia flip, which decides how to attach
    per-expansion options; a conformance corpus of tricky spans compares QNR with the
    scanner (there is no `inFence` flag: decision 41 never claims a span in a fence). Plan: EXR-8.
21. **knitr inline ids (EXR-12).** Keep the leading-statement id in both
    spellings, but record through knitr\'s `evaluate.inline` hook and not through a
    wrapper on the `inline` hook, which knitr never calls for a `NULL` or invisible
    value. An id left unserved is recorded as empty at the next call or at the end;
    there is no fallback to a bare counter. Plan: EXR-12.
22. **knitr cell location (EXR-10).** Cells are found by wrapper Div id, with a
    nesting corpus as a Phase 0 test and an ordinal from the shared scanner as the
    stated fallback. Plans: EXR-10, EXR-11.
23. **Jupyter kernel precedence and a corrected q1 claim (JOL-2a).** The
    `jupyter:` document key wins over `engine: [{jupyter: {kernel: ...}}]`, then
    language lookup. q1 does not send every cell to the one kernel: it builds its
    code-cell regex from the kernelspec language (`jupyter.ts:321-325`), so other
    languages stay display-only. Verified by running q1 with `jupyter: sos`: a
    `{python}` fence stays display-only and only a `{sos}` fence ran. Sending every
    Jupyter-owned cell to the kernel named by `jupyter:` is therefore q2 design
    beyond q1 parity, and JOL-2a is a behavior change for q1 documents. Plans:
    JOL-2a, JOL-2b.
24. **Record opt-in is a metadata key (EXR-5, EXR-13).** A plain `q2 render` writes
    a record only when the merged metadata has `execute: {record: true}`, following
    q1\'s `execute: freeze:` precedent; `q2 render -M 'execute:{record:true}'` is the
    command-line form and there is no dedicated flag in v1. Core always
    excludes `record` from the document-level hash, for every engine (decision 35),
    so toggling it never changes the execution-input hash or the overlay provenance. The hub writer gate of EXR-13 is
    a separate, release-sequencing switch. Plans: EXR-4, EXR-5, EXR-13.
25. **The record emits nbformat 4.5 cell ids (EXR-4).** Each cell gets a top-level
    `id` derived from its content hash (the 16-hex value, with `-2`, `-3` appended
    to later duplicates; the spec allows 1 to 64 characters of letters, digits, `-`
    and `_`), never the index-bearing `quarto_cell_id`, so the file stays a valid
    minor-5 notebook for Jupyter tools and freeze. Positron ignores the field. Plan:
    EXR-4.
26. **The executor is unchanged in this epic; the scanner records the
    differences (EXR-2).** The scanner returns indented, nested and attribute-tail
    every braced cell (each engine\'s view marks the ones it owns), but the Jupyter executor keeps ignoring what it
    ignores today. Phase 0 pins that behavior and the divergence list names each
    difference with its strand: list-nested cells run in q1 and not in q2
    (bd-9j6z311f), attribute-tail cells such as `{python .foo}` (bd-rh4cce9e),
    capitalized `{Python}` runs in q2 and not q1 (bd-7gvl455a). A fence indented four
    spaces (Q-2-59) and knitr-style options in the braces (Q-2-36) are deliberate q2
    parse errors where q1 runs the cell. Plan: EXR-2.
    **Amended 2026-10-31 (Gordon): two executor misparses are fixed, in EXR-2 Phase 4,
    not emulated.** The unchanged-executor rule covers cells the executor *chooses* not
    to run (the three strands above), which are policy questions. It does not cover
    two plain boundary bugs with no strand: a closing fence followed by a `{python}` line
    starts a cell because `\s*` between the fence and the brace spans a newline (quirk
    (b)), and a four-backtick line fails to close a three-backtick cell (quirk (h)). When
    `execute_qmd` and `IpynbReplayEngine` switch to the scanner in Phase 4 they take
    the CommonMark behavior for both, with named tests, and `runs_in_legacy_executor`
    stops encoding them.
27. **The record carries no presenter inputs (EXR-5, EXR-6).** The execute scope, the
    document source and the output format are pipeline context at present time, so a
    record stays format-neutral and a change to `echo` or the output format does not
    invalidate it. Replay supplies them again; if `ExecutionContext` does not reach
    `ReplayEngine::execute`, the missing inputs go in the trace, never the record.
    Plans: EXR-5, EXR-6.
28. **The Phase 0 goldens are kept after EXR-17 (EXR-6).** They move under the
    presenter\'s tests and are regenerated only by one documented command; they are
    the only record of the pre-record output format. Plan: EXR-6.
29. **The no-usable-result warning is a registered code (EXR-3).** It is a new
    engine-subsystem code, `Q-18-10` (amended 2026-10-31: JOL-2a took `Q-18-4`, and
    `Q-18-5` to `Q-18-9` are claimed by other plans), with a catalog entry, a docs page
    (`docs/errors/engine/Q-18-10.qmd`) and the same corpus and tests as the existing
    codes. Plan: EXR-3.
30. **Positron spike branches and the demo (EXR-16).** Branches are named
    `spike/q2-<id>-<topic>` in Gordon\'s fork, and the S6 demo ships a short screen
    recording plus a script. Plan: EXR-16.
31. **The presenter lives in a top-level `presenter/` module (EXR-1).** It is a
    sibling of `engine/`, because it serves every engine and the preview and keeps
    its WASM-clean surface explicit; `engine/` itself is not WASM-gated as a whole.
    Plan: EXR-1.
32. **How users receive the engine forks is outside this epic.** q2 consumes the
    forks by commit hash and its tests pin the fixtures, so no release is needed
    (decision 12); distribution and upstreaming are Gordon\'s separate work, and no
    plan tracks them. Plans: EXR-8, EXR-9.
33. **EXR-5 writes the record file for a plain render (decision 24).** When the merged
    metadata has `execute: {record: true}`, EXR-5 writes the coalesced record to the
    decision 3 path with deterministic bytes; EXR-13\'s gate covers only the hub
    writers. Plan: EXR-5.
34. **Matching uses the execution hash first (EXR-14, EXR-15, EXR-4).** Preview and
    hub splice match record cells to document cells by `quarto_exec_hash` first;
    `quarto_content_hash` (Positron-compatible) is the fallback and the only key for
    imported Positron records, which have no exec hash. Duplicates tiebreak by
    occurrence: the k-th document occurrence of hash H takes the k-th record cell with H, in
    record order (the index inside the id is not consulted). Matched outputs are presented with the
    current options and the current document scope layered over whatever options the
    record stores, so an `echo` edit neither re-executes nor strands the cell (a
    current header option wins over the record\'s value). Jupyter\'s record stores no
    presentation options, so the current ones apply as is; knitr\'s resolved options,
    which the knitr follow-on epic adds to its records, supply setup-chunk state such
    as `opts_chunk$set`. The exact layering is left to EXR-14 and that epic; whether
    the current options reach the splice presenter is an explicit EXR-14 task. A cell
    is `stale` only when no hash matches. Plans: EXR-4, EXR-14, EXR-15.
35. **The hash rule is declared by the engine, as data; core owns the algorithm.**
    No engine names appear in core hashing code. A `HashDefinition` (`id`, `version`,
    `granularity`, `presentation_cell_options`, `presentation_execute_keys`,
    `document_keys`) is defined by EXR-4; Rust engines supply it through
    `ExecutionEngine::hash_definition` (default: empty exclusion lists, so everything
    counts). The ordered unit hashes of an engine\'s units enter that engine\'s document hash for every engine; `granularity: Document` differs only in whether units are individually matchable. The unit hash holds no kernel (`quarto_kernel` is provenance only), TS engines through the optional `recordInfo.hashDefinition` (EXR-7). Jupyter
    declares its presentation allowlist in its own module; knitr declares empty lists
    and `document_keys: ["knitr"]`. Core always excludes `record` at document level.
    The record stores the full declaration in `quarto_engines[].hash_definition`, so a
    consumer with no engine loaded can recompute; each engine\'s document-level hash is
    stored in its entry and `quarto_execution_hash` hashes the ordered per-engine
    values. A declaration may also list named **profiles**, alternative cell-level
    rules, and a unit may name one in its cell metadata (`quarto_hash_rule`; absent
    means the declaration\'s default rule). This lets one engine\'s record hold units
    with different rules, as the knitr follow-on epic needs (structured units leave
    presentation options out of the hash, fallback units holding baked markdown count
    every option); an engine\'s document hash excludes only the `execute:` keys that
    every profile used in its record excludes. This epic\'s engines declare no
    profiles. A unit naming a profile the stored declaration lacks reads as stale. A
    rule that cannot be declared as data is a finding, not code to add in core. Open: whether a flat declaration is enough for Julia and marimo (EXR-8,
    EXR-9). Accepted: `.env`, data files and format-specific defaults merged after
    `ast.meta` stay outside the hash. Plans: EXR-4, EXR-7, EXR-8, EXR-9, EXR-11.
36. **Author-text hashes are computed in the engine-execution stage (EXR-4, EXR-5,
    EXR-14).** Crossref sugaring strips `#| label:` and caption lines from the
    engine-visible text, so the stage computes each unit\'s author-text hashes from the
    original text range (or the pre-sugaring AST; the mechanism is EXR-4\'s first
    task), fills the ids, and passes the record builder a total unit-to-author-text
    mapping (`AuthorTexts`, one entry per unit, each with its text and content hash). The
    execution hash is computed per engine from the same text, because it depends on that
    engine\'s `HashDefinition`.
    EXR-14\'s splice hashes the same range, with a Jupyter labeled-cell test beside the
    knitr one. Plans: EXR-4, EXR-5, EXR-14.
37. **The replay-engine move is its own plan, EXR-1b (EXR-1, EXR-2).** The
    `IpynbReplayEngine` move was EXR-1\'s trailing phase, gated on EXR-2 Phase 4, because
    `stored.rs` imports from the native-only `text_execute.rs` what EXR-2 Phase 4 replaces with
    `scan_units`. That left an edge in each direction between EXR-1 and EXR-2, so it is now
    EXR-1b, which depends on EXR-2 (start gate: Phase 4). EXR-1 Phase 3 still lands before
    EXR-2 Phases 1, 4 and 5 (same hunks of `text_execute.rs` and `stored.rs`); that is a phase
    gate, not a plan-level edge, and it was met on 2026-10-31. EXR-2 Phases 0, 2 and 3 may run in
    parallel. EXR-6 depends on EXR-1b. Plans: EXR-1, EXR-1b, EXR-2, EXR-6.
38. **Units are scanned once, in the stage, with ids filled by an EXR-4 function
    (EXR-2, EXR-4, EXR-7).** The stage scans on the text entering the engine stage,
    before any engine runs, fills each id via the EXR-4 id function and passes the same
    list to every engine through `ExecutionContext.units`. Wire units keep the original
    `id` and `index` and carry that engine's own `owned` flag (decision 42); `line` is located in the text that engine receives, and
    each engine validates counts for its owned units only. EXR-2\'s `Unit` has a typed id
    slot that EXR-4 fills; the span counter runs over all spans. Plans:
    EXR-2, EXR-4, EXR-7.
39. **Ids versus Positron\'s (amends decision 13).** Same id format and same content
    hash; the label segment and the index can differ, and matching uses the hash.
    Positron\'s label is the first option inside the fence braces, so `#| label:` is
    invisible to it, and its parser has no nesting rule, so indices differ on documents
    with blockquote, list or four-backtick cells and phantom cells can appear. CRLF is
    normalized on Positron\'s side (it splits on `\r?\n`). EXR-16 S1 records each
    divergence, and EXR-16 states that Positron\'s cache lookup consumes (deletes) a file
    on one matching cell hash. Plans: EXR-4, EXR-16.
40. **The scanner reports what the legacy executor skips (EXR-2).** `CellUnit` carries
    `indent`, `container_prefix`, `fence_len` and `attr_tail`; one quarantined function,
    `runs_in_legacy_executor`, holds today\'s executor rule. It is not deleted by this epic: it retires when the strands that decide the executor\'s skipped cells (bd-9j6z311f, bd-rh4cce9e, bd-7gvl455a) are each decided. (The opener-length skip, which no strand owned, was decided by bd-f63cqoke on 2026-11-11: any opener of three or more backticks runs.)
    Phase 0 pins the executor\'s quirks, a Phase 4 test shows the scanner filtered by it
    equals `parse_code_blocks`, and the mask\'s inline pattern is widened to the shared
    token class (a deliberate mask behavior change, consistent with decision 13). Plan:
    EXR-2.

41. **Where spans are claimed (EXR-2, EXR-3; Gordon, 2026-10-31).** Spans are found in
    prose and in the document\'s own front matter only; a span inside any fenced code block is never
    claimed (revised 2026-10-31; the nested-cell mask covers every non-cell code block, so a span
    shown in a ` ```r ` or ` ```text ` fence stays displayed). A span inside a braced fence is not evaluated: q1
    does evaluate one, by accident of its notebook conversion, and q2 deliberately does not.
    A span inside an HTML comment is not evaluated (q1 does; the scanner would otherwise
    disagree with an AST walker, Risk 1). A span in the document\'s own front matter is
    evaluated, as in q1 (q1 does evaluate them; the EXR-2 and EXR-3 text that said q1 skips
    them was wrong). "Own" is decided by provenance: the value\'s `SourceInfo` root file id
    is the document\'s file (Gordon, 2026-10-31). A span that came from `_quarto.yml`,
    `_metadata.yml`, an extension or a format default stays literal, because q1 scans only the
    document\'s own front matter. Document-owned metadata spans take ids and span counter
    numbers but are never spliceable. Classic `` `r expr` `` spans are not units (they stay
    knitr\'s private spelling, EXR-2 decision 2).
    Plans: EXR-2, EXR-3, EXR-14.

42. **Ownership is applied per engine, not in the stage (EXR-2, JOL-2a, EXR-3; Gordon,
    2026-10-31).** The stage's one scan applies no ownership filter: the list holds every
    cell and every span of any language, and each engine's view marks the units it owns by
    its own rule. This replaces the stage predicate that was the union of the owned languages
    (amends decision 38 on that point only). The union would have been empty for a span-only
    document, because the owned set comes from code cells. Jupyter owns a span when its
    language is in its owned set or is the language of the kernel the `jupyter:` key selects.
    JOL-2a also makes the resolver keep an explicitly declared engine when the document has
    no code cells (today it returns an empty list first, `resolution.rs:489-503`), as Q1 does
    (verified by run, 2026-10-31), so decision 11's `jupyter:` route works. A span no engine
    owns stays literal (for Julia, core adds the languages of such spans to `handledLanguages` so
    QuartoNotebookRunner cedes them, EXR-8). The stage passes only the shared list; each
    engine builds its own view with `view_for_engine`. JOL-2a\'s declared-engine rule applies
    only to an engine declared in the document\'s own front matter (provenance as in decision
    41): q1 ignores a project-level `engine:` for engine selection (verified by run,
    2026-10-31). Plans: EXR-2, EXR-3, JOL-2a, JOL-2b.

## Engines on three axes

| Engine | Granularity | Output kind | Driver | Notes |
|---|---|---|---|---|
| Jupyter | per unit | raw nbformat | core steps cells | q2 native; `execute_blocks_inner` |
| Julia (QuartoNotebookRunner) | per unit | raw nbformat | engine runs whole doc | returns a whole notebook; spans are substituted into markdown cells today (EXR-8 annotates them); QNR runs only top-level `{julia}`, `{python}`, `{r}` cells |
| knitr | per unit | rendered | engine runs whole doc | `knit()` is all-or-nothing; Quarto's own `hooks.R` bakes presentation |
| marimo | per cell | rendered | engine runs whole doc | per-cell island HTML or static markdown; count-validated against owned cells; header include; `canFreeze: false` |

Marimo is per-cell and rendered (corrected from the handoff, which called it
document-level). The reuse policy follows the driver only for single-cell
execution (later), not for reuse of a record.

## Plans and DAG

JOL plans keep the numbering from the JOL epic. JOL-1 is Plan 4d phases 4d-A/B
and JOL-4 is Plan 4d phases 4d-C/D/E; they need no new file.

| ID | Plan | Notes |
|---|---|---|
| JOL-1 | [Plan 4d-A/B: the Rust carrier](2026-07-06-plan4d-owned-languages.md) | `owned_languages_for`, `ctx.owned_languages` |
| JOL-2a | [Jupyter honors the `jupyter:` key](2026-10-28-jol-2a-jupyter-key.md) | shippable alone |
| JOL-2b | [Ownership gate and kernelspec lookup](2026-10-28-jol-2b-ownership-gate.md) | no runnable kernel: error in multi-engine documents, warning in single-engine ones |
| JOL-3 | [Several kernels per document](2026-10-28-jol-3-multi-kernel.md) | deferred: SoS (JOL-2a) covers mixed-language documents; reopen if it does not |
| JOL-4 | [Plan 4d-C/D/E: TS wire](2026-07-06-plan4d-owned-languages.md) | |
| JOL-5 | [Docs and end-to-end for owned languages](2026-10-28-jol-5-docs-e2e.md) | |
| EXR-1 | [Presenter extraction](2026-10-28-exr-01-presenter-extraction.md) | Rust; WASM-clean; golden parity |
| EXR-1b | [Replay engine out of the native-only module](2026-10-28-exr-01b-replay-engine-move.md) | split from EXR-1; WASM stored-notebook replay; first quarto-core WASM tests |
| EXR-2 | [Shared unit scanner](2026-10-28-exr-02-unit-scanner.md) | `Unit`, `view_for_engine`, `locate_units`, shared token and span patterns; moves `engine_cell_lang` |
| EXR-3 | [Jupyter execution of inline expressions](2026-10-28-exr-03-jupyter-iee.md) | closes bd-u996g8g2 |
| EXR-4 | [The record format](2026-10-28-exr-04-record-format.md) | library; freeze groundwork |
| EXR-5 | [Jupyter emits records (shadow)](2026-10-28-exr-05-jupyter-records-shadow.md) | |
| EXR-6 | [Jupyter presents from records (the flip)](2026-10-28-exr-06-jupyter-flip.md) | |
| EXR-7 | [Engine wire and API evolution](2026-10-28-exr-07-engine-wire.md) | optional, versioned |
| EXR-8 | [Julia adoption](2026-10-28-exr-08-julia.md) | includes the QuartoNotebookRunner fork changes |
| EXR-9 | [marimo adoption](2026-10-28-exr-09-marimo.md) | changes land on the marimo fork |
| EXR-10 | [knitr K0: per-cell options map](2026-10-28-exr-10-knitr-k0-options-map.md) | existing bd-n1vmlalw |
| EXR-11 | [knitr K1: chunk records](2026-10-28-exr-11-knitr-k1-chunk-records.md) | |
| EXR-12 | [knitr K2: inline expressions](2026-10-28-exr-12-knitr-k2-inline.md) | |
| EXR-13 | [Hub and WASM: reader, MIME, writer](2026-10-28-exr-13-hub-wire.md) | two halves in one file: 13a (reader, MIME, classify) and 13b (writer, gate); readers before writers; the `Legacy` arm seam |
| EXR-14 | [Preview: id-based splice](2026-10-28-exr-14-preview-splice.md) | per-unit staleness, spans, masking |
| EXR-15 | [Hub integration](2026-10-28-exr-15-hub-integration.md) | `provide-hub`, admin, guests, bd-cp3em |
| EXR-16 | [Positron interop](2026-10-28-exr-16-positron-interop.md) | q2-side import and fixtures; spikes S0 to S6 in the Positron fork |
| EXR-17 | [Legacy removal](2026-10-28-exr-17-legacy-removal.md) | trigger-gated; no date |
| EXR-18 | [Docs and end-to-end](2026-10-28-exr-18-docs-e2e.md) | |

**Future development:** widget outputs and HTML dependencies (found by the Julia flip, EXR-8 B4)
continue in the [widget epic](2026-11-02-widget-epic.md). They are not part of this epic.

Dependencies, one row per plan. This table is the source of truth: each plan\'s
Depends on and Blocks lines must agree with it. Hard edges are direct (an edge implied
by another edge is left out); a soft edge means the plan can start or land without it
but its end-to-end checks want it.

| Plan | Hard dependencies | Soft |
|---|---|---|
| JOL-1 | none | |
| JOL-2a | none | |
| JOL-2b | JOL-1, JOL-2a | |
| JOL-3 (deferred) | JOL-2b, EXR-2 | fake kernel (EXR-3, extended by JOL-5) |
| JOL-4 | JOL-1 | |
| JOL-5 | JOL-2b, JOL-4, EXR-3 | |
| EXR-1 | none | |
| EXR-1b | EXR-1, EXR-2 (Phase 4 only) | |
| EXR-2 | none | JOL-1 |
| EXR-3 | EXR-2, JOL-1, JOL-2a (core only: see the note below) | EXR-1 |
| EXR-4 | EXR-2 (Phase 3) | EXR-1 |
| EXR-5 | EXR-1, EXR-3, EXR-4 (Phases 0 to 4) | JOL-2b |
| EXR-6 | EXR-1, EXR-1b, EXR-4, EXR-5 | |
| EXR-7 | Wire-types half (Phases 0, 1a, 2): EXR-2, EXR-4 (Phase 1). Mapping half (Phase 1b onward): EXR-4 (Phases 0 to 4), EXR-5 (Phase 2) | JOL-4 |
| EXR-8 | Part A (QNR): EXR-2 (Phase 2, the span rule). Part B: EXR-1, EXR-2, EXR-4, EXR-5 (B2: Phase 2; B4: all), EXR-6, EXR-7, JOL-4 | |
| EXR-9 | 9A and 9B: EXR-7 (wire-types half). 9C: EXR-4 (Phases 0 to 5), EXR-5 (Phase 2), EXR-7 (Phase 1b) | JOL-4 |
| EXR-10 | none | |
| EXR-11 | 11S, 11A, 11B: EXR-10. 11C: EXR-2, EXR-4 (Phases 0 to 5), EXR-7 (Phase 1b) | |
| EXR-12 | EXR-2, EXR-4, EXR-11 | |
| EXR-13a | EXR-4 (Phases 0 to 5) | EXR-1, EXR-5 |
| EXR-13b | EXR-13a, EXR-4, EXR-5 (Phase 2) | |
| EXR-14 | EXR-1, EXR-2, EXR-4, EXR-13a | EXR-6, EXR-10, EXR-12 |
| EXR-15 | EXR-4, EXR-5 (Phase 2), EXR-13b, EXR-14 | EXR-11 |
| EXR-16 | EXR-2, EXR-4 (Phases 0 to 5; the Positron spikes need nothing) | EXR-3 |
| EXR-17 | EXR-6, EXR-8, EXR-9, EXR-11, EXR-12, EXR-13a, EXR-13b, EXR-14, EXR-15 | |
| EXR-18 | EXR-3, EXR-6, EXR-8, EXR-9, EXR-12, JOL-5 | every other plan (docs are written as each lands) |

Chains worth seeing at a glance: JOL-1 and JOL-2a lead to JOL-2b, then JOL-5 (JOL-3
hangs off JOL-2b and EXR-2 but is deferred); EXR-2 and EXR-1 lead to EXR-3 and
EXR-4 (and EXR-1b hangs off EXR-2 and joins at EXR-6), then EXR-5, then EXR-6 and EXR-13b (EXR-13a needs only EXR-4, then EXR-14), then EXR-15; EXR-4 and EXR-7
lead to the Julia, marimo and knitr plans, which all end at EXR-17.

There are no external gates in the graph: the engine forks are ordinary work inside
EXR-8 and EXR-9, so lane scheduling is unaffected by anyone else\'s release.

Phase gates (re-cut 2026-10-31, Gordon). Most consumers need an early phase of a plan, not
all of it, so the table names phases where that is true. A phase-qualified entry such as `EXR-4
(Phases 0 to 4)` is met when those phases are ticked on the integration branch; the plan merges
there at the end of that phase with `status: in-progress` and continues on the same branch
(rule 5). Gates, with the phases that follow them:

- EXR-2 merges at the end of Phase 3 (Unit, its id slot, the scanner, conformance fixtures);
  Phases 4 and 5 follow. EXR-4 starts at this gate. EXR-5 and EXR-7\'s mapping half still
  need EXR-2 Phase 4 through EXR-3, which depends on all of EXR-2.
- EXR-4 merges at the end of Phase 4 (types, hashes, ids, builder, writer) and again at the
  end of Phase 5 (reader, `place`, Positron import); Phase 6 (freeze checklist) gates nothing
  and comes last. EXR-5 and EXR-7 need Phases 0 to 4. The reader half of EXR-13, EXR-16,
  9C and 11C need Phase 5.
- EXR-5 merges at the end of Phase 2 (the `record` field, `want_record`, unit ids and author
  texts in the stage, coalescing, the record handoff to the capture writers); Phases 3 (shadow
  equivalence) and 4 follow. EXR-7\'s mapping half, the writer half of EXR-13, EXR-15, 9C and
  the Julia B2 need Phase 2. EXR-6 needs all of EXR-5, because Phase 3 is its baseline.
- EXR-7 has a wire-types half (Phases 0, 1a and 2: Rust types, TypeScript mirror, wire parity)
  and a mapping half (Phase 1b onward). The wire-types half needs only EXR-4 Phase 1; it is
  what the marimo engine side (9A, 9B) needs. EXR-7 merges at the end of Phase 2 for that
  gate, and the mapping half follows once EXR-5 Phase 2 has landed.
- EXR-13 is one file with two halves. 13a (Phases A, B, C: reader, MIME, classify) lands when
  they are ticked; 13b (Phases W1, W2, D: writer, gate, release sequence) lands when those
  are ticked. They share one `status:` line, as JOL-1 and JOL-4 do.
- EXR-8 Part A (QNR) has no q2 dependency beyond the span rule; EXR-9 Phases 9A and 9B and
  EXR-11 Phases 11S, 11A and 11B can run before the gates above land. The plans say which
  tasks wait for what.

Sequencing notes:

- JOL-1 and JOL-2a land first (small, independent). EXR-2 owns the `Unit` loop and
  the `kernel_for(unit)` seam (the `KernelRouter`, with today\'s first-cell behavior as
  the default); JOL-2a and JOL-2b fill the seam in (a reopened JOL-3 would vary it
  per unit) and EXR-2 never waits on them, so JOL-2b can land before or after EXR-2. EXR-2 also moves
  `engine_cell_lang` out of `capture_splice.rs` (its callers, the resolver and the
  nested-cell mask, are in EXR-2\'s scope). EXR-3 builds the base fake kernel
  (kernel-free tests of the unit loop); JOL-5 extends it for docs and end-to-end tests
  and EXR-5 reuses it. Both epics edit `execute_blocks_inner` and `ExecuteOptions`;
  reconcile the `owned_languages` and `units` fields in one pass (EXR-7 and JOL-4).
- EXR-1 also moves `IpynbReplayEngine` (Plan 7c, complete) out of the native-only
  module, but as a trailing task gated on EXR-2 Phase 4 (decision 37): `stored.rs`
  imports from `text_execute.rs` what that phase replaces with `scan_units`. That is
  the replay half of bd-9zmpzw3w; the strand stays open for its other half, the
  `.ipynb` file claim on `wasm32` (out of scope: this epic uses `.ipynb` as a record,
  not as an input format).
- Phase-order gate (the plan-level table is unchanged, because both lanes start now):
  EXR-1 Phase 3 lands before EXR-2 Phases 1, 4 and 5, since both edit the same hunks of
  `text_execute.rs` and `stored.rs`. EXR-2 Phases 0, 2 and 3 do not collide and may run
  in parallel with EXR-1.
- bd-6sb1z0i4 (label naming for Jupyter) must follow EXR-10 (bd-n1vmlalw).
- bd-5t6wvu7m (image outputs rendered as placeholders) is already closed
  (PR #412, 2026-07-24; `FigureWriter` writes real figures), so EXR-1 only
  keeps a fail-soft fallback for undecodable payloads.
- **Readers before writers.** EXR-13 ships in one release; record writing is
  enabled in the next or behind a flag.

## Running the plans in four lanes

There is no orchestrator. A session starts in one of four worker worktrees, reads this
section, picks its own work, merges back to the integration branch, and ends by printing a
handoff to the console. Nothing about progress lives in files outside the plans: the state
is git (branches, commits) plus each plan\'s checkboxes and `status:` line.

> **A plan is not finished until it has merged to the integration branch.** When the last
> item of a plan is ticked and its gates pass, the next action is rule 5 (rebase, re-run the
> gates, `merge --ff-only` into workspace-1), before the handoff and before starting the
> next plan. Merge earlier than that only when another plan\'s phase gate needs it (rule 5).
> A handoff that says `merged to integration: no` for a completed plan is a bug in the
> session, not a status.

**Integration:** branch `feature/execution-records-epic`, checked out in
`/Users/gordon/src/q2/.worktrees/workspace-1`. Nobody works in that worktree except to merge
into it and to edit this epic.

| Lane | Worktree | Queue, in order (a plan starts when its hard dependencies have landed) |
|---|---|---|
| A: Jupyter spine, then knitr inline and Positron | `workspace-4` | Done: EXR-2, EXR-3, EXR-5, EXR-6. Next, re-assigned by Gordon, 2026-11-01: EXR-12 (knitr K2 inline; its gate, EXR-11, landed at `d0112a569`, so branch from that or later), then EXR-16 (the Positron spikes need nothing; its q2-side half needs EXR-4 Phases 0 to 5, landed) |
| B: ownership and the record, then marimo | `workspace-5` | Done: JOL-1, JOL-2a, EXR-4, JOL-4, EXR-7, JOL-5. Open: JOL-2b (implemented; waits only on its `cargo xtask verify` gate). Next, re-assigned by Gordon, 2026-11-01: EXR-9 (9A, 9B and 9C; every gate has landed) |
| C: engines | `workspace-6` | Done: EXR-10, EXR-11. Now: EXR-8, Part A (QNR) first, then Part B (every hard dependency has landed). EXR-9, EXR-12 and EXR-16 moved to lanes B and A on 2026-11-01 |
| D: readers, preview, docs | `workspace-7` | EXR-1, EXR-1b (when EXR-2 Phase 4 has landed; it does not block the rest of the queue), EXR-13a, EXR-14, EXR-13b, EXR-15, EXR-18, EXR-17 (only when its trigger holds and Gordon says go) |

JOL-1 and JOL-4 live in `2026-07-06-plan4d-owned-languages.md` (Phases 4d-A/B and 4d-C/D/E).
JOL-3 is deferred and is in no lane. The knitr follow-on epic (workspace-3,
`feature/knitr-execution-records-epic`) is a separate stream, not a lane: do not touch its
branch, and leave its sessions to rebase themselves.

The critical path is EXR-2 (to Phase 3), then EXR-4 (to Phase 4), then EXR-5 (to Phase 2), then
EXR-7\'s mapping half and EXR-13b, and EXR-6 after the rest of EXR-5. After the 2026-10-31 re-cut,
lane C has EXR-11 (S, A, B) and EXR-8 Part A to do before the gates land,
and lane D has EXR-13a and EXR-14 once EXR-4 Phase 5 has landed; expect some idle time anyway; an idle lane says so in its handoff and stops (rule 6).
Session counts in the plan headers are estimates, and the lanes carry roughly 12, 14, 16 and
17 sessions of work.

**Definitions.**
- **JOL-2a exception (Gordon, 2026-10-31):** JOL-2a merges to the integration branch with
  `status: in-progress` once its core is done (resolver rule, `select_kernel`, engine-config
  source; all done on `exr/JOL-1`). Its unticked tail (SoS recorded run, `cell_kernel` research,
  cell-less behavior tests, profile-version bump, committed own-file integration test) must not
  hold EXR-3. For EXR-3 only, JOL-2a counts as landed at that merge; for everything else the
  rule below applies.
- A plan has **landed** when its front matter says `status: complete` on the integration
  branch. `status: draft` means not started; `status: in-progress` means the plan has begun but not
  finished: either it merged early for a phase gate, or a sibling plan sharing its file
  is still open. JOL-1 and JOL-4 share one file and one `status:` line, so each lands when its
  own phases (4d-A/B for JOL-1, 4d-C/D/E for JOL-4) are ticked on the integration branch.
- A hard dependency is **met** when the plan it names has landed. A phase gate (for example
  decision 37: EXR-1 Phase 3 before EXR-2 Phases 1, 4 and 5; EXR-2 Phase 4 before EXR-1
  Phase 5) is met when that phase\'s checkboxes are ticked on the integration branch. A table entry
  with phases, such as `EXR-4 (Phases 0 to 4)`, is a phase gate of the same kind. Check
  the dependency table in this epic, not memory.
- The lane is the worktree directory name: if `pwd` ends in `workspace-5`, you are lane B.

**What a session does.**
1. **Orient.** Confirm the lane. Run `git status`; the worktree must be clean. Run `braid
   list` before anything that could become a strand (strands are only for work outside every
   plan; plan work goes in the plan checklist). Look for `external-sources/` in the main repo
   (`/Users/gordon/src/q2/external-sources/`), not the worktree, and cite it as
   `external-sources/<name>/...`. If `braid` or `br` reports a missing redirect, the worktree\'s
   `.beads/redirect` must hold an absolute path to the main repo\'s `.beads/`.
2. **Pick the plan.** First, resume: if a branch `exr/<PLAN>` exists for a plan in your queue
   whose status is not `complete`, resume it (`git switch exr/<PLAN>`). Otherwise take the
   first plan in your queue, honoring the priority notes in the table, whose hard
   dependencies are all met on the integration branch. Read the plan and what it links. If
   no plan qualifies, go to rule 6.
3. **Start from the integration tip.** New plan: `git switch -c exr/<PLAN>
   feature/execution-records-epic`. Resumed plan: `git rebase feature/execution-records-epic`
   first, and re-run the plan\'s last completed gate.
4. **Do the checklist in order, tests first.** After each item: tick its box in the plan file
   in the same commit as the work. Per crate you touched, run `cargo clippy -p <crate>
   --all-targets -- -D warnings` and `cargo nextest run -p <crate>`. Do not run the workspace
   suite unprompted; recommend a wider run in the handoff. When an item settles a fact
   marked UNVERIFIED, replace the marker with the verified fact and its `file:line` in the
   same commit. If an item needs a decision that is not already settled in this epic, do
   not decide it: write it in the handoff, skip that item, and continue with the rest.
5. **Merge back when the plan is complete, or sooner when a dependency needs it.** This is
   the step that makes the work visible to the other lanes; do not skip it and do not defer
   it to the next session. Merge when:
   - **the plan\'s checklist is fully ticked** (the normal case; a plan\'s phases do not
     each merge), or
   - **another plan is waiting on a phase of this one**: the dependency table or a phase
     gate (for example decision 37: EXR-1 Phase 3 before EXR-2 Phases 1, 4 and 5; EXR-2
     Phase 4 before EXR-1 Phase 5) names a phase of your plan as the condition. Merge at the
     end of that phase, then continue on the same branch (rebase first, rule 3).

   Integration must stay green.
   - Set the plan\'s `status:` to `complete` in the merging commit when the plan is done, or
     to `in-progress` when you merge early for a phase gate or a sibling plan in the same
     file (JOL-1, JOL-4) is still open.
   - Render every plan file you edited: `q2 render --strict <files>` (the 32 Q-12-13 errors
     are unrelated; none may name your files).
   - `git rebase feature/execution-records-epic`, then re-run clippy and nextest for the
     crates touched.
   - `git -C /Users/gordon/src/q2/.worktrees/workspace-1 status --porcelain
     --untracked-files=no` must print nothing; if it does, stop and say so in the handoff.
   - `git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only exr/<PLAN>`. The
     fast-forward is the lock: if it fails because another lane merged first, rebase again,
     re-run the gates, and retry. Never force, never push, never merge into integration by
     any other means.
   - If the merged phase is a gate that another plan\'s table row names, **notify the
     lanes it unblocks**: find each lane\'s agent as in "Reaching other lanes" below and
     send the plan, the phase, the merge sha and one line on what is now startable. Skip
     your own lane. A lane you cannot find is not an error; name it in the handoff.
6. **Stalled or finished.** If nothing in your queue is startable, or the next item needs a
   decision or another lane\'s work, stop. Do not borrow another lane\'s plan, and do not
   start one out of order. Print the handoff naming exactly what you wait on.
7. **Print the handoff** (below) as the last thing in the session, after the merge of any
   completed plan. Do not write it to a file.

**Reaching other lanes.** Messages are best-effort hints and never replace git: the integration
branch and the plans\' checkboxes stay the only source of truth, and a lane confirms a gate
there (rule 2) before it starts, whatever a message says. To message a lane, run `ListAgents`,
keep the names that match `ipynb-execution-lane-<x>-<N>` for that lane letter, and send to the
one with the highest N (busy or idle does not matter; messages queue). A session is renamed to
bump N while it runs, so look the name up at send time and never copy it from an old handoff.
A bare `ipynb-execution-lane-<x>` counts as N = 0, until that session is renamed. If nothing
matches, skip the message and say so in the handoff. Uses:

- **Gate notice** (rule 5): the merging lane tells the lanes its table row unblocks.
- **Corrections:** a lane that finds an error in the table or in another plan\'s gate (for
  example a task the gate names that sits in a later phase) messages the lane that merged
  that gate, with the facts and `file:line`. It does not edit the table. If the error blocks
  it, it also writes a "Decisions for Gordon" item in its handoff.
- **Restart:** a lane idle on a gate names it in "Waiting on" (plan and phase). Whoever
  restarts it, Gordon or a gate notice, relies on it re-checking git first.

**Hot files.** These are edited by several plans, so rebases there will conflict: `text_execute.rs`
and `stored.rs` (EXR-1, EXR-2, JOL-2b), `execute_blocks_inner` and `ExecuteOptions` (EXR-3,
JOL-4, EXR-7; reconcile the `owned_languages` and `units` fields in one pass), `resolution.rs`
(JOL-2a, JOL-2b), `engine_execution.rs` (EXR-2, EXR-5, EXR-6, EXR-7) and this epic and the
plan files themselves (checkbox and status lines only). On a conflict, keep both sides\'
intent, run the gates, and if the resolution is not mechanical stop and put it in the
handoff. Never edit a settled decision or the dependency table to make a rebase easier;
changes to either belong to Gordon.

**Handoff format.** At most twelve lines, as the final console output; the next session may
get it pasted in, but it must be able to start from git alone (rule 2).

```
HANDOFF lane <A-D> (workspace-<4-7>), <date>
Plan and phase: <PLAN> <phase>; <n> of <m> items ticked
Branch: exr/<PLAN> at <sha>; merged to integration: yes (<sha>) | no (<why>)
Gates run: <crate>: clippy clean, nextest <pass>/<skip>
Next first item: <the plan\'s next unticked item, quoted>
Waiting on: <plan and phase, its lane, and the gate to check in git> | nothing
Decisions for Gordon: <each with lettered options and a recommendation> | none
Surprises and hot-file notes: <conflicts resolved, facts that differed from the plan>
Before pushing, recommend: <nextest on the direct dependents of the crates touched>
Notified: <lanes messaged at a gate merge, and any lane not found> | none
```

## Risks

1. **Unit identity disagrees between sites** (native record, WASM lookup, the
   knitr regexes, the mask, QuartoNotebookRunner chunking, Positron\'s TS
   parser). Silent failure. Verified 2026-10-29: the five scanners disagree
   today on the token class, on options inside the brace, on indentation and on
   nesting (QNR accepts exactly `{julia}`, `{python}` and `{r}` at the top
   level). Mitigation: one shared scanner and token rule (EXR-2), conformance
   fixtures (qmd to expected units and ids) run in q2 and, in a spike, through
   Positron\'s own parser (EXR-16, S1), count assertions wherever an engine scans itself, cross-site tests with the
   mask on and off. Units are scanned once, in the stage, and the same list goes to every
   engine (decision 38); Positron\'s parser diverges on nesting and labels, and S1 records
   each divergence (decision 39).
2. **Presenter parity.** The Rust presenter and the TS `toMarkdown` both turn
   nbformat into markdown. Golden tests on existing fixtures before anything
   builds on the extraction (EXR-1); a Julia parity suite before Julia flips
   (EXR-8). Keep the `.cell` wrapper shape until the legacy splice retires
   (`2026-07-01-bd-gthycd33-jupyter-cell-wrapper.md`).
3. **Hub schema transition.** Deployed docs, admin tooling and guests: readers
   first; old docs untouched; `classify.rs` updated.
4. **Committing the public API while unsettled** (bd-ixz93hjp): optional,
   versioned fields; no npm publish until the contract is exercised.
5. **Size** (bd-sr73): measured (EXR-4 Phase 0, re-measured on the hub store in
   EXR-15 Phase D, 2026-11-01). Decision: figures stay inline. A matplotlib figure plus a
   small table is 23.5 KB as a stored record document (24.8 KB as a legacy capture), about
   430 such figures to the 10 MB warning; a sidecar would only be considered past 1 MB
   gzipped per record. **Orphans:** each execution writes a new immutable document, so an
   edit-run-edit session leaves about 23.5 KB behind per run per document (10 runs measured
   at 235,206 bytes in the store, linear, no deduplication of an unchanged figure across
   documents); 20 runs on one document is about 470 KB until `hub admin` collects them. That
   is small enough that the single-cell upgrade path (content-addressed figure docs) stays
   deferred, and it is what `hub admin` aging has to keep up with.
6. **Never run both mechanisms on one record** (bd-mu9i0bct and bd-42202ipf
   were this bug class).
7. **Fork drift.** The three engine forks diverge from upstream. QNR\'s cede
   commit is in no upstream release (the pinned `=0.18.2` has no ceding), and
   0.18.2 to 0.18.6 brings upstream output-ordering and stderr changes whose
   effect on q2\'s Julia fixtures is unchecked. Mitigation: consume forks by
   commit hash, re-run the engine fixtures at each move (EXR-8, EXR-9); upstreaming
   is deliberately left out.
8. **Accepted: the hash does not see everything.** The execution-input hash leaves
   `.env` and `project_env`, data files, and format-specific defaults merged after
   `ast.meta` (for example docx and pptx figure sizes,
   `engine_execution.rs:965-1006`) outside it, so a change to those leaves a record
   fresh. Staleness is advisory (decision 8); a user re-executes by hand.
   **Julia (EXR-8 B1a, recorded 2026-11-02, "documented, no action"; revisit if staleness
   proves confusing).** Julia\'s flat declaration (`granularity: document`,
   `documentKeys: julia, format, fig-width, fig-height, fig-format, fig-dpi`) cannot say what
   QuartoNotebookRunner hashes for its own cache, in two ways. (a) QNR hashes the whole front
   matter (`title` included), while `documentKeys` is an explicit list, so a change to an
   unlisted key invalidates QNR\'s cache and not core\'s hash: core calls the record fresh and
   QNR reruns. The cost is time, never stale output, since outputs depend only on the keys QNR
   reads. (b) QNR hashes any markdown chunk that contains a span, so editing prose beside a
   span changes QNR\'s hash and not core\'s (core hashes the spans only); the span result
   depends on its code alone, so the output is unchanged. If a fix is ever wanted, (a) needs a
   "whole front matter" form in `HashDefinition` (EXR-7) and (b) a prose-sensitive granularity
   no flat declaration has. Source: `parsing.jl:170-266` in the QNR fork;
   `2026-10-28-exr-08-julia.md` (B1a).

## Out of scope

- Freeze and the `_freeze/` machinery; prefix reuse; Q1 `_freeze` import.
- Single-cell execution itself (the record is designed to carry it; Julia
  cannot do it until QuartoNotebookRunner supports it).
- Structured knitr outputs and presenting knitr from them (the follow-on knitr
  epic, decision 6).
- Keying the preview capture cache by the execution-input hash (that is reuse).
- Routing span-only documents to an engine (bd-my5fqvok).
- Rendering `.ipynb` files as input documents on `wasm32` (the `.ipynb` file claim,
  the other half of bd-9zmpzw3w).
- Removing `ReplayEngine` or the trace format.
- Upstreaming the engine fork changes, and how end users receive the forks
  (decision 32).
- Badges for stale units (UI work), structured automerge storage, figure
  sidecar docs.
- Contributing, proposing or sending anything to the Positron or Ark projects;
  the EXR-16 spikes stay in Gordon\'s fork, and any pull request or discussion
  is a separate decision after the findings.

## Open questions

Two questions live as question strands, outside the epic: bd-my5fqvok (should a
span-only document route to an engine, decision 11) and bd-30k2j00f (should an
empty-valued span render nothing instead of its source, decision 17). Everything
below is plan-scope; the owner is the plan that closes it. The inline-expression
divergences from q1, the QNR cell expansions, the hash field list, the kernel
precedence and the knitr id mechanism are decided (decisions 17 to 23, 34 to 42). The design
questions of the deferred JOL-3 stay in its plan and are not counted here.

Decisions still needed from Gordon: none.

Unverified facts that gate work (21):

1. What `RenderOverlay` does to project-level versus document-level metadata, which
   decides which overlay keys count as execution-relevant (EXR-15, first task).
2. Which metadata keys the TS engines (Julia, marimo, extensions) read, which EXR-4
   did not audit, and whether a flat `HashDefinition` is enough for Julia and marimo
   (EXR-4, EXR-8, EXR-9).
3. Whether `body_source_for` falls back to the whole block after a mid-body strip
   (reviewer-reported; EXR-4, first task).
4. How a span looks in the pre-engine AST inside a heading, a table cell and an
   attribute value (EXR-14, Phase B).
5. The mechanism that renders `None` as text when a cell yields no `execute_result`
   (EXR-3).
6. Whether `ExecutionContext` supplies the scope and target format to
   `ReplayEngine`; that no progress callback reads partial text (EXR-6).
7. Whether the current cell\'s options reach the splice presenter (EXR-14); whether
   nothing needs the replay-engine move earlier than EXR-2 Phase 4 (EXR-1b; checked
   2026-10-31: EXR-13 and EXR-14 use only the presenter, EXR-6 needs EXR-1b); whether
   the TS scanners in `quarto-api` (`breakQuartoMd`) need to join the scanner
   corpus (reviewer-reported; EXR-2).
8. Whether threading `handledLanguages` into span evaluation needs more than a
   keyword argument; the effect of QNR 0.18.2 to 0.18.6 on the Julia fixtures; the
   QNR environment is not instantiated and its tests were not run on the rebased
   fork branch; which Julia versions q2 users run, which decides `[sources]` versus
   `Pkg.add` (EXR-8).
9. Whether marimo\'s interactive hydration works offline (EXR-9).
10. The knitr version q2 pins, and the value visibility of two expressions in one
   `eval` (EXR-12; checked in spike S1).
11. How the legacy arm reaches the `ENGINE_CAPTURE_KIND` emission, and which
    deployed hub stores still hold legacy captures (EXR-17).
12. Whether plain `{ggsql}`-style fences on SoS need q2 to send SoS\'s `cell_kernel`
    in the execute request, whether `runtimelib` lets q2 add that field, and the
    manual check that `jupyter: sos` works through q2 against the real `sos`
    kernelspec (JOL-2a, JOL-5).
13. How cells with `eval: false` interact with the single-engine warning for an
    unrunnable owned language (JOL-2b).
14. Whether the existing source map misaligns non-ASCII input at runtime, an
    existing issue outside this epic that the dropped wire offsets avoid (EXR-7).
15. Whether a Positron dev build has the Quarto feature active, and the exact
    `session.execute` signature (EXR-16, S0).
16. What `SourceInfo` synthesized, format-default and extension metadata values carry,
    which decides the document-own front-matter test (EXR-2, decision 11, Phase 2
    provenance probe), and how `prepare_resolution` learns the document\'s own file id
    (JOL-2a).
17. How the deployed SPA surfaces an old reader\'s parse error on a record (EXR-13, Phase D
    compatibility matrix).
18. The guest and chapter-download strands bd-jddbirj3, bd-0lc6tqyq, bd-a5gp5y38,
    bd-7zygxe1e and bd-zykj3dmx (EXR-15, Phase C).
19. Whether `ctx` at the splice stage sees the mask\'s bookkeeping (EXR-14, Phase B
    characterization).
20. Whether QuartoNotebookRunner\'s CommonMark parser skips front matter as the agent\'s check
    showed (the depot version may differ from QNR\'s pin), what happens to a mixed
    `{python .marimo}` plus bare `{python}` document, and whether a Positron vitest harness
    provides an in-memory file service (EXR-8, EXR-9, EXR-16).
21. Which name the `Unit.in_front_matter` field takes and where the overlay-free record
    hash for `provide-hub -M` is computed (EXR-2 Phase 4, EXR-5 handoff task).

Deferred to freeze or later (2):

1. Which of the hub capture and the on-disk record wins is moot here: nothing in this
   epic reads the on-disk record, and the payload type is `CapturePayload { Empty, Legacy,
   Record }` dispatched on `nbformat` (EXR-13 decision 3, EXR-15 decision 8). Revisit if a
   consumer of the on-disk record appears.
2. Whether a shared-header reference should replace the marimo header carried in
   every record (about 52 KB), decided with the figure-sidecar work (EXR-9).

## Conventions for the plans in this epic

TDD with tests first; each plan names its exit criteria; plan-scope work goes in
the plan checklist, not in strands. Facts marked *verified* were checked in code
on 2026-10-28 (workspace-1 at `2ee0bd966`) or, where dated, on 2026-10-29;
anything else must be re-verified before relying on it.
