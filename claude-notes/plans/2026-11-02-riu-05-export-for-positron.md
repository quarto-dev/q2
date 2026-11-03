---
title: 'RIU-5: export for Positron'
date: 2026-11-02
description: 'A Rust export_for_positron function that turns a q2 execution record into a cache file Positron will accept and display, implementing the transform list that RIU-0 S7 settles, checked against the mirror of Positron loader in the conformance tests, and exposed through the CLI so the TypeScript commands do not reimplement it.'
status: draft  # written 2026-11-02; the transform list is provisional until RIU-0 S7 reports
---

**Part of:** [the records-in-use epic](2026-11-02-records-in-use-epic.md)
**Depends on:** [RIU-0](2026-11-02-riu-00-spikes.md) S7 (what Positron displays) and RIU-4
(the CLI to expose it).
**Blocks:** RIU-6 command 2.
**Estimated sessions:** 1.

> **For implementers:** tests first, against the `positron_loader` mirror in
> `positron_conformance.rs`, which is the oracle. S7 found what Positron does on real data;
> where S7 and the mirror disagree, fix the mirror in the same change and say so.

## Overview

The reverse of RIU-3: q2 has a record, and Positron needs a file it accepts. Positron deletes
invalid cache files, consumes a hand-placed file on a content-hash match, and rewrites it
keeping only `quarto_cell_id` and `quarto_content_hash` per cell. The function returns bytes;
it never writes into Positron\'s directory (the extension does, RIU-6).

## Transform list (provisional until S7)

- Gate each unit: Jupyter-owned, plain language, code cell kind, not front matter.
- Refuse, with a reason each, rendered units (`text/x-quarto-markdown`, including knitr
  fallback units until KNR-8), inline units and front-matter spans.
- Stringify data values: arrays joined into one string, JSON mime values serialized to strings
  (Positron requires strings).
- Always emit an `outputs` array on every cell, empty when there are none.
- `quarto_cache_version: 1`, `metadata.quarto_source` set to the document file URI the
  caller supplies, reduced per-cell metadata.
- For a duplicate-content group, emit the cells Positron\'s pairing expects; skip the rest of
  the group. **S7 decides the pairing rule.**
- Unverified, settled by S7 (U20): stream arrays, `image/png` display, error output shape, and
  the exact form of `quarto_source`.

## Phase A: tests first

- [ ] One test per transform: stringification (array and JSON mime), `outputs` always present,
      metadata reduction, version stamp, source URI passthrough.
- [ ] One test per refusal class, asserting the reason and that the other cells still export.
- [ ] A round trip: record to export to the `positron_loader` mirror accepts it, and the mirror
      yields the outputs the record held, for the Python fixtures from `positron_demo.rs`.
- [ ] A negative test: an export that the mirror would delete (for example missing `outputs`)
      cannot be produced. Build the bad case by mutating the output, not the function.
- [ ] The inverse check with RIU-3: export then import gives back the same outputs for the
      cells that exported.

## Phase B: code

- [ ] `export_for_positron(&Record, &ExportOptions) -> Result<ExportedCache, ExportError>`
      where `ExportOptions` carries the source URI. `ExportedCache` holds the bytes and the
      list of skipped units with reasons. No file writes, no environment reads.
- [ ] `ExportError` for what cannot be recovered (no exportable cells is not an error; an
      invalid record is).
- [ ] The CLI subcommand from RIU-4 calls this function. Nothing else reimplements it.
- [ ] Place it next to `positron_import.rs`, and keep it target-neutral so a WASM caller is
      possible later; do not add a WASM export in this plan.

## Phase C: verify

- [ ] `cargo clippy -p quarto-core --all-targets -- -D warnings`
- [ ] `cargo nextest run -p quarto-core`
- [ ] Copy-based check with a real Positron: put an exported file into a scratchpad copy of the
      cache layout and, with Gordon at the window, confirm the document shows the outputs
      (this is S7\'s method; reuse its fixtures). If no human is available, mark the step not
      run; the mirror test is not a substitute for seeing it.

## Exit criteria

- Export passes the mirror for every fixture and refuses what S7 says to refuse.
- The CLI contract from RIU-4 is implemented against this function.
- What Positron was seen to display is recorded; what was only checked against the mirror is
  labeled as such.

## Risks

- **The mirror is a model of Positron, not Positron.** Real-window confirmation is the only
  proof; keep S7\'s evidence linked.
- **Rendered and fallback units** are refused, so an R document run through knitr exports
  little until KNR-3 and KNR-5 land. That is expected; the demo does not include it.
