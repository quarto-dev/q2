---
title: 'RIU-7: flip the provide-hub writer default (deferred)'
date: 2026-11-06
description: 'Deferred until everything else in the records-in-use epic is in place and has had time to settle and be distributed. Flip the provide-hub writer gate on once the hosted hub runs a q2_ref that contains the record reader, and update the runbook to match.'
status: deferred  # moved out of RIU-1 Phase E on 2026-11-06; execute only when the epic's other plans have landed, released and settled
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Moved from:** [RIU-1](2026-11-02-riu-01-enablement-and-writer-flip.md), Phase E.
**Depends on:** RIU-1 to RIU-6 landed, released and settled; the hosted hub deployed from a
`q2_ref` that contains the record reader.
**Blocks:** nothing. No other plan in the epic waits on this one: `provide-hub` keeps writing the
legacy format, and `QUARTO_ENGINE_RECORD_WRITE=1` is the opt-in meanwhile (epic decisions 2 and 9).
**Estimated sessions:** under 1, plus the deploy, which Gordon runs.

## Overview

RIU-1 flipped the preview writer default on and left `provide-hub` off, because a remote hub's
hub-client is whatever was deployed and cannot be assumed to parse a record document (an older
reader shows a full-page "Render Error: Failed to parse capture"). This plan flips the
`provide-hub` default, the second half of that table, once the hosted hub can read records.

It is deliberately last. Do not start it until the other plans have shipped in a release and
people have had time to pick up the new binary and the new hub.

## Checklist

Sequence, to be confirmed with Gordon, who runs the deploys:

- [ ] (1) The reader and the RIU work reach `main` and ship in a release.
- [ ] (2) Run "Build and deploy artifacts" with `q2_ref=main` and deploy; `cat
      /etc/quarto-hub/version.json` on the instance names a ref that contains the reader.
- [ ] (3) Flip the `provide-hub` default in `WriterSurface::default_gate`
      (`crates/quarto-core/src/engine/capture_files.rs`), with its own test update
      (`gate_default_is_per_surface_and_overrides_apply_to_both`) and the comments that say
      `provide-hub` is off by default (`execute.rs`, `capture_files.rs`).
- [ ] Tell anyone running `provide-hub` against quarto-hub.com before step 3. `--server` or
      `$QUARTO_HUB_SERVER` can aim `provide-hub` at any hub whose version is not visible, so the
      flip also changes behavior for self-hosted or older hubs; say so in the release notes.
- [ ] Check a viewer sees the output with `q2 provide-hub --records positron` and the gate on,
      including what a guest on an older client sees (a full-page Render Error, not a fallback).
- [ ] At step 3, update the runbook ("Engine-record writer gate") so the `provide-hub` row of the
      per-surface rule reads on, keeping the rollback hazard (`quarto-hub-rollback` to a build
      without the reader strands record documents already written), and the user docs that say
      `provide-hub` still uses the older format.

## Exit criteria

- The `provide-hub` default is on, tested, and the runbook and docs say so.
- The deployed hub version was read from the instance, not assumed.
