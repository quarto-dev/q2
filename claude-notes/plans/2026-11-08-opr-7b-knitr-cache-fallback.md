---
title: 'OPR-7b: cached knitr chunks use the markdown fallback and emit no record'
date: 2026-11-08
description: 'Gordon decided (2026-11-08): a knitr chunk that asks for the cache keeps knitr''s cache and its finished markdown, and emits no record; it degrades quietly. Reverses the S4 rule (cache off while recording) after the OPR-0c spike shows a mixed document comes out right. Retires the Q-18-12 message and updates the test and the knitr plan note that pinned the old rule.'
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
knitr\'s cache, is not recorded, and uses knitr\'s finished markdown through the existing per-chunk **fallback unit**
(knitr epic decision 8; stored as `text/x-quarto-markdown`). It degrades quietly: no diagnostic. Chunks that do not
ask for the cache are recorded as before. LaTeX-family targets are already on the fallback route and keep their cache.

## Checklist (tests first; every box is `[unattended]`)

- [ ] Read OPR-0c\'s cache-spike results and the knitr epic\'s S4 section
      (`claude-notes/plans/2026-10-30-knr-01*.md`, "S4 decision").
- [ ] **Tests first** (R required, skip without it; follow knitr epic decision 17): a document with a cached chunk, run
      twice in fresh R processes: both runs give the same document; the cached chunk is a fallback unit with no
      structured outputs; the other chunks are structured; levels 1, 2 and 3; `dependson`; a document-level
      `execute: cache: true`; the cached chunk\'s figures and numbering. **Update the test that pins the old rule,**
      `a_cache_written_without_capture_is_not_replayed_by_a_recorded_run` (search for it in
      `crates/quarto-core/tests/`), and say how and why in the commit message.
- [ ] Change `capture_install` so a chunk that asks for the cache is neither forced off nor recorded, and the chunk
      hook writes the fallback unit. Keep `quarto_capture$cache_labels` only if something still reads it.
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
- [ ] **End-to-end:** `q2 render` a document with one cached chunk and two plain chunks, plain and with
      `execute: {record: true}`; read both outputs; record the invocations and what you saw.

## Exit criteria

- A cached chunk keeps its cache, is a fallback unit with no record, and the document is correct on first and second
  run; other chunks present from the record.
- No diagnostic is emitted for a cached chunk; the old pinned test is replaced; the knitr plan note is updated.

## Notes from the run

(Fill in at landing.)
