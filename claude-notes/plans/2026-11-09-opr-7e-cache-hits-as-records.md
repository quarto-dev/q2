---
title: 'OPR-7e: a cached knitr chunk is recorded and presented like any other chunk'
date: 2026-11-09
description: 'OPR-7b leaves a chunk that asks for knitr''s cache out of the record, so it never goes through the presenter. This plan records a cache hit for the common case, cache=TRUE (level 3), with no change to knitr: the chunk hook returns a self-describing span that knitr stores in its own cache entry (or, if the spike prefers it, a sidecar file), and a spike decides which. Level 1 is an optional second slice; level 2 is out of scope. Amends decision D5 (Gordon, 2026-11-09).'
status: complete
depends-on: [opr-7b]
lane: B
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first) ·
**Review:** [plan review, 2026-11-09](../research/2026-11-09-opr-7e-plan-review.md) (its findings are folded into this
plan; read it for the evidence behind the "(review)" marks).
**Branch:** `opr/7e-cache-hits-as-records` · **Lane B** (`workspace-4`) · **Blocks:** nothing, and nothing waits for it.
OPR-7d does **not** depend on this plan (decision 5 below). It is better to land first, because after 7d every plain
render builds ids and capture, but a 7d that lands first is correct: a cached chunk stays out of the record, quietly,
as OPR-7b leaves it.
**Needs:** R 4.3 with knitr, at the minimum knitr q2 supports (1.44) and at the newest installed. The tests that need R
skip without it, but this plan cannot be verified without R.

## Decisions (Gordon, 2026-11-09; not to be reopened by this plan)

1. **Level 3 first; level 2 cut.** The smallest valuable slice is `cache=TRUE`, which is nearly every real use of the
   cache (and what document-level `cache: true` means). Levels 1 and 2 are rare. Level 1 is an optional second slice
   (Phase E). Level 2 is out of scope: it stays as OPR-7b leaves it (absent from the record, quiet). If level 3 fails the
   spike, **stop**; do not ship level 1 alone.
2. **The unit-id fragility is accepted.** Level 3\'s cache key includes `quarto-unit-id`, whose index part changes when a
   chunk is inserted or reordered above a cached chunk, so that chunk misses and re-runs. That is correct, not a cost:
   code added above can have side effects that change the results below it. (knitr\'s own key already includes the chunk
   label, which for an unlabeled chunk shifts the same way.) Do not strip the id from the key.
3. **The spike also prototypes a sidecar** (S10) before the span is built, with a tie-break (see "Design under test").
4. **A persisted copy of a cached chunk\'s record is tolerated and documented.** The record rides in knitr\'s own cache
   (the span\'s string, or a sidecar beside the entry) and knitr discards it whenever it invalidates the entry. This is how
   knitr\'s cache works. It is recorded as an amendment under knitr-epic decisions 2, 8 and 24.
5. **Ordering is soft.** OPR-7d does not list this plan in `depends-on`. If this plan stops (`blocked-on-gordon`), 7d
   proceeds as in 7b; say so in the spike note.

## Start condition

[OPR-7b](2026-11-08-opr-7b-knitr-cache-fallback.md) is `complete` (it is). Nothing else. Phase A is a spike; Phases B to
D run for level 3 only; Phase E (level 1) runs only if its condition holds. See "Decision rules".

## Why this plan exists

OPR-7b lets a chunk keep knitr\'s cache, leaves it out of the record and out of the expected unit count, and leaves its
finished markdown in the skeleton. That is correct and quiet, but a cached chunk then bypasses the one presenter: it
gets knitr\'s own markdown, not the presenter\'s, so it cannot differ from the live route (or from Positron\'s view of the
record), and a cache hit changes the page\'s provenance. The epic\'s goal is that every recorded cell goes through one
presenter path. **Gordon, 2026-11-09: a cache hit should be a record cell, if it can be done without changing knitr.**
This amends decision D5 ("a cached knitr chunk ... uses the markdown fallback with no structured record"). It does not
reopen the rest of D5 (LaTeX family, `pptx`, `context`, no opt-out control).

The plans so far chose between "turn the cache off" (KNR-1 S4) and "keep it and record nothing" (OPR-7b). Neither asked
whether knitr hands q2 enough on a hit. This plan asks, for the case that matters.

