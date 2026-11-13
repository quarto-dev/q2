---
title: 'OPR-7e spike: recording a level-3 cache hit'
date: 2026-11-09
description: 'Phase A of OPR-7e. Plain-R prototypes of the span and the sidecar mechanisms on knitr 1.44 and 1.50 decide that level 3 holds and that Phase C builds the sidecar.'
---

**Plan:** [OPR-7e](../plans/2026-11-09-opr-7e-cache-hits-as-records.md). Marks: (ran) executed here, (read) source only,
(not run) not done in the spike; the plan\'s Phase B to D tests cover it.

Versions run: knitr 1.44 (evaluate 0.21, xfun 0.40) and knitr 1.50 (evaluate 1.0.4, xfun 0.52), from scratch libraries
built with `scratch-libs.R` (`Rscript crates/quarto-core/tests/fixtures/knitr-corpus/scratch-libs.R <dir> 1.44 1.50`)
and selected with `R_LIBS`. The newest installed knitr was 1.50 at the time, so 1.52 was not run in the spike; the follow-up addendum
below ran 1.52 as well. Scripts live in the session scratchpad
(`s1/mk.R`, `s1/spike_span.R`, `s1/spike_side.R`); the document has a live plot chunk and cached chunks at top level, in a
blockquote, in a list item, `include=FALSE` and `results='asis'`, each carrying `#| quarto-unit-id`. Each document is knit
twice in fresh R processes (miss, then hit).

## Findings

- **S1 span (ran, 1.44 and 1.50, plain knitr).** A chunk hook returning the chunk markdown between two comments, the closing
  one carrying a base64 payload, round-trips: the stored string comes back on a hit, miss and hit output are identical,
  and the comments survive in a blockquote (`> ` prefix) and a list item (indent). On a hit the chunk hook is not called, so
  only live chunks are recorded; the payload is the only route. Records would have to be merged back in document order by
  a normalizer pass.
- **S10 sidecar (ran, 1.44 and 1.50, plain knitr).** Replacing `cache$save` and `cache$output` in the knitr namespace
  (`assignInNamespace("cache", ...)`) is enough. `cache$save(keys, outname, hash, lazy)` receives `hash`, which is already
  the entry\'s path prefix (`cache/<label>_<md5>`), so the sidecar is `<hash>.q2rec.json`; no knitr internal beyond those two
  closures is used (`cache_path` is a closure local to `new_cache()` and is not needed). The record is written from state the
  chunk hook set just before `block_cache` runs. On a hit, `cache$output(hash)` is called inside `call_block` in document
  order, so a record registered there lands in document order with no merge step. Miss and hit output identical; hits
  recorded `u2 u3 u4 u6` (blockquote and list item included). The digest check works: a corrupted sidecar is a quiet miss
  for that chunk only.
- **`include=FALSE` (ran).** Recorded on a miss, absent on a hit (knitr returns `''` before the cache lookup), as predicted.
  The sidecar never sees it.
- **S4 id stability (ran).** A paragraph inserted above still hits. Changing a chunk\'s unit id (what an insertion or
  reorder above does) misses that chunk; its neighbors still hit. This is decision 2 and is accepted.
- **S6 versions (read and ran).** The `cache$output` call sites, `block_cache` and `cache_purge` are the same shape in v1.44
  and v1.50 (`git show v1.44:R/cache.R`). `cache_purge` removes only `rdb`, `rdx` and `RData`, so a sidecar is orphaned when
  knitr purges an entry; the digest check makes an orphan harmless (ran: after deleting an entry\'s data files, the chunk
  re-ran, was recorded live, and rewrote its sidecar). An entry written by 1.44 was read by 1.50 with sidecars intact (ran).
- **S7 scoping (read).** The engine runs one `Rscript` per call, so a namespace patch needs no restore. The wrappers must
  keep the pristine original, be idempotent and be gated on `quarto_capture$active`-style state, so a plain run is untouched.
- **S8 keys (ran, 1.50).** A document with the id lines and the same document without them produce different level-3 hashes
  (`cached_49a9...` against `cached_f5b9...`): plain and record runs never share an entry. No salt.
