---
title: 'OPR-7f: engines control the attributes and file names of what the presenter writes'
date: 2026-11-09
description: 'Adds one general channel by which an engine tells the presenter which id, classes and attributes to put on a cell wrapper or an output div, and has the Julia mapping use it, together with the existing quarto_file_path, so a recorded Julia render keeps the wrapper id, the output execution count, the figure output class and the figure file names of the plain route. Stops the engine route from writing data-quarto (a q2 annotation that the Quarto 1 attribute forwarding leaks into the HTML, with an absolute path) by adding it to the forwarding exclusion list. Removes all five OPR-7c allowlist entries about Julia markup. Lands before OPR-7d. No change to the Julia runner or engine fork.'
status: complete
depends-on: [opr-4d, opr-7a, opr-7c]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
**Review:** [plan review](../research/2026-11-09-opr-7f-plan-review.md) (2026-11-09; its findings are folded in below, marked
**R1** to **R12**).
**Evidence:** [OPR-7c parity report](../research/2026-11-09-opr-7c-parity-report.md), "Allowlist changes" and the proposed
entries in `crates/quarto-core/tests/fixtures/opr-parity/opr-parity-allowlist.toml`.
**Branch:** `opr/7f-presentation-attrs` · **Lane C** (`workspace-5`) · **Blocks:** OPR-7d.
**Needs:** Julia 1.12 for the oracle comparisons (they skip when absent). No push to any fork: nothing here changes the
Julia runner or the vendored engine.

## Branch and land (explicit)

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-5 && pwd
git switch -c opr/7f-presentation-attrs feature/one-presenter
# claim: set status: in-progress in this file, commit, land (protocol, "Claiming a plan")
# ... work; commit often ...
git rebase feature/one-presenter
git -C /Users/gordon/src/q2/.worktrees/workspace-1 merge --ff-only opr/7f-presentation-attrs
```

## Why

OPR-7c found five Julia differences between the plain (engine) route and the recorded (presenter) route that are not
bugs in either but things the engine route stamps and the presenter does not. They are allowlisted as proposed entries.
Gordon\'s view (2026-11-09): these are gaps in the presenter, and the engine should be able to control them. The
presenter already has two such channels, applied without interpretation: `metadata.quarto_file_path` on an image output
(the file name; knitr uses it) and the size read by `metadata_size`. This plan adds the missing one, for attributes,
and has the Julia mapping use both. It supersedes the allowlist entries instead of signing them: all five go.

| 7c entry | Resolution here |
|---|---|
| `julia wrapper id` (`id="cell-N"`) | Julia sends the id through the new channel. |
| `julia wrapper data-quarto` | **Fixed at the source, not allowlisted** (see Part C). `quarto` goes on the attribute-forwarding exclusion list in q2\'s `to-markdown.ts`, so the engine route stops writing it, and the presenter never did. Why it exists: [Why data-quarto exists](#why-data-quarto-exists). |
| `julia output data-execution_count` | Julia sends it through the new channel, on **execute_result outputs only** and on the **cell wrapper** too (R2). The plain route writes `execution_count=N` on the wrapper (`to-markdown.ts:763`) and on an output div only when the output\'s own count is numeric (`:883`), which in the fixtures is an execute_result, not a display_data (read). The 7c rule stripped both, so the allowlist reason ("on the output div") is incomplete. |
| `julia figure output class` (`cell-output` on a figure div) | Julia sends the class through the new channel. The presenter default stays knitr-style. |
| `julia figure file names` | No presenter change: Julia sends `quarto_file_path` per image, computed by the plain route\'s rule. |

## Part A: the channel (presenter and record)

**Shape (R6).** The runner is unchanged, so the channel lives in the **record**, not on a runner wire. One key, `quarto_attrs`,
with the `quarto_` prefix that `quarto_file_path` and the OPR-4d facts already use (a bare `quarto` key in a cell\'s `extra` would
count as an execution option, `CellMetadata::options()` in `record/types.rs`, which keeps every non-`quarto_` key):

- on a record **cell**: `cell.metadata.extra["quarto_attrs"]` (applies to the `.cell` wrapper). The mapping patches it in
  after `RecordBuilder::finish()`, keyed by unit id, the way `facts_as_cell_metadata` does (`engine/ts_record.rs`, read);
  `RecordBuilder::push` takes only a `UnitResult` and cannot carry it. The presenter reads it in `present_cell_step` into a
  new `CellOptions` field (the cell step sees the whole `RecordCell`; `wrap_cell` sees only `CellOptions`).
- on an **output**: `metadata.quarto_attrs` in the output\'s metadata object, beside `quarto_file_path` (display_data and
  execute_result only: `CellOutput` keeps no metadata for stream and error, and Julia does not need them; say so in a test).

```json
{"id": "cell-0", "classes": ["extra"], "attributes": {"execution_count": "1"}}
```

All three keys are optional. The record format does not need a `RECORD_VERSION` bump (an unknown flattened key round-trips and
an old reader ignores it, read); say that in the format description. The presenter merges and does not interpret.

**Where it goes in the emitted attribute list (R3, R7).** The qmd attribute grammar
(`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:612-651`, read) is ordered: id, then classes, then key/value pairs.
So an engine id replaces the missing id slot, engine classes are spliced **before** the first key/value pair, and engine
attributes go after the presenter\'s own pairs (`layout`, `keep-hidden`, forwarded `unknown`). The plain route writes
`#id .cell [classes] [forwarded] execution_count=N` (`to-markdown.ts`, ~680-765, read). Where the engine route puts a class
before the presenter\'s (the figure div is `.cell-output .cell-output-display`; the presenter writes `.cell-output-display`),
the engine class is **prepended** for that case: add an `order`/`position` field to the entry, or send the whole class list
in order and let the presenter replace its default list only for a figure div. Pick one in the first test and record it. The
parity harness compares raw text, so order is part of the exit criterion.

