# pandoc-goldens fixtures

Provenance ledger for the docx/pptx golden-parity harness (pandoc-hybrid
epic, P7 — `claude-notes/plans/2026-08-20-pandoc-hybrid-P7-format-tail.md`,
implementation companion `claude-notes/plans/2026-09-18-pandoc-hybrid-P7-implementation.md`,
Tasks 10 and 12).

Per the repo's External Sources Policy, every quarto-cli-sourced fixture
below is a **one-time copy**, never read from `~/src/quarto-cli` or
`external-sources/` at test/capture time. All 9 sourced fixtures (plus their
referenced image resources) were copied from `~/src/quarto-cli` at tag
`v1.11.3` (commit `7b88c77dbc22a5434ee7d050e633d66c647a0cf1`) on 2026-09-20.

## Fixture set

| # | Local path | Source (at `v1.11.3`) | Exercises | Engine-free? |
|---|---|---|---|---|
| 1 | `crossrefs/all-docx.qmd` (+ `crossrefs/img/thinker.jpg`) | `tests/docs/crossrefs/all-docx.qmd` (+ `tests/docs/crossrefs/img/thinker.jpg`) | numbered figure + captioned table + a theorem; Q1's own docx-targeted crossref doc | yes |
| 2 | `callouts.qmd` | `tests/docs/callouts.qmd` | all 5 callout types, captioned/uncaptioned, `collapse`, three `appearance` values, `icon="false"` — the docx callout-icon path | yes |
| 3 | `crossrefs/callouts.qmd` (+ `crossrefs/img/painter.jpg`, `crossrefs/img/abbas.jpg`) | `tests/docs/crossrefs/callouts.qmd` (+ `tests/docs/crossrefs/img/painter.jpg`, `tests/docs/crossrefs/img/abbas.jpg`) | **cross-referenceable** callouts — the Route-R Callout `order` reclassification (design §3). Copied its two referenced images too, per the same rule as fixture 1 (T10.4) | yes |
| 4 | `crossrefs/theorems.qmd` | `tests/docs/crossrefs/theorems.qmd` | `::: {#thm-line}` + an unlabeled `$$` | yes |
| 5 | `crossrefs/theorem-types.qmd` | `tests/docs/crossrefs/theorem-types.qmd` | one div per theorem type (`lem/cor/prp/cnj/def/exm/exr`) — and **not** `alg`, which is Task 12's point | yes |
| 6 | `crossrefs/equations.qmd` | `tests/docs/crossrefs/equations.qmd` | one labeled `$$ {#eq-black-scholes}` + an `@eq-` ref — the `<m:oMath>` path | yes |
| 7 | `smoke-all/crossrefs/theorem/proof-rendering.qmd` | `tests/docs/smoke-all/crossrefs/theorem/proof-rendering.qmd` | `.proof`, `.proof name=…`, empty `.proof`, `.remark` — P5's Proof Route-R shape | yes |
| 8 | `smoke-all/2025/01/08/7260.qmd` | `tests/docs/smoke-all/2025/01/08/7260.qmd` | the smallest clean `.panel-tabset` (Tab A / Tab B `{.active}`) — the Tabset Route-R reclassification | yes |
| 9 | `smoke-all/mermaid/backticks.qmd` | `tests/docs/smoke-all/mermaid/backticks.qmd` | **the one labeled accepted-divergence fixture** — see Task 11 / `DIVERGENCES.md` (`bd-h1ub8f8z`) | no engine cells, but a `mermaid` cell |
| 10 | `tabset-subfloat.qmd` | *(authored locally — no quarto-cli fixture covers this combination)* | a Tabset containing a FloatRefTarget subfloat (a captioned table) — P6 Finding 5's named case. Per P6 Finding 5 (confirmed dormant, `bd-plcqhfcn`), Q2 has no subfloat-lettering machinery, so this fixture's golden is expected to show **top-level** numbering of the nested table, not subfloat lettering | yes |

**The engine-free rule:** capture renders with a real `quarto`, so any
fixture with an `{r}`/`{python}`/`{julia}` cell needs that toolchain —
excluded for v1. All 9 sourced fixtures were checked and carry zero such
cells. Fixture 9's `mermaid` cell is a mermaid-engine cell, not
R/Python/Julia.

**Exclusions kept as-is:** the code-block `filename`-header fixtures and the
`crossref:`-presentation-option fixtures stay out (design §11/§12,
structural blind spots / accepted upstream gaps — see the P7 implementation
companion's Missing-test pass, envelope items 2/3/8).

## Task 12 — `algorithm` theorem-type evaluation

**Evaluation criterion:** there is no `.algorithm` class in Quarto; the
`algorithm` theorem type is selected by the div id prefix `#alg-` (and
`@alg-`/`@Alg-` references to it), exactly like `#thm-`. Grepping for
`.algorithm` alone would give a false negative regardless of whether any
fixture actually uses the type.

**Command run** (2026-09-20), against the fixture set above once copied in:

```
grep -rn -E '#alg-|@alg-|@Alg-' crates/quarto-core/tests/fixtures/pandoc-goldens/
```

**Result:** zero matches. No in-scope fixture uses the `algorithm` theorem
type.

**Outcome:** condition does not fire. Filed follow-on strand `bd-zhp098dt`
(type `task`, `discovered-from` the pandoc-hybrid epic `bd-jsbg`) naming
both gap sites — `THEOREM_CLASSES` (`crates/quarto-core/src/transforms/theorem.rs:61-70`)
and `RefTypeRegistry::BUILTINS` (`crates/quarto-core/src/crossref/registry.rs:78-106`)
— for whoever implements `algorithm`/`alg` support later.

This claim is machine-checked, not just recorded here: see
`test_no_fixture_uses_alg` in
`crates/quarto-core/tests/integration/pandoc_goldens_fixtures.rs` (T12.2),
and the frozen-table pins `test_theorem_classes_pinned_no_algorithm` /
`test_builtins_pinned_no_alg` (T12.1) in `theorem.rs` / `registry.rs`. If a
future fixture addition reddens T12.2, the condition has fired and the two
pin tests must be updated alongside landing the `alg`/`Algorithm` entries.

## Task 10 — real-`quarto` capture status

`cargo xtask capture-pandoc-goldens`'s `G`-tier real capture (T10.5) has
**not** run as of this writing — it needs a real pinned-release `quarto`
binary at tag `v1.11.3` (not the `~/src/quarto-cli` source checkout, and not
a dev build). See the plan's "Known blocker" note. The fixture manifest,
copy-in, and preconditions (T10.1-T10.4, T10.6, T10.7) are implemented and
tested without needing that binary.
