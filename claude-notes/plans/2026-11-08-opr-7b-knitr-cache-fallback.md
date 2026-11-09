---
title: 'OPR-7b: cached knitr chunks use the markdown fallback and emit no record'
date: 2026-11-08
description: 'Gordon decided (2026-11-08): a knitr chunk that asks for the cache keeps knitr''s cache and its finished markdown, and emits no structured record; it degrades quietly. Reverses the S4 rule (cache off while recording). OPR-0c found that a record merely missing the chunk is dropped whole by the unit-count check, so cached units are excluded from the expected count. Retires the Q-18-12 message and updates the test and the knitr plan note that pinned the old rule.'
status: ready
depends-on: [opr-0c]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Branch:** `opr/7b-knitr-cache-fallback` · **Lane B** (`workspace-4`) · **Blocks:** OPR-7c, OPR-7d.
**Needs:** R 4.3 with knitr (the tests that need R skip without it, but this plan cannot be verified without R).

## Start condition

OPR-0c\'s note (the cache spike) must say a mixed document comes out right (first and second run the same, cached
chunks fall back, structured chunks present). If it says otherwise, **stop** and report; the plan is then
`blocked-on-gordon` and its design needs Gordon.

**What OPR-0c found (2026-11-09; [note](../research/2026-11-09-opr-measurements.md) sections 4 and 5, ran).** The start
condition holds at the knitr level (miss and hit identical at all three cache levels, `dependson`, `autodep`, document
`cache`). Through q2 the plain rule "keep the cache, do not record the chunk" is **not enough on its own**: a throwaway
`capture.R` that kept `cache = TRUE` and returned before recording gave 3 records for a 5-unit document, and the splice
dropped the whole record (`Engine Execution Record Dropped`, empty `doc.ipynb`). The render fell back to R\'s markdown
and was correct (figures 1 to 3, refs resolved, miss equal to hit), so nothing breaks, but nothing is presented.

**Which knitr hooks run on a cache hit (ran, 2026-11-09, R 4.3.2, knitr 1.50; a four-chunk document knit twice).** On the
hit, `opts_hooks$cache` ran for every chunk. `evaluate` ran for none of the cached chunks. The `chunk` hook ran for
the level-1 and level-2 chunks (`cache=1`, `cache=2`) and **did not** run for the level-3 chunk (`cache=TRUE`).
Output was identical between the runs. So a cached chunk may reach the chunk hook with no evaluation behind it
(levels 1 and 2), and `capture_chunk_hook` would then record it as a source-only unit, which presents as an `eval: false`
chunk: wrong, and silent. Level 3 reaches no recording hook at all, so the hook cannot be what removes the chunk from the
record.

**Design (replaces the earlier "fallback unit" idea; checked against the Rust side by reading).**

- R side: `opts_hooks$cache`, the one hook that runs for every chunk on a miss and a hit at every level, notes the unit
  id (`options[["quarto-unit-id"]]`) of each chunk that asked for the cache, and does not force `cache` off. The chunk hook
  returns knitr\'s real markdown, with no marker, and records nothing, for any chunk in that set (this also covers
  levels 1 and 2 on a hit). `execute.R` reports the set in the result where it reports `cacheDisabled` today
  (`execute.R:291`).
- Rust side: the unit ids in that set are **excluded from the expected unit count** that raises
  `EngineRecordProblem::CountMismatch` (`engine/diagnostics.rs`), so a record of the other units is accepted. Their
  markdown is in the skeleton in place with no marker, and `rerange_onto_markers` (`engine/skeleton.rs:43`) already skips
  a unit whose marker is absent (read), so they are neither presented nor replaced. They are not part of the record, which
  is what the docs plan already says (OPR-8).
- A document where every chunk asked for the cache (a document-level `cache: true`) has no unit left to record: no
  record, the engine\'s own markdown, the same result as today\'s cache-refused case, and quiet.
- Expected result for the 0c mixed document: a record of the **three** units that are not cached (two figures and a
  text chunk), no `Record Dropped` warning, figures 1, 2, 3 across the live and cached chunks, the same output on a miss
  and a hit.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c opr/7b-knitr-cache-fallback feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7b-knitr-cache-fallback