## What knitr stores and returns on a hit

Facts from `external-sources/knitr` at `56f2cadf`, a development snapshot, and from the tags v1.44 and v1.50. Locate by
symbol; line numbers shift between releases. Marks: (read) was read in source by the reviewer of this plan, (ran) was
executed on knitr 1.50 (the only version installed when the plan was reviewed), (review) is a reviewer claim not yet
re-checked; the spike re-checks each on 1.44 and 1.50.

| | Stored on a miss | Returned on a hit |
|---|---|---|
| Level 3 (`cache=TRUE`) | `output`, the **chunk hook\'s return value** (`block_exec`, then `block_cache(options, output, objs)`) (read) | first, `if (!params$include) return('')`; then `cache$output(hash)` straight from `call_block`, before any hook runs, so neither `evaluate` nor the chunk hook is called (read; ran, OPR-0c) |
| Level 1 (slice 2 only) | the `evaluate()` list, `res.orig` | `cache$output(options$hash, 'list')` in `block_exec`, in place of `evaluate()`; the filters, `sew` and the chunk hook then run (read) |

- `cache$output` has exactly two call sites, one per hit path above (read, on the snapshot, v1.44 and v1.50; `grep
  'cache\$output' R/*.R`). `cache` is a list of closures in the knitr namespace, and q2 already replaces namespace
  bindings in `patch.R` (`assignInNamespace`, for `sew`, `add_html_caption`, `parse_block`), none of them restored; the
  engine runs one `Rscript` per call, so a patch dies with the process (review).
- **`include=FALSE` never reaches the payload on a hit** (read): the `return('')` comes first. The miss still runs the
  hook and records the chunk, so such a chunk is recorded on a miss and absent on a hit. It is a normal case, not a legacy
  one (see "Staleness").
- The level-3 key is every chunk option except `cache0.opts` (`include`, `out.width.px`, `out.height.px`,
  `cache.rebuild`), plus the chunk label (through `cache.path`), plus (dev snapshot only) the code of the chunks it depends
  on (`.deps`) (read). So it includes `quarto-unit-id` (decision 2). The level-1 key is `cache1.opts` (the code, `eval`,
  `message`, `warning`, `error`, and a few cache options) and does not include the id.
- Anything stored in the cache entry follows knitr\'s own invalidation (`cache.rebuild`, `dependson`, `autodep`). The
  dev snapshot\'s `cache_purge` removes every file `<hash>.*`; **v1.44 and v1.50 purge only `rdb`, `rdx` and `RData`**
  (review). So data inside the stored string follows invalidation on every version, and a sidecar file does not.
- On a hit knitr reloads the chunk\'s `knit_meta` itself (`.hash_meta`, `cache.R`, read), so widget dependencies reach the
  document independent of q2\'s record.
- q2\'s current hooks live in the knitr options list `execute()` hands to rmarkdown, which rmarkdown restores when the
  render ends. A namespace patch is not restored that way; it is harmless here because the process ends with the render.

## Design under test (a hypothesis; Phase A can overturn any part)

### Mechanism A, the span (the plan\'s first candidate)

The chunk hook for a chunk with `cache == 3` returns, instead of the bare marker it returns for an uncached chunk, the
chunk\'s finished markdown **between two comments**, the closing one carrying the chunk\'s capture record:

```text
<!-- q2-cache:ID:begin -->
...knitr's finished markdown for the chunk...
<!-- q2-cache:ID:end:BASE64 -->
```

`BASE64` is standard base64 (never base64url, whose `-` can put `--` inside a comment) of the JSON of the chunk\'s wire
record (the entry `quarto_capture$records` gets) plus the chunk\'s options digest and files. It also carries a
capture-format version, the output format, and the chunk\'s `include` value. The finished markdown is **not** stored twice:
Rust\'s `chunks` entry takes it from the span\'s inner text. Knitr stores this string on a miss and returns it on a hit.
One normalizing step over the knit result, **on the miss and on the hit**, replaces each span:

