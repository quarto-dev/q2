---
title: 'OPR-7f plan review: presentation attrs and Julia names'
date: 2026-11-09
description: 'A design and feasibility review of the OPR-7f plan before an agent runs it. Verdict, ranked findings with the decision on each, what the two review sub-agents disagreed on, and what was not checked.'
---

**Plan under review:** [OPR-7f](../plans/2026-11-09-opr-7f-presentation-attrs-and-julia-names.md) ·
**Epic:** [one presenter path](../plans/2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](../plans/2026-11-08-opr-agent-protocol.md).

Reviewed at integration tip `c73af8fef`. Two sub-agents (model sonnet, read-only, launched in parallel) surveyed data flow
(A) and design and process (B). Marks: (read) source only; (ran) executed; (relied) a sub-agent claim I did not re-check.
Neither sub-agent ran Julia or cargo; every (ran) below is a grep or `git` command. The finding ids R1 to R12 are the ones
used in the plan.

## Verdict

**Approve with changes; status stays `ready`.** The idea is sound and 7f is startable (4d, 7a and 7c are `complete`, B ran
grep). But the plan as written would fail in four places an unattended agent could not resolve alone: the Part B naming rule
is wrong, Part C misses a tracked compiled bundle that CI checks, the emitted attribute order would not match the engine
route or parse, and the wrapper `execution_count` is missed. All four are folded into the plan. No blocker needs Gordon.

## Findings, ranked

### R1. Part C is incomplete: a tracked bundle holds a second copy of the exclusion lists (blocks the exit criterion)

(ran) `ts-packages/quarto-engine-host-node/dist/engine-host-node.mjs` is tracked; it contains the compiled lists (I grepped
`"colab_type"`); `ts_process.rs:61` loads it; `ts-test-suite.yml:207-215` rebuilds it and runs `git diff --exit-code`.
"Only that file needs it" is false: without rebuilding and committing the bundle, CI fails and the `q2` binary keeps writing
`data-quarto`, so "Part C through the binary" fails. **Decision: fix the plan** (a checklist box with the rebuild command
and the freshness check).

### R5. The Part B naming rule is wrong: there are two counters (blocks the exit criterion)

(read, verified by me) `to-markdown.ts:619`: id is `label ? asHtmlId(label) : "cell-" + cellIndex` where `cellIndex` is the
raw 0-based index over **all** notebook cells (`toCellWithOptions(nb.cells[i], i, ...)`). `:833-835`: the figure base is
`label ? ... : "cell-" + (cellIndex + 1)` where `cellIndex` is the argument `++codeCellIndex` (`:1026`), a 1-based count of
code cells. So the first code cell is `#cell-0` with `cell-2-output-1.png`. The fixtures agree (verified):
`julia-presenter-parity/cell-classes.ts.md` has `#cell-0` and `cell-2-output-1.png`. The plan\'s "`cell-<notebook cell
index + 1>`" gives `cell-1` for a lone code cell. A and B both flagged it; the plan said the oracle would decide, but its
mapping test would have encoded the wrong rule. The output suffix `k` is the position among the **kept** outputs (streams
included), also unspecified (A, read). **Decision: fix the plan** with both formulas, concrete cases, a note that
`read_engine_notebook` already enumerates cells (`ts_record.rs:247-250`) but `NotebookCell` drops the index, and a test that
uses the repo fixtures as expected values. Risk kept: `k` depends on output filtering (hidden warnings, discardable
execute_results), which the mapping may not know; the oracle fixture with a print before a plot decides.

### R2. Wrapper `execution_count` is missed

(read, verified) `to-markdown.ts:763` writes `execution_count=N` on the `.cell` wrapper; the fixture shows
`#cell-0 .cell .plain execution_count=1`. On output divs it is written only for numeric `output.execution_count` (`:883`),
i.e. execute_result; the plan said "display or execute-result". The 7c rule `julia-data-execution-count` stripped
`data-execution_count` anywhere, so it hid the wrapper difference, and the allowlist reason ("on the output div") is
incomplete. **Decision: fix the plan** (mapping sets both; execute_result only on outputs).

### R7. Attribute order and class order

(read) The grammar is ordered: id, classes, key/values (`grammar.js:612-651`, A). The plan said "append classes", which for
a figure div gives `cell-output-display cell-output`; the engine route writes `.cell-output .cell-output-display` (verified,
`to-markdown.ts:871`; the presenter writes `.cell-output-display` only, `cell.rs:365,915`), and the harness compares raw text.
Appending a class after a key/value pair would not parse. **Decision: fix the plan:** engine classes spliced before the first
pair, engine attributes after the presenter\'s own, and the figure class prepended, with an order test.

### R3. The validation rules do not match the grammar

(read, A) An id is ASCII `[._A-Za-z0-9-]+`; a class must start with a letter; a quoted value is `'...'` with `\'`, shortcodes
are parsed inside it, so a trailing backslash or `{{<` breaks it; a newline inside the `{...}` head is dubious (A: guess).
The plan\'s "valid HTML id token" and "escape backslashes" were wrong. **Decision: fix the plan:** validate to the grammar,
refuse (do not escape) the unsafe value forms, and test by parsing the emitted markdown. I did not run the parser on these
forms (not checked); the implementer\'s tests do.

### R4. Labeled cells: the plan\'s claim is false

(read, verified) `cell_div_id` (`options.rs:570-577`) writes an id only for a `tbl-` label or a fig label on the cell, so a
plain `#| label: foo` gets no wrapper id on the recorded route; the engine route writes `#foo .cell` (`to-markdown.ts:619`,
`:692-695`). "A labeled cell keeps the label id the pipeline already gives it" is wrong. **Decision: fix the plan:** `attrs.id`
is `asHtmlId(label)` unless the presenter already writes an id. The container-label case (`shouldLabelCellContainer`) is left
to the oracle (risk).

