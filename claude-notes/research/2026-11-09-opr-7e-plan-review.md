---
title: 'OPR-7e plan review: cache hits as records'
date: 2026-11-09
description: 'A design and implementation review of the OPR-7e plan before an agent runs it. Verdict, ranked findings with proposed edits, what was verified first-hand versus relayed from two review sub-agents, open questions for Gordon, and a scope recommendation.'
---

**Plan under review:** [OPR-7e](../plans/2026-11-09-opr-7e-cache-hits-as-records.md) ·
**Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](../plans/2026-11-08-opr-agent-protocol.md).

Reviewed at integration tip `a12ebf9ad`. Two sub-agents (model sonnet) read the plan, knitr 1.50 (installed), the
knitr tags v1.44 and v1.50 and the dev snapshot `56f2cadf` in `external-sources/knitr`, and q2\'s Rust and R sources.
Marks: (read) is source only, (ran) was executed, (relied) is a sub-agent claim I did not re-check.

## (a) Verdict

**Approve with changes.** Every knitr mechanism the plan depends on is real (the two `cache$output` call sites, the
level-3 string round trip, namespace replacement, span survival in a list and a blockquote). But one premise is false
(capture-on and capture-off runs already do not share level-3 cache entries), level 3 carries two stale-state hazards the
plan does not name, the normalizer has unspecified edges that can drop a whole record, and the level priority is
backwards. None of this blocks the plan; all of it should be in the plan before an unattended agent starts the spike.

## (b) Findings that change the plan, ranked

### 1. Capture-on and capture-off runs already do not share level-3 entries (premise of S1(c), S8, "legacy entries", the PDF test)

Evidence. `insert_unit_ids` returns the text unchanged unless `ctx.builds_record()` (`engine/knitr/chunk_records.rs:61-64`,
`engine/context.rs` `builds_record`, read, verified). So a plain render has no `quarto-unit-id` option. The plan itself
says the level-3 key is every option except `cache0.opts`, so the id is in the key. The sub-agent ran it: adding only a
`#| quarto-unit-id: "u1"` line changed the hash (relied for the run, read for the mechanism). A plain run never hits a
record run\'s level-3 entry, and the reverse.

Consequences:

- "Capture-on and capture-off runs would invalidate each other\'s entries" is already true; it is not a cost of salting.
- The 7b test `a_cache_written_without_capture_is_replayed_by_a_recorded_run` (`capture_tests.rs:501`) builds both runs
  with `id_chunk`, which writes the id into the document text. It is artificial for level 3 (real plain runs carry no id).
- The Phase D item "a PDF render of a document whose cache was written by an html render" cannot be a capture-off hit:
  rmarkdown puts the output format in `cache.path`, so the PDF run misses (relied; the sub-agent ran it).
- A "legacy entry" is only an id-carrying entry written by a record run of 7b-era q2. Nothing has shipped (the epic is one
  unmerged PR), so such entries exist only on development machines.

Proposed edits.

Old (section "Cache entries written before this plan"): "**Default: do not salt the cache key** to force a rebuild (that
would make capture-on and capture-off runs invalidate each other\'s entries). The spike measures what a salt would cost;
the decision is the agent\'s, recorded in the notes."

New: "No salt. A plain run carries no `quarto-unit-id`, so its level-3 keys already differ from a record run\'s; the two
never share an entry. Legacy entries are those a record run wrote between 7b and 7e; nothing has shipped, so no salt is
needed."

S8 becomes "confirm that a plain run and a record run produce different level-3 keys (ran)", not a cost study. S1(c)
builds the capture-off hit from a document with ids in the text and capture off (as `id_chunk` does), and says why. The
PDF end-to-end item becomes "a `record: true` run, then a plain run of the same format; expect a miss both ways, and
record that". The unconditional normalizer stays, as insurance.

### 2. The unit id in the level-3 key includes the document-wide cell index