**Rules to pin by tests.**

- An id the presenter already writes (a `tbl-` label, or a `fig-` label that sits on the cell) wins over an engine id. An
  engine class never replaces a presenter class (see the order rule above). An engine attribute whose key collides with one
  the presenter writes is ignored.
- It is validated through one function that matches the **grammar**, not HTML (R3, read): an id is `[._A-Za-z0-9-]+`
  (ASCII; a unicode id does not parse); a class is `[A-Za-z][A-Za-z0-9_.-]*` (it must start with a letter); an attribute key
  is `[A-Za-z_][A-Za-z0-9_:.-]*` with `id` and `class` refused; a value is single-quoted with `\\'` for a quote, and is
  **refused** (not escaped) if it ends in a backslash, contains a newline, or contains `{{<` (a shortcode is parsed inside a
  quoted value). A malformed entry is skipped with a note naming the unit (the presenter already returns notes), and the
  rest of the cell is presented. Pin each form with a test that **parses the emitted markdown** (the tree-sitter parse, not a
  string compare), including an id equal to another cell\'s id (not deduplicated: the engine route does not either; say so).
- It is display-only: the record\'s hash does not change when `quarto_attrs` change (`hash_unit` hashes kind, language,
  code, execution options and the attribute tail only, `record/exec_hash.rs:215-233`, read; reuse the OPR-4d test that pins
  this for its keys, and build two records that differ only in `quarto_attrs`).
- It survives `coalesce`, `write_record` and `read_record`, `render_chunk` and the splice path. The emit sites are **not** one
  per div kind (R4, read): `push_figure` (`cell.rs:~365`), `fenced_output_div` (stream, text/plain, error), the asis widget
  div, the html div, the markdown div and the image placeholder, each a separate `format!`. Route all of them through one
  helper that builds the `{...}` head, and say in a test what happens where the output has no div (a bare fence under
  `collapse` or `results: asis`, a `bare_linked_image`): the attrs are dropped, silently, by design.
- A `.cell` wrapper that is not written (`needs_cell_div` false) drops the cell-level attrs too.
- An engine that sends nothing is byte-identical to today, for every engine and golden (the 13 presenter goldens and the
  knitr corpus gate must not move).

