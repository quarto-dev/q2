---
title: 'RIU-3: the Positron cache as a record source'
date: 2026-11-02
description: 'Promote the Positron cache importer from a library function with no caller to a record source: locate the cache file, build a display-only Record by hand with no engines, run the language gate at conversion, add an import-only provider path that skips the consent gate, and show a Python document in preview with no kernel and no engine.'
status: draft  # written 2026-11-02; design details marked provisional wait on RIU-0 S3, S4 and S5
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Depends on:** RIU-2 (the seam), and [RIU-0](2026-11-02-riu-00-spikes.md) S3 (consent skip),
S4 (the splice design) and S5 (where Positron keeps its cache).
**Blocks:** RIU-4, RIU-6.
**Estimated sessions:** 2.

> **For implementers:** tests first. The conformance fixtures from EXR-16
> (`positron_conformance.rs`, `positron_demo.rs`) and the importer
> (`positron_import.rs`) already exist; build on them, do not fork them.

## Overview

`import_positron_cache` takes bytes and returns a `PositronImport`; nothing calls it. This plan
makes Positron\'s inline-output cache one more record source (epic decision 4) with these
properties (decision 6): display-only, provenance-marked, content hashes only, language gated,
feeding no freeze or reuse decision.

## Design (from S4, provisional until S4 reports)

- Build a `Record` by hand: no engines, empty execution hashes. `match_units` skips an empty
  execution hash and matches by content hash with the occurrence rule; the splice has a
  content-hash-only fallback for a unit with no engine.
- Never call `record_is_stale` (it returns not stale when there are no engines) or
  `rehash_record` (it stamps a hash of an empty list). Per-unit staleness comes from the
  `SpliceReport`.
- `RecordBuilder` cannot produce an empty hash, so the record is built directly.
- Never add `From` or `Into` on `DisplayOnlyOutputs`. The probe test
  `imported_outputs_convert_to_nothing_a_reuse_decision_takes` guards this. Building a cell by
  hand from `outputs()` is allowed.
- The language gate runs at conversion, with the caller supplying the owner, because the owner
  is unknown inside the splice. Occurrences are counted per hash before gating.

## Phase A: tests first

- [ ] A conformance test per fixture: import, convert with `record_from_positron`, splice,
      assert the units that get outputs and the units that get none.
- [ ] A test that a unit whose language is not owned by the caller-supplied owner gets no
      outputs, and that the occurrence count still counts it (so a later identical unit
      matches the right cell).
- [ ] A test that a duplicate-content group matches by occurrence.
- [ ] A test that the record carries provenance (`positron`) and no `quarto_exec_hash`.
- [ ] A test (compile-fail or a probe) that nothing converts `DisplayOnlyOutputs` into a record
      cell by trait. Keep the existing probe; extend only if it does not cover the new function.
- [ ] A provider test that the import-only path never calls an engine and never prompts for
      consent, with a fake engine that fails if run.

## Phase B: the conversion

- [ ] `record_from_positron(&PositronImport, &[Unit], &AuthorTexts, owner) -> Record` in the
      record module, target-neutral.
- [ ] **Provisional (S4, U16):** `cargo check` on the WASM target. If it builds, say so in the
      module comment; if not, the conversion stays native-only and the reason goes in the plan.
- [ ] Provenance in the flattened metadata; `quarto_cache_version` unset.
- [ ] Share the importer gate widening for `r` units with KNR-5 (`fallback: bool` argument);
      consume it, do not add a second gate. Until KNR-5 lands, `r` units get no Positron
      outputs and the plan result says so.

## Phase C: locating the cache file

- [ ] Find the file for a document by scanning the cache directory for `metadata.quarto_source`
      equal to the document file URI, then by content-hash fallback. The cache file name is
      `<sha1(uri)[0:16]>.ipynb`, but the URI scheme and form Positron hashes are unverified
      outside S5, so scanning is the primary method.
- [ ] **Provisional (S5, U18):** the directories. Released builds use the `Positron` folder
      under the platform application-data directory; a dev build is read as
      `code-oss-dev/User/globalStorage/quarto-inline-outputs/` with the `User/globalStorage`
      segment unverified. Per-platform defaults for macOS, Linux and Windows, all overridable
      by a directory argument. Use `Path::join` and the platform data-dir crate the workspace
      already uses.
- [ ] Never write into Positron\'s directory from q2. Read only. A file that fails the
      importer\'s version check (`quarto_cache_version` other than 1) is a skipped source with
      a diagnostic, not an error.

## Phase D: the import-only provider path

- [ ] **Provisional (S3, U13):** provide-hub\'s consent gate is a single call after materialize
      and the review file. Branch before it for a source that does not execute, so the
      execution path is untouched. If S3 finds another gate, list it here.
- [ ] The path publishes through the RIU-2 seam with provenance `positron`.
- [ ] A document with no Positron cache file falls through to the next source in the list
      (RIU-4 defines the list); it is not an error.

## Phase E: documentation

- [ ] Update the sentence in the user docs that says Quarto does not read Positron\'s cache.
      Document the option after RIU-4 names it.

## Phase F: verify

- [ ] `cargo clippy -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`
- [ ] `cargo nextest run -p quarto-core -p quarto-preview -p quarto-hub-provider`
- [ ] Copy a real Positron cache file into the scratchpad (never use or modify the original),
      put it beside a copy of its document, and run preview with the source enabled. State
      whether a browser was used. Exit evidence: outputs shown for a Python document with no
      kernel installed or started.

## Exit criteria

- Preview (and provide-hub) show Positron\'s outputs for a Python document with no kernel and
  no engine, in a real browser if one can be driven.
- The conversion builds for WASM, or the plan says it does not and why.
- No `From` or `Into` on `DisplayOnlyOutputs`; the probe test passes.

## Risks

- **Positron\'s cache format can change.** The importer rejects any `quarto_cache_version`
  other than 1 and the conformance fixtures stay the check.
- **Stale outputs.** A Positron cache can be older than the document; the splice report marks
  units that matched nothing, and the viewer must show provenance so a user can tell. Do not
  hide mismatches.