- **S9 size (ran in the follow-up, see the addendum).** The spike guessed that a sidecar holds a digest and not the markdown, and
  is a few hundred bytes. Both guesses were wrong: the sidecar holds the full record (`pieces`) and the chunk record (`markdown`),
  about twice the chunk\'s output; the corpus median is 1.5 KB. No size cap is built, and none is enforced anywhere in `capture.R`.
- **S2 level 1.** Not measured; Phase E was skipped.
- **S5 through `capture.R` (ran in the follow-up, see the addendum).** `dependson`, `autodep`, `cache.lazy`, `cache.rebuild`,
  `child=`, a chunk in a callout, and `<doc>_files` deleted with the cache kept all behave: the hit is recorded like the miss, or
  is absent and quiet, and the document never differs. They are tests now. (The spike\'s own first run of the `_files` case was
  polluted by a corrupted sidecar from the digest test, so it proved nothing; the addendum run is clean.)
- **S3 level 2.** Cut (decision 1); stays as OPR-7b has it.

## Decision

Both mechanisms pass the plain-R round trip on 1.44 and 1.50. The tie-break (decision 3) picks the **sidecar**: it needs
only `cache$save` and `cache$output`, passes on both versions, registers hits in document order, needs no normalizer and
cannot leak a comment into any output. **Level 3 holds; Phase C builds the sidecar.** What the spike did not prove was checked
by the Phase B tests, and the cases it left open (S5, S9, versions, end to end) by the follow-up below.

Level 1 (S2) was not measured; Phase E was skipped.

## Addendum: the follow-up run (2026-11-09, lane B)

**Plan:** the same [OPR-7e](../plans/2026-11-09-opr-7e-cache-hits-as-records.md). This section settles what the spike marked
(not run). Everything below went through `capture.R` and `target/debug/q2`, not plain R, unless it says otherwise. Scratch
work lived in the session scratchpad.

### S5 through `capture.R` (ran; 7 tests added in `engine/knitr/capture_tests.rs`)

Each case is a miss then a hit in fresh R processes (`call_r` starts one `Rscript` per call). The invariants asserted for all:
the document is identical, and every unit the miss recorded is either recorded on the hit with an equal record (everything but the
run-local ordinal) or named in `cached_units`. Every case was green on its first run, because they pin behavior 7e already had; no
red-first run was possible.

| Case | Result on the hit |
|---|---|
| `dependson` (b on a) | both recorded, equal to the miss |
| `autodep: true` | both recorded, equal |
| `cache.lazy: false` | recorded, equal |
| `cache.rebuild: true` | the chunk runs again (a file it leaves behind proves it) and is recorded live; no sidecar is read |
| chunk in a callout | recorded, equal (the fenced div adds no indent) |
| `child=` with a cached inner chunk | the child chunk and the chunk after it are recorded, equal |
| chunk in a blockquote, in a list item | recorded, equal (7e tests) |
| `<doc>_files` deleted, cache kept | recorded, equal; the figure file is **not** regenerated |

The last row is knitr\'s own caveat, not a defect in 7e. knitr does not redraw a cached figure, so the markdown references a
missing file. A plain run without `record` gives the same Q-5-6 "Referenced resource not found" warning, and the render goes on;
a `record: true` run gives the same warning and keeps the cell. Output matches the unrecorded run, so the case is pinned, not
fixed.

### S9 sizes (ran)

The knitr corpus was rendered through `q2 render` with `execute: {record: true, cache: true}` (35 `.Rmd` fixtures copied to qmd, old-style
chunk headers rewritten to `#|` lines; fixtures the parser still rejects, such as the `bash` and `sql` chunks, were not
rendered). 71 sidecars: minimum 1,078 bytes, **median 1,502 bytes**, largest 12,471 bytes (a plotly widget; the next three are also
widgets at 12.3 to 12.4 KB). A plain plotly `plot_ly` widget with three points is about 12 KB, of which `record.pieces` is 9.4 KB
and the chunk\'s `markdown` is 1.9 KB.

Scaling, one chunk each (sidecar against output): a `createWidget` holding 1,000 random numbers, 42 KB; one holding 100,000,
**3.8 MB**; `print(runif(10000))`, 297 KB. The sidecar is about **twice the chunk\'s output**, because it carries both the record\'s
pieces and the chunk record\'s markdown. knitr\'s own `.rdb` for the same widget entries was 278 bytes, so the sidecar can be the
largest file in a cache entry.

