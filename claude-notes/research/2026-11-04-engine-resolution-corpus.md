---
title: 'Engine resolution corpus and design review (bd-yt2o2ovt)'
date: 2026-11-04
description: 'Review of the Jupyter/engine interface: Q1 behavior, the final proposed interface (D2b), amendments, and a resolver-level corpus of 49 cases that is the Phase B3 test specification.'
---

# Overview

This note accompanies the plan `claude-notes/plans/2026-11-04-engine-interface-review.md`,
which holds the findings, the decision (D2b) and the open recommendations (R1
to R7). It adds the two things the review owed: the final proposed interface in
one place, and the corpus.

Labels: **RAN** means executed and the output read; **DERIVED** means read from
code or reasoned. Every number in the corpus table below is a resolver result:
the `today` column was run against main `94e1f8afe`, and the target column was
checked by running the whole corpus against a temporary prototype of D2b (a
joint T1/T2 pass, about twenty lines in `run_tiers`, reverted afterwards).

# Final proposed interface

**Selection.** A language is owned by the best claim among the engines
resolution considers. Explicitly declared engines are the ones named by
`engine:` (scalar, list or map) or by a top-level key named after a registered
engine (`jupyter:`, `knitr:`) when `engine:` is absent.

**Precedence (D2b, decided).** Jointly in T1 and T2, an explicit engine\'s
`Fallback(p)` counts as `Primary(max(1, p))`. Candidates compare on
`(priority, class)`: a listed engine\'s own Primary beats a listed engine\'s
promoted Fallback, which beats an unlisted engine\'s Primary. Candidate order
(listed first, declared order) breaks remaining ties. T3 Interop and T4
implicit Fallback are unchanged.

**Spellings.** `engine: jupyter` and `jupyter: ark` (a Q1 spelling) select
Jupyter and, after D2b, let it own the languages no other listed engine claims
specifically. `engine: [a, b]` and the map form are q2 only (Q1 rejects them).
`claims:` tables narrow or widen an engine and replace its default claims.

**Kernel (D1, with R1 and R2 pending).** Explicit name from
`engine: {jupyter: {kernel}}`, then `jupyter: X` or `jupyter.kernel`, then (R2)
`jupyter.kernelspec.name`; else the kernelspec whose `language` matches the first
executable cell (case-insensitive); else the old name table. An unknown explicit
name is a hard error. One kernel per document; R1 recommends an error when a
Jupyter-owned cell\'s language differs from the kernel\'s declared language.

**Layering.** Arrays from project and document layers concatenate, so D2b
makes the result independent of layer order for the cases the review found
(C51 and C52 give the same owners). `!prefer` or a scalar document `engine:`
replaces the project value.

# Amendments to the plan and the design document

- D2 is replaced by D2b: T2 is not retired; it is evaluated jointly with T1
  (plan, decided).
- `engine-resolution.md` section 2 (the sentence saying explicit `engine:` does
  not preempt division), section 4 (tier text, T1 and T2 merged into one pass),
  and the section 4.4 table change with Phase B3.
- Of the existing unit tests, only `table_fallback_is_a_real_floor` fails under
  the D2b prototype (RAN: 724 of 725 `engine::` tests pass). The tests that pin
  T2 (`test_b2_t2_explicit_fallback_wins_unclaimed_language` and the
  `[knitr, jupyter]` cases) pass unchanged.
- Phase B2 gains R1 and R2 items; Phase B3 starts with this corpus; Phase B4
  holds the diagnostics (R3, R4). R5 (claims parser) and R7 (handler
  languages) are separate strands.

# The corpus

Source of truth: `crates/quarto-core/tests/integration/engine_resolution_corpus/cases.yaml`
(this table is generated from it by `render_table.rb` in the same directory:
`ruby render_table.rb > claude-notes/research/2026-11-04-engine-resolution-corpus.md`
from the repository root),
driven by `engine_resolution_corpus.rs` (module `engine_resolution_corpus` in
`crates/quarto-core/tests/integration/main.rs`). Run with
`cargo nextest run -p quarto-core --test integration engine_resolution_corpus`.

Result on branch `braid/bd-yt2o2ovt-ark-r-kernel` (main `94e1f8afe` plus the cases added since): 49 cases, 35 pass, 14 expected-fail.
A `pending-phase4` case pins today\'s behavior and fails once it meets the
target, so the marker gets flipped when Phase B3 lands. Registries: E1 is the
built-ins (knitr, jupyter, markdown); E2 adds a Julia extension at Primary(1);
E3 adds a marimo-style extension at Primary(1) on `python` with class `marimo`;
E3b is the same at Primary(2). Sequence order is exact for E1 and compared as a
set for E2, E3 and E3b (the registry contribution order has no public setter).

Each owner cell reads `sequence` / `language=owner`. The Q1 column says how Q1
behaved and how that was verified.