- capture on: the span becomes this run\'s marker line (`<!-- q2:NONCE:ID -->`) with the chunk\'s indent or blockquote
  prefix re-emitted in front of it; when this run did not record the id live (a hit), the step first adds the payload\'s
  record and chunk entry to the capture\'s results. The added entries go **in document order, merged with the live
  ones**: an out-of-order or unknown id makes the Rust `align` step drop the whole record, not one cell (review).
- capture off: the span becomes its inner markdown, with both comments dropped (insurance: capture-off runs do not write
  spans, so this arises only for an entry written by a record run and read by a run with ids but no capture).

Rules for the normalizer, all tested: it acts only on a begin and an end comment whose ids agree and whose payload
decodes; it accepts any leading indent (`"  "`, `"> "`, `"> > "`) before the comments; the span\'s lines carry the same
prefix the marker does (as `capture_chunk_hook` already does with `options$indent`); it runs at one place on the final
markdown (the end of `execute()` after the output file is read, which also sees spans that come back from a child
document; the spike confirms against the `document` hook). A payload\'s `ordinal` is ignored: nothing in non-test Rust reads
it (read).

### Mechanism B, the sidecar (S10 prototypes it)

On a miss, wrap `cache$save` (called by `block_cache` right after it assigns the hidden output variable) so the stored
string is the plain finished markdown, and write `<hash>.q2rec.json` beside the entry holding the record, the chunk
entry and a digest of that markdown. On a hit, wrap `cache$output`: read the sidecar, verify the digest against the
stored markdown, register the record and return the marker line; on any failure return the stored markdown unchanged
(OPR-7b\'s behavior). It removes the span, the normalizer, the capture-on and capture-off split and the "no comment may
leak" criterion. It costs: the namespace patch for level 3 (the span needs none), a dependence on `block_cache` assigning
the hidden variable before it calls `save`, and orphaned sidecars on 1.44 and 1.50 (the digest check makes an orphan
harmless), and a second durable file beside the cache.

**Tie-break (decision 3).** If both mechanisms pass S1 (the hit\'s record equals the miss\'s after the nonce is normalized,
and the hit\'s output is identical), build the sidecar when it needs no knitr internals beyond `cache$save` and
`cache$output`, and passes on 1.44 and 1.50; otherwise build the span. If only one passes, build that one. If neither
passes, stop.

`capture_chunk_hook` and `opts_hooks$cache` already identify cached chunks by the `quarto-unit-id` option. After this
plan `cached_units` holds only the ids that asked for the cache **and ended the run unrecorded** (an `include=FALSE`
hit, a legacy entry, a payload that fails a check), so the Rust exclusion from the expected count (`engine/knitr/mod.rs`,
the `cached_units` filter) needs no new rule.

## Staleness: what the key does not cover

A cell built from a stored payload was produced under an earlier document state. The key covers every chunk option except
four, so a change to a header option, to the document `execute:` scope or to a setup chunk\'s `opts_chunk` changes the key
and misses (conservative, correct). The cell hash and `quarto_exec_hash` come from the **current** document, never from
the payload (review), so a payload cell looks fresh to the hash.

- **`include`** is not in the key. Miss with `include: false` at document or setup scope, then flip it to true: the hit
  returns the stored string, whose markdown was built for `include=false` (just the indent). The rule: the payload stores
  its `include` value, and a payload whose value differs from this run\'s is **absent and quiet** (an `include=FALSE`
  hit returns before the payload, so the other direction is absent already). That is knitr\'s own behavior for the page
  (it returns whatever it stored), no worse than OPR-7b.
- **q2\'s own code** is not in knitr\'s key. The capture-format version in the payload covers an upgrade that adds a
  record field: a version mismatch is absent and quiet, never an error.
- **Figures.** A level-3 hit does not check figure files, so the record\'s `files` may name files that no longer exist.
  The presenter then falls back to the chunk\'s markdown, as decision 24 says for an unreadable figure. S5 deletes
  `<doc>_files` and keeps the cache to see what happens.
- **Whole-document reuse (knitr-epic decision 7).** A record that mixes live cells with payload cells is stamped fresh for
  the current document while some outputs are older. Record sources `q2` and `positron`, Positron import and replay then
  treat the cached output as authoritative. That is knitr\'s usual cache caveat; OPR-8 says so in the cache docs.

## Cache entries written before this plan

A plain run carries no `quarto-unit-id` (ids are inserted only when a record is built), so its level-3 keys already
differ from a record run\'s: the two never share an entry (review; S8 confirms). A "legacy" entry is therefore only an
id-carrying plain-markdown entry written by a record run of 7b-era q2. Nothing has shipped, so such entries exist only on
development machines. In a capture run such a chunk stays absent from the record, as in OPR-7b: correct, quiet, and gone
once the entry is rebuilt. **No salt on the cache key.**

## Decision rules (what the spike\'s result changes)

- **Level 3 holds (by the span or the sidecar):** implement it (Phases B to D). Then decide on Phase E.
- **Level 3 does not hold:** **stop**, whatever level 1 does. Write the spike note, set `status: blocked-on-gordon`, and
  say that an upstream knitr hook (one documented hook called on every cache hit with `(options, level,
  output-or-list)`) is the remaining option, and that OPR-7d does not wait. Land only the note.
- **"Does not hold"** means: the hit\'s record or output differs from the miss run\'s in a way the normalizer or a payload
  check cannot remove, or the mechanism changes the cache key or the page for a run that does not capture.
- Level 2 is not tested and not built (decision 1); say so in the note.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-4 && pwd
git switch -c opr/7e-cache-hits-as-records feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7e-cache-hits-as-records
```

Landing follows the protocol\'s "Landing a plan" steps 2 to 8 (reconcile the checklist, `status: complete`, re-run the
gates after the rebase, report, handoff). If a commit adds, changes or removes `.snap` or golden files, say how many and
what changed in the commit message and the report; the `knitr/cache-mixed` parity entry may be one.

## Phase A: the spike (a note; scratch scripts in your scratchpad)

Output: `claude-notes/research/2026-11-09-opr-7e-cache-record-spike.md`, with the questions below answered one by one,
each marked (ran) or (read), the knitr versions it ran on, and the scripts\' invocations. If a knitr version cannot be
installed, mark its items (read) from `git show v1.44:R/block.R` (the tags exist in `external-sources/knitr`) and say so.
Prototype in plain R with knitr first (as OPR-0c did), then through `capture.R` with a throwaway edit that you do not
land. Use the OPR-0c mixed document (`fig-live1`, `fig-cached`, a live text chunk, `fig-live2`, a cached text chunk,
three `@fig` references) as the base and add the cases named below. Each document is knit twice in **fresh R processes**
(a miss, then a hit). Pass means the stated equality holds; anything else is a finding to report, and a failure of a
level-3 item that the normalizer or a payload check cannot remove is a "does not hold" (see the rules).

- [x] **S1, level 3 round trip (span).** With the span in the chunk hook\'s return value: (a) the miss run\'s record equals *(Spike: plain R only, via the sidecar not the span; the real-record equality is Phase B/D.)*
      today\'s; (b) the hit run, capture on, yields a record and `chunks` entry equal to the miss run\'s after the nonce is
      normalized (the payload\'s `ordinal` and any figure paths that differ by run are listed, not ignored silently);
      (c) a hit run with capture **off** and ids in the document text (as the `id_chunk` test helper builds it) gives
      markdown identical to a never-cached run and no comment survives; (d) the final html of the miss and of the hit
      are identical (compare the two rendered html files of the same document; a blockquote with an `include=FALSE`
      chunk differs by knitr\'s own indent asymmetry, `block.R` `return('')` against the final indent line, and is not a
      failure); (e) `include` toggled between miss and hit gives absent-and-quiet, not a hidden cell with wrong markdown.
      Say what the stored string looks like (`cache$output(hash)` on the cache entry).
- [ ] **S2, level 1 (feeds Phase E only).** The `cache$output` wrapper for `mode = 'list'`: the hit\'s record equals the
      miss\'s, including `eval=c(1,2)` and `tidy=TRUE`, whose `code` the wrapper cannot see unmasked (review). Confirm by
      `grep` over the versions you test that the two call sites are the only ones and that `mode = 'list'` distinguishes
      them. On 1.44 the list also passes through `fix_evaluate` after the wrapper (`block.R`, review): run it there.
- [x] **S3, level 2.** Cut (decision 1). Nothing to measure; the note says level 2 stays as OPR-7b has it.
- [x] **S4, id stability.** Insert a paragraph above a cached chunk, add a chunk above it, and reorder two cached chunks.
      Prediction (decision 2): a paragraph above still hits; a chunk above or a reorder changes the id and misses. Record
      what happens; a hit that returns a payload whose id differs from the unit is a failure. (A hit always carries the id
      in the key, so the payload id equals the unit\'s by construction; there is nothing to match.)
- [ ] **S5, the awkward chunks at level 3.** Each gives a record equal to the miss\'s, or degrades to absent-and-quiet;
      wrong output is a failure. Cases: an htmlwidget chunk (check `includes` and dependencies, not only the markdown;
      knitr restores `knit_meta` itself on a hit), a `results='asis'` chunk, `include=FALSE` (expected: recorded on a miss,
      absent on a hit), a chunk with `child=` (knitr returns before the cache check; expect no change), `dependson` and
      `autodep`, `cache.lazy`, `cache.rebuild`, a chunk in a blockquote, in a list item and in a callout, and `<doc>_files`
      deleted with the cache kept.
- [ ] **S6, versions.** S1 and S5 on knitr 1.44 (the minimum), 1.50 and the newest you can install (KNR-1\'s matrix; see
      its S1 section and `KNR_R_LIBS` for how the corpus pins a library). Look specifically for: the `cache$output` call
      sites and their signature, `block_cache`, the `cache_purge` file set (matters for the sidecar), `.deps` in the key,
      and a level-3 entry written by 1.44 and read by 1.50. An unrecognized shape must degrade to today\'s
      absent-and-quiet, never to wrong output.
- [x] **S7, scoping.** One line, (read): the engine runs one `Rscript` per call and every existing patch is permanent for
      the process, so a patch needs no restore. If the spike installs a patch (the sidecar does), say whether it is
      idempotent (keeps the pristine original) and gated on `quarto_capture$active`.
- [x] **S8, keys.** Confirm (ran) that a plain run and a record run produce different level-3 hashes for the same chunk
      (the id is the only difference), so they never share an entry. That settles the "no salt" default.
- [ ] **S9, size.** The payload per chunk for the corpus (median, largest), with and without the markdown stored twice,
      and for a widget chunk specifically (its html lands in the cache entry). Figures are paths, not bytes. State a cap
      above which the chunk degrades to absent-and-quiet.
- [x] **S10, the sidecar.** Prototype Mechanism B (S1 (a) to (d) and S5\'s `<doc>_files` and `dependson` cases) and apply
      the tie-break above. Record which mechanism Phase C builds and why.
- [x] Write the note, apply the decision rules, and record the outcome (mechanism chosen, or stop) at the top of "Notes
      from the run" in this plan. If the rule says stop, stop here.

## Phase B: tests first (R required; skip without it, knitr epic decision 17)

Write each test, run it, see it fail for the stated reason, then implement. Tests live next to the OPR-7b ones
(`engine/knitr/capture_tests.rs`, `engine/knitr/gating_tests.rs`) and, for the end to end, in the knitr corpus runner.

- [x] `a_cache_hit_records_nothing_for_a_cached_chunk_at_any_level` pins the old rule for all levels. **Split it:** the
      level-3 case now expects the chunk recorded on a hit (it fails first for that reason); the level-1 and level-2 cases
      keep the old assertion until Phase E changes level 1. Say how and why in the commit message.
- [ ] A level-3 hit, capture on: the record, the `chunks` entry and the skeleton marker exist, with the live run\'s nonce;
      the payload comment is gone from the markdown. Repeat inside a blockquote and a list item.
- [x] A level-3 hit, capture **off**, ids in the document text: markdown equal to an uncached render; no `q2-cache`
      comment anywhere.
- [x] The 0c mixed document (`a_mixed_cached_document_records_the_uncached_units_on_a_miss_and_a_hit`): now a record of
      **all five** units on a miss and on a hit, the same figures 1, 2, 3, resolved references, no `Engine Execution
      Record Dropped`, identical output between runs. That test pins "three" today; update it.
- [x] A hit between two misses puts the added entries in document order (a document with a cached chunk between two live
      ones; the record is not dropped).
- [x] `include=FALSE` at level 3: recorded on the miss, absent on the hit, in `cached_units`, no record dropped. And
      `include` toggled false to true between miss and hit (document-level `execute: include`): absent-and-quiet, not a
      hidden cell.
- [x] A legacy level-3 entry (written with capture off, hit in a capture run): the chunk is absent from the record and
      from the expected count, quiet, as in 7b (`a_cache_written_without_capture_is_replayed_by_a_recorded_run` stays
      green; note it builds both runs with ids in the text, which a real plain run does not).
- [ ] A payload with a wrong version, a wrong format, mismatched begin and end ids, or truncated or corrupt base64
      degrades the chunk to absent-and-quiet, never to an error (run it red first against a stub that reads the payload
      without checking).
- [x] Replay: `a_replayed_record_with_a_cached_chunk_equals_the_live_run` still holds, and now the replayed cached chunk
      is presented from the record, not from stored markdown.

## Phase C: implement level 3 with the mechanism the spike chose

- [x] The level-3 hook and recording path, for the chosen mechanism. Span: `capture.R` and `hooks.R` make the level-3
      chunk hook return the span (inner markdown, closing comment with the payload); the payload holds the wire record,
      the options digest and files, the version, the format and `include` (locate by symbol: `quarto_capture$records`,
      `quarto_chunk_records$records`); chunks that do not ask for the cache are unchanged. Sidecar: the `cache$save` and
      `cache$output` wrappers, installed in `capture_install`, idempotent and gated on `quarto_capture$active`.
- [x] Span only: the normalizing step over the knit result, capture on and off as above, with the rules listed under *(n/a: the sidecar was built.)*
      Mechanism A. It must run on the miss as well, so the skeleton never holds a span.
- [x] `capture_chunk_hook`: a cached chunk that records no longer returns early. `cached_units` is what ends the run
      unrecorded (compute it at the end of the run, not in `opts_hooks$cache`).
- [x] `execute.R` reports the result as it does today (`pieces`, `chunks`, `cachedUnits`); `knitr/mod.rs` should need no
      change beyond the tests. If it does, say what and why.
- [x] Remove or reword any comment that says a cached chunk is never recorded (`capture.R` header, `capture_install`,
      the chunk hook).

## Phase D: parity and end to end

- [x] Re-run the parity gate on the cached documents (the harness from OPR-0b; OPR-7c found `knitr/cache-mixed`
      identical, so any new difference is this plan\'s); the command is under this list. Classify each difference as knitr
      epic decision 26 does. A new difference is a blocker for Gordon unless it is a plain bug in this plan\'s code.
- [ ] **End to end** (`target/debug/q2 render doc.qmd`), each run twice (miss, then hit), reading the outputs and logs:
      the 0c mixed document plain and with `execute: {record: true}`; a document with an htmlwidget chunk at level 3; a
      `record: true` run followed by a plain run of the same format (expect a miss both ways, because the plain run has no
      ids; record that); and a PDF render after an html render of the same document (a different cache directory, so a
      miss). Record the invocations and what you saw: the record\'s cell count (expect all included units), that the html
      is identical between runs, that no `q2-cache` comment reaches any output, the skeleton text in the stored trace, or
      any cell of the record file (`.quarto/execution-records/doc.ipynb`; `grep q2-cache` it), and that no `Record Dropped`
      appears.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core`; the
      Windows cross-clippy (it failed to build on a macOS host in OPR-7b and 7c; say so if it does again); `cargo xtask
      lint`; `target/debug/q2 render --strict` on this plan and the spike note. `npm run build:wasm` only if a file under
      `presenter/`, `record/` or `engine/{replay,ipynb_replay,skeleton}.rs` changed.

The parity command (Phase D, first item):

```bash
OPR_PARITY_REPORT=1 OPR_PARITY_ONLY=cache cargo nextest run -p quarto-core \
  -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture
```

## Phase E: level 1 (optional second slice)

Run only if Phases A to D are done with no open problem **and** S2 passed on 1.44 and 1.50. If not, skip the phase and
say so in the notes; do not leave it half built.

- [ ] Tests first: a level-1 hit records the chunk (the level-1 case of the split test above changes; the level-2 case
      keeps its old assertion), including `eval=c(1,2)` and `tidy=TRUE`.
- [ ] `capture.R`: the `cache$output` wrapper for `mode = 'list'`, installed in `capture_install`, idempotent, gated on
      `quarto_capture$active`, feeding `quarto_capture$slot` as `capture_evaluate_hook` does. It must rebuild `code` the
      way a miss passed it to `evaluate()` (after `tidy` and numeric `eval` masking) or degrade the chunk to absent. Level
      2 chunks are not touched: they keep OPR-7b\'s behavior.
- [ ] A level-1 fixture in the parity corpus, and the parity gate and end-to-end checks of Phase D repeated for it.

## Exit criteria

- The spike note exists and the decision rule\'s outcome is recorded.
- For level 3: a cache hit is a record cell and presents through the one presenter, on a miss and on a hit, with identical
  output between the two and no `Record Dropped`; a chunk that cannot be recorded (an `include=FALSE` hit, a legacy
  entry, a payload that fails a check) is absent and quiet.
- No payload or marker text reaches any output, in a capture run or a plain one.
- A cache written before this plan still renders correctly (absent from the record, quiet).
- The knitr corpus and the OPR parity gate show no new difference on the cached documents.
- Level 1 is either built (Phase E) or recorded as skipped, with the reason.

## Out of scope

- Level 2 (decision 1) and any knitr upstream change (only if the decision rules send us there, and then as a note).
- Stripping the unit id from the cache key (decision 2).
- Writing a payload for non-R engines or for `child=`.
- Docs: OPR-8 owns them. When this plan is `complete`, OPR-8 says that a cache hit presents like any other chunk, that a
  stale cached output is knitr\'s usual caveat, and mentions the legacy-entry case; OPR-8 reads this plan\'s notes.

## Notes from the run

**Spike outcome (Phase A, 2026-11-09):** level 3 holds; Phase C builds the **sidecar** (tie-break, decision 3). See [the spike note](../research/2026-11-09-opr-7e-cache-record-spike.md). The span was never needed. Items run only in plain R, not through `capture.R`, are marked in the note.

**What was built (sidecar, level 3 only).** `capture.R` wraps knitr\'s `cache$save` and `cache$output` in the knitr
namespace (`capture_patch_cache`: idempotent, keeps the pristine list, does nothing unless capture is active). A level-3 miss
swaps knitr\'s stored string (this run\'s marker line) for the finished markdown and writes `<hash>.q2rec.json` (version, id,
`include`, md5 digest of the markdown, the capture record, the chunk record). A hit reads it back, checks version, id,
`include`, digest and hook-override state, registers the record and chunk entry in document order with the next ordinal, and
returns this run\'s marker line. Any failed check leaves the chunk unrecorded and quiet (OPR-7b behavior).
`cached_units` is now computed at the end of the run (`capture_cached_units`: asked for the cache and has no record), so an
`include=FALSE` hit and a bad sidecar land in it. `knitr/mod.rs` needed only a comment. No Rust logic changed.

