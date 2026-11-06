---
title: 'EXR-16 S2b and S6: seeding Positron from a q2 record, and the three-leg demo'
date: 2026-11-02
description: 'A record written by q2 and dropped into Positron cache directory seeds a document with no cell run, and q2 reads back both Positron files by content hash. Findings: the file is consumed and rewritten with only cell id and hash, nbformat multiline arrays are shown as text, and an unknown JSON mime becomes a string.'
---

**Question.** S2b: what does the dev build do with a record q2 wrote when it is placed in
Positron\'s cache directory by hand? S6: do the three legs work in sequence on one document
(Positron writes, q2 reads, q2 writes and Positron reads)?

**Answer.** All three legs work at the file level, with the findings below. The one thing S6 did
not show is a rendered q2 preview of the imported outputs, because q2 has no consumer for them
yet (EXR-14 is the planned one). The importer is shown on real Positron files, not through
`q2 preview`.

**Branch.** The harness is `crates/quarto-core/tests/integration/positron_demo.rs` on
`exr/EXR-16`. Two tests, both `#[ignore]` and driven by environment variables: one writes a
real q2 record (`RecordBuilder`, `coalesce`, `write_record`) for a given `.qmd`, and one reads
a Positron cache file through `import_positron_cache` and prints what matched per unit. The
Positron side is the `spike/q2-s3-inline-exec` dev build. No Positron code changed for this.
The demo documents are `~/docs/inline-output/s6-demo.qmd`, `s6-seed.qmd` and `s6-seed2.qmd`;
the files that were read and written are kept in `/tmp/s6/`.

## Method

1. Leg 1: Gordon ran the three code cells of `s6-demo.qmd` in the dev build. I copied the cache
   file Positron wrote.
2. Leg 2: the harness read that file with `import_positron_cache` and matched it to the
   document\'s units by content hash.
3. Leg 3: the harness wrote a q2 record for a second document whose cell and span hashes differ
   from every other cache file (a hash scan loads a file when any one cell hash matches, so a
   shared hash would have picked the older file). I placed the record in the cache directory
   under an arbitrary name and Gordon opened the document without running anything. Then I read
   what Positron had written. This was done twice; the first record had a harness mistake (below).

## Results

**Leg 1 and 2 (Positron writes, q2 reads).**

- All three code cells matched q2\'s units by content hash; the hashes agree exactly.
- Outputs import as display-only (`execute_result` becomes `display_data`, as designed), and the
  `text/html` of the third cell arrived intact.
- The two inline spans match nothing. Positron\'s cache holds no inline units, since the S3 spike
  only draws decorations. That is expected, not a failure.

**Leg 3 (q2 writes, Positron reads).**

- With string values, Positron showed every output of the q2 record with no cell run: the stream,
  `q2-six`, the unknown-mime output and the HTML in bold. The two spans showed nothing.
- **The file is consumed.** The record disappeared from the directory and Positron wrote its own
  file under a new name, as S2a predicted from the code.
- **Only two cell keys survive.** In the rewritten file, every cell keeps just `quarto_cell_id`
  and `quarto_content_hash`; notebook metadata is `last_updated`, `quarto_cache_version` and
  `quarto_source`. Every other q2 key is gone. Nothing in the rewrite can be used to carry q2
  state.
- **Per-output metadata survives.** The extra `quarto_widget` key on an output came back
  unchanged. This confirms S7 (a) in the dev build, and it is better than S2a\'s vitest
  suggested for cell-level keys.
- **The inline cells survive too.** Both span cells and their outputs, including the
  `text/x-quarto-markdown` value, are in the rewritten file, with the span ids q2 wrote.
- **Positron reads the record\'s outputs unchanged.** It keeps all mimes of one output; the
  editor showed both the unknown mime and the `text/plain` of that single output.
- **Unknown JSON mime becomes a string.** The object `{"a":1}` was stored back as the string
  `"{\"a\":1}"` and shown as text. Nothing broke, but the structure is lost on the first rewrite.
- **Multiline arrays are not joined.** The first record had data values written as nbformat
  arrays of lines. Positron does not join them; it shows the array as text, brackets and quotes
  included (`["q2-six"]`). This was a harness mistake: q2\'s golden record and Positron\'s own
  files use strings. It is still a hazard for any q2 writer that emits arrays; a q2 record meant
  for Positron must hold plain strings for `data` values.
- **Read-back.** q2 read Positron\'s rewrite of the q2 record without error: all five hashes
  matched, with `display_data` outputs for the cells and the spans. The loop closes.

## Not tested

A rendered q2 preview of imported outputs (no consumer yet); a record whose only match is one
cell shared with an unrelated document (the any-one-hash rule is from the S2a reading and the
collision check in step 3, not run); a cell with an `execution_count`-only difference; images;
`NonInteractive` and other modes. The outputs in the demo record are made up, not produced by a
kernel (`q2-six` is the output of cell 2 whatever its code), so the demo shows the transport,
not execution.