### R6. The record location of `attrs` is unspecified

(read, B and A agree) The runner is unchanged, so `metadata.quarto.attrs` is not a wire the runner sends; the home is the
record. A bare `quarto` key in `extra` would count as an execution option (`CellMetadata::options()`, `types.rs:205`).
Precedent: `facts_as_cell_metadata` patches `extra` after `finish()` (`ts_record.rs:556-568`); `quarto_file_path` is the
output-level precedent. **Decision: fix the plan:** one `quarto_attrs` key on cell `extra` and on output metadata, no
`RECORD_VERSION` bump (an unknown flattened key round-trips; A, read).

### R12. Hash neutrality is easy

(read, A) `hash_unit` covers kind, language, code, execution options and the attribute tail only (`exec_hash.rs:215-233`);
staleness reads `extra` only for header keys. Adding `quarto_attrs` changes nothing. **Decision:** keep the plan\'s test and
point it at the OPR-4d test as a template.

### R4b. "One emit site per div kind" is false

(read, A and B agree) About six output emit sites (`push_figure`, `fenced_output_div`, asis widget, html, markdown, image
placeholder) and `wrap_cell`; `push_figure` has no access to output metadata, and a bare fence drops the attrs. **Decision:**
fix the plan: one head-builder helper, listed sites, a test that drops are by design. `CellOutput` keeps no metadata for
stream and error (Julia does not need it).

### R8. Expansions cannot be covered by a cell-level id

(read, A) A unit that expands (`qnr_expansion`) is several notebook cells on the engine route, each with its own id and
counters, folded into one record cell. Per-output `quarto_file_path` covers figure names; wrapper ids do not. **Decision:**
record as a known limit; if an oracle fixture hits it, report to Gordon; no allowlist, no widening.

### R9. Part C side effects and tests

(read) Only the Julia runner emits `metadata.quarto` (B); marimo and the echo engine use `quarto_cell_id`, unaffected. A
user-authored `#| quarto:` option stops being forwarded (negligible, accepted, noted). `to-markdown.test.ts` has no forwarding
case today (B, relied: grep), so the plan\'s test adds one. quarto-api vitest is wired in CI (`ts-test-suite.yml:372-374`, B,
read). **Decision:** fix the plan.

### R10. Harness removal is more than deleting five names

(read, B; the line numbers for the names I confirmed with grep) `RULES` and `apply_rule` hold the five; the `allowlist()`
test helper and about eight tests use `julia-cell-id` or `julia-figure-names` and need rewriting around a surviving rule.
`OPR_PARITY_ONLY` is a substring match. The plan\'s "see the 7c report for the command" is not enough: the report has only
the whole-gate command. **Decision:** fix the plan (edit list and the exact command). The figure-bearing documents are
blockers for other causes, so the check is a diff read, not a row count.

### R11. Stale statements elsewhere

(verified) The epic row says "four of the 7c Julia allowlist entries go"; the plan says five. **Fixed in this commit.** B
reports that the OPR-7d plan (`~:69`) and OPR-8 (`:72`) assume the wrapper has no `id=cell-N` and that Julia figure URLs change;
both become stale if 7f lands (relied). The plan now tells the implementer to correct them only if 7f lands differently.
Also (relied, B) `.cell-output` has more readers than the allowlist reason names (`dashboard/structure.rs:482`, JATS and
Hugo Lua filters); none checked for a Julia figure.

## Design question: is a general channel justified?

B argued against it: Julia is the only producer, the validation layer is large for fixed strings, wrapper classes and
`unknown` attributes already ride the resolved-options path, and a native presenter rule for `execution_count` would be
smaller but would move the Jupyter goldens (`cell.rs:1224-1311`). Of the five entries only the id, output count and figure
class need something new; names need nothing (`quarto_file_path` exists) and `data-quarto` needs Part C only.

**Decision: keep the channel, record the risk.** Gordon decided on 2026-11-09 that these are presenter gaps the engine should
control, and the channel\'s concrete benefit is that it is per engine and moves none of the 13 goldens (A and B both note it).
Two things shrink it: the keys live in the record under one `quarto_` name, and the validator is small once it matches the
grammar. If Gordon prefers the smaller change (three narrow keys: cell id, output count, output class), the plan changes in
Part A only. I did not change that decision.

## Where the sub-agents disagreed

- **Wrapper `execution_count`:** A put it in "other gaps"; B called it a headline and guessed the removed rule hid it. Both
  right; I verified the line (`to-markdown.ts:763`).
- **Where the labeled-cell id comes from:** both said false; A cited the cell id formula, B the presenter. Verified both.
- **Part C "one copy":** only B found the bundle. A reported the source copy only. B is right (ran grep, tracked file).
- **Record storage:** A said `quarto_`-prefixed key; B said "unspecified". Same finding; the plan now takes A\'s name.
- No agreement on the verdict was needed; neither argued the plan is unfixable.

## What I did not check

- That the engine route and the recorded route agree on `k` (output suffix) for all filter cases. Needs Julia.
- Any Julia run, any cargo test, any parser run on the unsafe attribute forms (A\'s guess about a newline in the head).
- The `shouldLabelCellContainer` case, the dashboard or JATS behavior with a Julia figure carrying `.cell-output`.
- `overlay.rs`, `positron_export.rs` and the freeze and replay paths for `extra` handling (A did not trace them).
- The count of presenter goldens (13), the claims about the runner fork commit `5e04284` and `external-sources/quarto-cli`.
- Whether the preview SPA shows the attrs (the plan does not change hub-client; verifying preview needs the full chain).
