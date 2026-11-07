---
title: 'RIU-3: the Positron cache as a record source'
date: 2026-11-02
description: 'Promote the Positron cache importer from a library function with no caller to a record source: convert it with the record_from_positron design proven in S4 (no engines, empty execution hashes, language gate at conversion, owner from the engine registry, hashes from the sugared author text), find the cache file by the SHA-1 of the document URI with two fallbacks (one native-only locator in quarto-core that serves the Positron source and a bare-path directory source), guard staleness for imported records, add an import-only provider path that skips the consent gate, and stamp the spliced output through the SourceInfo system as proven by spike P.'
status: 3a landed 2026-11-06; 3b (Phase E stamping) landed 2026-11-07 (the editor UI, Phase F browser items and live share are not run)  # corrected 2026-11-07 by the follow-up plan rewrite (the line said 3b never started); earlier: nothing started; written from RIU-0 S3 to S5 and RIU-0b spike J and spike P, checked against the tree on 2026-11-03 and revised after the second, third and fourth plan reviews; split into 3a and 3b; R through Ark is not in this epic
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Evidence:** [RIU-0c findings](../research/2026-11-03-riu-0c-findings.md) (spikes 1, 2 and 4: viewer editing, the stamp on other shapes, the compiled signatures); [RIU-0 findings](../research/2026-11-03-riu-0-spikes-findings.md), S3 (consent), S4
(conversion), S5 (cache location and file name); [RIU-0b findings](../research/2026-11-03-riu-0b-findings.md),
J6 (the owner and the gate) and P1 to P8 (the stamp).
**Depends on:** RIU-2 (the seam, the source list and walk with its outcomes, the `RecordLookup` trait, the
execution guard, the `describes_something` predicate and its `encode_capture_payload` change, record-only entries) and, for any real-browser check, the RIU-1
preview flip (an imported record reaches the page only through a record document, which needs the
writer gate on; until the flip, set `QUARTO_ENGINE_RECORD_WRITE=1`).
**Blocks:** RIU-4 (it hardens the flag 3a adds), RIU-6.
**Estimated sessions:** 4 to 6 in two halves (an estimate after the fifth review): 3a about 3 to 4, 3b about
1 to 2. 3a is Phases A (the tests for B to D), B, C and D; 3b is Phases E, F and the 3b items of G. Each half
has its own exit criterion (see Exit criteria) and can be met without the other.

> **For implementers:** tests first. The conformance fixtures from EXR-16
> (`positron_conformance.rs`, `positron_demo.rs`) and the importer (`positron_import.rs`)
> already exist; build on them, do not fork them. The S4 spike
> (`spike/riu-s4-positron-record`, commit `803608487`, never merged) holds a working
> `positron_record.rs` and four scratch tests, and the spike P branch
> (`spike/riu-s9-provenance`, commit `4c8fbb8e0`, never merged) holds the stamp and its tests. Port
> them, do not cherry-pick the branches (S4 is stacked on the S2 spike).

**Lanes:** before starting a phase or landing work, read the epic's [lanes, landing and the ready board](2026-11-02-records-in-use-epic.md#lanes-landing-and-the-ready-board): lane branches, rebase and landing rules, and the board of which phases may start.

## Overview

`import_positron_cache` takes bytes and returns a `PositronImport`; nothing calls it. This plan
makes the Positron inline-output cache one more record source (epic decision 4) with these
properties (decision 6): display-only, provenance-marked, content hashes only, language gated,
driving no automatic reuse or freeze decision in this epic (not a bar on a later explicit render use). It also marks the spliced output on the AST nodes (decision 14).

## The design (proven in S4)

On a real Positron cache (an R document, three cells) the converted record spliced in the
unchanged viewer with 0 kernels: stream, table, image, error and result all showed, and an edited
cell lost only its own output.

```rust
pub fn record_from_positron(
    import: &PositronImport,
    units: &[Unit],           // ids filled
    authors: &AuthorTexts,
    source: Option<String>,   // project-relative document path
    owner: &dyn Fn(&str) -> String,
) -> Record
```

- Per unit, the occurrence is counted per content hash **across all units**, then the language
  gate (`PositronImport::outputs_for(hash, occurrence, owner(lang), lang)`) decides. A gated-in
  unit becomes a `RecordCell` with `quarto_engine: None`, the real `quarto_content_hash`,
  `quarto_exec_hash: ""`, the label, the nbformat cell id (the hash, then `-2`, `-3` for
  duplicates) and the author text as `source`, with outputs read from
  `DisplayOnlyOutputs::outputs()`.
- No engines. `match_units` skips an empty execution hash and matches by content hash with the
  occurrence rule; the splice has a content-hash-only fallback for a unit with no engine.
- **Unclaimed cache cells.** The conversion builds cells only from the document's current units, so a cached
  cell that no unit pairs with (its code was edited or deleted since Positron wrote it) never reaches the
  record. The conversion therefore also counts them and stores the count in the record `extra` as
  `quarto_unclaimed_cache_cells` (a name used here; the value is a number). A cache cell is **claimed** when
  some unit pairs with it by content hash and occurrence, **whether or not the language gate then accepts
  the unit**, so an `{r}` chunk the gate refuses still counts as covered and never looks orphaned.
  `PositronImport` gains two read-only methods for this (the number of cells, and whether a cell exists for a
  hash and occurrence); it is target-neutral. The count is stored in the record, not recomputed, because the
  source list is read once at boot (epic decision 4): the staleness function must give the same answer at
  publish and after every file change, and only the record survives between them.
- **Provenance:** `quarto_provenance: "positron"` in the record `extra` and in each imported
  cell `extra`; it survives a write and read and is excluded from cell options. Whether it maps onto
  engine provenance was settled by spike P: it does not (a Positron record has no engine entry), so
  it stays, and the splice reads it to choose the node stamp's source (Phase E).
- `quarto_cache_version` is unset; `quarto_record_version` is 1.
- Never call `rehash_record` (it stamps a hash of an empty list). `record_is_stale` returns not
  stale with no engines, and an existing caller does reach it for an imported record; Phase B makes it
  skip that comparison for an imported record. Per-unit staleness comes from `match_units` (Phase B).
  `RecordBuilder` cannot produce an empty hash, so the record is built directly.
