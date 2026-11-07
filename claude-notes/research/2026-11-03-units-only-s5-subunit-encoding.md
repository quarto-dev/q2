---
title: "Units-only engines, spike S5: encoding sub-units in an nbformat cell"
description: "Whether a flat record cell can carry grouped sub-unit outputs through a quarto_subunits side table, and whether the result stays valid and presentable. Result: yes, with no new presenter code."
---

# Question

Some knitr chunks generate other chunks while they run (`child=` with a computed path, a loop
that calls `knit_child()`). The units-only epic proposes (decision 5) to record each generated
chunk as a **sub-unit** of the chunk that created it. nbformat has no nesting, so the epic
proposes a table in the parent cell\'s metadata, `quarto_subunits`, that says which outputs
belong to which sub-unit. Spike S5 asks: is that encoding possible, does the existing presenter
handle it, and does the cell stay valid nbformat?

This note covers S5 only. S2 (what knitr\'s capture can see for runtime-generated chunks) is
still open, and decision 5 depends on both.

# Method and what was run

- **(a) Schema check.** Read the nbformat 4.5 JSON schema
  (`nbformat/v4/nbformat.v4.5.schema.json`, from the Python `nbformat` 5.10.4 package found in
  a local virtual environment) with `jq`.
- **(b) Prototype.** Hand-built one parent cell, `for (f in c('a', 'b')) knit_child(f)`, with
  four flat outputs and a `quarto_subunits` table of two sub-units, in a temporary test
  module in `quarto-core` (since removed; nothing from the spike is committed).
- **(c) Presenter.** Presented each sub-unit through the existing
  `presenter::knitr_inputs::present_knitr_cell`, and presented the parent once with the table
  ignored.
- **(d) Validity.** Serialized the parent cell to a notebook and ran `nbformat.validate` on it,
  with a negative control. Read Positron\'s output-cache reader as source
  (`external-sources/positron/.../positronQuarto/browser/quartoOutputCacheService.ts`). I did
  **not** run a Positron build.

# Findings

## 1. Text outputs cannot be tagged one by one (S5a)

In the 4.5 schema, a `stream` output and an `error` output both have
`additionalProperties: false` and no `metadata` property. `display_data` and `execute_result`
do allow `metadata`. A negative control confirms it: `nbformat.validate` rejects a `stream`
output that carries a `metadata` key ("Additional properties are not allowed ('metadata' was
unexpected)"). So tagging each output with its sub-unit is impossible for the commonest
outputs, and a side table at the cell level is the only uniform way.

## 2. A sub-unit presents through the existing reader, unchanged (S5b, S5c)

The table row used in the prototype:

```json
{"id": "0-aaaaaaaa-unlabeled/1", "label": "child-a", "options": {}, "hash": "h1",
 "source": "a <- 1\nprint(a)\nb <- 2\nprint(b)",
 "outputs": [0, 3], "anchors": [2, 2, 4], "starts": [0, 1, 2, 3]}
```

To present a row, build a flat cell from it: the row\'s own `source`, the slice of the parent\'s
`outputs` that `outputs: [start, end)` names, the row\'s `options`, `anchors` and `starts`
under the existing cell keys (`quarto_resolved_options`, `quarto_output_anchors`,
`quarto_expression_starts`), and the matching slice of the parent\'s `quarto_output_kinds`.
Then call `present_knitr_cell`. Sub-unit A came out as:

```
a <- 1 / print(a)        (one source fence)
[1] 1                    (stdout)
careful                  (stderr, kept as a message)
b <- 2 / print(b)        (second source fence)
[1] 2                    (stdout)
```

with none of sub-unit B\'s source or output in it, and B came out as `z <- 3` and `z`, then
`[1] 3`, then `w <- 4`. The order is the proof that the row\'s anchors were applied: the message
sits between the two source runs.

Three things this showed:

- **No new presenter code is needed.** The reader takes a record cell, so a row only has to be
  turned into one.
- **A row\'s `source` is the sub-unit\'s own code, not a slice of the parent\'s.** The child
  chunk\'s code is not part of the parent\'s authored text, so anchors and expression starts
  are relative to the row\'s `source`. The presenter\'s `OutputGroup` already says this in its
  comment ("anchors relative to that sub-unit\'s own source") and **already has a `starts`
  field**, so expression starts are a per-group property there.
- **The S5 sketch lacks a field.** It lists `anchors` but not `starts`. `prompt` needs them, so
  the row needs both.

A reader that does not know about the table is also fine: presenting the parent with the table
ignored gives the parent\'s source followed by all four outputs in one cell. It looks coarser
but nothing is lost or garbled.

## 3. The cell stays valid nbformat (S5d)

The parent cell, with `quarto_subunits` among its `quarto_*` cell metadata keys, passes
`nbformat.validate` as nbformat 4.5. That is expected: the code-cell schema allows additional
cell metadata, and q2 records already rely on it.

Positron\'s cache reader (source only) builds its entries from named fields:
`quarto_cell_id`, `quarto_content_hash`, `quarto_label` and `outputs`. It does not look at other
cell metadata, so a `quarto_subunits` key would be ignored, not rejected. I did not trace
whether Positron ever opens a q2 record (its cache is its own file, marked by
`quarto_cache_version`); that question belongs to EXR-16 and KNR-5.

# Recommendation

Keep decision 5 as written, with these refinements:

1. The row is `{id, label, options, hash, source, outputs: [start, end), anchors, starts}`.
   Add `starts`. Per-output kinds stay in the parent\'s flat `quarto_output_kinds` and are
   sliced by the range.
2. The existing cell-level keys remain the flat form (a cell with no table is one group
   covering all its outputs). `quarto_expression_starts` should stay a separate key for now: a
   row carries the same pieces of information, so a row converts to a flat cell by copying each
   field to the key of the same name, which is all the prototype needed. Folding starts into
   the anchors key would also work (a row could nest the pair), but it changes the shape of a
   key that epic decision 21 settled and gains nothing the separate keys lack.
3. A reader must validate the table before using it, as it already does for anchors: ranges
   sorted, non-overlapping and inside the outputs list, ids unique. Otherwise ignore the table
   and present the flat cell. **This validation was not built or tested.**

# What this does not show

- **Real capture.** The record was hand-built. Whether knitr\'s hooks can produce the row
  (source, outputs per child, a parent link) is S2.
- **Parent outputs outside any range.** The prototype\'s ranges cover every output. A parent
  chunk that has its own output between two `knit_child()` calls was not tried; the rule for
  such outputs (presented with the parent, in order) is a guess.
- **Holds across sub-units.** `results = "hold"` and `fig.show = "hold"` move outputs to the
  end of a cell. Whether the hold applies per sub-unit or across the parent was not tried.
- **Index ranges are fragile.** Any tool that adds, drops or reorders a cell\'s outputs while
  keeping its metadata would shift every range. The validation in recommendation 3 limits the
  damage (the table is dropped) but cannot detect a shift that stays in bounds. Hashing each
  row\'s outputs would; that was not tried.
- **Matching.** How sub-unit ids (`parent/N`) interact with core\'s unit count check was not
  tried.
- **Positron.** Source reading only; no build was run.
