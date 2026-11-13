---
title: 'OPR-9c: a reserved option key gets a diagnostic (quarto-unit-id)'
date: 2026-11-10
description: 'The record path writes a quarto-unit-id option line into every knitr chunk it owns. A chunk that already carries the key makes R fail with Duplicate map key. Gordon decided the key is reserved and a diagnostic says so. The plain-render flip would otherwise expose the crash to every knitr render.'
status: complete
depends-on: [opr-9a]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9c-reserved-unit-id-key` · **Lane C** (`workspace-5`) · **Blocks:** 9f, 9g.
**Needs:** R 4.3 with knitr for the end-to-end check; no Julia.

> **Reading text copied from OPR-7d.** Passages copied from the old 7d plan say "this plan" or "7d". Read them as this
> piece of the series 9a to 9g (see the [tombstone](2026-11-08-opr-7d-switch-plain-render.md)). Marks: (read), (ran), (relied).

## The decision (Gordon, 2026-11-10)

The key `quarto-unit-id` is reserved by the record path; a diagnostic says so. Evidence (ran, the 7d flip experiment): with
`present_from_record` on, `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute` fails because the document has a
chunk option `#| quarto-unit-id: abc123` and knitr aborts with `Duplicate map key: 'quarto-unit-id'`. Cause (read):
`insert_unit_ids` (`engine/knitr/chunk_records.rs`, runs when `ctx.builds_record()` and there are units) prepends its own
`#| quarto-unit-id: "<id>"` line to every chunk knitr owns and does not look for an existing one. The recorded route (preview,
`execute: record: true`) fails the same way today (inferred from the same code; not run). The flip makes `builds_record()` true on every plain
knitr render. The key was added by the knitr execution-records work (EXR-11), not by this epic.

## Start condition

Start when 9a is `complete`. S5 has run (see [the note](../research/2026-11-10-opr-9a-spike-results.md)): `quarto-unit-id` is the only
key that does harm. The other `quarto_*` keys an author can type on a Julia cell (`quarto_expansion`, `quarto_file_path`,
`quarto_markdown_passthrough`) reach the cell\'s metadata but are inert, because the presenter reads them from output metadata; the
runner overwrites `quarto_attrs` and `quarto_output_kinds`; span ids come from the shared unit list. So this plan covers one key.
**Scope, signed off by Gordon (2026-11-10): one key, `quarto-unit-id`, on knitr chunks.** The check lives in `insert_unit_ids`, which only builds
the knitr record, so a Julia or Jupyter cell that sets the key is not diagnosed (it is inert there: S5). The error severity is also confirmed. The
prefix widening is not part of this plan.
S5 ran Julia only; a Jupyter cell was not run. Widening to the whole `quarto_`/`quarto-` prefix is deferred until another key bites
(Gordon\'s call).

## Design

- The check lives where the duplicate is created: in `insert_unit_ids`, before the id line is built, so it fires exactly where
  the crash fires today and does not change a render that builds no record. If an option line of the chunk already names a
  reserved key (the option block uses `#|`, or the comment prefix of the language: `comment_syntax_for`), return a diagnostic
  that names the key, says it is reserved by Quarto for execution records, points at the chunk, and tells the author to
  remove it. Severity: an error (a document that sets it cannot be recorded). Gordon confirmed the error (2026-11-10) over a warning that
  drops the author\'s line.
- After the flip every knitr render builds a record, so the error fires on every knitr render with the key. That is intended.
- A diagnostic with a `Q-*` code needs the catalog entry, its `docs/errors/<subsystem>/<code>.qmd` page and its
  `docs/_quarto.yml` sidebar entry in the same commit (`cargo xtask lint` rules `error-docs-page-missing` and
  `error-docs-sidebar-unlisted`; template `docs/errors/README.md`). Pick the code and subsystem from the neighbors of the other
  knitr diagnostics.

## Checklist (tests first; every box is `[unattended]`)

- [x] **Tests first.** (1) A unit test on `insert_unit_ids`: a chunk with `#| quarto-unit-id: abc123` returns the diagnostic naming the
      key; a chunk with other options and no key is unchanged; a chunk in a `{sql}` fence (comment prefix `--`) with the key is also
      caught. (2) The existing integration test `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute` changes
      meaning: it now asserts that the render fails with the diagnostic (or move its leak assertion to a chunk without the key).
      Watch (1) fail first, for that reason. Skip cleanly when R is absent, as the knitr tests do.
- [x] **Implement** in `insert_unit_ids`; add the catalog code, docs page and sidebar entry if a `Q-*` code is used.
- [x] **End to end** through the binary: `q2 render` of a knitr document with the key, once with `execute: record: true` (fails with
      the diagnostic today after this plan) and a normal one without the key (renders). Record the invocation and the message you saw.
- [x] **Gates:** `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the Windows
      cross-clippy; `cargo xtask lint`.

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- you find a second key an author can set that silently changes a render (S5 found none, Julia only): report it;
- the diagnostic needs a new field on `ExecutionContext` or a change to the engine wire: report it;
- R is not available: the knitr tests cannot run, so say so and do not land.

## Handoff

End the session with the protocol's handoff ("Ending your session"). Lane C has no further plan after this one. Say that 9f waits on 9b, 9d and 9e, and name which of them are complete.

## Exit criteria

A knitr chunk that sets `quarto-unit-id` produces a diagnostic naming the key as reserved, on every route that builds a record;
no render with the key reaches R\'s duplicate-key error.

## Notes from the run

- **Implementation.** `insert_unit_ids` now returns `Result` and fails with the new `ExecutionError::ReservedOption`
  (mapped in `engine/diagnostics.rs` to `Q-18-17`, "Reserved Chunk Option"; catalog entry, `docs/errors/engine/Q-18-17.qmd` and
  the sidebar entry are in the same commit). The check reads the chunk's option block (`#|` or the language prefix plus `|`,
  inside the blockquote prefix) and stops at the first non-option line, so the key written as code is not flagged.