**The cap.** None is enforced: `capture.R` writes the sidecar whatever its size, and nothing in `capture_cache_save` or
`capture_adopt_hit` compares a size. That is a finding, not a bug: a large sidecar is slower to read and write and doubles the
disk cost, but the output is correct. This note does not set a number; the measurements above are what a cap would be chosen
from. Degrading a chunk above the cap to absent-and-quiet would mean skipping the sidecar write in `capture_cache_save`, which is
the same path a failed check already takes on a hit.

### End to end with `target/debug/q2` (ran; outputs read)

- **(a) record run then plain run.** One chunk that leaves a file behind (`ran_N`), `cache: true`, the same `doc.qmd` with and
  without `execute: record: true`. Record run 1 ran the chunk (`ran_0`); record run 2 hit (no new file). Plain run 1 **missed**
  (`ran_1`); plain run 2 hit. A record run after that missed again (`ran_2`). **Conclusion: a plain run misses after a record run,
  and a record run misses after a plain run**, as predicted: the plain run carries no unit id, so knitr\'s key differs. They share one
  entry slot per chunk label (knitr purges the old entry of the same label on a miss), so alternating the two modes re-runs every cached
  chunk each time. The old sidecar stays as an orphan until the next record run overwrites it (same key, same file name), harmless
  because of the digest check. This is a cost of decision 2 (the key includes the id), not a wrong output.
- **(b) a second format.** `--to pdf` is not supported yet (the message is "Format 'pdf' is not yet supported"), so the PDF-producing `typst` format
  stands in. After an html render, a typst render **missed** (`ran_1`) and wrote its own `doc_cache/typst/` entry and sidecar; a
  second html render then hit. Different cache directories per format, no cross-format reuse.
- **(c) a widget at level 3.** A `plotly::plot_ly` chunk with `cache: true` and `record: true`, rendered twice. The two html files
  are identical after stripping hashes; both list the same 17 `script` and `link` tags and the same `doc_files` dependency
  directories (crosstalk, htmltools-fill, htmlwidgets, jquery, plotly-binding, plotly-htmlwidgets-css, plotly-main, typedarray); one
  `html-widget` element and one widget JSON block each. No warning. The dependencies are present on the hit, not only the cell.

### Versions (ran)

| knitr | evaluate | xfun | How | Result |
|---|---|---|---|---|
| 1.44 | 0.21 | 0.40 | scratch library, `R_LIBS` | all 36 `capture_tests` pass; end to end hit equals miss |
| 1.50 | 1.0.4 | 0.52 | installed default | all 36 pass; end to end hit equals miss |
| 1.52 | 1.0.5 | 0.62 | scratch library (`scratch-libs.R` now resolves it) | 35 of 36 pass; hit equals miss end to end |

The one 1.52 failure is `capture_classifies_pieces`: with evaluate 1.0.5 an error condition has class `rlang_error`, where the test expects
`simpleError`. It does not touch the cache or the sidecar and is not 7e\'s; it needs an owner (see the plan notes).
Across versions: an entry written by one knitr version and read by another **missed** in all three directions tried (1.44 then
1.52, 1.52 then 1.50), without a warning, which is the safe outcome (the key is not shared, so the sidecar is not even read). The
unrecognized-shape cases (corrupt JSON, another `version`, a wrong digest, a wrong id) are in `a_sidecar_that_fails_a_check_degrades_the_chunk_to_absent`
and pass on all three versions.

### The skeleton marker (ran)

`a_level_3_hit_puts_this_runs_nonce_in_the_skeleton_marker` runs a miss with nonce A and a hit with nonce B, at top level and in a
blockquote. The hit\'s document carries `<!-- q2:B:id -->` (with the `> ` prefix in the blockquote) and no trace of A. It fails when
the hit returns the stored string instead of this run\'s marker (checked by mutation).

### Not done

Windows cross-clippy: see the plan notes (it fails on `aws-lc-sys` on this host). S2 and level 1 (Phase E) stay skipped. Level 2 stays out of scope.