**Findings that were not in the plan.**

- At `cache$output` time knitr\'s current options still hold `cache = TRUE`; knitr turns it into 3 only later. The level-3 test
  accepts both.
- jsonlite reads a JSON `null` (an `NA` option such as `comment`) back as `NULL`, which it writes as `{}`; the sidecar reader maps
  null to `NA` (found by the miss-equals-hit test).
- A chunk-level `#| include:` change already changes knitr\'s key (the `#|` text is hashed), so only a document-level
  `execute: include` flip reaches the sidecar\'s `include` check; the test uses that.
- Three tests that pinned "level 3 is never recorded" were not listed in the plan and moved to level 1 or were updated:
  `capture_names_the_units_that_asked_for_the_cache`, `a_chunk_that_asked_for_the_cache_is_quiet_and_not_recorded`, and the
  fallback-table row "cache asked for" (now a level-3 row and a level-1 row). The all-cached document test now expects a
  two-cell record.

**Gates.** `cargo clippy -p quarto-core --all-targets -- -D warnings`: clean. `cargo nextest run -p quarto-core`: 7326 passed,
36 skipped, **1 failed**: `opr_parity_runner::tests::the_seeded_allowlist_parses_and_every_entry_is_proposed` (the allowlist has
8 entries, the test says 6). It reads a file this plan does not touch (`git diff feature/one-presenter -- crates/quarto-core/tests`
is empty); OPR-7c added two proposed entries and left the count stale. Not fixed here. `cargo xtask lint`: only the known
`positron_cache.rs:360`. **Windows cross-clippy** failed on this macOS host (`aws-lc-sys` C build; rerun in the follow-up, same result). `npm run
build:wasm` not run: nothing under `presenter/`, `record/`, the splice stage or `engine/{replay,ipynb_replay,skeleton}.rs`
changed (the R files and tests only). No snapshot or golden files changed (0).

