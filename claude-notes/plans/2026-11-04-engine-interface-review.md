---
title: 'Jupyter/engine interface: design review follow-through (Epic B)'
date: 2026-11-04
description: 'Open design work split out of the Ark epic: the engine-resolution redesign (D2b), kernel-versus-ownership rules, diagnostics, Q1 parity, and the resolution corpus that specifies them.'
status: design-open  # D2b is provisional; R1-R7 await the owner; no resolver or engine code changed
braid:
  strand: none  # owner to create an epic strand if wanted; see "Strands" below
---

# Overview

This plan holds the part of the 2026-11-04 design review that is **not**
needed to make Ark run. It was split out of
`2026-11-04-ark-r-kernel-epic.md` (Epic A), which keeps the known fixes:
base64, image diagnostics, kernel resolution (D1), and an end-to-end Ark
render using an explicit `claims` table.

Why the split: Epic A is decided and verifiable. This plan is the unknown
side. In particular the owner considers the T1/T2 redesign (D2b) to need more
study, and the answers to R1 to R7 will shape it. **Treat D2b below as
provisional**, even though it was chosen on 2026-11-04 between the two
alternatives then on the table. Do not implement it until the study phase
(B1) says so.

Artifacts that already exist and are shared with Epic A:

- The corpus: `crates/quarto-core/tests/integration/engine_resolution_corpus/cases.yaml`
  (49 cases; 35 pass today, 14 are `pending-phase4`, a historical marker name
  meaning "expected to fail until the resolver redesign lands"), driven by
  `engine_resolution_corpus.rs`.
- The rendered table and proposed interface:
  `claude-notes/research/2026-11-04-engine-resolution-corpus.md`.
- A reverted D2b prototype: Epic A's session scratchpad is gone with the
  session, but the mechanism is described under "Decided: D2b" below, and the
  corpus is the acceptance test.

How Epic A relates: Epic A's spikes (2026-11-04) confirmed that an explicit
claims entry gives `r` to Jupyter on the current resolver; the canonical
spelling is `jupyter: {kernel: ark, claims: {r: 1}}` with no `engine:` key, and
the same line works in `_quarto.yml`. Tie versus beat against an explicit
knitr was then tested (real Ark, corpus cases C45 to C48): with knitr and
jupyter both listed, a jupyter claim of `r: 1` ties knitr's Primary(1) and the
declared order decides (`[knitr, jupyter]` gives knitr, `[jupyter, knitr]`
gives jupyter), while `r: 2` beats knitr in either order. So `claims: {r: 2}`
is the only order-independent spelling when knitr is listed too. The
dead-claims instance of finding 6 is C35: `engine: jupyter` plus a top-level
`jupyter: {claims: {r: 1}}` map runs knitr because the bare-string `engine:`
entry carries no config. D2b should make C35's outcome moot (a sole explicit
engine owns `r`); that is reasoned from the D2b design, not re-run, since the
prototype is gone. The bare
spellings (`engine: jupyter`, `jupyter: ark`, `jupyter: {kernel: ark}`) owning
`r` are corpus cases C10, C30 and C31 and belong here.

# Note (2026-11-06): effect of the execution-records epic

A report for the owner. It decides nothing, and D2b and R1 to R7 stay open.
Probed on a build of `origin/feature/execution-records-epic` (details in
`2026-11-05-ark-on-execution-records-stack.md`):

- The epic did not change resolution. `claims_language` still returns
  `Fallback(0)` for every Jupyter language, so the corpus compiles with no
  edits and gives the same result: 35 pass, 14 expected-fail. The 14 pending
  cases still fail with their pinned behavior, and C10, C30 and C31 still run
  knitr.
- The epic added kernel selection by name and by language (`kernel_select.rs`),
  and two diagnostics that overlap R1: `Q-18-4` (a `jupyter:` map that names
  no kernel) and `Q-18-5` (an owned language with no runnable kernel). R1\'s
  kernel-versus-ownership check should be designed alongside them. The epic
  also has a notion of an "unrunnable owned language" that overlaps R1.
- A kernel/cell-language mismatch is still silent (a `jupyter: ark` document
  with a `{python}` cell runs the Python code on Ark without a warning). That
  is R1.