| id | declaration | cells | reg | target (D2b) | today | Q1 (verified) | verdict |
|---|---|---|---|---|---|---|---|
| C01 | `(none)` | `r` | E1 | `knitr` / `r=knitr` | same | knitr (processing file: doc.qmd) (RAN) | intuitive |
| C02 | `(none)` | `python` | E1 | `jupyter` / `python=jupyter` | same | jupyter (RAN) | intuitive |
| C03 | `(none)` | `r` `python` | E1 | `knitr` / `r=knitr, python=knitr` | same | knitr for both orders; python via reticulate (RAN) | intuitive |
| C04 | `(none)` | `python` `r` | E1 | `knitr` / `python=knitr, r=knitr` | same | knitr; order does not matter (RAN) | intuitive |
| C05 | `(none)` | `julia` | E1 | `jupyter` / `julia=jupyter` | same | jupyter picks the julia kernel by language (Q1 claims julia for its own julia engine) (DERIVED) | intuitive |
| C06 | `(none)` | `r` `bash` | E1 | `knitr` / `r=knitr, bash=knitr` | same | knitr runs bash (RAN) | intuitive |
| C07 | `engine: jupyter` | none | E1 | `none` / `none` | same | n/a (none) | intuitive |
| C10 | `engine: jupyter` | `r` | E1 | `jupyter` / `r=jupyter` | `knitr` / `r=knitr` | jupyter starts (python3 kernel); the R cell is inert code (RAN) | divergence |
| C11 | `engine: jupyter` | `python` | E1 | `jupyter` / `python=jupyter` | same | jupyter (RAN) | intuitive |
| C12 | `engine: jupyter` | `r` `python` | E1 | `jupyter` / `r=jupyter, python=jupyter` | `jupyter, knitr` / `r=knitr, python=jupyter` | python runs in the kernel; the R cell is inert (RAN) | surprising |
| C13 | `engine: knitr` | `r` `python` | E1 | `knitr` / `r=knitr, python=knitr` | same | knitr; python via reticulate (RAN) | intuitive |
| C14 | `engine: knitr` | `julia` | E1 | `none` / `none` | same | knitr fails: there is no package called JuliaCall (RAN) | surprising |
| C15 | `engine: markdown` | `r` `python` | E1 | `markdown` / `none` | same | markdown engine (DERIVED) | intuitive |
| C16 | `engine: jupyter` | `r` `bash` | E1 | `jupyter` / `r=jupyter, bash=jupyter` | `jupyter, knitr` / `r=knitr, bash=jupyter` | jupyter runs python-kernel cells only; r and bash are inert (DERIVED) | surprising |
| C20 | `engine: [jupyter, knitr]` | `r` | E1 | `knitr` / `r=knitr` | same | rejected by the schema (engine must be a string) (RAN) | q2-only |
| C21 | `engine: [jupyter, knitr]` | `r` `python` | E1 | `jupyter, knitr` / `r=knitr, python=jupyter` | same | rejected by the schema (RAN) | q2-only |
| C22 | `engine: [knitr, jupyter]` | `r` `python` | E1 | `knitr, jupyter` / `r=knitr, python=jupyter` | same | rejected by the schema (RAN) | q2-only |
| C23 | `engine: [knitr, jupyter]` | `r` `sql` | E1 | `knitr, jupyter` / `r=knitr, sql=jupyter` | same | n/a (none) | surprising |
| C24 | `engine: [jupyter, nosuch]` | `r` | E1 | `jupyter` / `r=jupyter` | `knitr` / `r=knitr` | rejected by the schema (RAN) | surprising |
| C25 | `engine: knitrr` | `python` | E1 | `none` / `none` | same | unknown engine name is ignored; language selection runs python under jupyter (no error) (RAN) | surprising |
| C30 | `jupyter: ark` | `r` | E1 | `jupyter` / `r=jupyter` | `knitr` / `r=knitr` | jupyter key selects Jupyter; R runs in the ark kernel (RAN) | divergence |
| C31 | `jupyter:;   kernel: ark` | `r` | E1 | `jupyter` / `r=jupyter` | `knitr` / `r=knitr` | works (undocumented kernel key) (RAN) | divergence |
| C32 | `engine:;   jupyter:;     kernel: ark` | `r` | E1 | `jupyter` / `r=jupyter` | `knitr` / `r=knitr` | rejected by the schema (engine must be a string) (RAN) | q2-only |
| C33 | `engine: jupyter; jupyter:;   kernel: ark` | `r` | E1 | `jupyter` / `r=jupyter` | `knitr` / `r=knitr` | works: Starting ark kernel, all chunks and the plot render (RAN) | divergence |
| C34 | `engine: knitr; jupyter:;   kernel: ark` | `r` | E1 | `knitr` / `r=knitr` | same | knitr wins when engine: knitr is present with a jupyter key (python3 probe) (RAN) | surprising |
| C35 | `engine: jupyter; jupyter:;   claims:;     r: 1` | `r` | E1 | `jupyter` / `r=jupyter` | `knitr` / `r=knitr` | n/a (claims is a q2-only key) (none) | surprising |
| C40 | `engine:;   jupyter:;     claims:;       python: 1` | `r` `python` | E1 | `jupyter, knitr` / `r=knitr, python=jupyter` | same | rejected by the schema (RAN) | q2-only |
| C41 | `engine:;   jupyter:;     claims:;       python: 1` | `julia` | E1 | `none` / `none` | same | n/a (none) | surprising |
| C42 | `engine:;   jupyter:;     claims:;       r: 1` | `r` | E1 | `jupyter` / `r=jupyter` | same | rejected by the schema (RAN) | q2-only |
| C43 | `engine: jupyter; engines:;   - knitr:;       claims:;         r: 2` | `r` `python` | E1 | `jupyter, knitr` / `r=knitr, python=jupyter` | same | n/a (none) | intuitive |
| C44 | `engines: [jupyter, knitr]` | `r` `python` | E1 | `knitr` / `r=knitr, python=knitr` | same | engines is a project key listing engine names; it does not select (DERIVED) | intuitive |
| C45 | `engine:;   - knitr;   - jupyter:;       claims:;         r: 1` | `r` | E1 | `knitr` / `r=knitr` | same | rejected by the schema (none) | surprising |
| C46 | `engine:;   - jupyter:;       claims:;         r: 1;   - knitr` | `r` | E1 | `jupyter` / `r=jupyter` | same | rejected by the schema (none) | surprising |
| C47 | `engine:;   - knitr;   - jupyter:;       claims:;         r: 2` | `r` | E1 | `jupyter` / `r=jupyter` | same | rejected by the schema (none) | intuitive |
| C48 | `engine:;   - jupyter:;       claims:;         r: 2;   - knitr` | `r` | E1 | `jupyter` / `r=jupyter` | same | rejected by the schema (none) | intuitive |
| C50 | `project: engine: [jupyter]` + `engine: knitr` | `r` `python` | E1 | `knitr` / `r=knitr, python=knitr` | same | n/a (none) | intuitive |
| C51 | `project: engine: [jupyter]` + `engine: [knitr]` | `r` `python` | E1 | `jupyter, knitr` / `r=knitr, python=jupyter` | same | n/a (none) | intuitive |
| C52 | `project: engine: [knitr]` + `engine: [jupyter]` | `r` `python` | E1 | `knitr, jupyter` / `r=knitr, python=jupyter` | same | n/a (none) | intuitive |
| C53 | `project: engine: knitr` + `engine: jupyter` | `r` `python` | E1 | `jupyter` / `r=jupyter, python=jupyter` | `jupyter, knitr` / `r=knitr, python=jupyter` | document engine: jupyter selects jupyter; the R cell is inert (DERIVED) | surprising |
| C54 | `project: engine: [jupyter]` + `engine: !prefer [knitr]` | `r` `python` | E1 | `knitr` / `r=knitr, python=knitr` | same | n/a (none) | intuitive |
| C55 | `project: engine: [jupyter]` + `jupyter:;   kernel: ark` | `r` | E1 | `jupyter` / `r=jupyter` | `knitr` / `r=knitr` | n/a (project-level engine lists are q2-only) (none) | divergence |
| C56 | `project: engine: [knitr]` + `jupyter:;   kernel: ark` | `python` | E1 | `knitr` / `python=knitr` | same | n/a (none) | surprising |
| C60 | `(none)` | `julia` | E2 | `julia` / `julia=julia` | same | julia engine (RAN) | intuitive |
| C61 | `engine: jupyter` | `julia` | E2 | `jupyter` / `julia=jupyter` | `julia` / `julia=julia` | Q1 picks the julia-1.11 kernel by language even with engine: jupyter (RAN) | divergence |
| C62 | `engine: [knitr, jupyter]` | `r` `julia` | E2 | `knitr, jupyter` / `r=knitr, julia=jupyter` | `knitr, julia` / `r=knitr, julia=julia` | n/a (none) | surprising |
| C63 | `(none)` | `python .marimo` | E3 | `marimo` / `python=marimo` | same | n/a (none) | intuitive |
| C64 | `engine: jupyter` | `python .marimo` | E3 | `jupyter` / `python=jupyter` | `marimo` / `python=marimo` | n/a (none) | surprising |
| C65 | `engine: jupyter` | `python .marimo` | E3b | `marimo` / `python=marimo` | same | n/a (none) | intuitive |
| C66 | `(none)` | `r` `python .marimo` | E3b | `knitr, marimo` / `r=knitr, python=marimo` | same | n/a (none) | intuitive |

