---
title: 'OPR-9c: a reserved option key gets a diagnostic (quarto-unit-id)'
date: 2026-11-10
description: 'The record path writes a quarto-unit-id option line into every knitr chunk it owns. A chunk that already carries the key makes R fail with Duplicate map key. Gordon decided the key is reserved and a diagnostic says so. The plain-render flip would otherwise expose the crash to every knitr render.'
status: ready
depends-on: [opr-9a]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Origin:** split from [OPR-7d](2026-11-08-opr-7d-switch-plain-render.md) (now a tombstone) on 2026-11-10 by Gordon's decision; evidence in the
[7d implementability review](../research/2026-11-10-opr-7d-implementability-review.md) and its addendum, and the [7d plan review](../research/2026-11-10-opr-7d-plan-review.md).
**Branch:** `opr/9c-reserved-unit-id-key` · **Lane B** (`workspace-4`) · **Blocks:** 9f, 9g.
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

Start when 9a is `complete`. Read its S5 (which other keys an author can reach). If S5 found more reserved keys, this plan covers
them too; if it found none, it covers this one.

## Design

- The check lives where the duplicate is created: in `insert_unit_ids`, before the id line is built, so it fires exactly where
  the crash fires today and does not change a render that builds no record. If an option line of the chunk already names a
  reserved key (the option block uses `#|`, or the comment prefix of the language: `comment_syntax_for`), return a diagnostic
  that names the key, says it is reserved by Quarto for execution records, points at the chunk, and tells the author to
  remove it. Severity: an error (a document that sets it cannot be recorded). **[Gordon, confirm]** if you prefer a warning that
  drops the author\'s line; the default here is the error.
- After the flip every knitr render builds a record, so the error fires on every knitr render with the key. That is intended.
- A diagnostic with a `Q-*` code needs the catalog entry, its `docs/errors/<subsystem>/<code>.qmd` page and its
  `docs/_quarto.yml` sidebar entry in the same commit (`cargo xtask lint` rules `error-docs-page-missing` and
  `error-docs-sidebar-unlisted`; template `docs/errors/README.md`). Pick the code and subsystem from the neighbors of the other
  knitr diagnostics.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Tests first.** (1) A unit test on `insert_unit_ids`: a chunk with `#| quarto-unit-id: abc123` returns the diagnostic naming the
      key; a chunk with other options and no key is unchanged; a chunk in a `{sql}` fence (comment prefix `--`) with the key is also
      caught. (2) The existing integration test `knitr_chunk_records::knitr_chunk_id_is_not_emitted_as_an_attribute` changes
      meaning: it now asserts that the render fails with the diagnostic (or move its leak assertion to a chunk without the key).
      Watch (1) fail first, for that reason. Skip cleanly when R is absent, as the knitr tests do.
- [ ] **Implement** in `insert_unit_ids`; add the catalog code, docs page and sidebar entry if a `Q-*` code is used.
- [ ] **End to end** through the binary: `q2 render` of a knitr document with the key, once with `execute: record: true` (fails with
      the diagnostic today after this plan) and a normal one without the key (renders). Record the invocation and the message you saw.
- [ ] **Gates:** `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the Windows
      cross-clippy; `cargo xtask lint`.

## Stop conditions

Stop, commit your progress, write "Notes from an earlier attempt" and tell Gordon (the protocol's "When you cannot
finish") if any of these happens:

- S5 found a key whose fix is not a diagnostic (an author can set it and the render silently changes): report it;
- the diagnostic needs a new field on `ExecutionContext` or a change to the engine wire: report it;
- R is not available: the knitr tests cannot run, so say so and do not land.

## Handoff

End the session with the protocol's handoff ("Ending your session"). Name 9d as the lane's next piece if it is startable.

## Exit criteria

A knitr chunk that sets `quarto-unit-id` produces a diagnostic naming the key as reserved, on every route that builds a record;
no render with the key reaches R\'s duplicate-key error.

## Notes from the run

(Fill in at landing.)