- The image-failure warning on this stack took `Q-18-11`, because the epic took
  `Q-18-4`. Codes `Q-18-6` to `Q-18-9` are still unclaimed here.

# Design decisions carried over

## D2. Explicit engines win their languages (bd-rorntup4) — approved 2026-11-04, AMENDED by D2b

**Superseded in part:** the design review below replaced the tie-break with D2b
(decided 2026-11-04). The text of D2 is kept for history. Where it says T2 is
retired, read the D2b section instead: T2 is resurrected and evaluated jointly
with T1.

**Decision: accepted (alternative A).** The proposal: treat an explicitly listed
engine\'s `Fallback(p)` claims as `Primary(max(1, p))` and delete tier T2.
Reasons: it gives Q1 parity for `engine: jupyter` + `{r}`, it needs no
single-engine special case, and ties already resolve by candidate order
(`resolution.rs` T1 uses a strict `>`, so the first candidate wins). This is
the only decision in the plan that changes documented design
(`claude-notes/designs/engine-resolution.md` §2 and §4), so it was gated on
your approval, now given.

Settled with it: the retired tier keeps its **T2 label** (marked retired, no
renumbering of T3 and T4). The bare `jupyter:` key needs no separate work: it is already an explicit
engine declaration, so this change makes `jupyter: {kernel: ark}` alone route
`{r}` to Jupyter exactly as `engine: jupyter` does. Phase 4 pins it with a test.

Consequences to carry into the tests:

- `[jupyter, knitr]` + `{r}` now gives `r` to jupyter (today: knitr). The
  declared order becomes the preference order.
- `[knitr, jupyter]` + `{r}` stays with knitr (tie, knitr listed first).
- Non-explicit `Primary(>=2)` still beats a promoted claim.
- Tests that pin T2 (`test_b2_t2_explicit_fallback_wins_unclaimed_language`
  and the `[knitr, jupyter]` cases near `resolution.rs:1305-1346` and `:1556`)
  change names and comments; most keep their expected owners.
- The worked-cases table in the design doc and the T1-T4 doc comments change.

Correction to the strand: a bare `jupyter:` key already counts as an explicit
engine declaration (`explicitly_declared_engines`), so it is covered by this
same change; see the Phase 4 test list. The strand\'s "Also open" note that q2
does not select Jupyter on a bare key is inaccurate in mechanism: Jupyter is
selected but loses `{r}` to knitr in T1.

If you reject D2, the done-when is unreachable without `claims`, and the epic
must be reworded.

# Design review of the Jupyter/engine interface (2026-11-04)

A review session ran two Sonnet subagents (a Q1 archaeologist and a q2
multi-engine matrix) and re-ran the claims below on the worktree build of main
`94e1f8afe`. Q1 was the dev checkout `quarto` 99.9.9 (works), with Python
3.13.9, R 4.3.2, knitr, reticulate, and the kernels python3, ark, ggsql,
julia-1.11, sos (no `ir`). The subagents' raw output lived in the session
scratchpad and is summarized here; the standalone review document
(`claude-notes/research/`) and the resolution corpus are still to be written (see
the Phase 4 checklist).

Labels: **RAN** means executed with the output read, **DERIVED** means read from
code or reasoned.

## What the review established

1. **Q1 model: one engine owns the whole document; foreign-language cells are inert.**
   Q1 picks the engine from a top-level key named after it (`jupyter:`,
   `knitr:`) or `execute.engine`, else from the first language with a claimant
   (knitr claims only `r`, jupyter only `julia`), else Jupyter for any other
   non-handler language (DERIVED from `external-sources/quarto-cli/src/execute/engine.ts`).
   RAN under Q1: `engine: jupyter` + `{r}` starts the python3 kernel and leaves
   the R cell as inert code; `jupyter: ark` alone selects Jupyter and runs R in
   Ark (python cell inert); `engine: jupyter` + `jupyter: ark` is the closest
   Q1-valid spelling of the epic target and renders all three chunks with the
   plot. q2 today runs knitr for every one of those spellings (RAN).