# Case notes

- **C10** (engine jupyter owns r): q2 runs the R cell in a kernel chosen by D1 (language lookup); Q1 leaves it inert. Bd-rorntup4.
- **C12** (engine jupyter, r plus python go to one jupyter): Works today (r in knitr, python in jupyter). After D2b both need one kernel, so R1 (kernel/ownership mismatch) matters here.
- **C14** (engine knitr with a julia cell and no julia engine leaves julia unowned): q2 passes the cell through unexecuted with no message (R3 should warn).
- **C16** (engine jupyter, r plus bash): A single jupyter leaves bash inert at execution (RAN: python, sql, bash under engine jupyter ran python only); see R4 for the multi-engine path.
- **C20** (\[jupyter, knitr\], r only: knitr keeps r): D2 as originally approved gave r to jupyter here; D2b keeps knitr.
- **C23** (\[knitr, jupyter\], r plus sql: jupyter owns sql but cannot run it): RAN: render fails with 'engine jupyter has no handler for language sql' (multi-engine path only). Pinned as current; R4 decides the outcome.
- **C24** (\[jupyter, nosuch\], r: an unknown name is ignored and jupyter wins r): No diagnostic for the unknown name today (R3).
- **C25** (engine knitrr (typo), python: nothing owns python): q2 renders with exit 0 and the cell unexecuted (RAN). R3 should warn with a did-you-mean.
- **C30** (jupyter: ark (string form) selects jupyter and owns r): Today the key selects Jupyter as an explicit engine but jupyter only has a Fallback claim, which loses r to knitr's Primary in T1, so knitr runs the cell.
- **C33** (engine: jupyter plus jupyter: \{kernel: ark\} (the epic target)): The Q1-valid spelling of the epic target is engine: jupyter plus jupyter: ark.
- **C34** (engine: knitr plus jupyter: \{kernel: ark\}: the jupyter key is ignored): q2 ignores the key silently (R3).
- **C35** (engine: jupyter plus a top-level jupyter claims map: the claims are dead): Finding 6: the bare-string engine entry carries no config, so the top-level jupyter map is never read for claims and knitr wins. D2b makes the outcome moot (a sole explicit engine owns r), but the claims remain silently dead for any other layout, e.g. when engine: lists a bare jupyter next to knitr. Spike-confirmed with real Ark (Epic A, S1). The working spelling is C42 or jupyter: \{claims: \{r: 1\}\} with no engine: key.
- **C41** (jupyter narrowed to python, julia cell is orphaned): Silent no-op today (R3).
- **C42** (jupyter claims r at Primary(1): the listed engine's claim beats unlisted knitr): The old workaround from the Ark example; D2b makes it unnecessary.
- **C45** (\[knitr, jupyter claims r: 1\]: equal priority, declared order decides (knitr listed first)): The tie: knitr claims r at Primary(1) and so does the table. Candidate order breaks it (strict > in T1). Run with real Ark (Epic A, tie spike): output class cell-output-stdout, i.e. knitr. The jupyter engine owns nothing, so it drops out of the sequence silently (R3).
- **C46** (\[jupyter claims r: 1, knitr\]: equal priority, declared order decides (jupyter listed first)): Mirror of C45; reordering the list flips the owner. Real Ark run: cell-output-display, i.e. Jupyter. knitr owns nothing and drops out.
- **C47** (\[knitr, jupyter claims r: 2\]: a higher priority beats knitr regardless of order): Real Ark run: Jupyter owns r although knitr is listed first. Priority 2 is the order-independent spelling; priority 1 is a tie (C45, C46).
- **C48** (\[jupyter claims r: 2, knitr\]: a higher priority beats knitr (jupyter also first)): Real Ark run: Jupyter. Same owner as C47, so order does not matter at priority 2.
- **C51** (project \[jupyter\] + document \[knitr\] (arrays concatenate)): Under D2 as first approved this flipped r to jupyter; D2b keeps knitr.
- **C53** (project knitr + document jupyter (scalars): the document wins): Same R1 caveat as C12.
- **C56** (project \[knitr\] + document jupyter: \{kernel: ark\}: an engine key anywhere disables the shorthand): The jupyter key is ignored with no message (R3).
- **C61** (engine: jupyter with a Julia extension: the explicit engine takes julia): Documented consequence of D2b. Escape: a jupyter claims table without julia.
- **C62** (\[knitr, jupyter\], r plus julia with a Julia extension: listed jupyter beats the unlisted extension): A user listing knitr and jupyter for r and python may not expect the Julia extension to lose; see R6.
- **C64** (engine: jupyter with a Primary(1) class claim: promoted jupyter wins python .marimo): R6: the real marimo extension claims at Primary(2) (C65) and is unaffected.
