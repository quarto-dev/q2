---
title: 'RIU-1: enablement and the writer flip'
date: 2026-11-02
description: 'Turn the engine-record writer on by default for the surfaces that RIU-0 clears, make the record: true file write fail soft, rewrite the tests and documents that assume the legacy capture is the default output, and state what a rollback leaves behind before the flip, not after.'
status: draft  # written 2026-11-02; provisional items are marked and are settled by RIU-0 S1 and S3
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Depends on:** [RIU-0](2026-11-02-riu-00-spikes.md) S1 (records work through the real preview
and hub) and S3 (the stranding map).
**Blocks:** nothing in code, but the flip must not ship in a release before RIU-2 lands
(the flip makes the cache hold records).
**Estimated sessions:** 1.

> **For implementers:** write the failing test first for every behavior change here
> (`AGENTS.md`, TDD). Items marked **provisional** wait on an RIU-0 result; do not start them
> before the findings note answers the named register row.

## Overview

`RECORD_WRITING_DEFAULT` is `false` (`crates/quarto-core/src/engine/capture_files.rs:259`), so
the preview writer (`capture_driver.rs:388`) and `provide-hub` (`execute.rs:361`) write the
legacy capture unless `QUARTO_ENGINE_RECORD_WRITE=1`. Plain `q2 render` does not consult the
gate. This plan flips the default, so the record becomes what those two surfaces write.

Settled with Gordon (epic decisions 1 and 2): the flip is the cutover from legacy captures to
records; `execute: {record: true}` controls only the optional copy under
`.quarto/execution-records/`; a plain `q2 render` keeps writing nothing under `.quarto/`.

## Phase A: tests first

- [ ] Rewrite `gate_defaults_off_and_parses_overrides` (`capture_files.rs:385`) to assert the
      new default and the override spelling (`0`, `false`, `off` turn it off).
- [ ] Find every test that reads the default writer's output as legacy and update it to read a
      record. Known from the survey: `eager_capture.rs`, `cache_hit.rs`, the provider
      `fetch_captures` helper, several unit tests in `capture_driver.rs`, and the
      `writer_gate_on_without_a_record_writes_legacy` expectation. Run
      `grep -rn "RECORD_WRITING_DEFAULT\|ENGINE_RECORD_WRITE" crates/` to confirm the list; do
      not trust this one.
- [ ] A test that a plain `q2 render` of a Jupyter document with no `record: true` creates no
      `.quarto/` directory, through `render_document_to_file`, not a library entry point.
- [ ] A test that `record: true` with an unwritable record directory renders successfully and
      carries a warning diagnostic (see Phase B). The unwritable directory is a regular file
      named `.quarto`, which fails directory creation on every platform with no permission
      bits (`.claude/rules/cross-platform.md`). Build expected paths with `Path::join`. Place
      it beside `execute_record_true_writes_the_record_file` in `engine_record_stage.rs`.
- [ ] **Provisional (S1 B10, U7):** a test for the mixed-engine rule chosen below.

## Phase B: code

- [ ] Fail-soft record write: drop the `?` at `engine_execution.rs:864` (the caller of
      `write_record_file`, defined near `:1091`) and push a warning through
      `ctx.add_diagnostic`. A failed optional copy must not fail a render.
- [ ] Flip `RECORD_WRITING_DEFAULT`.
- [ ] **Provisional (S3, U12):** if S3 shows a local preview cannot strand a record while
      `provide-hub` can, gate per surface: the default is on for the surface that writes a
      shared hub store only when the deployed hub clients can read records, and on for local
      preview now. Otherwise one default.
- [ ] **Provisional (S1 B10, U7):** the mixed-document rule. Today `encode_capture_payload`
      (`capture_files.rs:298`) writes the record when it has any engine entry and drops the
      legacy captures, so a document whose second engine builds no record loses that engine
      output. Options: write the record plus the legacy capture for the engine without a
      record (a record-with-legacy-sidecar payload is not an existing shape, so this needs a
      format decision), or keep legacy for the whole document when any engine lacks a record.
      S1 B10 decides which is needed once knitr-only documents are known to produce records.
- [ ] Update the stale comments: `capture_files.rs:255-258`, `execute.rs:526-529`,
      `capture_driver.rs:370`.
- [ ] Update `claude-notes/instructions/release-runbook.md` ("Engine-record writer gate").
- [ ] Update `docs/guides/authoring/execution-records.qmd:15-23`, which presents
      `record: true` as how you get a record. After the flip it is how you get the copy under
      `.quarto/execution-records/`. Keep the page user-facing: usage, not internals.

## Phase C: the rollback statement (before the flip)

- [ ] Write into this plan, before flipping, what a revert leaves behind: record documents
      already written to a hub store (parent decision 4: old documents are never rewritten),
      `<key>.record.bin` (after RIU-2, `<key>.ipynb`) files in a persisted `--data-dir`, and what an old reader does with
      them. S3 provides the facts.
- [ ] State how the runbook rule (readers in release N, flip in N+1) is met, per surface, using
      the hub deployment facts in the epic:
  - `q2 preview` hosts its own local hub and embeds its own SPA and WASM, so writer and reader
    ship together. **Provisional (S3, U12):** if S3 confirms it cannot strand a record, its
    default flips in the first release that carries the reader.
  - `q2 provide-hub` writes to a shared hub. Its default flips only after the hosted hub runs a
    `q2_ref` that contains the reader. The release number does not decide this; the deployed ref
    does (the hub and hub-client deploy together, by hand, from a `q2_ref`).
- [ ] Sequence for the `provide-hub` flip, to be confirmed with Gordon, who runs the deploys:
      (1) the reader reaches `main`; (2) run "Build and deploy artifacts" with `q2_ref=main` and
      deploy; check `/etc/quarto-hub/version.json` names a ref with the reader;
      (3) flip the `provide-hub` default in the next q2 change. Until then it stays off and a
      user can opt in with `QUARTO_ENGINE_RECORD_WRITE=1`.
- [ ] Rollback statement for `provide-hub`: rolling the hosted hub back to a build without the
      reader (`quarto-hub-rollback`) strands the record documents already written, so a hub
      rollback after the flip needs the gate turned off first and the stranded documents
      re-provided. Put this in `release-runbook.md` beside the existing gate section.
- [ ] Update the runbook section: it says "release N", and for the hub it is the deployed
      `q2_ref`. Keep the env-var override.

## Phase D: verify

- [ ] `cargo clippy -p quarto-core -p quarto-preview -p quarto-hub-provider --all-targets -- -D warnings`
- [ ] `cargo nextest run -p quarto-core -p quarto-preview -p quarto-hub-provider`
- [ ] Run the real binary: `q2 preview` on a Python document, in a browser if one can be
      driven (RIU-0 S0 says), and inspect the cache directory for the record file (`<key>.record.bin` until RIU-2, then `<key>.ipynb`). State
      plainly which steps had no browser.
- [ ] Recommend to Gordon a wider run on direct dependents (`cargo tree -i quarto-core`), and
      `cargo xtask verify --skip-hub-build` at least; he decides.

## Exit criteria

- The default is on for the surfaces S3 clears, a rollback is one revert, and the rollback
  consequences are written down.
- Documentation and comments match.
- A failed `record: true` write never fails a render.

## Risks

- **A latent defect in a path that has only run in tests.** RIU-0 S1 is the mitigation; budget
  for fixes in this plan if S1 finds them.
- **One-way door for shared hub stores.** Phase C exists so the consequences are known before
  the flip.
