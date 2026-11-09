---
title: 'OPR-7e spike: recording a level-3 cache hit'
date: 2026-11-09
description: 'Phase A of OPR-7e. Plain-R prototypes of the span and the sidecar mechanisms on knitr 1.44 and 1.50 decide that level 3 holds and that Phase C builds the sidecar.'
---

**Plan:** [OPR-7e](../plans/2026-11-09-opr-7e-cache-hits-as-records.md). Marks: (ran) executed here, (read) source only,
(not run) not done in the spike; the plan\'s Phase B to D tests cover it.

Versions run: knitr 1.44 (evaluate 0.21, xfun 0.40) and knitr 1.50 (evaluate 1.0.4, xfun 0.52), from scratch libraries
built with `scratch-libs.R` (`Rscript crates/quarto-core/tests/fixtures/knitr-corpus/scratch-libs.R <dir> 1.44 1.50`)
and selected with `R_LIBS`. The newest installed knitr is 1.50, so 1.52 was not run. Scripts live in the session scratchpad
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
- **S9 size (ran, rough).** A sidecar for the corpus chunks is a few hundred bytes of JSON. A widget chunk would hold its html
  in the stored markdown and so in the sidecar digest only, not twice (the sidecar stores a digest, not the markdown). Cap and
  widget measurement: (not run); no size cap was built.
- **S2 level 1, S5 (htmlwidget, `child=`, `dependson`, `autodep`, `cache.lazy`, `cache.rebuild`, `<doc>_files` deleted
  with the cache kept) through q2\'s `capture.R` (not run in the spike).** The first run of the `_files` case in the
  scratch log was polluted by a corrupted sidecar from the digest test (its mismatch message is that leftover), so it
  proves nothing. These cases are Phase B and D tests against the real path.
- **S3 level 2.** Cut (decision 1); stays as OPR-7b has it.

## Decision

Both mechanisms pass the plain-R round trip on 1.44 and 1.50. The tie-break (decision 3) picks the **sidecar**: it needs
only `cache$save` and `cache$output`, passes on both versions, registers hits in document order, needs no normalizer and
cannot leak a comment into any output. **Level 3 holds; Phase C builds the sidecar.** What the spike did not prove (S1(b)
through the real record, S5) is checked by the Phase B tests, which must fail first against a stub.

Level 1 (S2) was not measured here; Phase E runs only if Phases A to D finish clean and S2 is then run on both versions.