Evidence. `record/ids.rs:140`: a cell id is `{index}-{hash8}-{label|unlabeled}` (read, verified). Inserting or reordering a
chunk above a cached chunk changes its id, hence its level-3 key, so it re-executes. This already holds under 7b. After
OPR-7d (plain render becomes a record run) it also makes plain `cache: true` renders fragile, a regression against
today\'s id-less plain render. It caps the value of level 3.

Corollary: at level 3 a hit means the chunk carries exactly the id in the key, so the payload\'s id equals the live id by
construction. S4\'s "payload whose id is no longer a unit" and the Phase B test "a payload for an id that is no longer a
unit is ignored" cannot arise from a hit, so that test cannot fail first.

Proposed edits. S4 becomes an id-stability measurement with the prediction "any earlier insertion or reorder misses".
Cut the Phase B test. State the fragility in the plan so OPR-7d\'s decision is informed. A design option for Gordon
(question 3): strip `quarto-unit-id` from the key in an `opts_hooks` hook and side-channel it by label; then the payload\'s
id must be rewritten at absorb time. Do not build it unless Gordon asks.

### 3. A level-3 hit builds a cell from old state, and the hash cannot see it

Evidence. The cell hash and `quarto_exec_hash` come from the current document (`adapter.rs` `RecordBuilder::new` with
`ctx.units`, read, relied), never from the payload, so a payload cell looks fresh. The level-3 key covers every option
except `include`, `out.width.px`, `out.height.px`, `cache.rebuild`, so most option changes miss, which is conservative and
fine. The exception is `include`:

1. Miss with `include: false` at document or setup scope. The hook records the chunk with `options.include=false`.
2. Flip `include` to true. The key is unchanged, so it is a hit and `block.R:101` returns the span.
3. Under D3 the presenter does not layer document scope over a cell that carries resolved options, so the cell stays
   hidden until the cache is rebuilt.

(relied; the code path is read.) Separately, a payload bakes in this q2\'s `capture.R` and `hooks.R`; knitr\'s key does not
include q2\'s code, so an old payload after a q2 upgrade yields a silently degraded cell.

Proposed edits (Phase C and B). On absorb, replace the payload\'s `options` with the options resolved at hit time
(`opts_hooks` runs on a hit per the 7b notes); take only pieces, files and markdown from the payload. Stamp the payload
with a capture-format version and the output format; any mismatch is absent-and-quiet. Add a Phase B test: document-level
`execute: include` toggled between miss and hit.

### 4. `include=FALSE` at level 3 is recorded on a miss and absent on a hit

Evidence (verified, `block.R:99-101`): `if (!params$include) return('')` runs before `if (params$cache == 3) return(cache$output(hash))`.
So an `include=FALSE` cached chunk never returns the span on a hit. The miss records it (the hook still runs), the hit does
not. This is not a "legacy" case, and it contradicts the exit criterion "record of all five units on a miss and on a hit"
for any document with such a chunk. In a blockquote the miss also emits the indent line and the hit emits `''`
(knitr\'s own asymmetry, `block.R:100` against `:425`), so S1(d) "html identical" can fail for a reason unrelated to the plan.

Proposed edits. Add `include=FALSE` as a named case in the level-3 design: it stays in `cached_units` (the unit ended the
run unrecorded) on a hit, which `cached_units` computed at the end of the run already gives. Move it from S5 into Phase B.
Reword the exit criterion to "every unit that is included".

### 5. The normalizer has unspecified edges, and an error drops the whole record

Evidence (read, relied). The existing marker is emitted as `indent + marker + "\n"` (`capture_chunk_hook`); the span\'s
comments need the same indent or the normalizer must re-emit it. `align()` in `chunk_records.rs` and `build_structured`\'s
second `align` return a build error and drop **every** cell if one id is out of order or unknown, so a normalizer that
appends payload records out of document order costs the whole record, not one cell. The span tag has no nonce, so it can
collide with user text, which matters most for the capture-off path (no id list there).

Proposed edits (Phase C, normalizer item):

- Match the begin and end comments only when their ids agree, with an arbitrary leading indent or blockquote prefix, and
  re-emit that prefix on the marker line.
- Add the new payload records and chunks to the capture results in **document order**, merged with the live ones.
- Use standard base64 only (base64url can put `--` inside a comment).
- Derive `chunks.markdown` from the span\'s inner text and leave it out of the payload (the same markdown twice in the
  entry). The adapter drops the record when a cell has no finished markdown (`adapter.rs:102`), so absorb must always
  supply it or treat the chunk as absent. S9 reports sizes before and after.
