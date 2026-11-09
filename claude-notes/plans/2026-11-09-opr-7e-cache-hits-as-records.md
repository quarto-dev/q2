---
title: 'OPR-7e: a cached knitr chunk is recorded and presented like any other chunk'
date: 2026-11-09
description: 'OPR-7b leaves a chunk that asks for knitr''s cache out of the record, so it never goes through the presenter. This plan starts with a spike on whether a cache hit can be recorded at each of knitr''s three cache levels with no change to knitr (a wrapper around the cache read for levels 1 and 2, a self-describing payload stored in knitr''s own cache entry for level 3), and then implements what the spike supports. Amends decision D5 (Gordon, 2026-11-09).'
status: ready
depends-on: [opr-7b]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/7e-cache-hits-as-records` · **Lane B** (`workspace-4`) · **Blocks:** nothing hard. It should land before
OPR-7d, because after 7d every plain render writes cache entries, and entries written before this plan carry no record
payload (see "Cache entries written before this plan").
**Review:** [plan review, 2026-11-09](../research/2026-11-09-opr-7e-plan-review.md) (approve with changes; read it before
Phase A: it proposes design edits that Gordon has not decided yet).
**Needs:** R 4.3 with knitr, at the minimum knitr q2 supports (1.44) and at the newest installed. The tests that need R
skip without it, but this plan cannot be verified without R.

## Start condition

[OPR-7b](2026-11-08-opr-7b-knitr-cache-fallback.md) is `complete` (it is). Nothing else. Phase A is a spike; Phases B to
D run only for the levels the spike supports (see "Decision rules").

## Why this plan exists

OPR-7b lets a chunk keep knitr\'s cache, leaves it out of the record and out of the expected unit count, and leaves its
finished markdown in the skeleton. That is correct and quiet, but a cached chunk then bypasses the one presenter: it
gets knitr\'s own markdown, not the presenter\'s, so it cannot differ from the live route (or from Positron\'s view of the
record) and a cache hit changes the page\'s provenance. The epic\'s goal is that every recorded cell goes through one
presenter path. **Gordon, 2026-11-09: a cache hit should be a record cell, if it can be done without changing knitr.**
This amends decision D5 ("a cached knitr chunk ... uses the markdown fallback with no structured record"). It does not
reopen the rest of D5 (LaTeX family, `pptx`, `context`, no opt-out control).

The plans so far chose between "turn the cache off" (KNR-1 S4) and "keep it and record nothing" (OPR-7b). Neither asked
whether knitr hands q2 enough on a hit. This plan asks.

## What knitr stores and returns on a hit

Facts from `external-sources/knitr` at `56f2cadf`, a development snapshot (read, not run; locate by symbol, since line
numbers shift between releases; the released versions q2 supports are checked in the spike):

| Level | Stored on a miss | Returned on a hit |
|---|---|---|
| 3 (`cache=TRUE`) | `output`, the **chunk hook\'s return value** (`block_exec`, `block_cache(options, output, objs)`) | `cache$output(hash)` straight from `call_block`, before any hook runs, so neither `evaluate` nor the chunk hook is called (ran, OPR-0c) |
| 1 | the `evaluate()` list, `res.orig` | `cache$output(options$hash, 'list')` in `block_exec`, in place of the `evaluate()` call; the filters, `sew` and the chunk hook then run normally |
| 2 | the same list, with each plot reduced to a digest by `remove_plot` (`plot.R`) | the same call, so the plots arrive as digests |

- `cache$output` has exactly two call sites, one per hit path above (read: `grep 'cache\$output' R/*.R`). `cache` is a
  list of closures bound in the knitr namespace, and q2 already replaces namespace bindings in `patch.R`
  (`assignInNamespace`, for `sew`, `add_html_caption`, `parse_block`).
- The level-3 key is every chunk option except `cache0.opts` (`include`, `out.width.px`, `out.height.px`,
  `cache.rebuild`), so it includes `quarto-unit-id`. The level-1 and level-2 key is `cache1.opts` (the code, `eval`,
  `message`, `warning`, `error`, and a few cache options) and, for level 2, `cache2.opts` (the plot options). It does not
  include the id.
- `cache_purge` removes every file `<hash>.*`, so anything stored in the cache entry follows knitr\'s own invalidation
  (`cache.rebuild`, `dependson`, `autodep`) for free (read).
- q2\'s current hooks live in the knitr options list `execute()` hands to rmarkdown, so rmarkdown restores them when the
  render ends. A namespace patch would **not** be restored that way; the spike must say how to scope it (S7).

## Design under test (a hypothesis; Phase A can overturn any part)

**Levels 1 and 2: wrap the cache read.** While capture is on, replace `knitr:::cache` with a copy whose `output`
closure, when called with `mode = 'list'`, calls the original and hands the list to the same code that
`capture_evaluate_hook` uses (`capture_select_plots`, then `quarto_capture$slot`). The chunk hook then records the chunk
as it records any chunk. No new record format.

**Level 3: store a self-describing span in knitr\'s own cache entry.** The chunk hook for a chunk with `cache == 3`
returns, instead of the bare marker it returns for an uncached chunk, the chunk\'s finished markdown **between two
comments**, the closing one carrying the chunk\'s capture record:

```text
<!-- q2-cache:ID:begin -->
...knitr\'s finished markdown for the chunk...
<!-- q2-cache:ID:end:BASE64 -->
```

`BASE64` is the JSON of the chunk\'s wire record (the entry `quarto_capture$records` gets) and its `chunks` entry (the
finished markdown, options digest and files that `hooks.R` collects for a chunk). Knitr stores this string on a miss
and returns it on a hit. One normalizing step over the knit result, **on the miss and on the hit**, then replaces each
span:

- capture on: the span becomes this run\'s marker line (`<!-- q2:NONCE:ID -->`); when this run did not record the id
  live (a hit), the step first adds the payload\'s record and chunk entry to the capture\'s results, in document order;
- capture off: the span becomes its inner markdown, with both comments dropped.

The inner markdown is the point of the shape. The stored string must stay valid for a run that does not capture
(a LaTeX target, a plain render before OPR-7d, an older q2), and it must give Rust the retained fallback markdown that
KNR decision 24 keeps for a chunk it cannot present. A hit with no payload (see below) stays what OPR-7b made it: absent
from the record, markdown in the skeleton.

`capture_chunk_hook` and `opts_hooks$cache` already identify cached chunks by the `quarto-unit-id` option. After this
plan `cached_units` holds only the ids that asked for the cache **and ended the run unrecorded**, so the Rust
exclusion from the expected count (`engine/knitr/mod.rs`, the `cached_units` filter) needs no new rule.

## Cache entries written before this plan

A level-3 entry written by an earlier q2 (or by a render with capture off) holds plain markdown and no payload. In a
capture run its chunk stays absent from the record, as in OPR-7b: correct, quiet, and gone once the entry is rebuilt.
**Default: do not salt the cache key** to force a rebuild. (Correction from the review, read: a plain run carries no
`quarto-unit-id`, so its level-3 keys already differ from a record run\'s and the two never share an entry; a salt would
not change that. The spike confirms it; see the review note, finding 1.) The decision is the agent\'s, recorded in the notes.

## Decision rules (what the spike\'s result changes)

- **Levels 1 and 3 hold:** implement both. **Level 2 holds:** implement it. **Level 2 does not:** leave level 2 as
  OPR-7b has it (absent, quiet) and say so in the notes and the docs hand-off; do not build a partial level-2 path.
- **Level 3 does not hold but level 1 does:** implement level 1 only; level 3 stays as 7b.
- **Neither holds, or the payload changes the page or the cache key in a way the spike cannot contain:** stop. Write the
  spike note, set `status: blocked-on-gordon`, and say that an upstream knitr hook (one documented hook called on every
  cache hit with `(options, level, output-or-list)`) is the remaining option. Land only the note.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c opr/7e-cache-hits-as-records feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7e-cache-hits-as-records
```

## Phase A: the spike (a note; scratch scripts in your scratchpad)

Output: `claude-notes/research/2026-11-09-opr-7e-cache-record-spike.md`, with the questions below answered one by one,
each marked (ran) or (read), the knitr versions it ran on, and the scripts\' invocations. Prototype in plain R with
knitr first (as OPR-0c did), then through `capture.R` with a throwaway edit that you do not land. Use the OPR-0c mixed
document (`fig-live1`, `fig-cached`, a live text chunk, `fig-live2`, a cached text chunk, three `@fig` references) as the
base and add the cases named below. Each document is knit twice in **fresh R processes** (a miss, then a hit).

- [ ] **S1, level 3 round trip.** With the span in the chunk hook\'s return value: (a) the miss run\'s record equals
      today\'s; (b) the hit run, capture on, yields a record and `chunks` entry equal to the miss run\'s after the nonce
      and ordinals are normalized; (c) a hit run with capture **off** gives markdown identical to a never-cached run and
      no comment survives; (d) the final html of miss and hit is identical. Say what the stored string looks like
      (`cache$output(hash)` on the cache entry).
- [ ] **S2, level 1.** The `cache$output` wrapper: the hit\'s record equals the miss\'s. Confirm by `grep` over the knitr
      versions you test that the two call sites above are the only ones, and that `mode = 'list'` distinguishes them.
- [ ] **S3, level 2.** The wrapped list holds digests for plots. Does `capture_plot_files` find the figure files that
      the miss wrote, so the hit\'s record equals the miss\'s? If not, say exactly what is missing (a plot piece, a file
      path) and whether a rebuild from the options is possible.
- [ ] **S4, ids and keys.** Level 3 puts `quarto-unit-id` in the key. Insert a paragraph above a cached chunk, add a
      chunk above it, and reorder two cached chunks: does the entry still hit, and does the payload\'s id still match the
      unit? Say how Rust matches a payload whose id is no longer a unit (`match_cells` falls back to the content hash).
- [ ] **S5, the awkward chunks at level 3.** An htmlwidget chunk (its `knit_meta` rides in `.meta`), a `results='asis'`
      chunk, `include=FALSE`, a chunk with `child=` (knitr returns before the cache check; expect no change), `dependson`
      and `autodep`, `cache.lazy`, and `cache.rebuild`.
- [ ] **S6, versions.** S1 to S3 on knitr 1.44 (the minimum), 1.50 and the newest you can install (KNR-1\'s matrix; see
      its S1 section and `KNR_R_LIBS` for how the corpus pins a library). An unrecognized shape must degrade to today\'s
      absent-and-quiet, never to wrong output.
- [ ] **S7, scoping the namespace patch.** Show how the replaced `cache` binding is installed only while capture is on
      and is restored when the run ends (and on error), so a long-lived R process cannot keep a patched cache.
- [ ] **S8, the key salt.** What does forcing a rebuild of legacy level-3 entries cost (what keys change, does a
      capture-off run then thrash the cache)? One paragraph; the default stays "no salt".
- [ ] **S9, size.** The payload per chunk for the corpus (median, largest). Figures are paths, not bytes.
- [ ] Write the note, apply the decision rules above, and record the outcome at the top of "Notes from the run" in
      this plan. If the rule says stop, stop here.

## Phase B: tests first (R required; skip without it, knitr epic decision 17)

Write each test, run it, see it fail for the stated reason, then implement. Tests live next to the OPR-7b ones
(`engine/knitr/capture_tests.rs`, `engine/knitr/gating_tests.rs`) and, for the end to end, in the knitr corpus runner.

- [ ] A level-1 hit records the chunk (a test beside `a_cache_hit_records_nothing_for_a_cached_chunk_at_any_level`,
      which **pins the old rule and must be changed**: say how and why in the commit message). Likewise for levels 2 and
      3 as far as the spike supports.
- [ ] A level-3 hit, capture on: the record, the `chunks` entry and the skeleton marker exist, with the live
      run\'s nonce; the payload comment is gone from the markdown.
- [ ] A level-3 hit, capture **off**: markdown equal to an uncached render; no `q2-cache` comment anywhere.
- [ ] The 0c mixed document (`a_mixed_cached_document_records_the_uncached_units_on_a_miss_and_a_hit`): now a record of
      **all five** units on a miss and on a hit, the same figures 1, 2, 3, resolved references, no
      `Engine Execution Record Dropped`, identical output between runs. That test pins "three" today; update it.
- [ ] A legacy level-3 entry (written with capture off, hit in a capture run): the chunk is absent from the record and
      from the expected count, quiet, as in 7b (`a_cache_written_without_capture_is_replayed_by_a_recorded_run` stays
      green).
- [ ] A payload for an id that is no longer a unit is ignored, with no diagnostic and no wrong cell.
- [ ] Replay: `a_replayed_record_with_a_cached_chunk_equals_the_live_run` still holds, and now the replayed cached chunk
      is presented from the record, not from stored markdown.
- [ ] A stale or truncated payload (a corrupted comment) degrades the chunk to absent-and-quiet, never to an error.

## Phase C: implement what the spike supports

- [ ] `capture.R`: the `cache$output` wrapper for `mode = 'list'` (levels 1, 2), installed in `capture_install` and
      removed when the run ends, per S7. The wrapper feeds `quarto_capture$slot` as `capture_evaluate_hook` does.
- [ ] `capture.R` and `hooks.R`: the level-3 chunk hook returns the span (inner markdown, closing comment with the
      payload). The payload holds the wire record and the `chunks` entry; locate both by symbol
      (`quarto_capture$records`, `quarto_chunk_records$records`). Chunks that do not ask for the cache are unchanged.
- [ ] The normalizing step over the knit result (the `document` hook in `hooks.R` or the end of `execute()`; the spike
      says which), capture on and off as above. It must run on the miss as well, so the skeleton never holds a span.
- [ ] `capture_chunk_hook`: a cached chunk that records no longer returns early. `cached_units` is what ends the run
      unrecorded (compute it at the end of the run, not in `opts_hooks$cache`).
- [ ] `execute.R` reports the result as it does today (`pieces`, `chunks`, `cachedUnits`); `knitr/mod.rs` should need no
      change beyond the tests. If it does, say what and why.
- [ ] Remove or reword any comment that says a cached chunk is never recorded (`capture.R` header, `capture_install`,
      the chunk hook).

## Phase D: parity and end to end

- [ ] Re-run the parity gate on the cached documents (the harness from OPR-0b; OPR-7c found `knitr/cache-mixed`
      identical, so any new difference is this plan\'s); the command is under this list. Classify each difference as knitr epic decision 26 does. A new difference is a blocker for Gordon unless it is a
      plain bug in this plan\'s code.
- [ ] **End to end** (`target/debug/q2 render doc.qmd`), each run twice (miss, then hit), reading the outputs and logs:
      the 0c mixed document plain and with `execute: {record: true}`; a level-1 and a level-2 document; a document with
      an htmlwidget chunk at level 3; and a PDF render of a document whose cache was written by an html render (the
      capture-off hit). Record the invocations and what you saw: the record\'s cell count (expect all units), that the
      html is identical between runs, that no `q2-cache` comment reaches any output, the skeleton text in the stored
      trace, or any cell of the record file (`.quarto/execution-records/doc.ipynb`; `grep q2-cache` it), and that no
      `Record Dropped` appears.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy (it failed to build on a macOS host in OPR-7b and 7c; say so if it does again); `cargo xtask
      lint`; `target/debug/q2 render --strict` on this plan and the spike note. `npm run build:wasm` only if a file under
      `presenter/`, `record/` or `engine/{replay,ipynb_replay,skeleton}.rs` changed.

The parity command (Phase D, first item):

```bash
OPR_PARITY_REPORT=1 OPR_PARITY_ONLY=cache cargo nextest run -p quarto-core \
  -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture
```

## Exit criteria

- The spike note exists and the decision rule\'s outcome is recorded.
- For each level the spike supported: a cache hit is a record cell and presents through the one presenter, on a miss
  and on a hit, with identical output between the two and no `Record Dropped`.
- No payload or marker text reaches any output, in a capture run or a plain one.
- A cache written before this plan still renders correctly (absent from the record, quiet).
- The knitr corpus and the OPR parity gate show no new difference on the cached documents.

## Out of scope

- A knitr upstream change (only if the decision rules send us there, and then as a note, not work).
- Writing a payload for non-R engines or for `child=`.
- Docs: OPR-8 owns them. When this plan is `complete`, OPR-8 should say that a cache hit presents like any other chunk
  and mention the legacy-entry case; OPR-8 reads this plan\'s notes.

## Notes from the run

(empty)