```

## Goal (decision D5)

Why the cache is off today (read): the capture hooks knitr\'s `evaluate` step, and a cache hit skips it.
`cache=TRUE` (level 3) returns the cached output string before the chunk hook runs (`block.R:100-101` of knitr);
levels 1 and 2 cache the piece list but a hit reads it inside the code that would call `evaluate()`. Spike S4 (knitr
epic) therefore chose "cache off for the recording run": `capture_install`
(`crates/quarto-core/src/engine/knitr/resources/rmd/capture.R`) sets
`knitr$opts_hooks$cache <- function(options) {... options$cache <- FALSE ...}` for **every** chunk.

New rule: a chunk that asks for the cache (including through a document-level `execute: cache: true`) **keeps**
knitr\'s cache, is not recorded, and its finished markdown stays in the skeleton in place (design in the start-condition
section above). It degrades quietly: no diagnostic. Chunks that do not
ask for the cache are recorded as before. LaTeX-family targets are already on the fallback route and keep their cache.

## Checklist (tests first; every box is `[unattended]`)

- [ ] Read OPR-0c\'s cache-spike results and the hook finding above, and the knitr epic\'s S4 section
      (`claude-notes/plans/2026-10-30-knr-01*.md`, "S4 decision").
- [ ] **Tests first** (R required, skip without it; follow knitr epic decision 17): a document with a cached chunk, run
      twice in fresh R processes: both runs give the same document; the cached chunk is not in the record and its
      markdown is in place; the other chunks are structured; levels 1, 2 and 3; `dependson`; a document-level
      `execute: cache: true`; the cached chunk\'s figures and numbering. **Update the test that pins the old rule,**
      `a_cache_written_without_capture_is_not_replayed_by_a_recorded_run` (it is in
      `crates/quarto-core/src/engine/knitr/capture_tests.rs`, not under `tests/`; it passes today, OPR-0c ran it), and say
      how and why in the commit message. Add the 0c document as a test: chunks `fig-live1` (figure, caption), `fig-cached`
      (`cache: true`, figure, caption), a live text chunk, `fig-live2` (figure, caption), a cached text chunk, and
      references to the three figures; on a miss and on a hit expect figures 1, 2, 3, resolved references, a record of the
      three uncached units, **no** `Engine Execution Record Dropped`, and identical output between runs. Also pin the hook
      finding (an R-level test, skipped without R): after a miss, a hit runs no `evaluate` for cached chunks, and the
      recorder adds nothing for a level-1 or level-2 chunk even though the chunk hook ran. And a round trip: the replay
      path (`knitr_replay_equals_live`) on the mixed document is equal to the live render.
- [ ] Implement the design above: `capture_install` (note cached unit ids in `opts_hooks$cache`, stop forcing the cache
      off, chunk hook returns real markdown and records nothing for them), `execute.R` (report the set), and the Rust
      expected-count exclusion. Check that ordinals still line up for the units after a cached chunk. Keep `quarto_capture$cache_labels` only if something still reads it.
- [ ] **Q-18-12** (docs/errors/engine/Q-18-12.qmd; catalog entry in `crates/quarto-error-catalog/error_catalog.json`)
      says the cache is off while recording: stop emitting it. Leave the catalog entry and docs page in place (the
      error-docs lint requires a page for every catalog code) with one sentence that it is no longer emitted. Do not
      edit other docs; OPR-8 owns them.
- [ ] Add a dated note to `claude-notes/plans/2026-10-30-knr-06-flip-and-consumers.md`, risk 5 ("Caches and the flip"):
      superseded by this plan.
- [ ] If OPR-0b has landed, flip the corpus fixture marked `pending OPR-7b` to its new expectation; if not, leave
      it for OPR-7c and say so in the notes.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy; `cargo xtask lint`; `target/debug/q2 render --strict` on the edited plan note.
- [ ] **End-to-end:** `q2 render` the 0c mixed document above (and one with a cached chunk and two plain chunks), plain
      and with `execute: {record: true}`, each twice (miss, then hit); read the outputs and the record file; record the
      invocations and what you saw. Include that the log has no `Record Dropped` warning.

## Exit criteria

- A cached chunk keeps its cache, is excluded from the record and from the expected unit count, and the document is
  correct on first and second run; other chunks present from the record.
- No diagnostic is emitted for a cached chunk; the old pinned test is replaced; the knitr plan note is updated.

## Notes from the run

(Fill in at landing.)