- Add S5 cases: chunk in a blockquote and in a list item (the corpus has `structure/chunk_in_blockquote`).

Sub-agent 1 asked that the normalizer renumber `ordinal` by document position. Sub-agent 2 found `ordinal` read nowhere in
non-test Rust. I checked: only `presenter/cell.rs:2028` (a test fixture) mentions it, so I side with sub-agent 2: ordinals
need no renumbering; say in the plan that the payload\'s `ordinal` is ignored.

### 6. Slice and priority are backwards

Level 3 is `cache=TRUE` and document-level `cache: true`, which is nearly every real use. Levels 1 and 2 are niche, and
level 2 has its own hazards (knitr purges the cache and stops with an error when a figure file is gone, `plot.R:142-152`;
`fig.keep` other than `high` leaves an empty recorded plot, not a digest). The decision rule "level 3 fails but level 1
holds: ship level 1 only" ships the rare case and leaves the common one.

Proposed edits. Level 3 is slice 1; levels 1 and 2 are an optional slice 2; level 2 is cut unless S3 passes cleanly and
slice 1 is cheap. "Level 3 does not hold but level 1 does" becomes: stop, write the note, do not ship a niche path.

### 7. Levels 1 and 2: the wrapper cannot reproduce `code` after `tidy` or numeric `eval`

On a miss the slot\'s `code` is what `evaluate()` received, after `tidy` and `eval=<numeric>` masking (`block.R` ~263-291). On a
hit the wrapper sees only `options$code`, untidied and unmasked (relied). A level-1 or 2 hit would then differ from the
miss in the source piece. If slice 2 is built, S2 and S3 must test `eval=c(1,2)` and `tidy=TRUE`, and the wrapper must
build `code` the same way or degrade to absent. In knitr 1.44 the list also passes through `fix_evaluate` after the
wrapper (`block.R:259`, relied; read from the tag), so S2 and S6 must run level 1 on 1.44.

### 8. Documents that still say the old rule

- Knitr epic decision 8 lists "knitr cache hits" as a per-chunk fallback and says each unit uses one mechanism. The plan
  amends only D5. Add a dated amendment under decision 8 (and a pointer in KNR-6 risk 5, which 7b annotated).
- OPR-8 (`2026-11-08-opr-8-docs-and-end-to-end.md`, around line 76) says `cache: true` chunks are not part of the record;
  OPR-7d (around line 60) says "Cached knitr chunks fall back with no record (OPR-7b)". Add a line to each: "if OPR-7e
  is complete, read its notes and write from them; if blocked or cancelled, proceed as in 7b". Keep the ordering soft: do
  not add 7e to 7d\'s `depends-on`, or a `blocked-on-gordon` 7e would block 7d forever. 7e\'s stop outcome should say 7d does
  not wait for it. (relied)
- Decision 7 (whole-document reuse): a record that mixes live cells with payload cells is stamped fresh for the current
  document while some outputs are old. Record-source `q2`, `positron`, Positron import and replay then treat stale cached
  output as authoritative. That is knitr\'s usual cache caveat, but the plan and the OPR-8 hand-off should say so.

### 9. Smaller changes