2. **The epic example file cannot render under Q1.** `engine:` as a map or list
   and `claims` are q2-only; Q1\'s schema rejects them ("The value must instead
   be a string", RAN). Q1 fails on a YAML kernelspec literal naming `ark` (bare
   `path` error, RAN; DERIVED cause `notebook.py` reading `kernelspec.path`).
3. **Q1 kernel specification** (RAN unless noted): `jupyter: X`,
   `jupyter: {kernel: X}` (undocumented but working, not in the schema) and
   `jupyter: {kernelspec: {name, language, display_name}}` are all accepted. An
   unknown name errors for the first two with
   `Jupyter kernel 'nosuchkernel' not found. Known kernels: ...`; a kernelspec
   literal with an unknown name fails inside jupyter_client
   (`NoSuchKernel`). Language lookup picks the first kernelspec whose `language`
   string equals the cell language, exact and case-sensitive (DERIVED from
   `kernels.ts`; Ark declares `R`, so Q1 lookup would not pick it for `{r}`; q2
   D1 matches case-insensitively, an intentional improvement). Project-level
   `jupyter:` never reaches kernel selection in Q1.
4. **Reproduced q2 behavior** (RAN): `jupyter: nosuchkernel` renders silently;
   `engine: knitrr` + `{python}` exits 0 with the cell unexecuted and no
   message; a single-engine Jupyter run leaves `{sql}`/`{bash}` inert with no
   error, but `[knitr, jupyter]` + `{r}`+`{sql}` fails with
   `engine jupyter has no handler for language sql` (multi-engine path only).
   With Ark standing in as `ir` (temporary `JUPYTER_PATH`), a `{python}` cell
   `print(paste0("PY","WRONG"))` was executed by Ark and rendered with exit 0
   and no warning.
5. **Matrix result** (RAN at resolver level, 2160 rows over 27+ declarations,
   12 language sets, 4 registries; today and prototype-D2 from the same
   harness; diff and tables kept in the session scratchpad): D2 as approved
   changes the owner of `r` for `engine: jupyter`, `[jupyter, knitr]`, bare
   `jupyter:` keys and project/document layerings, and nothing else among the
   built-ins. Findings that motivated D2b: `[jupyter, knitr]` became identical
   to `jupyter` (knitr silently owns nothing); a project `engine: [jupyter]` +
   document `engine: [knitr]` (concatenated to `[jupyter, knitr]`) flipped `r` to
   jupyter while the reverse layering kept knitr, so layer order decided
   ownership; an explicit jupyter shadowed a Primary(1) extension engine
   (Julia) at equal priority.
6. **Other defects found** (RAN): object-form and bare-string `claims` in
   front matter (`claims: {r: {kind: primary, priority: 2}}`, `r: primary`) are
   silently dropped because the parser uses `as_str()` on a `PandocInlines`
   value, so knitr ran a document that claimed `r` for jupyter at priority 2;
   integer, `true` and list forms work. An `engine:` key anywhere disables the
   top-level `jupyter:` shorthand (a `jupyter:` key next to `engine: knitr` is
   ignored without a message); with several engine-named top-level keys only the
   alphabetically first is read. The resolver emitted zero notes across all
   2160 matrix rows.
7. **Existing test that D2 breaks and the plan did not list:**
   `engine::resolution::tests::table_fallback_is_a_real_floor` (a `fallback:`
   table on an explicit engine is also promoted). Re-check it under D2b; update
   its name and expectation if the promoted floor now loses or wins differently.
8. **Not verified:** the post-D2 behavior in the real binary (nobody rebuilt
   `q2`), a successful Julia run in either tool (QuartoNotebookRunner and IJulia
   environment problems), reticulate-absent runs, the real marimo extension's
   claim shape (modeled as Primary(2) `whenClass`, DERIVED from `mock_marimo`),
   and the exact text of the D1 "no R kernel" error.

## Decided: D2b (replaces D2\'s tie-break), 2026-11-04

**Decision:** explicit engines win their languages, and **specific beats
universal** among the engines the author named. T2 is *not* retired; it is
resurrected and evaluated **jointly with T1** (the owner\'s framing: it is what
T2 was trying to do, but T1 and T2 must be considered at the same time).