- Never add `From` or `Into` on `DisplayOnlyOutputs`. The probe test
  `imported_outputs_convert_to_nothing_a_reuse_decision_takes` guards this; building a cell by
  hand from `outputs()` is allowed.
- The conversion and `import_positron_cache` build for WASM (checked through
  `npm run build:wasm`; a bare `cargo check --target wasm32-unknown-unknown` fails in C build
  scripts, so use the npm script).
- **A duplicate-content pair** matches by occurrence, and editing the first copy leaves the
  second output orphaned, the same rule Positron uses. Recorded, not hidden.

### Where the hashes and the owner come from (the spike took shortcuts)

- **Author text.** S4 built `AuthorTexts` from a raw scan (`AuthorTexts::from_scanned`). The real
  splice path scans the **masked, serialized AST** and derives each unit's hash from
  `author_texts(&units, sugared_blocks)` (`record/staleness.rs:51`, `scan_and_fill_ids` at
  `engine_execution.rs:1018`), where `sugared_blocks` is the pre-sugaring snapshot. The two differ for a
  labeled or captioned cell (the test `a_labeled_cell_hashes_the_writers_range` in `record_splice.rs`
  is the proof). The source must therefore convert from the same inputs the splice uses:
  `compute_source_inputs` (`engine/preview_record.rs:286`) returns `input_qmd`, `metadata` and
  `sugared_blocks`, which `recompute_staleness` already consumes. Never convert from a raw scan.
- **Where a parsed document exists.** None exists before the pipeline: `record_one` holds only the
  paths, the project context, the runtime, an optional registry and the hub context
  (`capture_driver.rs:172-180`), and the provider holds the same plus a temp project
  (`execute.rs:272-286`). The only place a parsed `doc_ast` exists is inside
  `compute_source_inputs`, which also has the registry and the claimed engine, and which discards all
  three on return. `SourceInputs` carries no AST and no `ConfigValue` metadata.
- **Owner.** The spike hard-coded it. The owner of a unit's language is the engine registry's
  resolution of the document, `resolve_engines_for_document(meta, ast, registry, claimed, own_file)`
  (`engine/resolution.rs:439`, called with `Some(DOCUMENT_OWN_FILE_ID)` as at
  `engine_execution.rs:306-312`), whose `ownership` map gives language to engine. It cannot be fed from
  `SourceInputs` as it stands. The smallest design: `compute_source_inputs` computes the resolution from
  the parsed document it already holds, before it masks the document, and returns the `ownership` map in
  `SourceInputs` (with a registry-override parameter so a test registry is honored). The conversion is
  given `owner = |lang| inputs.ownership.get(lang)`. One pipeline run then yields the cache key, the
  units, the hashes and the owner. The resolver may load extension engines, which does not run cells;
  the real-registry test below covers it.
- **The gate.** `outputs_for` (`positron_import.rs:146`) now admits two cases: the owner `"jupyter"` with
  a plain language, and the owner `"knitr"` with the token exactly `r` (KNR-5, merged in `5db29f6bc`,
  constants `JUPYTER_ENGINE` and `KNITR_ENGINE`). It admits nothing else, and never a unit whose
  `fallback` argument is true. Spike J showed a Julia cell's owner is `"julia"` (J1, J2, J6), so this
  plan adds the owner `julia` for a plain-language unit, as a named table in one place with a test per
  entry. The `julia` entry is the only gate change here; the knitr entry is KNR-5's and is consumed as
  it stands. S4 showed R outputs only by forcing a Jupyter owner; that is no longer needed.

### R (settled 2026-11-05: not in this epic)

KNR-5's widening has merged, so a knitr-owned `r` unit can take Positron outputs through
`outputs_for`. This plan consumes that gate and adds no second one. The settled scope does not change:
R through Ark is not an acceptance case of this epic (decision 11, epic), the demo and the real-binary
checks are Python and Julia, and KNR-5's K-S2 cases stay KNR-5's. A test here covers the knitr entry
only so far as the conversion calls the gate with the right owner and language.

**The `fallback` argument at conversion.** `outputs_for` takes a `fallback: bool` that says the unit's
recorded output carries the `text/x-quarto-markdown` mime. `record_from_positron` builds engineless
cells from the document's current units and has no recorded output, so it passes `false`. This is safe
because the language test already excludes the only units that are fallback units in a real knitr
run: spike S3 of the [stack-rebase spikes](../research/2026-11-05-riu-stack-rebase-spikes-findings.md)
recorded a document with plain, labeled, `asis`, hidden-code and plot R chunks plus a bash chunk. Every R
chunk became a structured unit with ordinary `stream` or `image/png` outputs and only the bash chunk
became a fallback unit (`text/x-quarto-markdown`, no hash rule), and `lang == "r"` never admits bash.

## Decisions that shape this plan

From the epic's "Decisions recorded 2026-11-05", with the answers to the fourth review
([review 4](../research/2026-11-05-riu-plans-review-4.md)).

**Split (3a and 3b).** 3a is Phases B to D with their Phase A tests: conversion, locator, the walk's
Positron and directory arms, the provider path, the staleness rule and a minimal `--records` flag. 3b is
Phases E to G's stamp, TypeScript, documentation and the browser checks of the editor UI and live share.
RIU-4 keeps start-up validation, the defaults, the `provide-hub` rules, the `--join`, `--static` and
`--no-project` conflicts and the documentation of the option.

- [x] **The staleness rule (3a, Phase B).** An imported record never asserts freshness, but it may report
      staleness (epic decision 5 of 2026-11-05). The guard skips only the engine-hash comparison
      (`record_is_stale`). A page is stale when **at least one cached cell is orphaned**: its output belongs
      to code that is no longer in the document. A unit the cache does not cover (never run in Positron, or
      gated out, such as an `{r}` chunk) is not stale: the page lacks that output and stays quiet. One
      function in `quarto-core` takes the record and the `SourceInputs` and returns the staleness; it is true
      when `quarto_unclaimed_cache_cells` in the record is above zero **or** `match_units` over the record
      cells leaves `Matching::orphaned` non-empty. It must read only `orphaned` and **ignore**
      `Matching::staleness()` / `UnitStaleness::Stale`, which in `match_units` means a document unit no record
      cell matched (`matching.rs:131-163`), the opposite of this rule. The first term catches a cache older than
      the document at boot (the conversion already counted the unclaimed cells); the second catches an edit or
      a deletion after publish (the old record cell has no unit). `record_one` calls it when it publishes and
      `recompute_staleness` on each file change, so the two cannot disagree. To build the unit keys it reuses
      the splice's key construction for an engineless unit (`UnitKey` with an empty execution hash, the
      content hash, no `alt_exec_hashes` and no `header_keys`: the literal at `record_splice.rs:161-167`,
      the fallback of the closure that begins at `:135`), **extracted into a shared helper rather than
      copied**; it needs no `engine_for`, because an imported record has no engines. Extract the fallback
      literal only: the closure's first arm calls `UnitKey::from_author_text_with_profiles` for an owned
      unit, which an imported record never has. Imported cells carry no `quarto_header_keys`, so the
      splice's `lost_header_keys` demotion is a no-op for them; add a test that says so. A stale imported page stays stale
      until a restart or an engine re-execute: nothing re-reads the cache after boot.