## Why data-quarto exists

It is an accident of two features meeting, found in the sources on 2026-11-09 (read):

- The runner\'s `emitUnits` annotation (QuartoNotebookRunner fork, commit `5e04284`, `src/evaluation.jl`,
  `_unit_annotation`) writes `metadata.quarto = {unit, language, file, line}` (and, since OPR-4b, the capture facts) on
  each cell, "so a consumer can align the notebook with the document". It is q2\'s record wire. Stock Quarto 1 never
  asks for it.
- Quarto 1\'s Jupyter markdown writer (`external-sources/quarto-cli/src/core/jupyter/jupyter.ts`, "forward other
  attributes we don\'t know about") writes every cell metadata key that is not on its three exclusion lists as a div
  attribute `key='<json>'`, so a user\'s `#| foo: bar` becomes `data-foo`. q2\'s port (`ts-packages/quarto-api/src/jupyter/to-markdown.ts`,
  `cellOptionsFilter`) has the same code. `quarto` is on no list, so the annotation comes out as
  `quarto='{...}'`, which pandoc\'s HTML writer prefixes to `data-quarto`.

Nothing reads it back: q2 reads `metadata.quarto` from the notebook JSON, never from HTML (OPR-0c grep). It also hits
**today\'s plain Julia render**, not only a forced recorded one (seen in the OPR-7a end-to-end run), and it has grown
since OPR-4c, which added the capture facts to the same object. Each Julia cell in published HTML carries that JSON
and the absolute source path.

## Part C: stop the leak at the source