- **Decision (mine): chunk ordinal, not a line number.** The first draft reported a line, but the engine text is not the
  author's file (the e2e run printed line 140 for a line-8 option). The message names "code chunk N of the document"
  (`cell.index + 1`) instead.
- **Tests (red first).** Unit tests in `chunk_records.rs` (key caught, ordinal, `--|` prefix in `{sql}`, blockquote, no record means
  no check, code-line not an option, `Q-18-17` mapping). I disabled the check and saw `a_chunk_that_sets_the_reserved_key_is_an_error_naming_it`
  fail (got `None`, wanted the key) before enabling it. nextest stops at the first failure, so the other new tests were
  not individually seen red. Integration: `knitr_chunk_that_sets_the_reserved_key_fails_with_q_18_17` added;
  `knitr_chunk_id_is_not_emitted_as_an_attribute` now renders with `execute: record: true` and no author key, so its leak
  assertion checks core\'s own injected id (before, a plain render built no record and the key was the author\'s).
- **End to end (ran, inspected).** `target/debug/q2 render bad.qmd` (knitr, `execute: record: true`, chunk with
  `#| quarto-unit-id: abc123`) printed `Error [Q-18-17]: Reserved Chunk Option ... the knitr chunk option quarto-unit-id (in code
  chunk 1 of the document) is reserved by Quarto for execution records` and `1 error`, with no R duplicate-key text. The same
  document without the key rendered normally through knitr.
- **Gates (ran).** `cargo clippy -p quarto-core --all-targets -- -D warnings` clean; `cargo nextest run -p quarto-core`: 8262 passed, 36 skipped.
  `cargo xtask lint`: one violation, `metadata-as-str` at `positron_cache.rs:360`, a file this plan does not touch (pre-existing
  on the integration branch; not mine, not fixed here). The Windows cross-clippy could not run: `aws-lc-sys` needs
  MSVC headers (`stdlib.h`, `windows.h`) on this machine. The change uses no platform API.
- **Not done.** No WASM build (the change is in `engine/knitr`, native-only; not under the protocol\'s WASM list). No Julia/Jupyter run (out of scope).
- **Snapshots.** None changed.