- [x] **Wiring (3a).** The walk's `Positron` and `Dir` arms (RIU-2) call `import_record_source` through the
      locator; the preview passes its list in through `PreviewConfig.record_sources`. The minimal
      `--records` flag parses a comma list or repeated flag into RIU-2's list type with RIU-4's three
      classification rules (a known name, a path-like entry is a directory, anything else is an error) and
      sets `PreviewConfig.record_sources`. It does no validation, defaults or conflicts; those are RIU-4.
      Test it through the real argument parser.
      *Done 2026-11-06: `ImportContext` on `WalkContext`; `--records` is `FromStr for RecordSource` plus clap's comma delimiter and repeated flag, tested through the real parser in `main.rs`. Not parsed or checked: that a directory exists, defaults, and the `--join`/`--static`/`--no-project` conflicts (RIU-4).*
- [ ] **Tests (3a).** Editing a cell on an imported page flips the staleness (the viewer shows the
      Re-execute overlay; the editor UI label reads "code changed"). An unedited imported page stays quiet,
      including a page with units the cache does not cover and a page with an `{r}` chunk the gate refuses.
      Deleting a cell flags the page. A cache older than the document is flagged at boot (through the
      unclaimed count). Re-execute on a page with only an imported record runs the engine; with `engine`
      absent the same re-execute is refused (RIU-2's guard, exercised here through an imported page); the
      viewer still shows its Re-execute overlay on a stale page, and a click is refused with the guard's
      message, which the demo states rather than hides (RIU-6).
- [x] **A mis-paired expanded-cell Div (3b, Phase E): an interim rule, not a contract.** Read 2026-11-05:
      `splice_units` substitutes text, then reparses and reconciles (`merge_markdown_into_ast`). The
      `ReconciliationPlan` it returns holds only index alignments (`KeepBefore`, `UseAfter`,
      `RecurseIntoContainer{before_idx, after_idx}`), no unit ids or ranges, and the splice discards it
      (`record_splice.rs:257`), so the splice cannot name its own nodes and decision 7's fallback applies:
      the viewer's edit affordance refuses any Div whose class includes `.cell`. First step: with the spike 2
      fixture, find which attributes the merged Div carries (the author's or `.cell`), so the rule catches
      it; if it carries the author's attributes, stop and ask Gordon, because the rule would miss it. This is
      a stopgap: engines will eventually return provenance for the nodes they produce, and then the splice
      can say which nodes it made (echoed code, which is transcluded source, will need the same). Name the
      tests `interim_` and say so in a comment, so whoever upgrades the engines replaces them.
- [x] Keep the `Header` exemption (decision 9).
- [x] The language-gate test (Phase A): KNR-5 has merged, and `positron_import.rs` already tests knitr with
      the token exactly `r` (admitted, display-only) and a fallback unit (never admitted). This plan's new
      test is the `julia` owner entry, which can be written failing first; it also asserts that the
      conversion passes `fallback = false`.
- [ ] Positron-side limits KNR-5's presenter owns (empty `ename`, messages on stdout, ignored image
      width and height): RIU asserts them in tests, does not fix them. ANSI in tracebacks stays the
      open item in Phase E.
- [ ] **Acceptance cases for the browser checks and the RIU-6 demo, in Python and Julia** (not R: K-S2 is
      the Ark R document, which stays KNR-5's; its gate widening has merged and the env-gated check passes on a real Ark cache (S3b of the stack-rebase findings: all six units import; the first plot is lost to the unpadded-base64 bug bd-wzz90981, whose fix is not yet in this stack); preview only, and
      `q2 render` from a Positron record is a later follow-up, epic "Direction: render from a Positron
      record"). Each language gets: a plot (matplotlib; Plots), a data frame (pandas; DataFrames), a
      warning (`warnings.warn`; `@warn`), a message on stderr plus printed output, an error with
      `#| error: true`, and a labeled cell with figure options (`#| label: fig-x`, `#| fig-cap`,
      `#| fig-width`). A real Positron cache for each case needs Gordon at the window; without it, build
      the cache the way Phase A's real-data fixtures are built and say it was hand-built. Julia has no
      Positron-produced cache yet (RIU-0b), so its cases use a **hand-built** Julia cache in the Positron
      format (say so wherever it is used), or, once RIU-5 exists, a cache pushed through the J7 route.

## Phase A: tests first

- [x] Port the four S4 scratch tests into the record module: occurrence count before the gate (a
      marimo `x = 1` then a Python `x = 1` gives the Python unit the **second** cached cell);
      provenance survives write and read and is absent from options; no engines and empty exec
      hashes; duplicate-content pairing.
- [ ] A conformance test per EXR-16 fixture (not done in 3a: the durable-fixture tests cover the three real or hand-built caches end to end; the per-EXR-16-case import-convert-splice sweep is open): import, convert, splice, assert the units that get
      outputs and the units that get none.
- [x] **Durable fixtures.** The S4 real-data inputs are not durable: the R cache lives in
      `~/Library/Application Support/Positron/User/globalStorage/quarto-inline-outputs/2d70fef49b77f134.ipynb`
      and the document it came from, `ks3.qmd`, exists only in another session's scratchpad. Check in
      small copies under `crates/quarto-core/tests/fixtures/` instead: the R cache (rebuild the
      three-cell document from the `source` of each cached cell) and the **Python** cache the dev
      build wrote for `~/docs/inline-output/python.qmd`
      (`~/Library/Application Support/code-oss-dev/User/globalStorage/quarto-inline-outputs/bcfd94bdc5437bc3.ipynb`,
      two code cells). Copy, never modify the originals; replace `quarto_source` with a neutral
      `file://` URI so no user path is committed. Add a **hand-built Julia** cache in the same format (a stream,
      a string-valued result and a multi-MIME plot bundle, from the J5 and J7 shapes), marked hand-built in
      its header, since no Positron has produced one.
- [x] A real-data hash test with those fixtures: the `quarto_content_hash` of every cell equals the
      one q2 computes **through the splice path** (`scan_and_fill_ids` with the sugared snapshot) for
      the same document. Add a labeled cell so the sugared-text difference is covered.
- [x] A test that a unit whose language is not owned by the supplied owner gets no outputs, and a
      test of the gate table: `jupyter` and `julia` owners accepted, `knitr` and an unknown owner
      refused, `python.marimo` refused.
- [x] A test with the **real engine registry**: a Python cell and a Julia cell resolve to owners
      `jupyter` and `julia`, and both receive outputs from a cache that holds them.
- [x] A test that nothing converts `DisplayOnlyOutputs` into a record cell by trait (keep the
      existing probe; extend it only if it does not cover the new function).
- [x] Conversion tests: the unclaimed count is stored in the record `extra`; a cache cell no unit pairs with
      is counted; a cache cell whose unit the language gate refuses (an `{r}` chunk, a `python.marimo` unit)
      is **not** counted; a duplicate-content pair counts per occurrence.
- [x] Staleness tests (Phase B), written first: a record that is imported (marker present **and** no
      engines) never reaches `record_is_stale`; a record whose unclaimed count is above zero is stale at
      publish (the boot case); an edited unit leaves the old cell in `Matching::orphaned` and flags the page;
      a unit with no cached cell and no orphan does not (so a page with uncovered units stays quiet); the
      function ignores `UnitStaleness::Stale`; a record that carries the marker **and** engines (a
      hand-edited `.quarto` copy) takes the normal `record_is_stale` path and flags a changed document;
      `record_one` publishes the computed staleness for an imported record, and `recompute_staleness`
      returns the same answer for an unchanged document.
- [x] A provider test (Phase D) that publishing an imported record never calls an engine and never reaches
      the consent gate, with a fake engine that fails if run and a provider built with `AlwaysReject`.
- [x] A cache-locator test (Phase C) with a file name computed from a known URI, one test per
      directory rule (the locator takes the host OS and the environment as parameters, so the Linux
      and Windows rules are tested on any host), and the two fallbacks.
- [x] A directory-source test (Phase C): a bare-path directory holding a Positron-format file supplies
      a record through the same importer; a plain notebook without `quarto_cache_version` is rejected
      with a message.

## Phase B: the conversion and the staleness guard

- [x] `record_from_positron` in the record module, target-neutral, with the signature above.
- [x] `Record::describes_something()` and its use in `encode_capture_payload`
      (`capture_files.rs:303`), the cache hit and write filters and `run_has_content` come from
      RIU-2; assert here that an engineless record is treated as a record everywhere (the failure
      mode without it: an empty legacy list in the hub doc, a cache that misses).
- [x] `SourceInputs` gains the `ownership` map, **computed only when the caller asks for it** (an option on
      `compute_source_inputs`; every other caller pays nothing, because the cache-key path and each file
      change do not need it), and `compute_source_inputs` gains a registry-override parameter, with
      `resolve_engines_for_document` called on the parsed document before `mask_document` (see "Where a
      parsed document exists"; spike 4 type-checked exactly this call). The override must reach the
      `StageContext` registry that the resolution reads; say in the commit which seam it uses. It changes the
      5 callers (3 in `src`, 2 in tests), which pass `None` and do not ask for ownership. Then the import
      step, `import_record_source(bytes, &SourceInputs, source) -> Record`, in the native-only module of
      Phase C (it is what the walk's `Positron` and `Dir` arms call): `scan_and_fill_ids` fills the unit ids
      (it is `pub(crate)`, which is enough because the import lives in the same crate), then
      `import_positron_cache` and `record_from_positron` convert, with the owner from `inputs.ownership`. The
      import step is the only caller that asks for ownership.
- [x] **The guard in `recompute_staleness`** (`capture_driver.rs:324`, the Record arm, which calls
      `record_is_stale` on any record document including an imported one, and so violates the rule above).
      The guard is on provenance **and** on the engine list: when the record carries `quarto_provenance` in
      its `extra` **and** has no engines, skip only the `record_is_stale` comparison (narrowed 2026-11-05,
      epic decision 5: it does not return `Ok(false)` early, because an orphaned cell must still flag the
      page) and use the staleness function of the decisions above. Requiring both conditions keeps the
      guard to genuinely engineless imports: a record that has engines (an engine run never carries the
      marker, but a hand-edited `.quarto` or directory file can) goes through the normal `record_is_stale`.
- [x] The staleness function itself (see the decisions), called from `record_one` at publish and from
      `recompute_staleness` on a change; the splice's engineless key construction is extracted for it, not
      copied.
- [x] The walk's `Positron` and `Dir` arms and the minimal `--records` flag (see the decisions).
- [x] Run `npm run build:wasm` and say so in the module comment. (Run 2026-11-06 on the rebased branch, then `cargo xtask build-q2-preview-spa` and `cargo build --bin q2`: all three succeeded; the module comment of `positron_record.rs` says so.)
- [x] Gate `r` units through KNR-5's merged gate (`positron_import.rs:146`); do not add a second gate.

## Phase C: locating the cache file

A Positron cache file is `<directory>/<sha1(URI.toString())[0:16]>.ipynb`. The locator needs the
filesystem, the platform data directory (`dirs`) and SHA-1 (`sha1`, already a `quarto-core`
dependency), and `provide-hub` needs it too while `quarto-hub-provider` does not depend on
`quarto-preview`. So it is **one native-only module in `quarto-core`** (`cfg(not(target_arch =
"wasm32"))`), with `dirs` added to that crate. It must **not** go in the `record` module, which the WASM
client compiles. It takes the host OS and the environment as parameters, so each directory rule is
testable on any host.

**Directory (what each rule rests on).** Positron's cache is `globalStorageHome` of the *default
profile* plus `quarto-inline-outputs` (`quartoOutputCacheService.ts:133`), and the user data path is
built by `userDataPath.ts:50-83`:

| Rule | Source | Status |
|---|---|---|
| macOS release: `~/Library/Application Support/Positron/User/globalStorage/quarto-inline-outputs/` | observed on disk | **verified** |
| macOS dev build: `.../code-oss-dev/User/globalStorage/...` (product name `code-oss-dev` when `VSCODE_DEV` is set) | observed on disk | **verified** |
| Linux: `$XDG_CONFIG_HOME` or `~/.config`, then `Positron/User/globalStorage/...` (the **config** directory, not `dirs::data_dir()`) | read from `userDataPath.ts:110` | **not verified** |
| Windows: `%APPDATA%` (or `%USERPROFILE%\AppData\Roaming`), then `Positron\User\globalStorage\...` | read from `userDataPath.ts:96-103` | **not verified** |
| `POSITRON_PORTABLE` set: `<it>/user-data` | read from `:58-62` | **not verified** |
| `POSITRON_APPDATA` set: `<it>/<product>` | read from `:68-72` | **not verified** |
| `--user-data-dir` on the Positron command line | read from `:78`; invisible to q2 | cannot be derived: needs the override |
| A non-default profile | the cache always uses the default profile's storage | no extra layer |

So the locator takes an explicit directory override (an argument, and an environment variable for
the CLI) and otherwise tries the candidates for the platform in the order above. Use `Path::join`
and `dirs::config_dir()` on Linux, not `data_dir()`.

**File name.** `sha1(uri.toString())[0:16]` plus `.ipynb` (`_hashUri`, `:208-212`). Checked on two
files for a plain macOS `file://` path (`bcfd94bdc5437bc3` for
`file:///Users/gordon/docs/inline-output/python.qmd`). VS Code's `URI.toString()` is **not** the
`url` crate's serialization: it lowercases a Windows drive letter and writes the colon as `%3A`
(`file:///c%3A/Users/...`), percent-encodes with its own set, and encodes non-ASCII as UTF-8
percent-escapes (read from `src/vs/base/common/uri.ts:656-720`, not run). None of that was checked against a real Positron, so the implementation is a
documented port of the VS Code rules, tested against hand-computed names and marked unverified in a
comment for spaces, non-ASCII characters and drive letters.

**The fallbacks cover what the name cannot.** Every cache file Positron wrote carries
`metadata.quarto_source`, the document URI it hashed (observed in both real files), although Positron
does not read it when loading.

- [x] Compute the file name for the document's `file://` URI first and read that file.
- [x] If it is absent, scan the directory for a file whose `quarto_source` names the same path,
      comparing **decoded, normalized paths** (percent-decoded, drive letter case-folded, `%3A`
      read as `:`), not the raw string, so an encoding difference does not defeat the scan.
- [x] If that fails, match by content hash as Positron does (`findCacheByContentHash`,
      `quartoOutputCacheService.ts:611`, which exists for untitled documents): the file whose cells
      share the most content hashes with the document, newest `last_updated` on a tie. Read that
      function's tie-break before copying it, and refuse an ambiguous result with a diagnostic rather
      than guess.
- [x] Never write into the Positron directory from q2. Read only. A file that fails the
      importer version check (`quarto_cache_version` other than 1) is a skipped source with a
      diagnostic, not an error.
- [x] A document with no cache file, or a file in which no cell matches a unit, falls through to the
      next source (RIU-4's list); it is not an error.
- [x] **The directory source (a bare path in `--records`, RIU-4).** A directory holding Positron-format
      cache files is read by the same importer and the same three-tier locator as the `positron`
      source; the `positron` source is the same code pointed at the host's default Positron directory.
      A notebook without `quarto_cache_version` is rejected with a message (a q2 record file is the
      `q2` source, not a directory source). A directory entry is validated once at start (RIU-4).
- [x] **`provide-hub`.** It copies the project to a temp directory first, so a SHA-1 of that temp
      `file://` URI never matches the file Positron wrote; there only the content-hash fallback or a
      directory source can find a record (read from `execute.rs:272-286`, not run).

## Phase D: the provider path (3a)

The walk is shared (RIU-2), and RIU-4 makes the provider call it (its `execute_document` calls the walk when
the list names a source other than `engine`, and `run_engine` is `run_and_store`). This phase supplies the
provider's half that does not depend on RIU-4's wiring: publishing a resolved non-engine record. The consent
gate is consulted in one place, `self.consent.review` at `quarto-hub-provider/src/execute.rs:313`, inside
`run_and_store`, after `materialize_project` and `write_review_file` and before any engine runs (S3, settled). Consent
gates **engine** code only: `write_review_file` already runs the whole pre-engine pipeline (filters,
shortcodes, includes) before the prompt, because the reviewer must be shown the resolved document
(`execute.rs:448`).
Only the provider's `run_engine` reaches `run_and_store`, so only the engine source can reach consent; a
record from any other source never does. The CLI builds the provider with `AlwaysReject`, which is never
consulted and fails closed if a path ever reached it.

- [x] A provider method that publishes a resolved, non-engine record through the RIU-2 seam with its
      provenance (`positron`), writing the record document and the sidecar (with the staleness of the
      decisions above) without calling `run_and_store`. It takes the gate as an argument so a test needs no
      environment. Its test is the provider test in Phase A. Until RIU-4 wires `execute_document` to the walk
      it is reachable from tests only; RIU-3a's exit criterion for the provider is therefore this method's
      test, and the end-to-end provider path is RIU-4's.
- [x] With the gate off the seam refuses (an imported record has no legacy form); the walk then falls
      through (RIU-2), and `provide-hub` with a Positron source needs `QUARTO_ENGINE_RECORD_WRITE=1` until
      the hosted hub has the reader (epic decisions 2 and 9).
- [x] Not here: the provider's trait implementation, the call from `execute_document`, its constructor
      parameter for the gate and the start-up validation are RIU-4.

## Phase E: the spliced output carries provenance (spike P)

Output that came from a record is marked on the AST nodes through the existing `SourceInfo` system
(epic decision 14; [provenance contract](../designs/provenance-contract.md)). No viewer reads the
stamp in this epic except the atomic gate; the stamp is there for the first reader that wants it.
What spike P proved (RIU-0b findings, P1 to P8):

- **Which nodes.** After the reparse, a node is spliced output when its source info resolves into the
  file slot the reparse was registered under and its range lies inside one unit's presented
  markdown. Reconcile keeps the document's own nodes (document file id), so prose, stale cells and
  unmatched units are never touched. The unit's range is computed in the final text, through the
  unmask. A `Str` overlapping a span is stamped (the reparse merges `v1` and `.` into `Str("v1.")`);
  every other node needs containment. **`CodeBlock` is never stamped**: the copy-button decoration
  is keyed on an `Original` range (`CodeBlockDecorationKey::from_source_info`,
  `code_block_generate.rs:168-181`), and a stamped block lost it (the
  `record_splice_matches_legacy_splice_on_the_jupyter_fixtures` parity test failed until it was
  exempt). **`Header` is not stamped either** (decided with the second plan review, as a precaution: the
  heading-id step treats a `Generated` header as having a deliberate id, `heading_id.rs:56`). Spike 2
  stamped `Header` in a stored-markdown cell and found no render difference (ids, HTML and
  diagnostic text identical), the exemption stays (settled 2026-11-05, epic decision 9). A skipped node still sits under a stamped ancestor. Output text and echoed code are
  fenced code, so the node holding the bytes carries no mark and only its wrappers do, and the
  copy-button scaffold `Div` and `RawBlock` that the code-block transform builds around each
  `CodeBlock` keep a reparse-slot range (spike 2): they are never stamped, and there is nothing to
  skip.
- **The shape.** `SourceInfo::Generated` with `from` empty (pure synthesis, contract section 1) and
  `data = {"source": ..., "unit": ...}`. `source` is the record's `quarto_provenance` (`positron`),
  or `engine` for a record without one; `unit` is the unit id.
- **The kind and atomicity (decided, epic decision 14).** Reuse `By::jupyter_output()`: it is already
  defined in `quarto-source-map` 0.3.1 as the kind for execution-engine output, and it is atomic.
  Atomic is right because output is regenerated or imported, never edited in place (contract section
  7). The name says "jupyter" for a Julia, knitr or Positron record too; `data.source` carries the real
  origin. The viewer condition attached to the choice is resolved (spike 1): echoed code is not
  editable today (its source info resolves into the reparse slot, so it never gets the edit
  affordance) and the atomic gate only silences `setLocalAst`, which a `CodeBlock` never calls. So the
  **cell Div is stamped and atomic**, as planned.
- **Where the walk runs.** In `record_splice.rs`, right after the merge, for every record splice
  (an engine-run record and an imported one), with `source` telling them apart. The merge returns the
  file slot through a new sibling of `merge_markdown_into_ast`
  (`merge_markdown_into_ast_with_slot`, `engine_execution.rs`); the other caller is unchanged.
- **The walker.** The spike copied `remap.rs` into `quarto-ast-reconcile` to get a traversal that
  hands each node's `SourceInfo` to a closure. Production adds one such traversal and rebuilds
  `remap_file_ids` on it, so there is one walk of every block, inline, caption, table and custom
  node, not two.
- **What holds** (all tested in the spike): the wire format (the strict JSON reader accepts every
  node; kind and source reach the pool), the prose-edit round trip (the incremental writer reads the
  untransformed AST and never sees the spliced one, so `apply_node_edit` on the source with a
  destination taken from the spliced AST changes only that paragraph), the tiling auditor (no
  `SiblingOverlap`, `ContainmentViolation` or `ScatteredConcat`; a `GeneratedNoInvocation` census
  entry per `from: []` node, the same finding any such kind gives), and the structural hash
  (source info is excluded).

Tests first, in this order:

- [x] **The viewer regression test** (port spike 1's `spike-riu-s10-edit.integration.test.tsx`): a code
      block that resolves into the reparse slot gets no edit affordance, and an atomic cell Div
      (`jupyter-output`) leaves an author-owned code block's affordance unchanged.
- [x] The **shape test** beside the splice tests (contract section 3, step 4): a stamped node is
      `Generated`, `from` empty, kind `jupyter-output`, `data.source` and `data.unit` set, and
      `by.is_atomic_kind()` is true.
- [x] Identification: every matched cell stamped and prose not; a stale cell not stamped; an inline
      span stamps the value `Str` only; a mixed document (a Positron record covering Python, an `{r}`
      chunk it does not cover) stamps only the Python unit; no edit means no stamp; an engine-run
      record is stamped with `source: "engine"`.
- [x] A `CodeBlock` inside a stamped unit keeps its `Original` and keeps its decoration; the parity
      test stays green.
- [x] **The shapes** (spike 2 ran each through the real splice; port them as tests with these expected
      results). Stamped correctly: a cell in a div, callout, tabset, list item and blockquote (the
      container keeps its source info); a labeled and captioned cell; a cell with no outputs (its cell Div
      is still stamped); a stored markdown cell (every non-code node stamped); an inline value beside
      prose, in a heading, a link, an image caption and a footnote (only the value `Str`); an inline value
      alone in a paragraph (the `Str` stamped, the `Para` not). Pin as a known behavior: an inline value
      alone in a table cell stamps the cell `Plain`. Pin as a known limit (see the interim rule in the decisions): an expanded
      cell that shifts the block count lets reconcile pair the author's next Div with a spliced `.cell`
      Div, which keeps the author's source info and is not stamped. In every shape, the HTML
      and the diagnostic text are the same with the stamp on and off.
- [x] **The interim `.cell` rule** (see the decisions; port the spike 2 fixture): with `--allow-edit`, the
      mis-paired Div gets no edit affordance, and an author-owned Div that is not a `.cell` keeps its
      affordance. The test name starts `interim_`.
- [x] **The stamp switch.** The stamping post-walk takes an explicit boolean (default on) from the splice
      entry point, so a test renders the same document with the stamp off and on. It is a test seam, not a
      user option; no flag, setting or environment variable is added. The shape, render-invariance, wire and
      structural-hash tests below use it.
- [x] **The render-invariance test.** One stored-markdown fixture (an explicit heading id, a duplicate
      implicit heading, a crossref-targeted table, and a duplicate crossref id with the author's own
      heading) rendered with the stamp on and with it off: identical HTML and identical diagnostic text.
      Spike 2 showed this holds, and that only the raw location of the Q-15-1 diagnostic differs. A kind
      that ever breaks it is added to the list of kinds the stamp skips.
- [x] The wire round trip with the strict reader, the `apply_node_edit` prose round trip (the cell
      fences and the other prose byte-identical), the tiling auditor on the untransformed AST (no
      findings) and on the stamped AST (no gated kinds), and the structural-hash equality with and
      without the stamp. Run the existing idempotence integration test too; it has no engine cells,
      so it cannot exercise a record, and the hash test above is the stand-in.
- [x] The TypeScript set: add `jupyter-output` to `ATOMIC_KINDS`
      (`ts-packages/preview-renderer/src/utils/sourceInfo.ts:54`) with a unit test; update
      `utils/sourceInfo.test.ts:80-95`, which pins the set. Rust `is_atomic_kind` has no production
      consumer, so the viewer's set is the stamp's only visible effect. The set also lacks
      `citeproc`, which the Rust function already treats as atomic; fix both together and say so,
      since that file's comment requires the two to agree.

Then the code, in the order of the list above (the slot-returning merge, the walker, the stamp, the
TypeScript set).

- **No banner** (epic decision 15). The "Showing executed output" label
  (`hub-client/src/components/render/PreviewStatusBar.tsx:204`) reads the sidecar `CaptureRef` and is
  left alone, so no `hub-client/` file changes in this plan.
- [x] The TypeScript set change is under `ts-packages/`, which `hub-client` bundles from source: run
      `npm run build:all` from `hub-client/` and `npm run test` in the preview-renderer package even
      though no `hub-client/` file changes (so no changelog entry).
- [ ] Verify how an imported Ark error traceback shows, against a real Ark cache (settled 2026-11-05: a
      check, not a build). Positron tracebacks carry raw ANSI escape sequences, which the preview
      displayed as literal text (S4; Positron itself rendered them in color, S7). KNR-5 reports ANSI
      stripped through its presenter on a real cache (2026-11-04, its plan). The mechanism is
      `format_error_text` and `strip_ansi_codes` in `presenter/cell.rs` (this branch strips the
      traceback lines at `:280`; on `knr-5-positron-interop` it also strips `evalue` and handles an empty
      `ename`). It strips CSI sequences only. Add a test with escapes in a traceback; if they show, the fix is in KNR-5's
      presenter, not here.
- [x] Stamped nodes lose their resolved location in the JSON (`Generated` serializes offsets 0 and 0,
      `json.rs:380-398`), so `data-loc` and scroll sync (`iframe/scrollSyncDom.ts:145`) can reach an
      author-owned ancestor instead. Check click-to-source and scroll sync in a browser on a document
      with a stamped cell and say what was seen.
- [x] This reaches the WASM and the SPA, so run the full chain (`npm run build:wasm`,
      `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`).

## Phase F: documentation

- [x] Update the sentence in the user docs that says Quarto does not read the Positron cache, and document
      the minimal `--records` flag as it exists after 3a (the names `cache`, `q2`, `positron`, `engine` and a
      bare directory path), in `docs/guides/authoring/execution-records.qmd`; RIU-4 revises that text for the
      hardened flag. Also say, in `docs/guides/projects/preview.qmd`, what an imported record looks like in
      the viewer and in the editor UI, and that the content-hash fallback of the locator can serve the host's
      Positron outputs to `--share` guests and can pick another document's cache. The embedded docs go stale
      with these edits: run `cargo xtask build-agents-docs` and `cargo build --bin q2`, then check
      `target/debug/q2 docs llms --embed-info` and render with `target/debug/q2 render docs/`.
- [x] Update `claude-notes/designs/provenance-contract.md`: the catalog in section 2 lacks
      `jupyter-output`, `citeproc` and `callout`, and section 3 says to add a constructor in
      `crates/quarto-source-map/`, which is now an external crate (a new kind needs a release of it).

## Phase G: verify

Use `target/debug/q2`, never the `q2` on PATH (an old nightly that cannot read records). The import
itself needs no kernel; Python-side real data needs none either.

- [x] `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider -p quarto-ast-reconcile --all-targets -- -D warnings` (`quarto` holds the minimal flag and its argument-parser test)
      *Run for the four crates 3a touched (`quarto-ast-reconcile` is 3b's); clean.*
- [ ] The Windows cross-check (the locator builds paths and the fixtures carry file URIs):
      `cargo clippy -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider -p quarto-ast-reconcile --all-targets --target x86_64-pc-windows-msvc -- -D warnings`.
      Cross-checking proves compilation only; the Windows directory and URI rules need a Windows run,
      which no one has done.
- [x] `cargo nextest run -p quarto -p quarto-core -p quarto-preview -p quarto-hub-provider -p quarto-ast-reconcile`
      *Run for the four crates 3a touched: 9153 run, 9151 passed, 2 failed before the last fixture fix (the reveal vendoring test, known noise, and the Julia fixture test, fixed since and re-run green). The reveal test still fails in this worktree (`node_modules/reveal.js` 6.0.0 against the 6.0.2 pin).*
- [ ] **3a, real data, needs a browser (Playwright on the preview iframe):** *Partly done 2026-11-06, no browser; since then RIU-6 saw the cached output, the edit overlay, the revert and the 403 on Re-execute with Gordon at the Positron window (2026-11-06), and drove the Python case through Playwright (2026-11-07); not seen: `--records positron,engine` running the engine, and the cache-older-than-document case in a browser.* Real binary, from the rebuilt `target/debug/q2`, on a temp project holding `positron-cache/python.qmd` and, in a separate directory, `python.cache.ipynb`: `q2 preview doc.qmd --records <cache dir> --no-browser --port 0`. With the writer gate on (and also with the variable unset, so the RIU-1 preview flip is on by default) the log read `recorded engine capture(s) rel_path=doc.qmd engines=` then `recorded engine captures count=1` three milliseconds after the hub listened: published with no engine and no kernel. With `QUARTO_ENGINE_RECORD_WRITE=0` the page got `Q-PREVIEW-CAP-1` (the refusal, RIU-2's design). `--records bogus` fails at argument parsing with the message naming the choices. **Not seen:** the rendered page, the Re-execute overlay after an edit, the Julia and R documents, `--records positron,engine`. Those need a browser run. Original text: three documents: (1) the R
      fixture (expect nothing with the real owner until R routes to Jupyter), (2) the **Python** fixture and
      (3) the **hand-built Julia** fixture (say it was hand-built; `julia` must be on PATH only if the engine
      is in the list, and it is not). Run the default viewer, `q2 preview --records positron <doc>`, with
      the writer gate on (RIU-1's flip, or `QUARTO_ENGINE_RECORD_WRITE=1`) and with the locator pointed at the
      fixture directory (a bare-path source, or the locator's directory override, because a hand-built cache
      is not at the URI-hashed name), and inspect it; state what was not seen. Exit evidence: outputs shown
      for the Python and the Julia documents with no kernel installed or started; an edit to a cell shows the
      Re-execute overlay; a cache older than the document shows it at boot; with `--records positron,engine`
      the overlay's Re-execute runs the engine.
- [ ] **3b, editor UI and live share (product behavior, not the demo), needs a browser:**
      `q2 preview --ui editor --records positron <doc>` and the same with `--share --allow-edit` and a guest
      (`--allow-edit` is needed for the `.cell` edit-refusal check of Phase E). Check and record what is
      seen: the status bar reads "Showing executed output" with "code changed" after an edit; whether Run or
      Re-run appears in a preview (expected: no, because no executor is online; inferred from the code, not
      yet seen); "Clear results" removes the record for every collaborator and it does not return until a
      restart. That a guest cannot start a kernel with `engine` absent is RIU-4's share test, not repeated
      here.
- [ ] Recommend to Gordon `cargo xtask verify` (this changes `quarto-core` and the SPA); he
      decides.

## Exit criteria

**3a** (met without 3b):

- `q2 preview --records positron` shows Positron outputs for a Python document with no kernel and no
  engine, in the default viewer in a real browser, with the writer gate on. The provider publishes an
  imported record with no consent and no engine (tested); with the gate off it refuses with a message
  (RIU-2), never an empty page.
- A Julia unit gets Positron outputs through the same path (the gate admits its owner); tested with the
  real registry, and shown in the browser from the hand-built fixture.
- The staleness behavior of the decisions holds in tests: an imported record never reaches
  `record_is_stale`; an edit or a deletion flags the page, and a cache older than the document is flagged at
  boot through the unclaimed count; uncovered and gated-out units do not.
- The minimal `--records` flag parses through the real argument parser.
- The conversion builds for WASM. No `From` or `Into` on `DisplayOnlyOutputs`; the probe test passes.

**3b** (met without 3a's browser run, but after 3a's code):

- The spliced output of any record is stamped, with every test in Phase E green, and the viewer's atomic
  gate honors the kind.
- The interim `.cell` rule is in with its `interim_` tests.
- Click-to-source and scroll sync were checked in a browser on a document with a stamped cell.
- The editor UI and live share checks of Phase G are recorded, and the documentation of Phase F is
  written for the flag as 3a leaves it (RIU-4 revises it for the hardened flag).

**3b done 2026-11-07** (branch `feature/riu-3b-stamping`). What landed: `walk_source_infos` and
`SourceRole` in `quarto-ast-reconcile` (one walk under `remap_file_ids` and the stamp);
`merge_markdown_into_ast_with_slot`; `splice_record_with(.., stamp)` and the stamp post-walk in
`record_splice.rs` (test seam: a thread-local read only under `cfg(test)`, so no flag, setting or
environment variable exists); `jupyter-output` and `citeproc` in `ATOMIC_KINDS`; the interim `.cell`
rule in `blocks/Div.tsx`; the tests in `record_splice/tests/stamp.rs` and
`record-output-stamp.integration.test.tsx`; the docs of Phase F.

- **The `.cell` question.** With the spike 2 fixture the mis-paired Div carries the class `cell` (and the
  author's source info); the author's `.note` Div comes out under reparse source info. So the rule
  catches it. The test is `interim_the_mispaired_div_carries_the_cell_class_not_the_authors`.
- **Browser (Playwright, real `target/debug/q2`, the `python` fixture, hand-copied cache directory
  via `--records <dir>`, content-hash fallback).** Viewer: the cache output renders with no kernel; both
  `.cell` Divs and their `pre`s carry no `data-block-pool-id` and no `data-loc`, so
  `closest('[data-loc]')` finds nothing and a click inside a stamped cell maps to no source line (it does
  nothing); prose paragraphs keep both. Editor UI (`--ui editor`, after `cargo xtask
  build-hub-client-embed`): status bar "Showing executed output", then "Showing executed output ·
  code changed" after editing a cell on disk; no Run or Re-run control appears; "Clear results…" is
  present. **Not seen:** a real click in the editor's preview pane and scroll sync in motion (checked
  the DOM they key on, not the gestures), `--share --allow-edit` with a guest, "Clear results"
  clicked, the `.cell` edit refusal in a live browser (covered by the jsdom test only).
- **3a leftovers.** Done: Re-execute on an imported page refused with no engine and running it when
  listed (`re_execute_on_an_imported_page_is_refused_without_engine_and_runs_it_when_listed`; it passed
  on first run because the guard predates it). Not done: the per-EXR-16-case sweep and the labeled
  cell with `fig-cap` acceptance case.
- Windows cross-clippy still not run. `cargo xtask verify` is recommended, not run.

## Risks

- **Positron cache format can change.** The importer rejects any `quarto_cache_version` other
  than 1 and the conformance fixtures stay the check.
- **Stale outputs.** A Positron cache can be older than the document; the splice marks units that
  matched nothing, so a user can tell the unit is source, and an orphaned cached cell flags the page stale
  (the decisions above). Do not hide mismatches.
- **The interim rules are not a contract.** The `.cell` edit refusal and the stamp by post-walk exist
  because engines do not yet return provenance data; revisit both when they do.
- **Directory and file-name rules beyond macOS are unverified.** The scan fallback and the
  content-hash fallback exist for that; the override argument is the escape hatch.
- **A `.quarto` or directory file can carry `quarto_provenance`, and `data.source` is copied from it
  unvalidated.** The staleness guard requires the empty engine list too, so such a file with engines is
  still staleness-checked; the stamp's `source` is a label, not a trust signal.
- **Other transforms keyed on an `Original` range.** Spike P found one (code-block decoration) by
  accident, through a parity test. Phase E's tests and the full suites are the net; a new consumer of
  `Original` ranges needs the same check.