Mechanics: in the combined T1/T2 pass, an explicitly listed engine\'s
`Fallback(p)` counts as `Primary(max(1, p))` ("promoted"). Candidates are
compared on `(priority, class)` in that order, where class ranks:

1. a **listed** engine\'s own `Primary` claim;
2. a **listed** engine\'s promoted `Fallback`;
3. an **unlisted** engine\'s `Primary`.

Candidate order (listed engines first, in declared order) breaks any remaining
tie. T3 (Interop) and T4 (implicit Fallback) are unchanged; T4 stays
implicit-only.

Consequences (all DERIVED from the rule; the corpus will pin them):

| Declaration | Languages | Owners after D2b | Today |
|---|---|---|---|
| `engine: jupyter` | `{r}` | r=jupyter | r=knitr |
| `jupyter: ark` / `jupyter: {kernel: ark}` (no `engine:`) | `{r}` | r=jupyter | r=knitr |
| `[jupyter, knitr]` | `{r}`+`{python}` | r=knitr, python=jupyter | same |
| `[knitr, jupyter]` | `{r}`+`{python}` | r=knitr, python=jupyter | same |
| project `[jupyter]` + document `[knitr]` (concat) | `{r}`+`{python}` | r=knitr, python=jupyter, in either layer order | order-dependent under approved D2 |
| `engine: jupyter` + engine `claims` or `engines:` table giving an unlisted engine `Primary(2)` | the claimed language | unlisted `Primary(2)` wins (priority first) | same |
| `engine: jupyter` | `{julia}` with a Primary(1) Julia extension | julia=jupyter (explicit wins; escape: a `claims` table without julia) | julia=extension |

Why: it keeps the Q1 meaning of `engine: jupyter` (Jupyter owns the document),
keeps a listed second engine meaningful, and removes the merge-order
dependence the review found. Only the unlisted-versus-listed case changes
behavior for documents that work today.

# Recommendations awaiting the owner

The owner had no energy to answer these on 2026-11-04. Each carries a
recommendation; none is implemented or decided. Do not start the dependent
checklist items until answered.

## R1 (D1 amendment). Kernel versus ownership

Today and under D1 as written, one kernel is chosen (first executable cell) and
applied to every cell Jupyter owns. RAN: that runs a python cell in Ark silently.
D2b makes it more reachable: `engine: jupyter` + `jupyter: ark` + `{r}`+`{python}`
sends both to Jupyter. **Important regression to weigh:** `engine: jupyter` +
`{r}`+`{python}` works today (python in Jupyter, r in knitr, RAN); after D2b both go
to one kernel, so one language runs in the wrong kernel. Options:

- **Error on mismatch (recommended interim).** Hard error naming the cell, its
  language and the kernel\'s declared `language` (case-insensitive). Cheap: the
  kernelspec language is already read by `find_kernelspec_for_language`. Escape:
  narrow with `claims` or list another engine.
- Warn and skip mismatched cells (Q1 parity; Q1 leaves them inert), at the cost
  of silently missing output.
- Multiple kernels per document, one per language. Least surprising, but
  `execute_blocks_async`, `partition_cells` and `daemon::kernel_scope` assume
  one kernel; likely its own epic. The regression above is the strongest
  argument for it. If you choose the interim error, file the per-language
  kernel work as a follow-up strand.

## R2 (D1 amendment). Accept the `kernelspec` spelling

Q1 accepts `jupyter: {kernelspec: {name, language, display_name}}`. Recommend D1
step 1 also read `jupyter.kernelspec.name` as an explicit name. Precedence:
`engine: {jupyter: {kernel}}`, then `jupyter: X` / `jupyter.kernel`, then
`jupyter.kernelspec.name`; a warning when two sources name different kernels
(today `engine: {jupyter: {kernel: a}}` + `jupyter: {kernel: b}` silently uses a).
Keep the string form `jupyter: ark` (Q1 accepts it, and it is the Q1-valid epic
spelling). Do not support Q1\'s literal-kernelspec failure mode.

## R3. Diagnostics (new phase)