- S7 is moot as scoping: `call_r` spawns a one-shot `Rscript`, and the existing patches (`sew`, `parse_block`,
  `knit_child`, `valid_path`, `wrap`) are permanent for the process (relied, `patch.R`, `hooks.R:~268`). Replace S7 with one
  line confirming that, and, if a long-lived process is ever added, an idempotent wrapper with a pristine original and an
  `on.exit` restore.
- "`cache_purge` removes every file `<hash>.*`" holds only for the dev snapshot; v1.44 and v1.50 purge only `rdb`, `rdx`
  and `RData` (relied; `git diff v1.50 56f2cadf -- R/cache.R`). Anything stored inside the string follows invalidation on
  every version, so the span design is fine; a sidecar file would not. Qualify the bullet "(read, snapshot only)".
- "`remove_plot` ... digest" holds only for `fig.keep = 'high'`; otherwise `plot.R:305-310` stores an empty classed list and
  R 4.3 prints a deprecation warning from knitr (relied; sub-agent 1 ran 1.50).
- S3 has no pass threshold; S5 has no per-chunk pass criterion. Add: each awkward chunk either gives a record equal to the
  miss\'s or degrades to absent-and-quiet; wrong output is a stop. Define "contain" in the stop rule: the output differs from
  the miss run\'s, or the cache key changes.
- S6: only knitr 1.50 is installed. Say: if a version cannot be installed, mark its S-items (read) from
  `git show v1.44:R/block.R` and say so.
- "Branch and land" shows only claim, rebase and merge. Add one line pointing at the protocol\'s "Landing a plan" steps 2 to
  8 (reconcile checklist, `status: complete`, re-run gates after the rebase, handoff). Phase D lists no snapshot statement;
  add "say how many `.snap` or golden files changed (the `knitr/cache-mixed` parity note may)".
- "after 7d every plain render writes cache entries" is loose: plain renders write cache entries today when the author asks
  for the cache. What 7d changes is that plain renders gain ids and capture.

### Sub-agent disagreement resolved

