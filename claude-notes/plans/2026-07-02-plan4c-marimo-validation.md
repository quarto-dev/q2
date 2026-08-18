# Plan 4c: Marimo Engine Validation (+ sql-interop feature)

**Status:** COMPLETE (2026-07-03) — plus additive Phase 4cH (marimo preview e2e-pw, user-requested 2026-07-03, in progress). All phases 4c0/4c0-eng/4cA/4cB/4cB2/4cC/4cD/4cE/4cF/4cG executed SDD-style, every task review-approved. Seams SC1-SC20 disposed (SC1-SC18+SC20 green with dated corrections where noted in-row; SC19 green upstream + rebundled; canFreeze accepted-untested → bd-mx5x609r). Four q2-core findings fixed en route (pampa writer 411380777; TS-includes 13f697c85; claimsFile-short-circuit seam corrections; handledLanguages leave-alone inversion 77c15c8+b4f4f52bf). Migration guide: claude-notes/research/2026-07-03-marimo-migration-guide.md. Verification: adopted merged-state full verify green (see 4cF). NOT pushed/merged to main — awaiting cumulative approval. **Phase 4cH DONE (2026-07-03)** as SC21-NEG — marimo through q2 preview is a limitation-pinning canary (marimo capture records server-side but never splices into the pane; FINDING #5, strand bd-5jxcio5d): live GREEN 7.5s + skip-clean + marimo-leg revert RED (SC8's literal two-part entry-only revert: remove only the `python:` claim entry + `claims-files: []` → render fails jupyter-unavailable → no capture). Spec `q2-preview-spa/e2e/engine-capture-splice-marimo.spec.ts` + additive `previewServer.ts serverLog()`.

*(original status below for history)*
**Status (historical):** plan — revised 2026-07-02 (2nd pass) after source review + implementer
review. Ready once the Julia plan's extension-build scaffolding is confirmed and
Phase 4c0 + 4c0-eng land.
**Created:** 2026-07-02. **Revised:** 2026-07-02 (2nd pass).
**Sequence:** parallel to Plan 4b; a second real-world TS-engine validation
alongside the Julia validation. Reuses the Julia plan's `build-ts-extension` +
fixture-copy structure.
**Depends on:** Plans 1a–c, 1b, 2A, 2 (the QuartoAPI surface marimo touches is
all non-`jupyter`). **Plan 3 (jupyter) is required for the coexistence tests
(4cD) only** — see *Relationship to other plans*. Plus `marimo` + `uv` installed
on the test machine (skip tests when absent, like Julia's `julia` binary).
**Validation target:** the marimo engine at `~/src/quarto-marimo`
(`src/marimo-engine.ts`), git `76d6f1d` at review time.

> **Scope note — this is a feature build, not just validation.** Decision
> 2026-07-02 (Option B): marimo should treat **bare `{sql}` as an `Interop`
> language** (rides along when marimo is already present via a python-marimo
> primary), matching knitr's `sql: Interop`. The shipped marimo engine does
> **not** do this (its `claimsLanguage` returns `false` for bare sql). Delivering
> it requires two things beyond validation:
> 1. **4c0** — widen q2 static claims to **`Vec`-per-language** (one `sql` key
>    must hold *both* a primary-when-tagged claim and an interop-otherwise claim).
> 2. **4c0-eng** — modify the marimo engine itself so its **live `claimsLanguage`
>    returns interop for bare sql** *and* it **actually executes** bare sql cells
>    when q2 says it owns them. This is required (not optional) because q2
>    **hard-errors on any static-vs-dynamic claim mismatch** (`ts_engine.rs:286`):
>    a declared static interop that the module's `claimsLanguage` contradicts
>    fails the render. These engine changes are upstreamable to `quarto-marimo`.
>
> All of this is in-scope plan work (required to complete the chosen behavior),
> not separate braid strands.

## Corrections from review (2026-07-02)

Recorded so the decisions are durable.

1. **Marimo is a python *and* sql engine.** `marimo-engine.ts:158-168`:
   `(python|sql)+firstClass "marimo" → 2`, `python.marimo|sql.marimo → 1`, else
   `false`. q2 already treats `sql` as a shared language — **production knitr**
   claims it `Interop(0)` (`crates/quarto-core/src/engine/knitr/mod.rs:245`), and
   §4.4 line 248 flags `{sql}`+explicit-jupyter as a §10 loud failure.
   (`resolution.rs:702` is the test *mock* illustrating the same shape.)

2. **The `python.marimo` dotted branch is LIVE in q2.** Verified with pampa:
   | source | pampa `CodeBlock` classes | resolver sees `(lang, first_class)` |
   |---|---|---|
   | `{python .marimo}` | `["{python}", "marimo"]` | `("python", Some("marimo"))` |
   | `{python.marimo}`  | `["{python.marimo}"]`     | `("python.marimo", None)` |
   | `{sql.marimo}`      | `["{sql.marimo}"]`        | `("sql.marimo", None)` |
   `engine_cell_lang` (`crates/quarto-core/src/engine/capture_splice.rs:74`)
   strips the outer `{…}` and returns the inner token verbatim → the dot-joined
   form is a *distinct language token* with no first_class, needing its **own**
   claim key. (Gordon confirmed Q1's
   parser did the same; the dotted form is still supported, if discouraged.)

3. **sql = Interop for bare `{sql}`, Primary for tagged sql (Option B).** This is
   a **q2/team behavior the shipped engine does not implement** — see the scope
   note. Requires 4c0 (Vec) + 4c0-eng (engine change).

4. **Static (§3.3) and dynamic (§3.2) are separate paths.** A declared `claims:`
   block resolves at Pass-1 without loading the engine; the JS `claimsLanguage`
   is never called for resolution. `number → Primary(n)` normalization is already
   unit-tested (`mapLanguageClaim`, `host.test.ts:430`; object forms 479/534). 4c
   ships two fixtures: static (4cA/4cB) and dynamic (4cB2).

5. **`markdownForFile` IS called by q2** (`ts_protocol.rs:71`, `host.ts:513`) —
   not inert. Only `partitionedMarkdown` and `postprocess` are inert (no wire
   message / dispatch case). `target` is required, called inside execute
   (`host.ts:647`); `dependencies` is called (`host.ts:799`).

6. **HTML flows through `include-in-header`, not `store_html_dependencies`.**
   Engine returns `includes["include-in-header"]` (a `PandocIncludes` temp file,
   engine 300-310) and inlines figures as raw `{=html}`/`![](…)` via
   `render-output.ts`. `generatesFigures: true` has **no consumer in q2**
   (acceptable). 4cC validates the real sink.

7. **Copy list + loader shim.** `_extensions/marimo/marimo-engine.js` is a
   GitHub-release *downloader shim*, not the engine. The default (uv) render path
   calls `command.py` (was omitted). `_extension.yml` also contributes a
   `marimo-deprecated.lua` filter → **omit it from the fixture** (a deprecation
   shim irrelevant to validation).

8. **`breakQuartoMd`'s custom-regex arg is supported** (`markdownRegex/index.ts:764`,
   4th param `startCodeCellRegex?: RegExp`).

9. **q2 hard-errors on static-vs-dynamic claim mismatch** (`ts_engine.rs:242-293`,
   error at `:286`). When the engine loads (at execute time), q2 compares every
   static answer it recorded against the module's live `claimsLanguage` and
   **fails the render** on mismatch. This is *why* 4c0-eng is mandatory: the
   declared bare-sql `Interop` must match a modified `claimsLanguage`.

10. **Per-language single owner — no python split in one doc (v1 limitation).**
    §4.2: ownership is keyed by **language**, not `(language, first_class)`;
    `walk_block_for_langs` (`resolution.rs:127-150`) records only the *first*
    occurrence's `first_class` per language. A doc mixing `{python .marimo}` and
    plain `{python}` and wanting **different engines** is a documented v1
    limitation (same as Q1's single-winner-runs-all). **The original plan's
    headline "marimo cells → marimo, plain python → jupyter in one doc" was
    wrong and is removed.** `first_class` drives *selection of the one owner*;
    to show marimo-vs-jupyter you use *separate docs* (4cD). Single-doc
    multi-engine coexistence must use *different languages* (e.g. marimo-python +
    jupyter-bash), not two flavors of python.

11. **`handledLanguages` is plumbed to the engine's execute options — but the
    marimo engine doesn't read it yet** (implementer review, B2). The **host**
    side is wired: `crates/quarto-core/src/stage/stages/engine_execution.rs:345`
    `resolution.handled_languages_for(engine.name())` → `with_handled_languages`
    → `host.ts:680` passes `handledLanguages` into execute options. But
    `marimo-engine.ts:214` reads only `{ target, format }` — consuming
    `handledLanguages` is **net-new plumbing** in 4c0-eng, not a tweak. And
    `extract.py` receives raw markdown on stdin with **no** ownership signal, so
    the ownership gate must be threaded **TS-side** (see 4c0-eng).

12. **Spike (2026-07-02) — bare-`{sql}` execution is FEASIBLE with a ~3-line
    `extract.py` change; `{sql .marimo}`/`{sql.marimo}` already execute today.**
    `extract.py` hands the doc to marimo's own parser (`MarimoMdParser`) — there
    is **no `.marimo` gate in `extract.py`**; the gate is entirely TS-side
    (`marimo-engine.ts` routing). marimo classifies a cell as SQL only when the
    fence is the qmd-form `sql {.marimo}` (language before brace); the existing
    `SQL_DOT_FENCE_REGEX` pre-rewrites `{sql .marimo}`/`{sql.marimo}` into that
    form. Bare `{sql}` isn't covered → misclassified as `python` → syntax error.
    Fix: a sibling `BARE_SQL_FENCE_REGEX` rewriting bare `{sql …}` →
    `sql {.marimo …}` (proven working in the spike; the SQL bridge
    `sql_code_to_python` already exists). SQL execution needs runtime deps
    `duckdb`+`sqlglot`+(`polars`+`pyarrow` or `pandas`) in the eval env, declared
    via the `pyproject`/uv `--with` flags the engine already threads through
    `command.py`. (Spike used marimo 0.23.1; confirm against the pinned version.)

## What's genuinely different from Julia (the value of 4c)

1. **`first_class`-driven *selection*.** Marimo owns `python` only when the first
   python cell is `{python .marimo}`; otherwise jupyter does. First real exercise
   of the `first_class`/`whenClass` dimension (§4.2/§3.3). (Shown across separate
   docs — not a same-doc split; see correction 10.)
2. **`Interop` presence-gating via sql (the new feature).** Bare `{sql}` rides
   along only when marimo is already present — the first `whenClass`+`Interop`
   combination on one language key, and the first engine that *executes* an
   interop-owned language. Julia has none.
3. **The dotted-language token (`python.marimo`/`sql.marimo`).** Distinct language
   tokens with their own claim keys. Julia never touches this.
4. **Single-doc two-engine coexistence via distinct languages.** e.g.
   `{python .marimo}` (marimo) + `{bash}` (jupyter). Exercises `handled_languages`
   enforcement (§5) with two live engines.
5. **`canFreeze: false`.** Julia validates the `true` path; marimo the `false`.

## Static-claims design (Option B) + required engine parity

Cell shapes, the static claim that must match, and what the **modified** engine
`claimsLanguage` must return so static == dynamic (correction 9):

| you write | `(lang, first_class)` | static claim | modified `claimsLanguage` |
|---|---|---|---|
| `{python .marimo}` | `("python","marimo")` | `Primary(2)` | `2` |
| `{python.marimo}`  | `("python.marimo",—)` | `Primary(1)` | `1` |
| `{sql .marimo}`    | `("sql","marimo")`    | `Primary(2)` | `2` |
| `{sql.marimo}`     | `("sql.marimo",—)`    | `Primary(1)` | `1` |
| `{sql}` (bare)     | `("sql",—)`           | `Interop(0)`  | `{kind:"interop"}` ← **change** |
| `{python}` (bare)  | `("python",—)`        | `None`        | `false` |

Only the bare-sql row changes in the engine (`false` → `{kind:"interop"}`); the
rest already agree. `priority` values mirror Q1 but are **cosmetic here** —
`python`(2) and `python.marimo`(1) are different keys that never compete; only
the *kind* matters for correctness.

Fixture `_extension.yml` (`sql` carries **two** claims — the Vec form from 4c0):

```yaml
name: marimo
claims:
  python:
    - { whenClass: marimo, kind: primary, priority: 2 }
  "python.marimo":
    - { kind: primary, priority: 1 }
  sql:
    - { whenClass: marimo, kind: primary, priority: 2 }   # {sql .marimo} self-activates
    - { kind: interop }                                    # bare {sql} rides along
  "sql.marimo":
    - { kind: primary, priority: 1 }
```

**Combine rule for a Vec (4c0):** map each claim via
`static_claim_to_language_claim` (yields `None` on `whenClass` mismatch), drop
`None`, reduce by a **new explicit `ClaimKind` comparator** — Primary > Interop >
Fallback, priority as tiebreak. This mirrors the *intent* of the "kind dominates
priority" note at `resolution.rs:321`, but that is a **doc-comment, not a reusable
function** — cross-engine precedence there is emergent from running the T1→T4
tiers in sequence, so the per-Vec reducer is **genuinely new code** (define the
`ClaimKind` ordering deliberately). Marimo only exercises Primary-over-Interop
(for `{sql .marimo}`, both claims match → `Primary(2)` wins; bare `{sql}` → only
interop matches → `Interop(0)`). Interop-vs-Fallback within one key is defined
but unused.

> **Edge (documented, not fixed):** explicit `[marimo, jupyter]` makes §4.4's
> T2 explicit-Fallback preempt T3 Interop, diverting bare `{sql}` to jupyter.
> Under normal usage (`engine: marimo`, jupyter only implicit) T3 precedes T4 and
> marimo keeps sql.

## QuartoAPI surface

Touched: `quarto.console`, `quarto.system.pandoc` (pdf/latex/typst only, via
`htmlToMarkdown`), `quarto.mappedString.fromFile`, `quarto.markdownRegex`
(`extractYaml`/`partition`/`breakQuartoMd` **with a custom cell regex**). All
non-`jupyter`, Plan-2-complete. Instance methods: `target`/`execute`/
`markdownForFile`/`dependencies` **live**; `partitionedMarkdown`/`postprocess`
**inert**. Engine bypasses `PlatformHost` in spots (raw `Deno.readTextFileSync`/
`Deno.Command`) — fine for subprocess, not WASM-portable; note only.

## Test Seam Spec (frozen — prevalidated 2026-07-02)

One row per test: **tier · real unit (never mocked) · seam · mock boundary ·
named revert hunk → RED assertion**. Once a test is green its harness +
assertions are **frozen** (never edited to go green — fix production or the
spec, not the test). Tiers: `unit-rs`/`unit-ts`/`unit-py` (pure), `int-rs`
(real `resolve` over a claims registry — **no subprocess**, since static-claim
resolution is Pass-1), `e2e` (real `q2 render` + marimo/uv subprocess —
env-gated skip when absent). **Ownership is Pass-1 → most seams need no marimo
binary; only execution/output/dynamic-path seams do.**

**int-rs faithfulness (B-1/B-2 — mandatory):** every `int-rs` row builds a
**real** engine registry from the fixture `_extension.yml` via
`build_engine_registry`/`read_extension` and calls `resolve_engines`. The marimo
`TsEngine` answers `claims_language` from its parsed static-claim **Vec** with
**zero subprocess** (short-circuit at `ts_engine.rs:601-624`; proven by
`test_p1_12_static_zero_load`, `ts_engine.rs:1628`), so the unit under test is the
real `read.rs` parse + `types.rs` combine/`whenClass`/interop — **not** a
`resolution.rs` `MockEngine` closure (which would bypass all 4c0 code and be
vacuous). The discriminator engines (jupyter, knitr) are the **real builtins**
`JupyterEngine::new()`/`KnitrEngine` (`registry.rs:87`) — native, no subprocess.
So "Mock boundary: none" on int-rs rows means *no closures*; the parsed Vec + real
builtins are the units.

| ID | Phase | Tier | Real unit | Seam → assertion surface | Mock boundary | **Revert hunk → RED** |
|----|-------|------|-----------|--------------------------|---------------|-----------------------|
| SC1 | 4c0 | unit-rs | `lookup_static_claim` + combine reducer | map `sql → [ {interop}, {whenClass:marimo,primary,2} ]` (**interop listed FIRST**); call with `Some("marimo")` and `None` → returned `LanguageClaim` | none | Reducer → "return first non-`None`": `lookup(sql,Some("marimo"))==Primary(2)` RED (yields `Interop(0)`) |
| SC2 | 4c0 | unit-rs | `parse_claims_map`/`parse_static_language_claim` | parse a `ConfigValue` seq `sql: [ {…primary…}, {kind:interop} ]` → `map["sql"].len()`/kinds | none | New YAML-sequence arm (falls to `_=>None` today) → `map["sql"].len()==2` RED (key absent) |
| SC3 | 4c0 | unit-rs | `claims_language` fallback-key path (`ts_engine.rs:604-607`) | registry (real parsed claims) with a `fallback` key + a normal claim → lookup an unclaimed lang | mock transport (no load) | Revert the `map.get("fallback")` site to the pre-Vec single-claim `static_claim_to_language_claim(fb,…)` call → `claims_language(unclaimed)==Fallback(n)` RED (compile-error against the Vec type counts as RED) |
| SC4 | 4c0-eng | unit-ts | `claimsLanguage` | `claimsLanguage("sql",undefined)` **and** `("python",undefined)` → return value | none | New `if(lang==="sql") return {kind:"interop"}` → `("sql",undefined)` `toEqual({kind:"interop"})` RED (`false`) |
| SC5 | 4c0-eng | unit-ts | **new** gated predicate `cellOwnedByMarimo(cell, handledLanguages)` (a *new* fn — do NOT overload the existing `isMarimoCell(cell)` at `is-marimo-cell.ts:9`, which is called `isMarimoCell(cell)` at `marimo-engine.ts:271`) | bare-sql cell with `handled=["sql"]` **and** `[]`; plain-python cell with `["sql"]` | none | `handled.includes("sql")` gate → `pred(bareSql,[])===false` RED (returns true) | (2026-07-02, task 4cB2-fix, FINDING #4: **semantics corrected, controller-ratified.** `handledLanguages` is q2's leave-alone set, not positive ownership — bare-sql `handled=[]` (sql NOT left alone) → OWNED (`true`); `handled=["sql"]` (sql left alone for someone else) → NOT owned (`false`) — the inverse of the row's original fed values/expectations above. RED captured against the flipped `cellOwnedByMarimo` implementation with the OLD (pre-correction) test file: 81 passed/2 failed, exactly the two direct-gate assertions; corrected assertions then GREEN, 83/83. See `~/src/quarto-marimo` commit fixing FINDING #4 and `.superpowers/sdd/task-4cB2-fix-report.md`.) |
| SC6 | 4c0-eng | unit-py | `rewrite_bare_sql(text,enabled)` (factored from `convert_from_md_to_pandoc_export`) | bare `{sql}` @`enabled=True`/`False`; `{sql.marimo}`, `{python}` @`True` | none | `BARE_SQL_FENCE_REGEX.sub` → `enabled=True` output contains `sql {.marimo` RED |
| SC7 | 4c0-eng | unit-ts | `containsMarimoFence` | `containsMarimoFence("```{sql}\n…")` → `false`; `("```{python .marimo}…")`→`true` | none | **B1 guard**: dropping the `(?=.*\.marimo)` lookahead from the shared regex → bare-sql `===false` RED (true) |
| SC8 | 4cB | e2e | full render pipeline + marimo subprocess | `q2 render` a `{python .marimo}` `1+1` doc → grep HTML | marimo/uv (env skip) | Revert the marimo `python` claim → HTML lacks a **marimo-specific** marker RED. **Assert the marimo signature, not just `2`** (jupyter also emits `2`): the injected `include-in-header` marimo header / `<marimo-code` / `__MARIMO_EXPORT_CONTEXT__` (extract.py 293-327) **and** `2` (2026-07-02: named revert corrected to `claims-files:[]` + python-claim removal; original single-part revert non-binding due to `EngineClaimsFileStage` whole-file `claimsFile` short-circuit at `engine_execution.rs:225`; RED verified) |
| SC9 | 4cB2 | e2e | dynamic path (`ts_engine.rs:625` else) + subprocess | render `{python .marimo}`+`{sql}` doc via **claims-less** fixture → `ownership["sql"]` | marimo/uv (env skip) | SC4 (claimsLanguage interop) → dynamic `ownership["sql"]=="marimo"` RED (jupyter); parity with static breaks | (2026-07-02, task 4cB2: **NOT GREEN — BLOCKED, NEEDS_CONTEXT.** The resolver-level assertion IS achievable and confirmed correct (`ownership["sql"]=="marimo"` via the dynamic path, after the pre-authorized `claims-files:[]` fix), but the *behavioral* observable this row implies — marimo's rendered output showing it executed the sql cell — is blocked by a second, independent defect: `marimo-engine.ts`'s `bareSqlOwned` gate reads the wire `handledLanguages` (a leave-alone set) with inverted sense, so it never fires when marimo genuinely owns bare sql. See `marimo_engine_e2e.rs`'s SC9 doc comment and compat doc §13 for the full evidence trail. Not committed as a passing test pending controller adjudication.) (2026-07-03, task 4cB2-completion: **GREEN — CLOSED.** FINDING #4 fixed and controller-ratified (`~/src/quarto-marimo` `77c15c8`, rebundled into this fixture at q2 `b4f4f52bf`): `bareSqlOwned`/`cellOwnedByMarimo` flipped to `!handledLanguages.includes("sql")`. `write_claims_less_extension_yml` now always appends `claims-files: []` (the anti-vacuity correction this row's own evidence justified), so every dynamic-path test — including this one — genuinely exercises per-language resolution, never the whole-file short-circuit. Committed test: `sc9_bare_sql_interop_dynamic_path_marimo_executes_sql_cell` in `marimo_engine_e2e.rs`. Behavioral-proxy observable (unchanged from the original choice): rendered HTML contains a `<marimo-table>` island with `data-data` carrying the computed `{"x":2}` row, inside `<marimo-cell-output>` — firsthand-inspected. RED-by-revert (SC4's named revert, run against the SAME claims-files:[]-fixed variant): reverting the TEMPDIR bundle's `claimsLanguage` bare-sql branch to `false` reproduces the exact same hard failure as before the fix — `Error: Engine 'jupyter' is registered but its runtime is not available...` — captured verbatim in the test's doc comment; fixture restored byte-identical, re-confirmed GREEN. `cargo nextest run -p quarto-core -E 'test(marimo_engine_e2e)'`: 3/3 green.) |
| SC10 | 4cC | e2e | render + `include-in-header` wiring | render a `mo.ui`/plot doc → grep HTML for header content + raw-`{=html}` figure | marimo/uv (env skip) | engine `includes["include-in-header"]` (300-310) → header markup present RED | (2026-07-03, task 4cC: **GREEN — CLOSED.** Committed test `sc10_widget_render_shows_header_include_and_body_island` in `marimo_engine_e2e.rs`, static-claims path, conjunctive assertions (header marker in `<head>` AND `<marimo-island>`/`<marimo-ui-element>` in `<body>`). RED-by-revert (TEMPDIR-only bundle copy, neutered the `include-in-header` population) reproduces exactly the header-marker half failing while the body island half still passes — captured verbatim in the test's doc comment; fixture restored byte-identical, re-confirmed GREEN. See compat doc §14.) |
| SC11 | 4cD | int-rs | real registry: parsed fixture claims + `JupyterEngine` builtin | doc A `{python .marimo}`-only → `ownership["python"]`; doc B `{python}`-only → same | none (real builtins) | (a) python claim → A `=="marimo"` RED; (b) `whenClass` guard → **B `=="jupyter"` RED** |
| SC12 | 4cD | int-rs | `resolve` | `{python.marimo}` / `{sql.marimo}` cells → ownership | none | dotted claim keys → `ownership["python.marimo"]=="marimo"` RED |
| SC13 | 4cD | int-rs | `resolve` | `{sql .marimo}`-only doc (no python) → `ownership["sql"]` | none | `sql` primary-when-`marimo` claim → `=="marimo"` RED (self-activation lost) (2026-07-03, task 4cD-e2e: the *renders* e2e half is GREEN — `marimo_engine_e2e.rs::sc13_e2e_tagged_sql_self_activation_renders`. A companion `{python .marimo}` import-only cell was needed for genuine `mo.sql()` execution (marimo's own runtime requirement, confirmed empirically — see the test's doc comment); the revert-hunk drops the ENTIRE `sql:`+`"sql.marimo":` claim keys (not just the whenClass-primary entry — see the test's anti-vacuity note on why the interop entry must also go, given the companion cell keeps marimo "present") plus `claims-files: []` (SC8 finding #3 precedent); RED verified: `Error: Engine 'jupyter' is registered but its runtime is not available...`.) |
| SC14 | 4cD | int-rs | `resolve` | `{python .marimo}`+bare `{sql}` doc → `ownership["sql"]` | none | `sql` interop claim → `=="marimo"` RED (jupyter) (2026-07-03, task 4cD-e2e: the "both execute via marimo" e2e half is GREEN on the STATIC path — `marimo_engine_e2e.rs::sc14_e2e_static_sql_interop_both_execute_via_marimo` (SC9 already covered the dynamic path). The SC4-style `claimsLanguage` revert is vacuous here (static resolution never calls it); the correct revert-hunk substitution is the `execute()` LEAVE-ALONE gate (`bareSqlOwned`) flipped back to its pre-finding-#4 inverted sense → sql cell renders unexecuted (`class="{sql}"` literal marker), RED verified verbatim in the test's doc comment.) |
| SC15 | 4cD | int-rs | `resolve` | bare `{sql}`-only doc → `ownership["sql"]` | none | interop→primary mistake → `!="marimo"` RED (**presence-gating**: this reddens if interop is wrongly primary) |
| SC16 | 4cD | int-rs + e2e | `resolve` + `handled_languages_for`; render | `{python .marimo}`+`{r}` (or env engine) → both engines' `handled_languages`; render leaves non-owned cell alone | 2nd engine (env skip) | §5 enforcement / B2 gate → marimo `handled` excludes the other lang RED (2026-07-03, task 4cD-intrs: int-rs half GREEN — corrected per finding #4, `crates/quarto-core/src/engine/jupyter/text_execute.rs:600-655`: `handled_languages_for` returns the LEAVE-ALONE set, not the owned set, so the non-vacuous assertion is marimo's handled set CONTAINS "r" and knitr's handled set CONTAINS "python", the inverse of this row's original "excludes" phrasing; RED verified by flipping `handled_languages_for`'s `!=` to `==` — see `marimo_resolution.rs::sc16_coexistence_handled_languages_leave_alone_semantics`. e2e half remains for the e2e stream.) (2026-07-03, task 4cD-e2e: the e2e half is GREEN — `marimo_engine_e2e.rs::sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone`, gated on deno+uv+Rscript+knitr, all present, no skip. **Dated finding:** rendering through the COMMITTED static fixture does NOT actually exercise coexistence — the same whole-file `claims_file` short-circuit as SC8's finding #3 makes marimo claim the entire render and knitr never runs (confirmed firsthand: the `{r}` cell renders as raw unexecuted source through the unmodified fixture); used the already-ratified dynamic-claims-less derivation (`setup_marimo_project_dynamic`) instead. RED-by-revert: the row's two suggested single edits (widen the regex; neuter the sql-only check for "r") were both tried and found VACUOUS — no spare `marimoExecution.output` exists to inject wrong content for an over-claimed cell with no real output, so the fallback always reconstructs the original text byte-identical. The actual observable revert — neutering `cellOwnedByMarimo` UNCONDITIONALLY (`return true;`) — scrambles marimo's own output-index accounting, causing its OWN real python cell (not knitr's r cell) to fall through unexecuted (`class="{python}"` literal marker); full trail in the test's doc comment. The preceding annotation's closing sentence is retained above per append-only convention; it is superseded by this one.) |
| SC17 | 4cD | int-rs | `resolve` + `walk_block_for_langs` | mixed `{python .marimo}` then `{python}` → `ownership["python"]` | none | first-wins (`resolution.rs:132 !seen_set.contains`) → last-wins flips `marimo`→`jupyter` RED |
| SC18 | 4cD | e2e | `execute()` catch (marimo-engine.ts:319-329) | render a syntactically-bad marimo cell → error-marked output substring | marimo/uv (env skip) | try/catch → "Error executing marimo" output RED (render throws) (2026-07-03, task 4cD-e2e: GREEN — `marimo_engine_e2e.rs::sc18_e2e_execute_catch_shows_error_marker_not_crash`. **Dated finding:** a Python `SyntaxError` cell body (e.g. `def (:`) does NOT reach this catch — marimo's own per-cell parse-error isolation (`extract.py`'s `_ParseError` sentinel) swallows it first, rendering `<pre class="marimo-error">...</pre>` with exit 0. Used an unresolvable `pyproject` dependency name instead, which fails at the `uv run` subprocess level and genuinely reaches `execute()`'s outer catch. RED verified: neutering the catch (rethrow) makes the render fail outright, captured verbatim in the test's doc comment.) |
| SC19 | 4cE | unit-ts | **new** factored `buildCommand(metadata)` (extract the `useExternalEnv` branch out of `execute()`, `marimo-engine.ts:222,234`) | `buildCommand({external-env:true})` vs `buildCommand({})` → returned `[cmd, …args]` | none (command shape is pure; `uv`-flag call stubbed) | Revert the `if (useExternalEnv)` branch → `buildCommand({external-env:true})[0]==="python"` RED (yields `uv`). (Command selection isn't observable in rendered HTML — must be unit-tested, not e2e.) |
| SC20 | 4c0-eng | unit-ts (deno) | `claimsFile`/`containsMarimoFence` — **doc-level** B1 negative | write a bare-`{sql}`-only temp file → `claimsFile(path,".qmd")` → `false` | filesystem (temp) | Drop the `(?=.*\.marimo)` lookahead from the shared `MARIMO_CELL_REGEX` → `claimsFile(bareSqlOnly)===false` RED (whole file wrongly self-claimed). Distinct from SC7 (pure `containsMarimoFence` unit) and SC15 (resolve-level) — this is the file-routing proof. |

**Vacuity notes (the traps §2 catches):**
- **SC1** — the discriminator is *Vec order-independence*. Interop MUST be listed **first** so a naive "first non-`None`" reducer returns `Interop(0)` while the correct strongest-kind reducer returns `Primary(2)`; asserting the `Some("marimo")` case alone with Primary-first would pass vacuously. Also assert `lookup(python,None)==None` (all-claims-mismatch → `None`).
- **SC11** — two docs, two revert hunks. Doc A (marimo) proves the claim exists; **doc B (jupyter) is the real discriminator that `whenClass` gates** — without it, an unconditional python-Primary passes A and the test is theater. Revert the `whenClass` guard → B reddens.
- **SC13 + SC14 + SC15 bind presence-gating as a set.** SC13 = tagged sql self-activates (primary); SC14 = bare sql rides along *when present*; SC15 = bare sql alone does *not* activate. If the interop claim were wrongly a Primary, **SC15 reddens** (sql-alone would wrongly activate marimo) — that is the assertion proving interop ≠ primary. Keep all three.
- **SC5 exercises both gate states** ("path was actually exercised"): a predicate that always returns `true` passes the `["sql"]` case; only the `[]`-→`false` assertion proves the gate fires.
- **SC7 is the B1 guard**: it fails loudly if an implementer widens the *shared* regex instead of using a separate execution-split matcher.

**Missing-test pass (§3 — accepted-untested logged, not silently omitted):**
- **Vec back-compat for existing extensions**: the julia-engine fixture's single-claim resolution must still pass after 4c0. → *Spec:* re-run the existing julia resolution test post-widening (regression); it's compile-plus-behavior enforced. Not a new test file, but named as a required-green in 4cF.
- **`canFreeze:false` (4cE)**: the observable ("no freeze cache read") requires q2 freeze to be wired to `canFreeze`. **Accepted-untested** unless freeze consults `canFreeze` today — verify at 4cE; if wired, add: revert `canFreeze:false→true` → "engine re-executes under freeze config" RED; if not wired, log as accepted-untested (no production hunk to bind).
- **Over-claim hard-error mechanism (`ts_engine.rs:286`)**: we *rely* on it but our fixture is designed so static==dynamic (never fires). A deliberately-mismatched-claim test would bind it — **accepted-untested here** (pre-existing mechanism, not our change); confirm existing `ts_engine.rs` coverage rather than duplicate.
- **`extract.py` actually *executes* bare sql** (vs. just rewrites the fence): SC6 is pure-regex (CI-safe); real execution → `[{"x":1}]` output is env-gated. → *Spec:* an env-gated `test_extract.py` integration case (skip without duckdb/sqlglot/polars), separate from SC6.

## Work Items

### Phase 4c0: Vec-per-language static claims (TDD — q2 code change)
- [x] **Failing tests first.**
  - `types.rs`: `lookup_static_claim` on a `sql` Vec
    `[{whenClass:marimo,primary,2},{interop}]` → `Primary(2)` for
    `first_class=Some("marimo")`, `Interop(0)` for `None`. Migrate the existing
    single-claim lookup tests (`types.rs:372-405`) to the Vec form.
  - `read.rs`: `parse_claims_map` parses a YAML **sequence** value into a
    `Vec<StaticLanguageClaim>`; scalar bool/int / single object still parse to a
    1-element Vec (back-compat).
- [x] `EngineContribution::External.claims`:
      `Option<HashMap<String, StaticLanguageClaim>>` →
      `Option<HashMap<String, Vec<StaticLanguageClaim>>>` (`types.rs:108`).
- [x] `lookup_static_claim` (`types.rs:164`): implement the combine rule,
      including a **new explicit `ClaimKind` comparator** (Primary > Interop >
      Fallback, priority tiebreak) — not a reuse of `resolution.rs:321` (D1). Keep
      `static_claim_to_language_claim` as the per-claim converter.
- [x] `parse_claims_map` / `parse_static_language_claim` (`read.rs:439/460`):
      accept a sequence; wrap scalar/object as a 1-element Vec.
- [x] **The two `fallback`-combine sites** (implementer review, blocker 2):
      `ts_engine.rs:257-260` (validation) and `604-607` (`claims_language`) call
      `map.get("fallback")` then `static_claim_to_language_claim(fb,…)`. With a
      Vec value these no longer compile — apply the combine rule to the fallback
      Vec at **both** sites. The `lookup_static_claim` callers at `256`/`603`
      themselves are otherwise logic-unaffected (signature unchanged).
- [x] `ts_engine.rs` map type at `141`/`170` + test helpers `935`/`961-975`:
      **keep `single_claim`** (return a 1-element-Vec map — ~6 existing tests use
      it: `test_p1_12`, `test_hint_prefilter_no_load`, …) **and add `multi_claim`**
      for the Vec cases.
- [x] `cargo nextest run -p quarto-core` green; document the Vec form + combine
      rule in `engine-resolution.md` §3.3.

### Phase 4c0-eng: marimo engine — bare-sql interop feature (TDD in `~/src/quarto-marimo`)
Make the engine's live behavior match the declared interop. Three small,
mechanism-complete changes (spike-confirmed, correction 12): TS claim, TS
execution gate, Python `extract.py` regex. Do this **upstream** in
`~/src/quarto-marimo` (deno tests: `tests/claims-language.test.ts`,
`tests/is-marimo-cell.test.ts`, `tests/cell-execution-regex.test.ts`; python
tests: `tests/python/test_extract.py`), then re-bundle into the fixture (4cA).
Upstreamable. No marimo-internal work.

- [x] **TS `claimsLanguage`**: return `{kind:"interop"}` for bare `sql`
      (`language === "sql"` without `firstClass === "marimo"`). Widen the return
      type `boolean | number` → `boolean | number | LanguageClaim`. Keep `2` for
      tagged, `1` for dotted, `false` for bare python. Update
      `claims-language.test.ts`. (Required for static==dynamic parity, correction 9.)
- [x] **TS execution gate (net-new — B1/B2).** `execute()` currently reads only
      `{ target, format }` (`marimo-engine.ts:214`); it must now read
      `options.handledLanguages` and process bare `{sql}` cells **only when it
      includes `"sql"`** — otherwise leave them for the owning engine.
      **B1 — do NOT widen the shared `MARIMO_CELL_REGEX`.** That constant also
      feeds `containsMarimoFence` → `claimsFile` (`marimo-engine.ts:40-44,152`),
      which runs at file-routing time *upstream* of any `handledLanguages` gate;
      widening it would make a bare-`{sql}`-only doc self-claim marimo, breaking
      the 4cD negative test + correction 3. Keep `containsMarimoFence`/`claimsFile`
      `.marimo`-requiring, and use a **separate widened matcher** only for the
      execution split + a **new** gated predicate `cellOwnedByMarimo(cell,
      handledLanguages)` (do **not** overload `isMarimoCell(cell)` — its call site
      at `marimo-engine.ts:271` passes no ownership). The bare-`{sql}`-only-not-
      claimed regression is **SC20**.
      **B2 — thread ownership to `extract.py` via argv.** `extract.py` gets raw
      stdin with no ownership signal, so push a **new positional** after the
      existing three at `marimo-engine.ts:247` (`input`, `mime`, `eval`) — a
      `bare_sql: yes|no` derived from `handledLanguages.includes("sql")` (argv
      index 4). **Widen `extract.py`'s arity assert** (`assert len(sys.argv) in
      (…)` near `extract.py:459`) to accept the extra positional and parse
      argv[4], or the render `AssertionError`s at runtime *past* SC6's pure-regex
      test. `extract.py` applies `BARE_SQL_FENCE_REGEX` only when `yes`. Update
      `is-marimo-cell.test.ts` / `cell-execution-regex.test.ts`.
      **(Mechanism corrected 2026-07-02, task 4cB2-fix, FINDING #4:**
      `handledLanguages` is q2's leave-alone set, not a positive "assigned to
      me" set (`resolution.rs:292`) — `bareSqlOwned` must read
      `!handledLanguages.includes("sql")`, not
      `handledLanguages.includes("sql")` as originally written above. Fixed
      upstream in `~/src/quarto-marimo` (`marimo-engine.ts`,
      `lib/is-marimo-cell.ts`), rebundled into the q2 fixture, and documented
      on the `handled_languages` field in
      `crates/quarto-core/src/engine/ts_protocol.rs`.)**
- [x] **Python `extract.py`**: add a `BARE_SQL_FENCE_REGEX` sibling to the
      existing `SQL_DOT_FENCE_REGEX`, rewriting a bare `{sql …}` fence (no
      `.marimo`) into marimo's qmd-form `sql {.marimo …}`, applied in
      `convert_from_md_to_pandoc_export` before the existing SQL rewrite. ~3 lines
      atop the already-complete SQL bridge (`sql_code_to_python`). Use lookaheads
      `(?![\w.])` and `(?![^}]*\.marimo)` so `{sql.marimo}`, `{sqlfoo}`,
      `{python}`, and already-qmd-form `sql {.marimo}` are left untouched (exact
      spike-proven regex in the 2026-07-02 spike report). **Apply it only when the
      `bare_sql` argv flag is `yes`** (B2 gate above) so a doc where marimo does
      not own sql doesn't get its bare-sql cells executed. Add a `test_extract.py`
      case for both flag states. `{sql .marimo}`/`{sql.marimo}` already execute
      today — no change for those.
- [x] **Runtime deps** (done 2026-07-03 via 4cB2/4cD-e2e: `SQL_INTEROP_DOC` in `marimo_engine_e2e.rs:248` declares the pyproject `dependencies = ["duckdb","sqlglot","polars","pyarrow"]` block; resolved duckdb 1.5.4/sqlglot 30.12.0/polars 1.42.1/pyarrow 24.0.0; env-gated skips per julia precedent): bare-sql execution needs `duckdb`+`sqlglot`+(`polars`+
      `pyarrow` or `pandas`) in the eval env. The 4cD sql fixture qmd must declare
      them in its front-matter `pyproject` block (marimo inline-script metadata:
      `dependencies = ["duckdb","sqlglot","polars","pyarrow"]`), which
      `command.py`'s `construct_uv_flags` turns into `uv run --with …` flags.
      Tests skip when the environment can't resolve them (like the `julia`-absent
      skips).
- [x] Re-bundle and confirm test suites green (done cumulatively: rebundles at 4cA/8f9→fcfd26a50, finding-#4 fix b4f4f52bf, SC19 b3b432344; deno 83→87/87, pytest 47/47 at upstream 2a2f312; version pin-check: uv resolved marimo 0.23.13 vs spike's 0.23.1, recorded compat doc §9) — deno: `deno test --no-check
      --allow-read --allow-write --allow-env` (the repo's `deno task test`);
      python: `pytest tests/python/` inside the marimo eval env. Confirm behavior
      on the marimo version the engine pins (`uv.lock` resolves `>=0.23.1`; the
      spike used 0.23.1 — a floating `>=` could drift, so pin-check).

### Phase 4cA: Set up the marimo engine fixture
- [x] Fixture dir: `crates/quarto-core/tests/fixtures/extensions/marimo/`
      (mirror the existing `.../extensions/julia-engine/` and `echo-engine/`
      layout). The e2e driver that copies a fixture to a tempdir + strips its
      pre-built `dist/*.js` lives at
      `crates/quarto/tests/integration/build_ts_extension_e2e.rs`.
- [x] Reuse the Julia plan's (`claude-notes/plans/2026-04-16-julia-validation.md`)
      `resources/extension-build/deno.json` scaffolding + fixture-copy pattern.
      (No new import-map aliases needed — marimo only imports `path`, already
      added by the julia plan's parity fix.)
- [x] Copy (co-located so `dirname(fromFileUrl(import.meta.url))` resolves):
      the **modified** `src/marimo-engine.ts` + `lib/*.ts` (bundled, from
      `<marimo-repo>/lib/`, **not** `src/lib/`), and runtime files `extract.py`
      **and `command.py`** (uv path). **Omit `marimo-deprecated.lua`.**
- [x] **Do NOT copy marimo's root `deno.json`** (it maps `@quarto/types` to a
      mock and `path` to deno.land, and would win config-precedence tier 2).
      Build with `--workspace`/shipped config so `@quarto/types` resolves real.
- [x] Remote import `delay` (`https://deno.land/std@0.224.0/async/delay.ts`) is a
      fully-qualified URL — `deno bundle` fetches it (network / warm `DENO_DIR`).
      Optional: swap to a bare `@std/async` specifier for offline parity.
      (Left as-is per brief; fetched and inlined cleanly, confirmed no live
      import remains in the bundle.)
- [x] Write `_extension.yml` with the **Option B** `claims:` map + `name: marimo`;
      point engine `path:` at the locally rebundled output (not the shim).
- [x] Rebundle: `q2 build-ts-extension _extensions/marimo` (the literal
      `src/marimo-engine.ts` invocation from the plan doesn't work as written —
      needed the same build-time symlink workaround julia's Phase 4A hit; see
      the compat log §5).
- [x] Log adaptations (Q1→q2): claimsLanguage interop widening, bare-sql
      execution gate, mock-`@quarto/types` remap, `deno.json` drop, inert
      `partitionedMarkdown`/`postprocess`, loader-shim replacement. See
      `claude-notes/research/2026-07-02-marimo-engine-q2-compat.md`.

### Phase 4cB: Minimal marimo render (static path)
- [x] Doc with one `{python .marimo}` cell (`import marimo as mo; 1 + 1`).
      Committed as `crates/quarto-core/tests/fixtures/extensions/marimo/minimal.qmd`
      + sibling `_quarto.yml` (`project: type: default`), mirroring the julia
      fixture's root layout. No `pyproject`/dependency frontmatter needed — the
      engine's `uv run --with marimo` path supplies marimo itself, and
      `command.py`'s `PyProjectReader.from_script` accepts an empty header.
- [x] `cargo run --bin q2 -- render <file>.qmd`; debug first failure (Deno start,
      bundle, `uv`/`marimo`/`command.py`, protocol). Render now SUCCEEDS (exit
      0, HTML produced) after the pampa writer fix (`411380777`) — but the
      brief's/SC8's specific marker requirement is still not met; see the
      SECOND BLOCKING FINDING below.
- [x] **Success criterion:** rendered HTML contains the evaluated result (`2`).
      (No `intermediateFiles` concern: the host optional-chains it —
      `host.ts:554` `?.() ?? null` — so a missing method is already safe.)
      **CONFIRMED (attempt 3, post-FIX #2, firsthand re-render):** `2` present
      (`<marimo-cell-output><pre class='text-xs'>2</pre></marimo-cell-output>`)
      **AND** the named marker present (`__MARIMO_EXPORT_CONTEXT__` script +
      `<marimo-code hidden>` in `<head>`, temp-path leak gone). Both halves of
      the success criterion are now met.
- [x] **De-risk note:** this python-primary path needs neither 4c0 nor 4c0-eng
      (a python-only claim is a single claim matching the unmodified engine), so
      it proves the build→bundle→subprocess→protocol pipeline independently of the
      sql feature. It does **not** exercise bare-sql. Confirmed: 4c0/4c0-eng
      code paths were not exercised by this render (sql untouched).
- [x] FIX: pampa QMD writer round-trips bracket-wrapped language pseudo-class
      (`{python .marimo}`)
- [x] FIX #2: TS-engine wire includes are file paths — read into content at
      translate_includes
- [x] SC8 written (`crates/quarto-core/tests/integration/marimo_engine_e2e.rs`,
      registered alphabetized in `main.rs`), GREEN, and RED-by-revert proven
      using the corrected two-part revert (controller-approved 2026-07-02 —
      see BLOCKING FINDING #3 and the Test Seam Spec table's in-row
      annotation on SC8). RED evidence captured verbatim in the test file's
      doc comment; fixture restored byte-identical (`git diff` clean);
      re-confirmed GREEN against the pristine fixture. **4cB fully DONE.**

**BLOCKING FINDING #3 (2026-07-02, 4cB attempt 3) — RESOLVED (controller
sign-off received) — SC8's specified named revert ("remove the `python`
claim from `_extension.yml`") is VACUOUS; a DIFFERENT mechanism
(`EngineClaimsFileStage`'s whole-file dynamic `claims_file`) already assigns
marimo ownership independent of the per-language `claims:` map.**
Empirically proven, not a guess — see the task report's "Final completion"
section for the full trace. The corrected two-part revert (`claims-files:
[]` + remove the `python:` claim) is now SC8's official named revert (see
the Test Seam Spec table's in-row annotation); RED evidence captured
verbatim in `marimo_engine_e2e.rs`'s doc comment; fixture restored
byte-identical.

- Applying exactly the brief's specified revert (deleting the `python:` key
  from `_extension.yml`'s `claims:` map, everything else unchanged) does
  **NOT** redden `sc8_minimal_marimo_render_shows_marimo_signature_and_result`
  — it stays GREEN. Confirmed by direct `RUST_LOG=debug` render trace: marimo's
  subprocess still runs ("Executing marimo cells...").
- Root cause: `crates/quarto-core/src/stage/stages/engine_claims_file.rs`'s
  `EngineClaimsFileStage` runs BEFORE any per-language tier resolution and
  asks every registered engine `claims_file(file, ext)` — "first claimer
  wins" — for the **whole file**, regardless of extension (`.qmd` included).
  A claiming engine's answer is recorded as `ctx.claimed_engine_name`, which
  `engine_execution.rs:225` (comment, verbatim) "**short-circuits ALL tier
  evaluation and returns exactly that engine**" — i.e. it behaves exactly
  like an explicit `engine: marimo` declaration, bypassing the per-language
  `claims:` map (T1-T4 in `resolution.rs`) entirely.
- The marimo fixture's `_extension.yml` declares no `claims-files:` key, so
  `TsEngine.claims_files` is `None` → `ts_engine.rs:716-717`'s
  content-inspecting DYNAMIC path fires: it loads the (unmodified,
  excluded-from-editing) engine module and calls its live `claimsFile`,
  which does its own independent regex scan
  (`containsMarimoFence`/`MARIMO_CELL_REGEX` in `marimo-engine.ts`) of the
  raw file text for ANY `.marimo`-tagged fence — found unconditionally in
  `minimal.qmd`, **regardless of what the `claims:` YAML map says**.
- **Validated corrected revert** (temporary, tested then reverted — NOT
  applied to the committed fixture): add `claims-files: []` alongside
  removing the `python:` claim. `claims-files: []` makes
  `self.claims_files = Some(vec![])` → `claims_file` answers via the
  now-authoritative-but-empty static list (`ts_engine.rs:717`), `false`,
  zero-load, no whole-file short-circuit — so per-language resolution alone
  decides, correctly falls through to jupyter (unavailable) → render fails →
  **genuine RED**, confirmed:
  `Error: Engine 'jupyter' is registered but its runtime is not available.`
- I did NOT apply this corrected revert to the committed test/fixture
  without approval — it changes what the frozen spec's named revert
  literally says (adds a key, not just removes one), which is exactly the
  "if you believe the row is wrong, STOP and report" case. SC8's GREEN proof
  stands (real, unmocked, full-chain render); its RED-by-revert proof is
  pending this sign-off.

**BLOCKING FINDING #2 (2026-07-02, 4cB attempt 2, resume after the pampa fix) —
engine-contributed `include-in-header` is treated as literal content, not a
file path; marimo-engine.ts sends a temp-file PATH.** Not something this
task's brief authorizes fixing (either side of the fix is out of scope: the
q2-core drain in `crates/quarto-core/src/stage/stages/include_resolve.rs`, or
`marimo-engine.ts` itself, excluded fixture-engine source) — reported for
controller triage, full evidence in `.superpowers/sdd/task-4cB-report.md`.

- The rendered HTML's `<head>` contains, verbatim as text, the temp file's
  PATH string (e.g.
  `/var/folders/.../quarto-pipeline_xt0wdy/marimo-header-d92e0802c8b987d3.html`)
  instead of that file's contents.
- Confirmed the temp file itself DOES contain the expected markers
  (`__MARIMO_EXPORT_CONTEXT__` script + `<marimo-code hidden>…</marimo-code>`)
  — read directly off disk before pipeline cleanup. So `extract.py`/
  `marimo-engine.ts`'s header-construction is correct; the break is entirely
  in how q2 consumes `ExecuteResult.includes["include-in-header"]`.
- Root cause: `crates/quarto-core/src/engine/ts_engine.rs::translate_includes`
  (~line 440) passes each wire string straight through into
  `PandocIncludes.header_includes` with no file read. This is CORRECT per the
  current architecture: `IncludeResolveStage`'s module doc
  (`crates/quarto-core/src/stage/stages/include_resolve.rs:1-56`) explicitly
  distinguishes AUTHORED YAML-key includes (bare path / `{file:}` / `{text:}`
  — these DO get `SystemRuntime::file_read`) from ENGINE-CONTRIBUTED
  `PandocIncludes` (folded verbatim as literal text by
  `append_pandoc_includes`, `include_resolve.rs:262-278`, called from both
  `IncludeResolveStage` and `ApplyTemplateStage`'s late drain — neither path
  reads files for this channel).
- But `marimo-engine.ts`'s `execute()` (line ~340-346) writes header content
  to a temp file and sends **the file's path** as the wire value — "(like
  Jupyter does)", per its own comment — i.e. it assumes Q1/Pandoc-style
  file-path semantics (matching knitr's native-Rust `convert_includes`,
  `crates/quarto-core/src/engine/knitr/mod.rs:306-336`, which DOES
  `std::fs::read_to_string` before populating the same `PandocIncludes`
  struct). No other TS-engine fixture (julia, echo) has ever exercised
  `includes["include-in-header"]` before this task, so this mismatch was
  previously unexercised/latent — consistent with the plan's own correction
  #6 flagging "4cC validates the real sink" as the phase that was expected to
  first prove this out.
- Two possible fixes, either out of my scope: (a) q2-core — make the
  engine-contributed drain also read file-path values (mirroring knitr's
  precedent), restoring TS/knitr parity; or (b) engine-source — change
  `marimo-engine.ts` to send literal header content instead of a temp-file
  path (an "upstream 4c0-eng defect", per the brief's own language for this
  category of finding). Controller call.
- Note: `<marimo-island>`/`<marimo-cell-output>`/`<marimo-cell-code hidden>`
  DO appear in the rendered body (from marimo's own `stub.render()`, not
  `extract.py` 293-327) and are unambiguously marimo-specific — but SC8's
  frozen spec text names specific markers sourced from extract.py 293-327
  only, so I did not substitute these unilaterally.

**BLOCKING FINDING #1 (2026-07-02, 4cB attempt 1) — RESOLVED — pampa
QMD-writer round-trip bug
on the space-separated `{lang .firstclass}` fence syntax.** Not a marimo-engine
defect (marimo-engine.ts/lib/extract.py/command.py all behave correctly given
what they're handed) and not something this task's brief authorized fixing
(pampa's writer is core q2 infrastructure, well outside 4cB's declared scope) —
reported here for controller triage, full evidence in
`.superpowers/sdd/task-4cB-report.md`.

- Root cause: pampa's parser deliberately encodes the `{python .marimo}` fence's
  first class as the literal string `"{python}"` (braces included) — see
  `crates/quarto-core/src/engine/capture_splice.rs:74` (`engine_cell_lang`),
  which strips the `{…}` wrapper back off when resolving the cell's language.
  But `crates/pampa/src/writers/qmd.rs`'s `write_attr` (line 431:
  `write!(writer, ".{}", class)?;`) doesn't know about this special encoding —
  for a 2+-class `CodeBlock` (`write_codeblock`, lines 664-672, takes the
  `write_attr` branch whenever more than one class is present, which is always
  true for `{lang .firstclass}` cells) it blindly prefixes every class with
  `.`, turning classes `["{python}", "marimo"]` into the malformed fence
  `` ```{.{python} .marimo} ``.
- Confirmed via `cargo run --bin pampa -- -f markdown -t qmd`: round-tripping
  `{python .marimo}` produces the malformed fence above; round-tripping the
  dotted form `{python.marimo}` (single class `"{python.marimo}"`, hits
  `write_codeblock`'s single-class bare-word branch) round-trips correctly.
- Impact on the render: `serialize_ast_to_qmd`
  (`crates/quarto-core/src/stage/stages/engine_execution.rs:569`) is what feeds
  `target.markdown` to every TS engine — so the malformed fence is what marimo's
  `execute()` actually receives. marimo's own Python-side `MarimoMdParser` is
  lenient enough to still find and execute the cell (uv resolved marimo
  **0.23.13**, python 3.13.7; `extract.py` exited 0, reporting `count: 1`), but
  the TS-side `breakQuartoMd(target.markdown, …, MARIMO_CELL_REGEX)` split does
  **not** recognize the malformed fence as a marimo-owned cell boundary (0
  matches), so `execute()` splices nothing back in and passes the malformed
  fence straight through in `processedMarkdown`. That text then fails
  downstream tree-sitter re-parse with the generic pampa fallback
  (`quarto-parse-errors`'s `"Parse error" / "unexpected character or token
  here"`), which is the error `q2 render` surfaces.
- This blocks **any** TS engine consuming a `{lang .firstclass}`-syntax cell via
  the standard `serialize_ast_to_qmd` path — not marimo-specific, and not
  something 4cB should route around by silently switching the fixture to the
  dotted `{python.marimo}` form (that would exercise SC12's claim key, not
  SC8's `whenClass: marimo` claim the brief specifies).
- **Next step (controller-level):** fix `write_attr`/`write_codeblock` in
  `crates/pampa/src/writers/qmd.rs` to detect a bracket-wrapped first class
  (`class.starts_with('{') && class.ends_with('}')`) and emit it as a bare
  unprefixed language token (mirroring the reader's encoding and
  `engine_cell_lang`'s unwrap), then re-attempt 4cB.

### Phase 4cB2: Dynamic-claims fixture
- [x] Second fixture: identical engine, **`claims:` removed** from `_extension.yml`
      (keep `name`), forcing q2's dynamic path (`ts_engine.rs:625` else-branch →
      `ensure_loaded` + `ClaimsLanguage` wire call). **Implementation choice
      (2026-07-02, task 4cB2):** no second committed bundle — derived at
      test-setup time by rewriting the tempdir copy's `_extension.yml`
      (`write_claims_less_extension_yml` in `marimo_engine_e2e.rs`, truncates
      at the `claims:` key, keeps `name`/`title`/`author`/`version`/
      `quarto-required`). Zero drift risk against the static fixture (same
      `marimo-engine.js`/`command.py`/`extract.py`); committed static fixture
      untouched (`git diff` clean).
- [x] Render the 4cB doc (python-only `minimal.qmd`, no sql); **assert same
      ownership as the static path.** DONE and GREEN:
      `p4cb2_dynamic_path_parity_minimal_render_matches_static` in
      `marimo_engine_e2e.rs` — same markers/result as SC8, via the dynamic
      `ClaimsLanguage` wire path. This is the *first end-to-end*
      load→ask→resolve with a real engine (not re-testing number
      normalization — `host.test.ts:430` owns that).
- [x] **Bare-sql interop parity (SC9) — CLOSED GREEN (2026-07-03, task
      4cB2-completion), after being BLOCKED, NEEDS_CONTEXT (2026-07-02,
      task 4cB2).** FINDING #4 (the defect this item's history describes
      below) is fixed and controller-ratified upstream in
      `~/src/quarto-marimo` (`77c15c8`) and rebundled into this fixture at
      q2 `b4f4f52bf`: `bareSqlOwned`/`cellOwnedByMarimo` flipped to
      `!handledLanguages.includes("sql")`. `write_claims_less_extension_yml`
      now always appends `claims-files: []` (the anti-vacuity correction
      this item's own step-2 evidence justified), so the dynamic-path
      variant never hits the whole-file short-circuit. Committed, GREEN
      test: `sc9_bare_sql_interop_dynamic_path_marimo_executes_sql_cell` in
      `marimo_engine_e2e.rs` — asserts the rendered HTML contains a marimo
      `<marimo-table>` island whose `data-data` carries the computed
      `{"x":2}` row (the bare `{sql}` cell's result), conjunctive with the
      python cell's markers. RED-by-revert (SC4's named revert, same
      claims-files:[]-fixed variant): reverting `claimsLanguage`'s bare-sql
      branch to `false` reproduces the exact pre-fix hard failure
      (`Error: Engine 'jupyter' is registered but its runtime is not
      available...`), captured verbatim in the test's doc comment; fixture
      restored byte-identical, re-confirmed GREEN.
      `cargo nextest run -p quarto-core -E 'test(marimo_engine_e2e)'`: 3/3
      green. Full trail: `.superpowers/sdd/task-4cB2-report.md` (appended
      section) and compat doc §13's closing update. Original NEEDS_CONTEXT
      history retained below for the record. Following the brief's Risk-1
      evidence-first procedure to
      completion surfaced a SECOND, independent, previously-undiscovered
      defect, not just the anticipated claims_file-short-circuit vacuity:
      even after the pre-authorized `claims-files: []` fix correctly makes
      the dynamic resolver assign `ownership["sql"]=="marimo"` (confirmed via
      temporary, reverted instrumentation), the rendered HTML still shows the
      sql cell as a plain unexecuted code block, not marimo's executed
      splice. Root cause: `marimo-engine.ts`'s `execute()`
      (`bareSqlOwned = handledLanguages.includes("sql")`) and
      `lib/is-marimo-cell.ts`'s `cellOwnedByMarimo` both assume
      `handledLanguages` is a *positive* "languages assigned to me" set: it
      is actually q2-core's *leave-alone* set (`EngineResolution::
      handled_languages_for`, "leave-alone set" per its own doc comment,
      confirmed by the passing `jupyter/text_execute.rs:600-655` unit test),
      which *excludes* languages the engine itself owns. So `bareSqlOwned`
      evaluates `false` exactly when marimo correctly owns bare sql — the
      execute()-time splice never fires, regardless of the resolver's
      (correct) answer. This is a 4c0-eng defect (its B2 design item embeds
      the same inverted assumption), not something fixable within this
      task's scope: `marimo-engine.ts`/`is-marimo-cell.ts` are excluded
      fixture/engine source (no pre-authorization covers flipping
      `bareSqlOwned`'s sense), and a sound fix needs a NEW positive
      "owned languages" wire field (a naive negation of the leave-alone set
      can't distinguish "I own it" from "nobody owns it" — exactly the
      ambiguity SC15's presence-gating negative case exists to catch), which
      is a quarto-core wire-protocol change outside this task's scope. Full
      evidence trail (both directions of the SC4-revert experiment, exact
      HTML snippets, confirmatory failure mode) in
      `marimo_engine_e2e.rs`'s doc comment ahead of the (uncommitted, not-
      passing) SC9 section, and in `.superpowers/sdd/task-4cB2-report.md`.
      **Controller decision needed** before SC9 can be completed as
      literally specified (see the Test Seam Spec row annotation below and
      the compat doc's dynamic-path findings entry, §13).

### Phase 4cC: Marimo widgets / figures
- [x] Doc with a `mo.ui` widget / plot. **Success criterion:** grep the rendered
      HTML for the header content routed via `includes["include-in-header"]` and
      the raw-`{=html}`/`![](…)` figure output from `render-output.ts`. Do **not**
      assert a `store_html_dependencies` path; note `generatesFigures` is inert.
      (2026-07-03, task 4cC: **DONE.** Committed fixture `widget.qmd` (marimo
      fixture root, alongside `minimal.qmd`) — one `{python .marimo}` cell
      calling `mo.ui.slider(1, 10, value=5)` (pure `mo.ui`, no matplotlib/
      altair). Manual e2e render (`target/release/q2 render widget.qmd` from
      a scratch copy) confirmed the success surface firsthand: `<head>`
      carries `__MARIMO_EXPORT_CONTEXT__` + `<marimo-code hidden>` (the
      `includes["include-in-header"]` sink), `<body>` carries a
      `<marimo-island><marimo-cell-output><marimo-ui-element>...
      <marimo-slider .../></marimo-ui-element></marimo-cell-output>
      <marimo-cell-code hidden>...</marimo-cell-code></marimo-island>` — the
      widget's raw `{=html}` island output, not a plain code block.
      SC10 committed as `sc10_widget_render_shows_header_include_and_body_island`
      in `marimo_engine_e2e.rs` (static claims path, deno+uv gated),
      conjunctive assertions (header marker AND body island markup) — GREEN.
      RED-by-revert: in a TEMPDIR-ONLY bundle copy, neutered the engine's
      `include-in-header` population (`if (outputFormat === "html" &&
      marimoExecution.header)` → `if (false && ...)`, corresponding to
      upstream `marimo-engine.ts` ~300-310) — re-render still SUCCEEDS
      (unlike SC8/SC9's revert) but `<head>` loses both header markers while
      `<body>` still shows the island, reproducing the exact
      conjunctive-assertion failure the test is written to catch (verbatim
      grep evidence in the test's trailing doc comment). Fixture restored
      byte-identical (`git diff` clean), re-confirmed GREEN.
      `generatesFigures`/`store_html_dependencies` correctly NOT asserted
      (plan correction 6, no q2 consumer). `cargo nextest run -p quarto-core
      -E 'test(marimo_engine_e2e)'`: 4/4 green (SC8, 4cB2 parity, SC9, SC10).
      One `cargo nextest run -p quarto-core`: 2634 passed, 33 skipped, 0
      failed (julia transients did not recur). See compat doc §14 and task
      report `.superpowers/sdd/task-4cC-report.md`.)

### Phase 4cD: first_class selection + sql-interop coexistence
- [x] **first_class selection (two separate docs, not one — correction 10):**
      doc A `{python .marimo}`-only → `ownership[python] == marimo`; doc B plain
      `{python}`-only → `ownership[python] == jupyter`. (2026-07-03, task
      4cD-intrs: `marimo_resolution.rs::sc11_first_class_selection_two_docs`,
      pure resolution — no render involved in this checklist item's wording.)
- [x] **Dotted syntax:** `{python.marimo}` / `{sql.marimo}` cells → marimo via
      their Primary(1) keys. (2026-07-03, task 4cD-intrs:
      `marimo_resolution.rs::sc12_dotted_syntax_primary_keys`.)
- [x] **sql-only-tagged self-activation (Q1 parity):** a doc with only
      `{sql .marimo}` / `{sql.marimo}` (no python) activates marimo and renders.
      (2026-07-03, task 4cD-intrs: the *activation* half is GREEN —
      `marimo_resolution.rs::sc13_tagged_sql_self_activation` proves
      `ownership["sql"] == "marimo"` via the real resolver. (2026-07-03, task
      4cD-e2e: the *renders* half is now GREEN too —
      `marimo_engine_e2e.rs::sc13_e2e_tagged_sql_self_activation_renders`,
      static claims path, both the space-form `{sql .marimo}` and dotted-form
      `{sql.marimo}` cells execute with distinct, distinguishable results.
      RED-by-revert (tempdir-only): drop the whole `sql:`+`"sql.marimo":`
      claim keys + `claims-files: []` → jupyter-unavailable hard failure,
      captured verbatim in the test's doc comment. A companion
      `{python .marimo}` import-only cell was required for genuine
      execution — see the test's doc comment for why this doesn't weaken the
      claim (the zero-python resolution proof already stands at the int-rs
      tier).))
- [x] **sql-interop when present (the new feature):** doc with `{python .marimo}`
      + bare `{sql}` → marimo owns **both** python (Primary) and sql (Interop);
      both execute via marimo. `ownership[sql] == marimo`. (2026-07-03, task
      4cD-intrs: the ownership half is GREEN —
      `marimo_resolution.rs::sc14_bare_sql_rides_along_when_present`.
      (2026-07-03, task 4cD-e2e: "both execute via marimo" is now GREEN on
      BOTH paths — SC9 already covered the dynamic-claims-less path;
      `marimo_engine_e2e.rs::sc14_e2e_static_sql_interop_both_execute_via_marimo`
      closes the STATIC path (committed fixture, unmodified). RED-by-revert:
      the SC4-style `claimsLanguage` revert is vacuous on the static path
      (zero-load, never calls it); the correct revert-hunk substitution is
      flipping the `execute()` LEAVE-ALONE gate
      (`bareSqlOwned = !(...).includes("sql")` back to the pre-finding-#4
      inverted form) → sql cell renders unexecuted
      (`class="{sql}"` literal marker), captured verbatim.))
- [x] **Negative:** bare `{sql}`-only (no marimo tag) → marimo not present →
      `ownership[sql] != marimo` (jupyter/knitr, or §10 loud-fail if no kernel).
      (2026-07-03, task 4cD-intrs:
      `marimo_resolution.rs::sc15_bare_sql_alone_does_not_activate_marimo`,
      pure resolution — presence-gating negative bound as a set with the two
      items above per the Test Seam Spec vacuity note.)
- [x] **Two-engine single-doc coexistence via distinct languages:**
      `{python .marimo}` + a second language the *available* engine genuinely
      executes → marimo owns python, the other engine owns the other language;
      each executes only its owned cells (`handled_languages`, §5). Pick the
      second language to match a working engine (e.g. `{r}`+knitr if R is present;
      confirm jupyter actually runs the chosen language before relying on `{bash}`
      — it may not be a stock jupyter kernel). Env-dependent; skip if unavailable.
      (2026-07-03, task 4cD-intrs: the resolution + `handled_languages_for`
      half is GREEN — SC16's int-rs half,
      `marimo_resolution.rs::sc16_coexistence_handled_languages_leave_alone_semantics`,
      using the real `{r}`+`KnitrEngine` per this item's own suggestion.
      (2026-07-03, task 4cD-e2e: the "each executes only its owned cells"
      runtime half is now GREEN —
      `marimo_engine_e2e.rs::sc16_e2e_marimo_knitr_coexistence_leaves_each_other_alone`,
      gated on deno+uv+Rscript+knitr (all present, no skip). **Dated finding:**
      rendering through the COMMITTED static fixture (as originally worded)
      does NOT exercise real coexistence — the SAME whole-file `claims_file`
      short-circuit as SC8's BLOCKING FINDING #3 makes marimo claim the
      ENTIRE render, and knitr never runs at all (confirmed firsthand: the
      `{r}` cell renders as raw unexecuted source). Used the already-ratified
      dynamic-claims-less fixture derivation (`setup_marimo_project_dynamic`,
      same machinery SC9/SC14 use) instead. RED-by-revert: the brief's two
      suggested single edits (widen the cell-split regex; neuter
      `cellOwnedByMarimo`'s sql-only check for "r") were BOTH tried and found
      VACUOUS (no spare `marimoExecution.output` exists to inject wrong
      content for an over-claimed cell with no real output, so the fallback
      always reconstructs the original text byte-identical); the actual
      observable revert — neutering `cellOwnedByMarimo` UNCONDITIONALLY
      (`return true;`) — scrambles marimo's own output-index accounting,
      causing its OWN real python cell to fall through unexecuted
      (`class="{python}"` literal marker) instead of knitr's r cell. Full
      trail in the test's doc comment.))
- [x] **Pin the v1 limitation (correction 10):** a doc mixing `{python .marimo}`
      and plain `{python}` resolves python to a **single** owner (whichever the
      first python cell's first_class selects). Add a test asserting the actual
      single-owner behavior so the limitation is documented, not a surprise.
      (2026-07-03, task 4cD-intrs:
      `marimo_resolution.rs::sc17_first_occurrence_wins_v1_single_owner`.)
- [x] Error handling: a syntactically-bad marimo cell yields a useful message
      (assert the error substring), not a crash. (2026-07-03, task 4cD-e2e:
      GREEN — `marimo_engine_e2e.rs::sc18_e2e_execute_catch_shows_error_marker_not_crash`.
      **Dated finding:** the originally-suggested trigger (a Python
      `SyntaxError` cell body, e.g. `def (:`) does NOT reach `execute()`'s
      outer catch — marimo's OWN per-cell parse-error isolation
      (`extract.py`'s `_ParseError` sentinel) swallows it first, rendering
      `<pre class="marimo-error">SyntaxError: ...</pre>` with a normal exit
      0. Used an unresolvable `pyproject` dependency name instead, which
      fails at the `uv run` SUBPROCESS level and genuinely reaches
      `execute()`'s outer try/catch, producing the "Error executing marimo"
      substring this row specifies. RED-by-revert: neuter the catch
      (rethrow) → render fails outright, captured verbatim in the test's doc
      comment.)

### Phase 4cE: Marimo-specific features
- [x] **SC19 fixture rebundle.** Recopied the changed upstream
      `src/marimo-engine.ts` (2026-07-03, task 4cE) from
      `~/src/quarto-marimo` @ `q2-bare-sql-interop` `2a2f312` (the `SC19`
      `buildCommand` factoring) into the fixture; `lib/*.ts`, `command.py`,
      `extract.py` diff-verified byte-identical (unchanged). Rebundled via
      the symlink workaround (compat doc §5); symlink removed before/after,
      confirmed absent (`find … -type l` → no output). Bundle sanity:
      `buildCommand` present (3 hits: definition, call site, export list),
      22033 bytes. Regression gate:
      `cargo nextest run -p quarto-core -E 'test(marimo_engine_e2e) or
      test(marimo_resolution)'` → **15/15 green** against the new bundle (one
      pre-existing benign LEAK annotation, PASS, same as prior tasks). One
      full `cargo nextest run -p quarto-core`: **2645 passed, 0 failed, 33
      skipped** (no julia transient this run; no isolation needed).
- [x] `external-env` vs `uv` env modes (`target.metadata["external-env"]`,
      `pyproject`) — the two subprocess-construction branches (`command.py` in the
      uv branch only). **Disposition: manual proof, not a committed test**
      (2026-07-03, task 4cE) — command-selection shape is unit-bound
      upstream (SC19); attempted the scratch-venv e2e proof per the brief.
      Built `uv venv <scratch>/venv` + `uv pip install --python
      <scratch>/venv/bin/python marimo` (0.23.13 resolved), rendered a doc
      with front-matter `external-env: true` + `{python .marimo}` computing
      `21 + 21` with `<scratch>/venv/bin` prepended to `PATH`. Render
      succeeded (exit 0); rendered HTML contains `42` +
      `__MARIMO_EXPORT_CONTEXT__`/`marimo-cell-output` markers. **Rigor
      check:** re-ran with `uv` entirely absent from `PATH` (confirmed via
      `which uv` → not found) — render still succeeded with the same `42` +
      markers, proving the code path used the venv's own `python` directly
      and never touched `uv`. Not committed as an automated test: unlike
      the existing skip-gates (`deno_available`/`uv_available`, cheap
      version-check probes), a faithful gate here would need to actually
      construct a venv + install marimo over the network as test setup —
      a materially heavier and network-dependent fixture step than any
      existing row, and the kind of setup-time flakiness risk the brief
      calls out. Full commands + output in
      `.superpowers/sdd/task-4cE-report.md` and compat doc §16.
- [x] `checkInstallation` is a **no-op stub** (`delay(2000)` + spinner; checks
      nothing) — assert only that it runs without error. **Disposition:
      inert in q2** (2026-07-03, task 4cE) — grepped the full `ToEngine`
      wire-message enum (`crates/quarto-core/src/engine/ts_protocol.rs:33-101`)
      and every call site in `ts_engine.rs`/`ts_process.rs`/`ts_protocol.rs`:
      there is no `checkInstallation` wire variant and zero references to
      the method name anywhere in q2's engine/protocol code (only the type
      declaration in `ts-packages/quarto-types/src/execution-engine.ts:209`
      and the two fixture engines — julia, marimo — that implement it). q2
      never invokes it; no call site to cite as "runs during every launch."
      Ticking with this finding (correction-6 style), not a new test — same
      disposition as the plan's own instruction for the "never invoked"
      branch.
- [x] `canFreeze: false` respected — **success criterion:** with freeze enabled
      in config, marimo re-executes (does not read a freeze cache).
      **Disposition: ACCEPTED-UNTESTED** (controller-ratified, pre-decided
      per task brief — ticked here without further investigation, per
      instruction). q2 has no freeze mechanism: `canFreeze` flows
      wire → store → `TsEngine::can_freeze()`
      (`crates/quarto-core/src/engine/ts_engine.rs:614`) and dead-ends at a
      `Debug` impl (`crates/quarto-core/src/engine/registry.rs:316`);
      confirmed `RenderOptions.use_freeze` is constructed `false` at every
      call site (`crates/quarto-core/src/render.rs:643`,
      `crates/quarto-core/src/project/pass2_renderer.rs:809,1066`) — no
      freeze-consulting code path exists to bind a test to. Strand
      `bd-mx5x609r` holds the freeze-epic-time test spec for when q2 grows a
      freeze mechanism.

### Phase 4cF: Regression audit
- [x] (2026-07-03: satisfied by the ADOPTED full `cargo xtask verify` run on the merged feature branch — plan-4c complete + bd-h4rhohhy julia/preview fixes — "All verification steps passed!" incl. workspace nextest + hub/WASM legs; log /tmp/bd-h4rhohhy-p3-logs/merged-verify.log; run owned by the bd-h4rhohhy session, adoption recorded in the SDD ledger) `cargo nextest run --workspace` + `cargo xtask verify` (4c0 touches
      `quarto-core` resolution). New Rust integration tests live in
      `tests/integration/<name>.rs` + registered in `tests/integration/main.rs`
      (`.claude/rules/integration-tests.md`); 4c0 unit tests stay inline.
- [x] File braid strands for any QAPI gaps or resolution-model issues discovered. (Filed continuously during execution: bd-i0jn2wqy pampa inline-code reader round-trip P2; bd-bmrs6ec8 knitr silent-drop vs TS loud-error divergence P3; bd-8dq4pv5s positive ownedLanguages wire-field design note P3; bd-mx5x609r freeze-epic canFreeze binding test P4.)

### Phase 4cH: Marimo through `q2 preview` (browser-level e2e — ADDED 2026-07-03, user-requested)

Additive to the frozen SC1-SC20 spec (Plan-4 4J precedent). The preview-capture
plan (2026-07-02-preview-capture-delivery.md) established the e2e-pw tier —
Playwright specs in `q2-preview-spa/e2e/` driving the REAL `q2 preview` binary:
the echo-engine spec `engine-capture-splice.spec.ts` (always-on CI guard; seam id "PC5" in that plan) and the julia spec `engine-capture-splice-julia.spec.ts` (real julia, opt-in via the `QUARTO_PC6_LIVE=1` env flag; seam id "PC6"). Marimo gets the same kind of real-engine browser-level evidence.

**SC21 — CORRECTED 2026-07-03 (dated annotation; original row below retained for
history):** the original GREEN case is UNIMPLEMENTABLE — architectural FINDING #5
(strand bd-5jxcio5d): the preview capture-splice (`capture_splice.rs`
`derive_cell_outputs`/`is_cell_wrapper`) anchors only on `::: {.cell}` wrappers;
marimo emits bare `{=html}` islands with no wrapper, so its capture records
server-side but never splices — the pane shows inert source regardless of the
delivery chain's health. Additionally the original assertion (b)'s premise was
wrong: the literal source ALSO survives in the include-in-header
`notebookCode:` script, not only URL-encoded. **SC21 is REFRAMED as SC21-NEG, a
limitation-pinning canary** (controller adjudication, user asked, AFK —
best-judgment per recommended option; reversible):
- Same spec file/gates/harness/temp-copy discipline as below. Assertions
  (conjunctive): (a) the preview server log records the marimo capture
  (`recorded engine capture(s)` line with `marimo`) — proves the engine ran and
  the recording half works; (b) after a bounded wait the pane STILL contains
  the literal inert `40 + 2` AND does NOT contain `marimo-cell-output` — pins
  the limitation exactly.
- **Named revert/tripwire:** this canary is EXPECTED TO REDDEN when
  bd-5jxcio5d's fix lands (either fix shape makes the island reach the pane,
  breaking (b)) — at which point the fixer flips it into the positive splice
  test (marimo-cell-output + 42 present; literal-absent check scoped to the
  pane body EXCLUDING the head script, per the corrected premise). The
  marimo-leg fixture revert (remove python claim + claims-files:[]) still
  binds (a): with it applied, no marimo capture is recorded → (a) reddens —
  implementer must prove that RED once.
- sql-in-preview remains accepted-untested; the inherited set_capture hunk
  note is moot for the NEG form (the chain's tail is exactly what's severed).

**SC21 (additive seam — PREVALIDATED 2026-07-03, frozen):**
- **Tier:** e2e-pw (real `q2 preview` binary + embedded SPA/WASM + real chromium
  + real uv/marimo subprocess). Correct tier: the unit is the live delivery
  chain (set_capture → samod sync → onCapturesChange → WASM splice) + the SPA
  pane — jsdom cannot host it; no lower tier is faithful.
- **Real unit mounted:** `previewServer.ts` harness spawning the real binary on
  a TEMP COPY of the marimo fixture project (never the committed fixture);
  marimo doc = one `{python .marimo}` cell `import marimo as mo` / `40 + 2`
  (distinctive literal `40 + 2`, distinctive result `42` — avoids the ambient-`2` regex-dodging the julia preview spec needed for its `1 + 1` doc).
- **Seam (mount + wait + assertion surface):** new
  `q2-preview-spa/e2e/engine-capture-splice-marimo.spec.ts`, mirroring
  `engine-capture-splice-julia.spec.ts`'s shape. Gates: deno+uv presence AND
  opt-in `QUARTO_SC21_LIVE=1` (user directive; same pattern as the julia
  spec's `QUARTO_PC6_LIVE`). Wait via `waitForFunction` on the pane
  (NO reload — the eager capture is recorded at server startup, so the first SPA render may already be spliced; the echo spec documents these semantics in its header). CONJUNCTIVE
  assertions on the final pane DOM:
  (a) executed-marimo marker present: pane HTML contains `marimo-cell-output`
      (engine-specific markup) AND the evaluated `42`;
  (b) inert-source ABSENT: the literal token `40 + 2` does NOT appear in the
      pane — non-vacuous because marimo's executed output carries the source
      only URL-ENCODED inside `<marimo-code hidden>` (`40%20%2B%202`-style,
      per compat doc §fix2 evidence), so the literal form exists ONLY in the
      unexecuted/inert cell. This is the splice-replaced-not-appended
      discriminator, marimo-flavored.
- **Mock boundary:** none (real binary, real browser, real engine).
- **Named revert hunks → RED (two, layered — BOTH lines binding):**
  1. *Chain hunk (inherited, NOT re-proven):* `set_capture`
     (`crates/quarto-preview/src/capture_driver.rs:193`) → RED-by-timeout.
     Revert-PROVEN during the preview-capture plan (echo-spec leg, its P2 task);
     SC21 inherits that proof by reference exactly as the julia spec does.
     Do NOT re-run this revert.
  2. *Marimo-leg hunk (NEW — implementer MUST prove RED once):* apply SC8's
     ratified two-part fixture revert (remove the `python:` claim + add
     `claims-files: []`) to the spec's TEMP project copy only → marimo never
     owns/executes the cell → no marimo capture → assertion (a) never
     satisfied → spec fails. Capture the failure verbatim in the spec's doc
     comment; temp-copy-only, committed fixture byte-untouched (`git diff`
     clean). This binds the MARIMO ENGINE's participation, which hunk 1
     cannot (hunk 1 reddens for any engine).
- **Vacuity notes:** (a) alone could not distinguish executed-from-capture vs
  any hypothetical pre-rendered content; (b) closes that. The `42`/`40 + 2`
  pair keeps both assertions non-ambient. A skip without `QUARTO_SC21_LIVE=1`
  is BY DESIGN (continuous-integration runs stay fast; live engine runs are
  opt-in), so the deliverable includes one recorded LIVE run
  (flag set) with timing — a skip-only green does not close this phase.
- **Preconditions (procedural, in-row so they cannot be skipped):** verify
  preview-binary freshness per
  `claude-notes/instructions/preview-spa-rebuild.md` (include_dir! trap)
  BEFORE trusting any pass/fail; no isolateJuliaProject-equivalent needed
  (marimo has no shared daemon/transport), plain temp project copy suffices —
  state this in the spec header.
- **Missing-test pass (logged, not silent):** marimo bare-sql-interop through
  preview (the two-cell SQL doc in the pane) = ACCEPTED-UNTESTED for 4cH —
  minimal-doc parity with the julia preview spec is the scope; sql-in-preview folds into any
  future preview-engine hardening pass.

- [x] Write the spec (gating: deno+uv presence AND opt-in `QUARTO_SC21_LIVE=1`,
      mirroring the julia preview spec's `QUARTO_PC6_LIVE` opt-in flag — settled per user directive 2026-07-03;
      record the live-run timing). *(2026-07-03, NEG: `engine-capture-splice-marimo.spec.ts`
      is the SC21-NEG limitation canary — asserts (a) the server records the marimo
      capture and (b) the pane STILL shows inert `40 + 2` / no `marimo-cell-output`,
      NOT the original "executed output appears". Live GREEN 7.5s; skip-clean confirmed.
      Server-log access added additively to `previewServer.ts` (`serverLog()`).)*
- [x] Ensure preview-binary freshness per claude-notes/instructions/
      preview-spa-rebuild.md before trusting any result (include_dir! trap).
      *(2026-07-03: chain was stale (dist/binary older than WASM); ran full rebuild
      npm run build:wasm → cargo xtask build-q2-preview-spa → cargo build --bin q2;
      verified mtime ordering before trusting results.)*
- [x] Green run recorded with timing; pane content evidence in the report.
      *(2026-07-03, NEG: the "green run" is the canary GREEN — inert pane pinned.
      Marimo-leg revert proven RED once via SC8's literal two-part entry-only
      revert (remove ONLY the `python:` claim entry + `claims-files: []`, keeping
      the static map → static short-circuit returns None, never reaching dynamic
      `claimsLanguage` → render fails jupyter-unavailable → no capture recorded).
      Evidence in .superpowers/sdd/task-4cH-report.md.)*
- [x] Compat doc note: marimo-in-preview findings (incl. how include-in-header
      content behaves in the SPA pane vs static render — observation, not a
      gate). *(2026-07-03: added to 2026-07-02-marimo-engine-q2-compat.md — FINDING #5:
      marimo renders fully via q2 render but does NOT splice into q2 preview
      (`.cell`-anchored capture-splice; marimo emits bare `{=html}` islands);
      strand bd-5jxcio5d.)*

### Phase 4cG: Adaptation documentation
- [x] Summary of every change to `marimo-engine.ts`/fixture (Q1→q2), categorized
      (claimsLanguage interop, bare-sql execution gate, API-shape, dropped-method,
      first_class/dotted-language, deno.json/mock, loader-shim). Feeds the
      extension-author migration guide alongside Julia's, and the upstream PR to
      `quarto-marimo`. **Done (2026-07-03, task 4cG):** written as a separate
      file — [2026-07-03-marimo-migration-guide.md](../research/2026-07-03-marimo-migration-guide.md)
      — matching the Julia precedent's separate-file structure (compat doc
      itself already covers the same ground in prose across §1-§16; the guide
      cross-references rather than duplicates). Also includes the "q2-core
      changes marimo forced" section (pampa writer round-trip fix `411380777`,
      TS-includes path→content `13f697c85`, `ts_protocol.rs` doc pin
      `b4f4f52bf`) framed as "fixed in q2, no engine action needed" for the
      migration guide and as context (not a merge prerequisite) for the
      upstream PR notes. Every claim traces to a commit hash, compat-doc
      section, or plan correction/seam id — no from-memory assertions.

## Relationship to other plans
- **Julia validation (`2026-04-16-julia-validation.md`)** — parallel; 4c reuses
  its build scaffolding and adds first_class + shared-language (python/sql) +
  Vec-static-claim + interop-execution coverage Julia can't.
- **Plan 3 (jupyter)** — not needed to *run marimo* (no `jupyter` QAPI
  namespace), but **required for 4cD's coexistence** (jupyter must execute the
  plain `{python}` / `{bash}` cells). Resolution doesn't need it; execution does.
- **Plan 4b (shadow-engine features)** — 4c gives 4b a real second engine;
  two-engine single-doc scenarios must use *distinct languages* (correction 10).
- **Plan 6 (Pass-1 lift)** — marimo is fully-static via `whenClass` + the 4c0 Vec
  form (zero-load, Pass-1-resolvable), so a Julia+marimo project resolves at
  Pass-1; 4c is the first exercise of first_class-conditioned static claims.
- **Plan 8** — unrelated (mermaid/dot); both extend the "engine claims its
  language" validation set.