The resolver emitted no notes in 2160 matrix rows, yet these outcomes are
silent or surprising (all RAN unless noted). Recommend resolver notes, surfaced
as warnings, with Q-codes, catalog entries and docs pages per the lint rules:

- a language with no owner when the sequence is explicit (`engine: knitr` +
  `{julia}` with no julia engine; narrowed jupyter; `engines:` claims alone):
  warn naming the language and the declared engines (the cell stays
  unexecuted);
- a listed engine that owns nothing (`[jupyter, knitr]` where knitr loses
  everything);
- an unknown engine name in `engine:` with a did-you-mean (`knitrr`,
  `[jupyter, nosuch]`); unknown engine-looking top-level keys only as an
  optional lint;
- a top-level `jupyter:` key ignored because `engine:` selects another engine,
  and more than one engine-named top-level key;
- a kernel named by two sources with different values (R2);
- `engine: jupyter` + `{r}` with no R kernel: the D1 error should say the cell
  language has no registered kernel and name `jupyter: {kernel: ...}` as the fix
  (the existing `KernelspecNotFound` text already lists searched paths).

## R4. `{sql}`/`{bash}` under Jupyter in the multi-engine path

Single-engine Jupyter leaves them inert with no error (RAN), but in
`[knitr, jupyter]` + `{r}`+`{sql}` Jupyter owns sql via the promoted/explicit
fallback and the run fails with `engine jupyter has no handler for language sql`
(RAN, unchanged by D2b). Options: (a, recommended minimal) make the multi-engine
path match the single-engine path, leaving the cell inert with a warning; (b)
have Jupyter decline languages it cannot execute so knitr\'s Interop claim takes
sql/bash, which is the most intuitive but changes the design rule that Jupyter
never enumerates languages (`engine-resolution.md` §4.3).

## R5. Claims parser bug (out of plan scope)

Object-form and bare-string `claims` are silently dropped in document front
matter (`as_str()` on `PandocInlines`; the `metadata-as-str` lint exists for
exactly this). Not needed for the epic: docs can use the integer form
(`claims: {julia: 2}`), which works. Recommend a separate braid strand, fixed
before any docs show the object form.

## R6. Class-specific claims versus promoted universal claims

A Primary(1) extension claim on `{python .marimo}` loses to an explicit jupyter
under D2b (the real marimo extension claims at Primary(2), so it is unaffected).
Recommend documenting, not building: a `whenClass` claim at the default
priority should win ties against a promoted universal claim only if a real
extension needs it.

## R7. Q1 divergences to document, not fix

Q1 leaves foreign-language cells inert under a single engine; q2 runs them in
the owning engine or leaves them inert plus a warning (R3). `engine:` maps and
lists, `claims`, and q2 language-lookup by kernelspec are q2-only. Q1 handler
languages (`{ojs}`, `{mermaid}`) were not rendered by this q2 build under either
engine (RAN, A3e); not investigated here, recommend its own strand.

# Checklist

Corpus-first, TDD: write the failing test, watch it fail for the stated
reason, then fix. Gate each phase on
`cargo clippy -p quarto-core --all-targets -- -D warnings` and
`cargo nextest run -p quarto-core`.

## Phase B1: study and owner decisions (do first)

- [ ] Walk R1 to R7 with the owner, one at a time, with the recommendation
  stated; record each answer in this file.
- [ ] Re-examine D2b with the answers in hand. Questions to settle: does R1
  (kernel versus ownership) change which engine should own `{r}` under
  `engine: jupyter`; is a joint T1/T2 pass still the right mechanism, or does a
  narrower rule (for example, only a sole explicit engine claims) do; what does
  the `(priority, class)` key mean for extension engines (R6).
- [ ] If D2b changes, update the corpus `expect` blocks first (they are the
  specification), then the rest of this plan.

## Phase B2: kernel amendments (pending R1, R2)

Moved from Epic A's Phase 3; they extend D1 in the Epic A plan.

- [ ] (pending R2) also read `jupyter.kernelspec.name`; define precedence and
  warn on conflicting kernel sources.
- [ ] (pending R1) validate the explicit kernel\'s language against each owned
  cell language; error on mismatch. Needed as a gate before B3 lands, because
  B3 widens what Jupyter owns.
