---
title: 'OPR-7f: engines control the attributes and file names of what the presenter writes'
date: 2026-11-09
description: 'Adds one general channel by which an engine tells the presenter which id, classes and attributes to put on a cell wrapper or an output div, and has the Julia mapping use it, together with the existing quarto_file_path, so a recorded Julia render keeps the wrapper id, the output execution count, the figure output class and the figure file names of the plain route. Stops the engine route from writing data-quarto (a q2 annotation that the Quarto 1 attribute forwarding leaks into the HTML, with an absolute path) by adding it to the forwarding exclusion list. Removes all five OPR-7c allowlist entries about Julia markup. Lands before OPR-7d. No change to the Julia runner or engine fork.'
status: ready
depends-on: [opr-4d, opr-7a, opr-7c]
lane: C
---

**Epic:** [one presenter path](2026-11-07-one-presenter-path-epic.md) ·
**Protocol:** [agent protocol](2026-11-08-opr-agent-protocol.md) (read it first).
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
| `julia output data-execution_count` | Julia sends it through the new channel (the notebook already has the count). |
| `julia figure output class` (`cell-output` on a figure div) | Julia sends the class through the new channel. The presenter default stays knitr-style. |
| `julia figure file names` | No presenter change: Julia sends `quarto_file_path` per image, computed by the plain route\'s rule. |

## Part A: the channel (presenter and record)

**Shape.** `metadata.quarto.attrs` on a notebook cell (applies to the `.cell` wrapper) and on an output (applies to the
output div; the same metadata object that carries `quarto_file_path`, under the same `quarto` namespace):

```json
{"id": "cell-1", "classes": ["extra"], "attributes": {"execution_count": "1"}}
```

All three keys are optional. The presenter appends and does not interpret.

**Rules to pin by tests.**

- An author-supplied id (a label) wins over an engine id. An engine class is appended after the classes the presenter
  computes and never replaces one. An engine attribute whose key collides with one the presenter writes is ignored.
- It is escaped through one function: an id must be a valid HTML id token, a class a non-empty token without
  whitespace, an attribute key `[A-Za-z_][A-Za-z0-9_:.-]*` with `id` and `class` refused, and a value is quoted and
  escaped for the pandoc attribute syntax (quotes, backslashes, newlines). A malformed entry is skipped with a note
  (the presenter already returns notes) naming the unit, and the rest of the cell is presented.
- It is display-only: the record\'s hash does not change when `attrs` change (test it by building two records that
  differ only in `attrs`).
- It survives `coalesce`, `write_record` and `read_record`, `render_chunk`, the splice path and the preview path, which
  all present through the one cell step (OPR-1a, OPR-1b); one emit site per div kind, not per caller.
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
`ts-packages/quarto-api/src/jupyter/to-markdown.ts`. Only that file needs it: `ts-packages/quarto-api` is the one
copy (q2 builds extensions from its `src`, `crates/quarto-core/src/extension/build.rs`), and the vendored julia-engine
subtree has none. The forwarding of user-authored metadata and options stays as it is. No fork and no push is involved.

## Part B: Julia uses it (q2 side only)

In the Julia notebook mapping (`engine/ts_record.rs`, where OPR-4d maps the capture facts; the reader already sees every
notebook cell, including the markdown cells the plain route counts), fill:

- `quarto_file_path` on each image output: `<stem>_files/figure-html/<name>.<ext>`, where `<name>` follows
  `ts-packages/quarto-api/src/jupyter/to-markdown.ts` exactly: the label (without `#`, `:` as `-`, as
  `pandocAutoIdentifier`) if the cell has one, otherwise `cell-<notebook cell index + 1>`, then `-output-<n>`. The plain
  route\'s id uses a different base (`cell-<index>` at `to-markdown.ts:~619`, the figure name `cell-<index + 1>` at `:~835`);
  replicate each as it is. Do not assume the two agree; the oracle test below decides.
- `attrs.id` on the cell: `cell-<index>` when the cell has no label (a labeled cell keeps the label id the pipeline
  already gives it).
- `attrs.attributes.execution_count` on each display or execute-result output that has a count.
- `attrs.classes: ["cell-output"]` on the output div of a figure.
- Nothing for `data-quarto`: Part C removes it from the engine route.

An engine may send the same keys itself later; the channel is not Julia-specific. Marimo and every other engine send
nothing and are unchanged.