Add `"quarto"` to `kJupyterCellThirdPartyMetadataKeys` (or a new q2-internal list concatenated into
`cellOptionsFilter`, named for what it is: metadata q2 itself writes) in
`ts-packages/quarto-api/src/jupyter/to-markdown.ts`. `to-markdown.ts` is the only **source** copy (the vendored julia-engine
subtree has none), but the compiled lists are also in the **tracked bundle**
`ts-packages/quarto-engine-host-node/dist/engine-host-node.mjs` (R1, ran: the file is tracked, holds the exclusion lists,
and `crates/quarto-core/src/engine/ts_process.rs:61` loads it; `.github/workflows/ts-test-suite.yml` \"Check
engine-host-node bundle freshness\" runs the bundle step and `git diff --exit-code` on it). So Part C is not done until the
bundle is rebuilt and committed: `cargo xtask build-engine-host-bundle` (or `npm run bundle -w @quarto/engine-host-node`),
then `cargo build --bin q2`. The forwarding of user-authored metadata and options stays as it is. A user-authored `#| quarto:`
option stops being forwarded (R9, accepted: negligible; say so in the notes). No fork and no push is involved.

## Part B: Julia uses it (q2 side only)

In the Julia notebook mapping (`engine/ts_record.rs`, where OPR-4d maps the capture facts). **R5 (read, verified):** the plan\'s
first draft said "notebook cell index" as if it were one number. The engine route uses **two counters** over the notebook
the engine returned (the same notebook q2 receives, a deep copy taken before `toMarkdown` mutates it, `julia-engine.ts:264-291`):

- **wrapper id:** `label ? asHtmlId(label) : "cell-" + i`, with `i` the **raw 0-based index over every notebook cell**
  (markdown, raw, code_prefix and inline-span cells included; `to-markdown.ts:619`);
- **figure file name:** `[outputPrefix-]pandocAutoIdentifier(base, true)-output-<k>`, with `base = label` (without `#`,
  `:` as `-`) or `"cell-" + (c + 1)`, where `c` is the **1-based count of code cells only** (`++codeCellIndex`,
  `to-markdown.ts:~1026`, `:833-835`), so the first and only code cell is `cell-2`; and `k` is the 1-based position among the
  outputs **kept** after the engine route filters them (stderr dropped when warnings are hidden, a discardable execute_result
  dropped when an image is present; `:621-640`, `:836-839`), counting streams and text outputs, not only figures. A cell that
  prints and then plots has `-output-2`.

Repo fixtures show it (read): `julia-presenter-parity/cell-classes.ts.md` has `#cell-0` with `cell-2-output-1.png`;
`hidden-then-plot.ts.md` has `#cell-1` with `cell-3-output-1.png`. For `[code]` the wrapper id is `cell-0` and the figure `cell-2`; for `[md, md, code]` they are `cell-2` and `cell-2`; for
`[md, code, code]` the second code cell has id `cell-2` and figure `cell-3`. Do not collapse the two counters.

`read_engine_notebook` already iterates `cells.iter().enumerate()` (`ts_record.rs:247-250`) but `NotebookCell` discards the
index and has no code ordinal: add both there, computed over **all** notebook cells (not from unit ordinals and not after
pairing). Then fill:

- `quarto_file_path` on each image output: `<stem>_files/figure-html/<name>.<ext>`, with `<name>` per the rule above (`k`
  needs the same output filtering; if the mapping cannot know whether warnings are hidden, say what it assumes and let the
  oracle fixture with a print before a plot decide).
- `quarto_attrs.id` on the cell: `cell-<i>` when the cell has no label; with a label, `asHtmlId(label)` **unless the presenter
  already writes an id** (R4: today the presenter writes a wrapper id only for a `tbl-` label or a `fig-` label with both
  `fig.cap` and `fig.subcap`, `presenter/options.rs:570-577`, read, so a plain `#| label: foo` currently gets no id where the
  engine route writes `#foo .cell`; the engine puts the label on a container instead when `shouldLabelCellContainer` says
  so, decide that case from the oracle).
- `quarto_attrs.attributes.execution_count` on the cell (when the notebook cell has a numeric count) and on each
  execute_result output with a numeric count.
- `quarto_attrs.classes: ["cell-output"]` on the output div of a figure, placed **first** (R7: the plain route writes
  `.cell-output .cell-output-display`; the presenter\'s figure div writes only `.cell-output-display`).
- Nothing for `data-quarto`: Part C removes it from the engine route.

**Known limit (R8, read).** A unit that expands into several notebook cells (`qnr_expansion`) is one record cell: the
engine route gives each expansion its own wrapper id, counters and figure names, and the reader folds them
(`attach_expansion`). Per-output `quarto_file_path` can carry the figure names; a cell-level `quarto_attrs.id` cannot give
per-expansion ids. If an oracle fixture hits this, report the residual difference to Gordon; do not allowlist it, and do not
widen the channel.

An engine may send the same key itself later; the channel is not Julia-specific. Marimo and every other engine send
nothing and are unchanged.

## Checklist (tests first; every box is `[unattended]`)

- [x] **Presenter tests first** (`presenter/cell.rs` tests, and a golden only if a new fixture is needed): a cell and an
      output with `quarto_attrs` get the id, classes and attributes in the grammar\'s order; the prepend case for the figure
      class; precedence (a presenter id wins); collisions ignored; each malformed form (including a trailing backslash, a
      newline, `{{<`, a non-letter class start, a non-ASCII id) is skipped with a note and the rest still renders, checked by
      parsing the emitted markdown; a stream or error output and a bare fence drop the attrs; absent attrs are byte-identical.
- [x] **Record tests first:** `quarto_attrs` do not change the unit hash or staleness; they round-trip through `write_record`
      and `read_record` and through `coalesce`; a record without them reads as today.
- [x] **Splice and preview path:** one test that a spliced cell (`record_splice`) and a CaptureSplice/preview record carry the
      attrs into the emitted markdown (no hub-client file changes, so no changelog entry; if one does change, add it).
- [x] Implement Part A: one validate-and-merge function and one head-builder helper, called from `wrap_cell` (via the new
      `CellOptions` field) and from every output emit site listed above.
- [x] **Mapping tests first** (`engine/ts_record.rs`, beside the `JDOC` tests), using the repo\'s fixture notebooks
      (`julia-presenter-parity/`) as the expected values: the wrapper id uses the raw index and the figure name the code-cell
      ordinal plus one (so `[code]` gives `#cell-0` and `cell-2-output-1`, `[md, md, code]` gives `#cell-2` and
      `cell-2-output-1`); a code_prefix or raw cell counts in the first counter only; a cell that prints then plots gets
      `-output-2`; a labeled cell uses its label; the wrapper and execute_result counts are set; a cell with no figure gets no
      `quarto_file_path`.
- [x] **Oracle test** (skips without Julia, using `julia_available()` from `tests/integration/julia_engine_e2e.rs`, as
      `opr_parity_runner.rs` does): add a `julia/` parity fixture (the committed way to get both routes) and show that the
      figure file names, the wrapper id and attributes, the output execution counts and the figure output class (text, in
      order) are equal and that the only difference left is `data-quarto`, which Part C removes from both. Include an
      unlabeled cell after a markdown cell, a labeled figure cell, a cell with two figures, and a cell that prints before it
      plots.
- [x] Implement Part B.
- [x] **Part C test first** (`ts-packages/quarto-api/src/jupyter/to-markdown.test.ts`, vitest; the file has no forwarding
      case today, R9, read): a cell whose metadata has `quarto: {unit: 1, ...}` produces no `quarto=` attribute, while another
      unknown metadata key and a cell option are still forwarded as attributes (so the feature is unchanged for users). Watch
      it fail, then add the exclusion.
- [x] **Rebuild and commit the bundle (R1):** `cargo xtask build-engine-host-bundle`; commit
      `ts-packages/quarto-engine-host-node/dist/engine-host-node.mjs`; confirm `git diff --exit-code` on it is clean after a
      second `npm run bundle -w @quarto/engine-host-node` (the CI freshness gate); then `cargo build --bin q2`.
- [x] **Part C through the binary** (after the bundle step): `grep -c data-quarto` on the HTML of a plain `q2 render` of a
      Julia document prints 0, and on the recorded route (harness) prints 0; a Jupyter document with a user-authored unknown
      option still gets its `data-` attribute. Record the invocations and counts.
- [x] **Allowlist and harness (R10, read):** remove the five entries and the five names from `RULES`
      (`opr_parity_harness.rs:109-118`) and their `apply_rule` arms (`:126-157`). The harness tests that use
      `julia-cell-id` or `julia-figure-names` (the `allowlist()` helper at `~:452-470`, `~:477`, `~:612`, `~:629`, and the
      figure-name and cell-output-class tests) are **rewritten around a surviving rule** (`julia-code-leading-newline`), not
      just deleted, keeping the blocker-without-entry and inside-the-scope cases; delete the cell-output-class test. Keep the
      other three entries untouched; leave the historical mentions in the 0b baseline and 4c plan. Re-run the Julia part of
      the gate and add the counts to the notes:
      `OPR_PARITY_ONLY=julia/ OPR_PARITY_REPORT=report.md cargo nextest run -p quarto-core -E 'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture`
      (`OPR_PARITY_ONLY` is a substring match, `opr_parity_runner.rs:82`). The figure-bearing documents are blockers for other
      causes (Plots scripts, bare-PNG size), so read the diffs for leftover id, name, count and class differences instead of
      counting rows. Do not change any other entry or sign anything: signing is Gordon\'s.
- [x] **Stale statements (R11):** correct the epic row (\"four of the 7c Julia allowlist entries\" to five), and note in the
      OPR-7d and OPR-8 plans only if 7f lands differently from this plan (7d still says the wrapper has no `id=cell-N` unless
      7c kept it; OPR-8 says Julia figure URLs change at the flip). Correct the `.cell-output` readers in the allowlist reasons
      that are removed with them (they name only the dashboard script; `dashboard/structure.rs:482`, the JATS and Hugo Lua
      filters also read it, none checked for a Julia figure).
- [x] **End-to-end through the binary:** render the oracle fixture\'s document (a bare plot, a labeled plot, an unlabeled
      cell after prose) on the engine route (`q2 render`) and on the recorded route through the parity harness (the committed
      way; `OPR7A_FORCE` is not in the tree). Read both HTML files; record the invocation and the diff.
- [x] `cd hub-client && npm run build:wasm` (the presenter and `record/` are in the WASM; builds only, no hub-client source changes).
- [x] Update the execution-record format description if one documents `metadata.quarto` or `quarto_file_path` (grep `docs/`
      and `claude-notes/designs/`); add `quarto_attrs`; user docs stay for OPR-8.
- [x] `ts-packages/quarto-api` tests (`npm test -w ts-packages/quarto-api`, the package\'s `vitest run`) and its build
      (`npm run build -w ts-packages/quarto-api`) pass; the package\'s tests are wired into CI
      (`ts-test-suite.yml:372-374`, \"Run quarto-api tests\", read); the engine-host-node tests and the bundle freshness gate
      are separate steps (R1).
- [x] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core` (includes the
      knitr corpus gate, the presenter goldens and `marimo_engine_e2e::`); the Windows cross-clippy (say if it cannot
      run); `cargo xtask lint`; `q2 render --strict` on this file.

## Exit criteria

- On the oracle fixtures, the parity report\'s `julia/` rows show no difference in figure file names, wrapper id and
  attributes (including wrapper `execution_count`), output execution count or figure output class text and order, other than
  the causes already listed as out of scope; `grep -c data-quarto` prints 0 on the HTML of both routes; the committed bundle
  is fresh (the CI gate\'s command is clean).
- The allowlist has five fewer entries; no other entry changed; nothing signed.
- Every golden, the knitr corpus gate and the marimo tests are unchanged.

## Out of scope

The per-expansion wrapper ids (R8). The other five OPR-7c differences (sub-figure group, ANSI escapes, the Plots scripts, the bare-PNG size, interleaving)
are separate decisions or investigations for Gordon. The channel can carry some of them later (the size already has
`metadata_size`).

## Notes from the run

(Run on 2026-11-09, Lane C, branch `opr/7f-presentation-attrs`.)

**What landed (commits, oldest first):** `engine_attrs` module and presenter wiring (wrapper and every output
emit site); `PresentNote::EngineAttrsRejected` and the cell step reading the cell\'s `quarto_attrs`; record and
splice tests; `engine/ts_presentation.rs` (the Julia mapping) wired into `build_record`; the `quarto` exclusion in
`to-markdown.ts` and the rebuilt, committed bundle; the `julia/presentation-attrs` oracle fixture; the five
allowlist entries and their rules removed.

**Decisions made while implementing (not in the plan):**

- Engine classes go **after** the presenter\'s on the `.cell` wrapper and **before** them on an output div
  (`ClassPlacement`).
- A class may not contain a dot (the grammar allows a dot only in the first class), and a value that starts with
  whitespace is refused, alongside the plan\'s other refusals.
- Output-level attrs are read from the output\'s own `metadata.quarto_attrs` at the emit site, not from a parallel
  vector, so `select_by_kind` cannot drift an index.
- A refused entry becomes `PresentNote::EngineAttrsRejected { unit, reason }`, reported as a warning (the splice
  keeps its existing note handling). `present_cell_step` now returns `(markdown, reasons)`.
- Part B lives in `engine/ts_presentation.rs` and is applied to the QuartoNotebookRunner-annotated notebook branch
  (`Positional` with `metadata.quarto`), so there is no engine-name test (D8). The label is read from the cell\'s
  `#| label:` line; `pandocAutoIdentifier` is ported without the accent table (a non-ASCII id is dropped for the
  file name and refused, with a note, for the id).
- The figure file name uses the label **after** `asHtmlId` (so `Fig:My Plot` is `figmy-plot` in both the id and
  the file), which is what the plain route does.
- `warning: false` is read from the cell\'s own `#|` lines only; the document-level `execute: warning` is not
  consulted when numbering outputs (`k`). The oracle fixture does not cover it.
- The wrapper `execution_count` is written quoted (`'1'`) by the presenter and unquoted by the plain route: the
  same HTML.

**Tests changed on purpose (not weakened):** `ts_record::core_presents_the_record_as_the_markdown` and two
`ts_engine::handshake_tests` asserted a bare `::: {.cell}`; they now assert `{#cell-N .cell execution_count='1'}`.
`opr_parity_runner::the_seeded_allowlist_parses...` expects 3 entries (was 8). `julia_engine_e2e::j8_record_row_core_presents_the_output` used
`data-execution_count` as its "core presented it" marker (the engine wrote it, core did not); core now writes it
too, so the marker is a bare `<div class="cell">` (an expansion wrapper, which has no id: the R8 limit). The harness tests that used the
removed rules were rewritten around `julia-code-leading-newline`.

**Ran (evidence):**

- Julia 1.12.7: `OPR_PARITY_ONLY=julia/ OPR_PARITY_REPORT=... cargo nextest run -p quarto-core -E
  'binary(integration) & test(opr_parity_runner::opr_parity_report)' --no-capture` (about 115 s). 7 identical, 0
  allowlisted, 8 unallowlisted, 1 same-message failure. I read the `presentation-attrs` diff in full and searched every other `julia/` diff for id, name, count and
  class lines (`id=`, `cell-N-output-M`, `execution_count`, `cell-output `): **none left**; I did not read each of
  the other seven diffs end to end, so the causes named below are from the lines I saw. The new `julia/presentation-attrs` document differs only by the print-then-figure cell,
  where the recorded route splits the code block at the output anchors (the out-of-scope interleaving item).
  The others are unrelated causes: `capture-facts` (interleaving, `data-prompt` not forwarded, stderr),
  `docs-index-chunk` (requirejs and jQuery scripts, a widget-dependency gap), `engine-reordering` (ANSI escapes),
  `fig-subcap` (sub-figure group), `plot-website` (bare PNG size, since fixed by OPR-7h after this run), and
  `positron-cache`.
- Not checked through the binary: `data-quarto` on the recorded route (the harness showed no `data-quarto` line
  in any diff, but I did not grep its HTML), and a Jupyter document with a user-authored unknown option (covered
  only by the vitest case that keeps `foo='bar'`).
- Binary: `target/debug/q2 render doc.qmd` on the fixture (copied to the scratchpad): `grep -c data-quarto
  doc.html` printed **0**; the wrappers read `<div id="cell-1" class="cell" data-execution_count="1">` through
  `cell-9`; the HTML was read. The recorded route is the harness above.
- Bundle: `cargo xtask build-engine-host-bundle`, committed; a second `npm run bundle -w @quarto/engine-host-node`
  left `git diff --exit-code` on the file clean.
- `cd hub-client && npm run build:wasm` succeeded. No hub-client source changed, so no changelog entry.
- `ts-packages/quarto-api`: `npm test` and `npm run build` pass (new vitest case failed before the exclusion).
- `cargo clippy -p quarto-core --all-targets -- -D warnings` clean; `cargo nextest run -p quarto-core --no-fail-fast`
  after the rebase onto `feature/one-presenter` (OPR-7e/7h landed there): 8205 passed, 36 skipped, 0 failed. The
  pre-rebase run listed 8189 tests; the 52 more are this plan\'s new tests plus the ones the rebase brought in, and I
  did not separate the two. The 13 presenter goldens, the knitr corpus gate and the marimo tests are in that run
  and unchanged (no `.snap` or golden file touched).
- `q2 render --strict` on this plan and on `engine-api-surface.md`: only the known Q-12-13 listing errors.

**Not done or could not run:**

- The Windows cross-clippy (`--target x86_64-pc-windows-msvc`) cannot run here: `wasmtime` and `aws-lc-sys` compile
  C for the target and fail from macOS. A Windows user should run `cargo clippy -p quarto-core --all-targets -- -D
  warnings`. The new code uses no platform API and only forward-slash paths in `quarto_file_path`.
- `cargo xtask lint` reports one violation, `positron_cache.rs:360` (`metadata-as-str`), in a file this plan did
  not touch (`git diff feature/one-presenter` on it is empty). Not fixed here.
- Per-expansion wrapper ids (R8) are not handled, as planned; no oracle fixture hit it.
- Nothing signed, nothing pushed, no fork touched.