**Parity.** `OPR_PARITY_ONLY=cache`: `knitr/cache-mixed` (miss and hit) and `execute_cache_front_matter` Identical; no new
difference.

**End to end** (`target/debug/q2 render`, rebuilt after the R change; scratch dir in the session scratchpad; outputs read):

- The 0c mixed document with `execute: {record: true}`, twice: `doc.ipynb` has **5** cells on the miss and on the hit; Figures 1
  to 3 with resolved references; HTML identical between runs (after stripping hashes); `grep q2-cache` finds nothing in the html or
  the record; no `Record Dropped` or other warning; two sidecars written.
- The same document without `record`, twice: no warnings; HTML identical between runs.
- A `record: true` run followed by a plain run of the same file: no warning. I did **not** verify the expected miss both ways
  (the cache-file count check I ran was not conclusive).
- A trivial htmlwidget chunk (`createWidget`) at level 3, twice: one cell both times, no warning. The widget\'s dependencies were
  not inspected, and widget sizes (S9) were not measured.
- A PDF render after an html render was not run.

**Not done in the run, then run in the follow-up (2026-11-09, lane B; evidence in the
[spike note\'s addendum](../research/2026-11-09-opr-7e-cache-record-spike.md)).** S2 and Phase E (level 1) stay skipped and level 1
stays as OPR-7b has it; nothing here touched `presenter/cell.rs`, `engine/ts_record.rs` or the parity allowlist, so OPR-7f needs
no rebase around this plan.

- **S5 through `capture.R`, ran, now tests** (`capture_tests.rs`, all green on the first run since they pin existing behavior):
  `dependson`, `autodep`, `cache.lazy: false`, `cache.rebuild` (runs again, recorded live), a callout, `child=`, and `<doc>_files`
  deleted with the cache kept. Every hit is recorded like the miss, or absent and quiet, and the document never differs. The
  deleted-`_files` case keeps knitr\'s own caveat (the cached figure is not redrawn; the render warns Q-5-6, as an unrecorded run does).
  No case produced wrong output.
- **S9 sizes, ran.** Corpus sidecars: median 1.5 KB, largest 12.5 KB (a plotly widget). A sidecar is about twice the chunk\'s output
  (it holds the record\'s pieces and the chunk markdown), 3.8 MB for a 100,000-value widget. **No size cap is enforced** in `capture.R`;
  reported as a finding, no cap invented. This corrects the spike\'s "digest only, a few hundred bytes" guess.
- **The skeleton marker on a hit, now a unit test** (this run\'s nonce, with the blockquote prefix, checked by mutation).
- **End to end, ran:** a record run followed by a plain run of the same file **misses** (and the reverse); they share one entry slot per
  chunk label and evict each other. A second format (`typst`; `pdf` is not supported yet) misses in its own cache directory and html
  then hits. A widget chunk at level 3 twice gives identical html with all 17 script and link tags and the widget dependency
  directories present on the hit.
- **Versions, ran:** knitr 1.44, 1.50 and 1.52. All `capture_tests` pass on 1.44 and 1.50; on 1.52 one test fails,
  `capture_classifies_pieces` (evaluate 1.0.5 reports `rlang_error` where the test expects `simpleError`), which is not about the cache and
  needs an owner. An entry written by another knitr version is a quiet miss, not a misread.
- **Windows cross-clippy, ran, failed as in 7b, 7c and 7e:** `aws-lc-sys` fails to build for `x86_64-pc-windows-msvc` on this macOS host
  (`cc` error compiling the jitterentropy C files). Not worked around; the new tests use no platform API and no path literal.