## Checklist (tests first; every box is `[unattended]`)

- [ ] **Presenter tests first** (`presenter/cell.rs` tests, and a golden only if a new fixture is needed): a cell and an
      output with `attrs` get the id, classes and attributes; precedence (label id wins); collisions ignored; each
      malformed form is skipped with a note and the rest still renders; absent `attrs` is byte-identical.
- [ ] **Record tests first:** `attrs` do not change the hash; they round-trip through `write_record` and `read_record`
      and through `coalesce`.
- [ ] Implement Part A: one escape-and-merge function, called from `wrap_cell` and the output-div builders.
- [ ] **Mapping tests first** (`engine/ts_record.rs`, beside the `JDOC` tests): from a QNR-shaped notebook with
      markdown cells between code cells, the mapping sets exactly the keys above, the figure names use the notebook
      index (not the unit ordinal), a labeled cell uses its label for the name and no id, and a cell with no figure
      gets no `quarto_file_path`.
- [ ] **Oracle test** (skips without Julia): render one fixture on the engine route and on the recorded route and show
      that the figure file names, the wrapper id and the output execution counts are equal and that the only
      difference left is `data-quarto`. Include an unlabeled cell after a markdown cell, a labeled figure cell and a
      cell with two figures.
- [ ] Implement Part B.
- [ ] **Part C test first** (`ts-packages/quarto-api/src/jupyter/to-markdown.test.ts`, vitest): a cell whose metadata has
      `quarto: {unit: 1, ...}` produces no `quarto=` attribute, while another unknown metadata key and a cell option
      are still forwarded as attributes (so the feature is unchanged for users). Watch it fail, then add the exclusion.
- [ ] **Part C through the binary:** a plain `q2 render` of a Julia document has no `data-quarto` in the HTML; a Jupyter
      document with a user-authored unknown option still gets its `data-` attribute. Record both.
- [ ] **Allowlist and harness:** remove all five entries and their built-in rules (`julia-cell-id`,
      `julia-data-quarto`, `julia-data-execution-count`, `julia-cell-output-class`, `julia-figure-names`) and their
      harness tests (keep the other three entries untouched). Re-run the Julia part
      of the gate (`OPR_PARITY_ONLY=julia/`, see the OPR-7c report for the command) and add the counts to the notes.
      Do not change any other entry or sign anything: signing is Gordon\'s.
- [ ] **End-to-end through the binary:** render a Julia document with a bare plot, a labeled plot and an unlabeled cell
      after prose on the engine route and on the recorded route (the OPR-7a scratch switch, `OPR7A_FORCE`, is not in the
      tree: re-add it uncommitted, as the OPR-7a notes describe, or use the harness). Read both HTML files; record the
      invocation and the diff.
- [ ] `cd hub-client && npm run build:wasm` (the presenter and `record/` are in the WASM).
- [ ] Update the execution-record format description if one documents `metadata.quarto` (grep `docs/` and
      `claude-notes/designs/`); user docs stay for OPR-8.
- [ ] `ts-packages/quarto-api` tests (`npm test -w ts-packages/quarto-api`, the package\'s `vitest run`) and its build
      (`npm run build -w ts-packages/quarto-api`) pass; check the package\'s tests are wired into CI
      (`.github/workflows/ts-test-suite.yml`) and say so in the notes.
- [ ] Gates: `cargo clippy -p quarto-core --all-targets -- -D warnings`; `cargo nextest run -p quarto-core` (includes the
      knitr corpus gate, the presenter goldens and `marimo_engine_e2e::`); the Windows cross-clippy (say if it cannot
      run); `cargo xtask lint`; `q2 render --strict` on this file.

## Exit criteria

- With the Julia mapping on and Part C in, the recorded route\'s figure file names, wrapper id, output execution count
  and figure output class equal the engine route\'s on the oracle fixtures, and neither route writes `data-quarto`.
- The allowlist has five fewer entries; no other entry changed; nothing signed.
- Every golden, the knitr corpus gate and the marimo tests are unchanged.

## Out of scope

The other five OPR-7c differences (sub-figure group, ANSI escapes, the Plots scripts, the bare-PNG size, interleaving)
are separate decisions or investigations for Gordon. The channel can carry some of them later (the size already has
`metadata_size`).

## Notes from the run

(Fill in at landing.)