- [ ] If the owner picks per-language kernels, open a follow-up strand
  (`execute_blocks_async`, `partition_cells`, `daemon::kernel_scope` assume
  one kernel).

## Phase B3: explicit engines win their languages, D2b (bd-rorntup4)

- [x] **Corpus first (the test specification).** Written 2026-11-04:
  `crates/quarto-core/tests/integration/engine_resolution_corpus/cases.yaml`
  (44 cases at the time; 49 after the Epic A tie spike added C35 and C45 to
  C48) driven by one test, `engine_resolution_corpus.rs` (registered in
  `main.rs`), plus the rendered table
  `claude-notes/research/2026-11-04-engine-resolution-corpus.md` (generated by
  `render_table.rb` next to the cases). Result on main `94e1f8afe`: 44 cases,
  31 pass, 13 expected-fail (`pending-phase4`, each pinning today\'s behavior);
  with the five additions, 49 cases, 35 pass, 14 expected-fail (the new
  expected-fail is C35).
  A temporary prototype of D2b (joint T1/T2 pass, reverted) turned all 13
  expected-fail cases into passes and kept all 31 passing cases passing; of the
  existing `engine::` unit tests only `table_fallback_is_a_real_floor` failed
  under it (724 of 725 passed). Phase 4 work: flip each `pending-phase4` marker
  to `pass` (the test fails until you do) and drop its `today` block. Extend the
  corpus, with expected-fail markers, as R1 to R4 are answered.
- [ ] Tests first: `engine: jupyter` + `{r}` gives `r` to jupyter;
  `[jupyter, knitr]` + `{r}` keeps `r` with knitr and gives python to jupyter;
  `[knitr, jupyter]` unchanged; project `[jupyter]` + document `[knitr]` gives
  the same owners in both layer orders; non-listed `Primary(>=2)` beats a
  promoted claim; a listed engine\'s Primary(1) beats another listed engine\'s
  promoted Fallback; an unlisted Primary(1) loses to a listed promoted Fallback.
- [ ] Test: a bare `jupyter: {kernel: ark}` key with no `engine:` gives `r` to
  jupyter, and the same for the string form `jupyter: ark`.
- [ ] Implement the joint T1/T2 pass in `run_tiers` (promotion plus the
  `(priority, class)` comparison) and keep T4 implicit-only. T2 keeps its label.
- [ ] Update the pinned tests, including
  `table_fallback_is_a_real_floor` (found by the review, not previously listed),
  the tier comments, and `claude-notes/designs/engine-resolution.md` (§2
  wording, §4 tiers, §4.4 table).
- [ ] Check user-facing docs under `docs/` for any statement of the old
  "explicit engine preempts selection but not division" behavior.

## Phase B4: diagnostics from the review (pending R3; do not start before answered)

- [ ] Corpus cases for each silent outcome in R3, marked expected-to-fail until
  implemented.
- [ ] Resolver notes plus Q-codes, catalog entries, docs pages and sidebar
  entries in one commit per the lint rules.
- [ ] (pending R4) decide the multi-engine `{sql}`/`{bash}` outcome.

## Phase B5: docs and Q1 parity

- [ ] Document the bare spellings and, once decided, the ownership rules, in
  user terms (not resolver internals), under `docs/`.
- [ ] Document the Q1 divergences from R7.
- [ ] Update `claude-notes/designs/engine-resolution.md` to whatever B1 decides.

## Strands (out-of-plan items, owner to approve)

- [ ] R5: claims parser drops object-form and bare-string `claims`
  (`as_str()` on `PandocInlines`). Small bug, independent of everything here.
- [ ] R7: `{ojs}` and `{mermaid}` handler languages not rendered by this q2
  build; needs its own investigation.

# Ordering and risk

B1 gates everything. B2\'s R1 gate must precede B3. B3 is the only phase with a
user-visible change for documents that work today (the unlisted-versus-listed
case in the D2b table), so it needs the owner\'s approval at that time.
`quarto-core` has downstream dependents (`wasm-quarto-hub-client`); before
pushing, recommend `cargo xtask verify --skip-hub-build` at minimum plus the
WASM leg.