- **`knit_meta` on a hit.** Sub-agent 1 says knitr reloads it (`cache.R:75-79`, `.hash_meta` into `.knitEnv$meta`);
  sub-agent 2 says knitr never restores it. I read `cache.R:70-82`: sub-agent 1 is right (ran: no; read, verified).
  Widget dependencies reach the document from knitr\'s own cache on a hit, independent of q2\'s record. What stays open is
  the record\'s own `knit_meta` piece (decision 27): it is redundant but harmless. S5 should check `includes`
  and dependencies, and S9 should measure widget chunks (the payload stores a widget\'s HTML twice).
- **Ordinals.** See finding 5.

### Design alternative worth a spike item (sub-agent 1, not prototyped)

A sidecar instead of a span: wrap `cache$save` and `cache$output(mode='character')`; on a miss, overwrite the hidden
output variable with the real markdown before the original save and write `<hash>.q2rec.json` (record plus a markdown
digest); on a hit, read and verify the sidecar, register the record and return the marker. It removes the span, the
normalizer, the capture-on and capture-off dichotomy and the "no `q2-cache` comment may leak" criterion. Costs: v1.44 and
v1.50 do not purge sidecars (the digest check makes an orphan harmless), level 3 needs the namespace patch too, and the
wrapper needs the current unit id and record inside `save`. I would add it as spike item S10 ("prototype the sidecar
before committing to the span"), since the payload trick is the plan\'s riskiest part.

## (c) What I verified and what I relayed

Verified myself:

- (read) `block.R:80-104` and `:420-428`: `include` returns `''` before the level-3 return; the snapshot folds `.deps` into the key.
- (read) `cache.R:70-82`: `knit_meta` is reloaded on a hit (settles the sub-agent disagreement).
- (read) `chunk_records.rs:61-64` and `context.rs` `builds_record`: no unit ids without a record run.
- (read) `record/ids.rs:140`: the cell id includes the document-wide index.
- (read) a grep for `ordinal` in `crates/quarto-core/src`: no non-test reader.
- (read) knitr epic decisions 8, 24, 27 and risks 2 to 4; epic D5 as amended.
- (read) the plan\'s status on the integration branch is `ready` (not claimed).

Relayed (not re-run or re-read by me): the key-changes-with-id run; the rmarkdown `cache.path` format claim; the
v1.44 `fix_evaluate`, `cache_purge` and `.deps` diffs; level-2 digest and figure-file behavior; span survival in a
blockquote and list item (sub-agent 1 ran it on knitr 1.50); `align`/`build_structured` dropping a whole record; the
`include` flip scenario (path read, outcome predicted); the OPR-7d and OPR-8 line references; the payload size concerns.
Only knitr 1.50 was available; every 1.44 statement is (read) from a git tag.

## (d) Open questions for Gordon

1. Should level 3 be slice 1 and level 2 be cut (finding 6)?
2. Is the id fragility of finding 2 acceptable for plain `cache: true` renders after 7d, or should 7e strip the id from
   the key (extra design work)?
3. Do you want the sidecar alternative prototyped in the spike (S10) before the span is built?
4. A cached chunk\'s output is persisted next to its record in knitr\'s cache directory. Decision 24 allows the shipped
   markdown from R to Rust; this is a new durable copy. Tolerable because knitr invalidates it, or a decision to record?
5. If 7e ends blocked, do you want 7d to proceed as in 7b (the soft ordering I propose)?

## (e) Scope recommendation

Build **level 3 only** (with the stale-`include`, version-stamp, document-order and indent fixes above); make levels 1 and
2 an optional second slice, and cut level 2.

## (f) What each sub-agent was asked

- **Sub-agent 1, "does knitr actually allow this?":** verify the plan\'s table and bullets against `block.R`, `cache.R`
  and `plot.R` and look for paths the plan misses; whether anything alters the span between the hook and the document;
  whether `knitr:::cache` can be replaced and restored safely and whether a safer seam exists for levels 1 and 2; what
  `remove_plot` leaves and whether a level-2 hit can equal a miss; `htmlwidgets`, `include=FALSE`, `results='asis'` and
  missing figures at level 3; version risk across 1.44, 1.50 and the snapshot; and a simpler seam than the payload trick.
- **Sub-agent 2, "does it fit q2, and is the plan runnable?":** conflicts with epic D1 to D13 and knitr-epic decisions 8,
  10, 11, 21 to 27; the Rust trace (`mod.rs`, `adapter.rs`, `chunk_records.rs`, `skeleton.rs`, `replay.rs`,
  `document.rs`); stale-data hazards; consumers (preview, hub guests, replay, Positron, OPR-7d); plan quality (S1 to S9
  pass/fail, decision rules, test-first order, missing gates); cost and value and the smallest slice; and protocol
  compliance and the plan\'s own references.

## (g) Decided (Gordon, 2026-11-09), and where the plan departs from this note

The five open questions in (d) were answered the same day and folded into the plan (see its "Decisions" section):

1. Level 3 is slice 1; level 1 is an optional second slice; level 2 is cut; if level 3 fails the spike, stop.
2. The unit-id fragility is accepted: code added above a cached chunk can change what runs below it, so a re-run is
   correct.
3. The sidecar is prototyped as S10, with a tie-break in the plan.
4. A persisted copy of the record in knitr\'s own cache is tolerated and documented (knitr-epic decisions 2, 8, 24).
5. Ordering with OPR-7d is soft: 7d does not wait for 7e.

The plan departs from this note in one place. Finding 3 proposed refreshing the payload\'s `options` from the hit\'s
current options. Writing the plan showed that is not enough: the stored markdown for an `include=false` miss is just the
indent, so refreshed options would present an empty cell. The plan instead stores the `include` value in the payload and
treats a mismatch as absent-and-quiet (knitr\'s own behavior for the page, no worse than OPR-7b). `include` is the only
chunk option outside the level-3 key that changes presentation, so no other option needs refreshing.
