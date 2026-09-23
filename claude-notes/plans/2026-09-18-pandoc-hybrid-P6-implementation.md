# P6 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-18
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P6-numbering-wiring.md`](2026-08-20-pandoc-hybrid-P6-numbering-wiring.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md) (§4 C, §7, §11, §12)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Depends on:** P3, P5 (per the epic's graph) — and therefore transitively on P1/P2/P4.
**Status:** Ready for subagent-driven execution. Six tasks; one of them (Task 3) has **no
production file changes at all**, and its honest binding is cross-plan — see
`## What a revert hunk means for a "confirm the passthrough" test`.
**Updated 2026-09-18:** all three of Gordon's decisions on this plan are applied — **Task 4 is now the formal owner** of
the positive external-mode "Figure N:" golden (moved here from P3's checklist item 3), and **T2.1's
RED shape is a warn-and-skip rather than a render crash**, because the missing callout
`order == nil` guard is now anchor **A7** in P3 Task 3's upstream patch.

This file adds nothing to P6's scope — it converts P6's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P6 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P6 + the design doc; where this file and the plan disagree, the plan wins.

---

## Tiers used in this file

| Tier | What it is | How it runs |
|---|---|---|
| **U** | Rust unit test — `#[test]` in a `mod tests` inside the crate under test | `cargo nextest run -p <crate>` |
| **I** | Rust integration test — `crates/<crate>/tests/integration/<name>.rs`, registered as `pub mod <name>;` in that crate's `tests/integration/main.rs`. **Never** a top-level `crates/<crate>/tests/<name>.rs` (`.claude/rules/integration-tests.md`) | `cargo nextest run -p <crate>` |
| **L** | Lua/pandoc integration test — a real `pandoc --data-dir … -L main.lua` invocation against P4's materialized vendored Q1 tree | `cargo nextest run -p quarto-core`, hard-gated on pandoc (see the gate policy below) |
| **G** | Dev-only golden capture — needs a real Q1 `quarto` binary and/or `external-sources/`. Local/dev gate only (CLAUDE.md External Sources Policy: not in CI). Committed artifacts are `insta` snapshots. |

### L-tier gate policy (stated once; referenced by every L row)

**No silent skip — a silently-skipping test is a vacuous test.** Adopt the existing in-repo
precedent verbatim: `crates/pampa/tests/integration/test.rs:160-180`'s
`assert_good_pandoc_version()` **panics** with an actionable message when the local `pandoc` is
outside its calibrated window (`PANDOC_ORACLE_MIN_VERSION = (3, 6)` /
`PANDOC_ORACLE_MAX_VERSION = (3, 10)`, `test.rs:117-118`), with one deliberate escape hatch
(`PAMPA_PANDOC_ORACLE_BYPASS_VERSION_GATE=1`). Every L row below inherits that shape: **absent or
out-of-range `pandoc` ⇒ the test fails loudly.** P4 Task 7 owns reconciling the version floor
(the `v1.11.3` `configuration` file declares `PANDOC=3.10`; this repo's CI installs 3.8.3; the
dev machine used for this pass has 3.8.1); every L row here names that reconciliation as a
prerequisite rather than assuming it, and must be registered in whatever equivalent of
`ORACLE_TEST_NAMES` (`crates/xtask/src/pandoc_check.rs`) P4's preflight introduces, in the same
commit.

### Which tree does a hunk live in?

Four trees are in play and a revert-hunk claim is meaningless without naming which:

1. **Upstream quarto-cli** — `/Users/gordon/src/quarto-cli/src/resources/filters/…`. P6 changes
   **nothing** here. P6 only *reads* it, and depends on P3's patch having landed in the vendored copy.
2. **The vendored pinned copy inside q2** — P4's `resources/pandoc-filters/` (verified 2026-09-18:
   `ls resources/` shows `pandoc/` but **no `pandoc-filters/`** — the directory does not exist yet;
   P4 Task 1 creates it). Carries P3's `QUARTO-PATCH(upstream PR …)` edits.
3. **"Ours," alongside the vendor** — P5's shim file, a sibling of `customnodes/*.lua` but
   **outside** the `v1.11.3` pin (P5's checklist: "P4's README should list it as 'ours,' not part
   of the `v1.11.3` pin, so a re-vendor delete-and-recopy doesn't remove it"). **Task 2's hunks
   live here.**
4. **q2 proper** — `crates/quarto-core/…`. **Task 1's hunk lives here** (P4's params-blob builder).
   Tasks 3–6 add test code here and no production code.

---

## Verified anchor inventory (read 2026-09-18 against the real trees)

The quarto-cli checkout is at **`v1.11.5-1-g83d48d8e8`**, *not* the `v1.11.3` P5's plan describes
("the local checkout (`dcffbfade8`) is `v1.11.3` + one doc-only commit") — the checkout has moved
since that pass. **`git diff --stat v1.11.3..HEAD` is empty for every file P6 cites** (verified
across `mainstateinit.lua`, `customnodes/{theorem,callout,floatreftarget,proof}.lua`,
`crossref/{custom,refs,format,index,sections,equations}.lua`, `modules/callouts.lua`,
`quarto-init/metainit.lua`), so every line number below is simultaneously valid for the pinned
tag and for today's HEAD. Cited **identifier-first, line-number-second** per P3's anchor-text
discipline.

| Anchor (identifier) | File | Line today | P6's citation | Verdict |
|---|---|---|---|---|
| `crossref.categories.all` literal | `mainstateinit.lua` | `categories` 32, `all` 33, closes ~118 | `32-119` | **OK** |
| `setup_crossref_category_indices()` / `add_crossref_category()` | `mainstateinit.lua` | 124 / 133 | (not cited) | added here |
| `theorem_types` table (`alg` at 48-52) | `customnodes/theorem.lua` | 7-53 | `7-53` | **exact** |
| `initialize_custom_crossref_categories(meta)` | `crossref/custom.lua` | 6; `add_crossref_category(obj_entry)` at 67; file is **157** lines | `6-158` | **drift (1 line over EOF)** |
| `valid_ref_types()` | `crossref/refs.lua` | 198-212 | `198-212` | **exact** (already corrected 2026-09-18) |
| `title(type, default)` reading `param("crossref-"..type.."-title", default)` | `crossref/format.lua` | 4-7 | `4-7` | **exact** |
| `crossref_callouts()` (`callout.order = add_crossref(...)` at 477) | `customnodes/callout.lua` | **469-482** | `467-480` | **drift (−2)** |
| `decorate_callout_title_with_crossref` early return | `modules/callouts.lua` | fn 20, gate 22 | `22` | **exact** |
| `callout_title_prefix`'s `fail("unknown callout prefix …")` | `modules/callouts.lua` | fn 6, `fail()` 9, `titlePrefix(...)` 17 | `6-11` (via P5) | **OK** |
| `prependSubrefNumber(caption_content, float.order)` | `customnodes/floatreftarget.lua` | 221 (sibling at 275) | `221` | **exact** |
| `local order = thm.order` in the Theorem renderer | `customnodes/theorem.lua` | **222** | `220` (P6 Finding 4) | **drift** — P5 already corrected this to 222; P6 still says 220 |
| `if order == nil then return el end` | `customnodes/theorem.lua` | 278 | — (P3's) | **exact** |
| `indexNextChapter(index, appendix)` setting `crossref.startAppendix` | `crossref/index.lua` | **31-43** | `31-41` | **drift (−2 on the end)** |
| appendix label formatting | `crossref/format.lua` | 111-112, 156-157 | `111-112,156-157` | **exact** |
| `initialize_custom_crossref_categories(meta)` call site | `quarto-init/metainit.lua` | 10 | `10` | **exact** |
| `init_crossref_options(meta)` → `readFilterOptions(meta, "crossref")` | `crossref/options.lua` | 5-6 | (not cited) | added here — **this is the second unconditional `Meta` reader of the `crossref:` block**, alongside `custom.lua`, and it matters to Task 3 |

Rust side (q2 proper, this worktree):

| Anchor | File | Line | P6's citation | Verdict |
|---|---|---|---|---|
| `BUILTINS` — **21 entries**, `demo` at 104 | `crates/quarto-core/src/crossref/registry.rs` | 78-106 | "21 entries" | **exact** (fig,tbl,lst,eq,sec,thm,lem,cor,prp,cnj,def,exm,exr,sol,rem,nte,wrn,tip,imp,cau,demo) |
| `classify_cite_id` | `crates/quarto-core/src/crossref/registry.rs` | 178 | `178-181` (via P5) | **OK** |
| `RefTypeSource::{BuiltIn, CustomFromMetadata, Promised}` | `…/crossref/registry.rs` | 61-70 | `Promised` | **exact** |
| `pub fn read(meta, registry)` — "we map this **verbatim** to the Q1 schema" | `…/crossref/metadata.rs` | doc 14-16, fn 96 | `metadata.rs::read()` | **exact** |
| `entries.retain(\|e\| e.key != "format")` — the only meta key filtered | `…/stage/stages/metadata_merge.rs` | **460** | `459-460` | **exact** |
| `has_crossref_plain_data` (the `identifier`+`ref_type`+`kind` triple) | `…/transforms/crossref_index.rs` | 318-333 | by name | **exact** |
| `index_custom_target` — per-ref-type counter, writes `plain_data.order = {section, order}` | `…/transforms/crossref_index.rs` | 250-311 (order write ~285-296) | by name | **exact** |
| `parent: None, // subfloats deferred` / `in_appendix: false, // deferred` | `…/transforms/crossref_index.rs` | 304 / 307 | `in_appendix` "deferred" | **exact** — see Findings #2 |
| `test_callout_with_crossref_id_gets_plain_data_triple` | `…/transforms/callout.rs` | 989 | by name | **exact** |
| Callout's `plain_data` triple write | `…/transforms/callout.rs` | 277-300 (`ref_type` at 293) | `callout.rs` construction step | **exact** |
| Proof: "Intentionally no `ref_type` / `kind`" | `…/transforms/proof.rs` | 148; doc at 14 | `145-148` | **OK** |
| `THEOREM_CLASSES` (8 entries, no `algorithm`) | `…/transforms/theorem.rs` | 61 | `61-69` (via P5) | **OK** |
| `crossref-numbering` anywhere in `crates/` | — | **zero hits** | — | confirmed absent; Task 1 introduces it |
| `PipelineProfile` anywhere in `crates/` | — | **zero hits** | — | confirmed absent; P1 Task 1 introduces it |

---

## What a revert hunk means for a "confirm the passthrough" test

P6 is unusual: a large fraction of its scope is **confirming that something already works**
(`crossref.custom` passthrough, Task 3) and **documenting that something deliberately does not**
(Finding 2's Q2-only / `Promised` categories). Both are the exact shape the vacuity check exists
for — *an assertion that reads identically whether or not any P6 work happened at all.*

Stated plainly, once, so no task section has to hedge:

- **A "confirm the passthrough" test has no P6-owned revert hunk, because P6 adds no production
  code for it.** P6's own plan says so in as many words: "no new export code, just the test."
- **It does have a real, nameable hunk — it just belongs to someone else.** The discriminating
  line is `crates/quarto-core/src/stage/stages/metadata_merge.rs:460`,
  `entries.retain(|e| e.key != "format")` — the *only* key filtered out of the merged
  `doc.ast.meta` map. Widening that predicate to `&& e.key != "crossref"` reddens Task 3's
  assertions. That is a **negative-space guard on a pre-existing line**: the test does not prove
  P6 did anything, it prevents a future change from silently removing a behavior three plans
  depend on. Call it that; do not call it a binding.
- **Its second, genuinely cross-plan hunk is P2's `Meta` serialization** (P2 Task 5's
  `write_config_value_as_meta` path, reached from `stream_write_pandoc`,
  `crates/pampa/src/writers/json.rs:4238`). If the wire output's `meta` object stops carrying the
  `crossref` subtree, Task 3 reddens. **A cross-plan guard is legitimate. A pretended local
  binding is not.**
- **There is no hunk anywhere for the three registration-free built-in mechanisms** (Finding 1:
  `theorem_types`, bespoke `eq`, bespoke `sec`). There is nothing to revert because there is
  nothing to register. "They still work untouched under external mode" is bound only
  *indirectly*, via Task 4's decoration rows — see the Missing-test pass item 6 for the
  per-mechanism verdict.

No seam in this file asserts against a registration or export mechanism. P6 Finding 1 disproved
that such a mechanism exists for built-ins, and commit `5a872ed17` retitled the plan from "seed"
to "passthrough" for exactly that reason. **Any seam phrased as "assert Q2's registry was
exported into Q1" would be theater** — the one real lever for built-ins is the display-name
override `crossref-<type>-title` (`crossref/format.lua:4-7`), and that is **P4 Task 5's**
params-blob work, not P6's. No row below claims it.

---

## Discriminating fixture properties (the vacuity-critical ones, stated once)

Four fixture requirements are load-bearing across several tasks. Stating them here means each
task's table can reference them instead of re-deriving them.

### D1 — Q2's and Q1's numbers coincide for any simple fixture. Injected order must differ.

Verified against both sides: Q2's `index_custom_target`
(`crossref_index.rs:250-311`) keeps a **per-ref-type counter starting at 0 and incrementing in
document order**; Q1's `indexNextOrder(type)` (`crossref/index.lua:47-57`) does exactly the same
thing with exactly the same partition (`crossref.index.nextOrder[type]`). **So for a flat
single-file document, `Q2's order == Q1's order` for every type.** A golden that extracts
"Figure 1:" therefore **survives the revert of the numbering-suppression hunk**: with suppression
reverted, Q1's own assign-group runs and computes `1` too. This is precisely the collapsed
discriminator the discipline warns about, and it is the same trap P3's companion recorded for its
own Task 8.

**The discriminating fixture:** one in which Q2 injects an order Q1 would **not** compute —
concretely, a single labeled figure whose wire `plain_data.order` is `{order: 7, section: []}`,
making the expected prefix `Figure`+NBSP+`7:`. Under external mode the output reads `7`; with the
suppression hunk reverted, Q1 overwrites it and the output reads `1`. Used by **T4.1**.

Things that do *not* discriminate, checked and rejected:
- **`number-offset`** — affects section numbering only, and P6/design §11 already decided not to
  forward it (design doc §11, decided 2026-09-18 with Gordon).
- **`chapters`-scoped numbering / appendix** — Q2 hardcodes `in_appendix: false, // deferred`
  (`crossref_index.rs:307`) and never produces a chapter-scoped order, exactly as P6 Finding 5
  concluded. No live input.
- **A `Proof`** — Q1's `crossref_theorems` does assign `proof.order`, but `proof.lua`'s renderer
  never reads it (P3's audit, independently confirmed), so both states render identically.
- **A duplicate id** — Q2 *skips* numbering the duplicate (`index_custom_target` returns before
  incrementing, `crossref_index.rs:262-271`) where Q1 would number it, so this genuinely
  discriminates — but the fixture also emits a Q2 duplicate-id diagnostic, so the test would be
  asserting a diagnostic path as its numbering discriminator. Rejected as too indirect; recorded
  so it is not rediscovered.

### D2 — the second, order-free discriminator: `sections.lua`'s collateral suppression

Design doc §11/§12: under `crossref-numbering: external` the whole `quarto_crossref_filters`
group is skipped, and `sections()` is inside it — so `number-sections: true` silently loses
section numbers. **That loss is the cleanest available proof that the assign-group did not run**,
because it involves **no order injection at all**: Q2 injects nothing for headers
(`crossref_index.rs:212-214`'s `visit_header` only advances the counter stack; it never registers
the header as a target), so the surface changes on the suppression flag *alone*. Used by **T4.2**
as a **labeled accepted-divergence golden** — the same pattern P7 already uses for mermaid. This
does not fix, reopen, or relitigate §11; it captures the frozen loss as a reviewable artifact and
gets a second independent discriminator for Task 1's hunk for free.

### D3 — the Callout fixture must use a `ref_type` Q1's category table knows

Finding 4's whole point is that a Route-L Callout silently loses its "Note N:" prefix, so Task 2
asserts the prefix is *present*. That assertion discriminates **only if** P5's `fail()`-guard
fallback does not fire for an unrelated reason. The guard's corrected predicate (P5, round 4) is
`crossref.categories.by_ref_type[ref_type] ~= nil` — **not** `valid_ref_types()`, which is a
strict superset (`refs.lua:198-212` adds every `theorem_types` key plus `"eq"` and `"sec"`).
Required fixture properties, all three:

1. **`ref_type` must be `nte`/`wrn`/`cau`/`tip`/`imp`** — one of `crossref.categories.all`'s
   `kind = "Block"` entries, present in `by_ref_type`. `#nte-setup` is the canonical choice.
2. **A `#thm-…` id on a callout is the trap, not the fixture.** Q2's `classify_cite_id`
   (`registry.rs:178`) splits on the first hyphen with no regard for the div's classes, so
   `::: {#thm-foo .callout-note}` yields `ref_type: "thm"` — which passes `valid_ref_types()`,
   fails `by_ref_type`, and takes P5's fallback. A prefix-absent assertion on *that* fixture is a
   **false RED that reads as a false confirmation**. It belongs in P5's
   `callout-foreign-category.qmd` error-path fixture, not in any P6 numbering seam.
3. **The callout must be crossref-eligible in Q2 too** — i.e. Q2's callout pass must actually
   write the `plain_data` triple. That requires `ctx.ref_type_registry` to be populated; the
   existing `test_callout_with_crossref_id_gets_plain_data_triple`
   (`callout.rs:989-1031`) is the reference for the required setup, and its negative siblings at
   `callout.rs:1041-1110` show what an *un*eligible callout looks like (no `ref_type`, no
   `identifier` in `plain_data`).

### D4 — "the path was actually exercised" assertions, for the two tests that assert an absence

Two P6 assertions are satisfied trivially by a broken render, and each needs a positive companion:

- **Finding 3's deliberately-unnumbered Proof** (T5.4): "the Proof has no number" is satisfied by
  an empty document, a crashed filter chain, or a dropped node. Companion assertion: the proof's
  **body text** and its **`proof_types` label** ("Proof") are present in the output — the label is
  produced only by `proof.lua`'s own renderer (`add_renderer("Proof", …)`, which P3 confirmed
  never reads `.order`), so its presence proves the Route-R Proof node reached a real Q1 renderer.
- **Every "prefix absent" row** (T4.2, T2.2): additionally assert the caption/heading **body text**
  is present, so an empty or failed render cannot pass. This is the same guard P3's companion
  applies to its M2–M4 rows.

---

## Task 1: Set `crossref-numbering: external` for `Pandoc(fmt)` profiles

**Scope.** P6 supplies the *value*; P4's existing params-blob plumbing carries it. One
profile-gated insertion into the `QUARTO_FILTER_PARAMS` map, plus the negative case (no other
profile sets it). This is the hunk the rest of P6 discriminates against.

**Files.**
- `crates/quarto-core/src/…` — **P4 Task 4's params-blob builder** (q2 proper). The module does
  not exist yet; P4 Task 4 creates it ("structural + core keys, the synthetic project value").
  Verified 2026-09-18: `grep -rn 'crossref-numbering' crates/` returns **zero hits**, and
  `grep -rn 'PipelineProfile' crates/` returns **zero hits** — both are introduced by predecessor
  plans (P1 Task 1 for the profile enum, P4 Task 4 for the builder).
- No Lua changes. No vendored-tree changes. P3 already made `main.lua`'s
  `assignCrossrefNumbers` predicate read this param (P3 Task 2, anchor
  `if enableCrossRef then`, `main.lua:718`/`:719`-after-P4's-splice).

**Acceptance criterion.** For `PipelineProfile::Pandoc(_)` the decoded `QUARTO_FILTER_PARAMS`
blob contains `"crossref-numbering": "external"`; for every non-Pandoc profile the key is
**absent** (not `"quarto"` — Q1's `param("crossref-numbering", "quarto")` default already covers
that, and emitting the string would make the vendored default unreachable and untested). A real
`--to docx` invocation through `PandocWriteStage` produces a blob containing the key.

**Prerequisite.** Requires **P1 Task 1's `PipelineProfile` enum + `FormatIdentifier::Pptx`**,
**P4 Task 4's params-blob builder**, **P4 Task 3's base64 codec**, and — for T1.3 — **P4 Task 9's
`PandocWriteStage` / `render_qmd_to_pandoc` entry point** and **P4 Task 2's materialized
vendored tree**. Named by capability *and* task number because those companions exist on disk.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T1.1 | U | P4 Task 4's real params-blob builder function | build params for `PipelineProfile::Pandoc("docx")` → `assert_eq!(params["crossref-numbering"], "external")` | none — pure function over a profile value + a `Format` | the profile-gated insert added by this task |
| T1.2 | U | same builder | build params for `HtmlRender` **and** `Preview` → `assert!(!params.contains_key("crossref-numbering"))` | none | the *gate* on that insert (as opposed to the insert) |
| T1.3 | L | `PandocWriteStage` → P4's codec → a real `pandoc -L main.lua` run; the param is read by the vendored `main.lua`'s `assignCrossrefNumbers` | drive `render_qmd_to_pandoc` (or the `--to docx` CLI path) on a one-figure fixture → decode the `QUARTO_FILTER_PARAMS` the stage actually built → assert the key is present with value `external` | environment only: `QUARTO_SHARE_PATH`, `--data-dir`, the `pandoc` binary. No Lua and no builder is mocked. | the profile-gated insert, **plus** the fact that the builder is wired into the Pandoc stage at all |

**Revert hunks, stated exactly:**

- **T1.1** — Revert ⟨the `crossref-numbering` → `"external"` insertion in P4 Task 4's params-blob
  builder, under the `PipelineProfile::Pandoc(_)` arm⟩ → ⟨T1.1's
  `assert_eq!(params["crossref-numbering"], "external")`⟩ **RED** (key absent).
- **T1.2** — Revert ⟨the `Pandoc(_)`-only gate, i.e. hoist the insertion out of the match arm so
  it runs for every profile⟩ → ⟨T1.2's `assert!(!params.contains_key("crossref-numbering"))` for
  `HtmlRender`⟩ **RED**. Without T1.2, T1.1 passes for an unconditional insert — which would set
  external mode on legs that have no Q1 to suppress, a silent mis-wiring with no other detector.
- **T1.3** — Revert ⟨the same insertion⟩ → ⟨T1.3's decoded-blob assertion⟩ **RED**. T1.3 is the
  **"the path was actually exercised"** row: T1.1/T1.2 test a function; T1.3 tests that the
  Pandoc leg calls it. CLAUDE.md's end-to-end rule requires this row, and the 2026-04-20
  `CodeHighlightStage` incident is exactly the failure it prevents (every unit test green, no real
  render affected).

### Refactor-induced vacuity check

- **T1.1's expected value (`"external"`) is a bare string literal appearing in both the
  production hunk and the test.** That is fine here and *not* a collapsed discriminator: the
  states this row distinguishes are *key present* vs *key absent*, and those differ regardless of
  the literal. But it means T1.1 cannot catch a **typo shared between hunk and test**
  (`"exernal"` in both). T1.3 closes that: the vendored Lua compares against the literal
  `"external"` (P3's `~= "external"` / `== "external"`), so a shared typo makes T1.3's downstream
  behavioral consequence — Task 4's `Figure 7:` — disappear. Do not drop T1.3 as redundant.
- **T1.2's `absent` vs `"quarto"` choice is load-bearing, not stylistic.** If the builder emitted
  `"quarto"` for non-Pandoc profiles, T1.2 would have to assert a value rather than an absence,
  and P3's `param("crossref-numbering", "quarto")` default literal would become unreachable —
  which P3's companion already logged as `accepted-untested` on the grounds that no input
  discriminates the two spellings. Keeping the key absent preserves that analysis. Stated so an
  implementer does not "tidy" it into an explicit `"quarto"`.

---

## Task 2: Land the Callout Route-R reclassification in P5's shim

**Scope.** P6 Finding 4, decided with Gordon: route `Callout` through Q1's `quarto.Callout(...)`
constructor and assign `.order` post-construction, reusing — not redesigning — P5's Theorem
mechanism. The design-doc §3 table update already landed (commit `8e9e546b4`); this is the
implementation half, and it lands in **P5's file**, not a P6 file.

**Files.**
- P5's shim, in the **"ours" tree** alongside the vendored `customnodes/*.lua` (path owned by
  P4 Task 1's README and P4 Task 8's `main.lua` splice; P5's checklist requires it be listed as
  "ours," outside the `v1.11.3` pin). Two hunks:
  - **H2a** — a `Callout` arm in the shim's type dispatch that calls
    `quarto.Callout{type, appearance, icon, title, content}` (field-map frozen in design doc §3:
    `plain_data.{type,appearance,icon}` + `slots.{title,content}`), rather than reconstructing a
    raw classed Div.
  - **H2b** — `tbl.order = plain_data.order` on the **second** return value of
    `quarto.Callout(...)`. P5's Theorem finding is normative for the API shape:
    `quarto.<AstName>(params)` returns `(scaffold_node, data_table)`
    (`ast/customnodes.lua:263`, via the `create_emulated_node` branch at `:453` — *not*
    `448-458`, which an implementer reading only that range would misread), and the renderer reads
    `.order` off the data table, mirroring `local order = thm.order` at
    **`customnodes/theorem.lua:222`** (P6 Finding 4 still cites the stale `:220`; see Task 6).
- No changes in q2 Rust. No changes in the pinned vendored tree.

**Acceptance criterion.** Under external mode, a `::: {.callout-note #nte-setup}` whose wire
`plain_data` carries the crossref triple plus `order = {order: 3, section: []}` renders a callout
whose title begins with the "Note 3:" prefix that
`decorate_callout_title_with_crossref` (`modules/callouts.lua:20-22`) →
`callout_title_prefix` (`:6-17`) → `titlePrefix` produces; and an *unlabeled*
`::: {.callout-note}` renders as a real Q1 callout with **no** number and no warning.

**Prerequisite.** Requires **P5's Route-R construction + post-construction order-assignment
mechanism** (P5's checklist item "Route-R reconstruction … then post-construction `order`
assignment"), **P5's bottom-up traversal contract** (P5's round-4 sub-finding — a `FloatRefTarget`
inside a `Callout`'s `content` slot must be converted inner-first), **P4 Task 8's shim splice
positioned before `quarto_normalize_filters`** (without that position, the wire Div's *retained*
`callout`/`callout-<type>` classes are picked up by Q1's own class-keyed dispatcher —
`ast/parse.lua:6-13` — and `Callout.parse()` rebuilds a callout with no `order`: a quiet
mis-render, not a crash), and **Task 1's param** for the external-mode cells.
**P5's companion has since landed** (`2026-09-18-pandoc-hybrid-P5-implementation.md`); the
capability this task's Callout work builds on is its **Task 3** (Route R for Callout and Tabset)
plus the shared post-construction `order` assignment specified in its **Task 2**.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | L | P5's shim `Callout` arm + real `quarto.Callout` constructor + real `decorate_callout_title_with_crossref` + real `callout_title_prefix`/`titlePrefix` | external-mode render of a `#nte-setup` callout with injected `order = {order: 3, section: []}` → assert the callout title contains `Note`+NBSP+`3:` **and** the callout's body text | environment only (pandoc, datadir, blob). No Lua mocked; `quarto.Callout` is the unit under test's collaborator, not a stub. | **H2b** |
| T2.2 | L | same chain, negative cell | external-mode render of an **unlabeled** `::: {.callout-note}` → assert body text present, assert **no** digit-bearing `Note …:` prefix, assert stderr carries no `unknown callout prefix` | same | the `plain_data.order` presence check guarding H2b (blanket assignment) |
| T2.3 | L | the shim's **dispatch**, not its field-map | same fixture as T2.1 → assert an output shape only `callout.lua`'s registered renderer produces (for `docx`: the callout's boxed/table wrapper structure; assert it by a marker the generic unwrap path cannot produce) | same | **H2a** |
| T2.4 | L | the nesting contract | a `FloatRefTarget` inside the `#nte-setup` callout's `content` slot, both carrying injected orders → assert **both** the callout's `Note`+NBSP+`3:` **and** the inner float's `Figure`+NBSP+`N:` appear | same | P5's traversal direction (cross-plan) |

**Revert hunks, stated exactly:**

- **T2.1** — Revert ⟨**H2b**, the `tbl.order = plain_data.order` assignment on
  `quarto.Callout`'s second return value⟩ → `callout.order` is nil →
  `callout_title_prefix` passes nil into `titlePrefix` → ⟨T2.1's `Note`+NBSP+`3:`
  assertion⟩ **RED**. **Updated 2026-09-18 — the RED is now a warn-and-skip, not a crash.**
  P3's companion established that this path had **no nil guard**, so reverting H2b used to abort
  the whole render (`modules/callouts.lua:17` → `titlePrefix` → `numberOption` →
  `formatNumberOption`'s `local num = order.order`, `crossref/format.lua:124,140`, raising
  *attempt to index a nil value*). **Gordon decided to add the guard** — it is now anchor **A7**
  in P3's upstream patch, mirroring `float_title_prefix`'s
  `if float.order == nil then warn(...) return {} end` (`crossref/tables.lua:229-231`). So with
  A7 in place, reverting H2b yields: pandoc exits **0**, stderr carries
  `field 'order' is missing from callout`, and the callout renders **without** the `Note`+NBSP+`3:`
  prefix. T2.1 should therefore assert the prefix string **and** `exit == 0` — the exit assertion
  is what distinguishes "H2b is missing" from "the render died for an unrelated reason", and it is
  also what would catch A7 being dropped on a future re-pin.
  **Prerequisite consequence:** T2.1's RED shape depends on A7, which lands in **P3 Task 3**. P6
  already depends on P3, so this is within the graph, but it is a concrete dependency on a
  specific anchor rather than just "P3's external mode" — worth naming because if A7 were ever
  dropped from the PR, T2.1 silently reverts to crash-shaped RED and its `exit == 0` assertion
  becomes the thing that tells you.
- **T2.2** — Revert ⟨the `if plain_data.order ~= nil` guard around H2b, i.e. assign
  unconditionally⟩ → an unlabeled callout gets `order = nil` assigned explicitly (no behavior
  change) *or*, if the implementer synthesizes a default order, gets a spurious `Note 1:` →
  ⟨T2.2's "no digit-bearing prefix" assertion⟩ **RED** in the latter case. **Honest limitation:
  T2.2 does not discriminate the nil-assignment spelling**, only a synthesized-default one; the
  two nil spellings are behaviorally identical, the same shape P3's companion logged for its
  `"quarto"` default literal. Recorded rather than overstated.
- **T2.3** — Revert ⟨**H2a**, i.e. remove `Callout` from the shim's dispatch table⟩ → the wire
  node falls to P5's unrecognized-`type_name` path, which "unwraps the scaffold to its slot
  content and drops the wrapper" → the title and body survive as plain blocks, **and so does the
  prefix-free text** → ⟨T2.3's callout-renderer shape assertion⟩ **RED**. This row exists because
  **T2.1 alone would also redden on H2a's revert, but for the wrong reason** — and P5's own
  round-4 finding names exactly this hazard: an unhandled type's unwrap path preserves slot
  content, "a zero-byte snapshot diff for a fully semantics-stripped node." T2.3 is the
  *shape/gating* row that makes T2.1's RED attributable.
- **T2.4** — Revert ⟨P5's bottom-up traversal, i.e. make the shim group topdown⟩ → the outer
  Callout is converted first and `quarto.Callout` receives raw, unconverted wire Divs in its
  `content`, which the shim's own group has already walked past → ⟨T2.4's inner
  `Figure`+NBSP+`N:` assertion⟩ **RED** while the outer `Note 3:` stays green.
  **That hunk belongs to P5** (its round-4 traversal sub-finding, with P4 Task 8 owning the
  filter-group literal). T2.4 is a **cross-plan guard**, listed here because P6 Finding 5 is the
  reason the nesting case is in scope at all.

### Refactor-induced vacuity check

- **Finding 4's expected value is "the prefix is present," and that is the value most at risk of
  collapsing.** Three distinct states produce a prefix-free callout: (i) H2b missing (order never
  assigned), (ii) P5's `fail()`-guard fallback firing on an unregistered `ref_type`, (iii) the
  shim not dispatching Callout at all (H2a). **D3 removes (ii) by construction** — `nte` is in
  `crossref.categories.by_ref_type`, verified against `mainstateinit.lua`'s `kind = "Block"`
  entries — and **T2.3 separates (iii) from (i)**. Without both, a green T2.1 and a red T2.1 are
  each consistent with two different worlds.
- **The `Note`+NBSP+`3:` form, not the bare word `Note`.** `Note` appears in the callout's own
  default title regardless of numbering, so asserting the bare word is a fully collapsed
  discriminator — it matches the unnumbered output too. Assert the **digit** and the
  **delimiter**, and assert the NBSP: P3's companion records upstream's own docx test commenting
  on the non-breaking-space subtlety (`tests/smoke/crossref/docx.test.ts`).
- **`order = 3`, not `order = 1`.** Per **D1**: a single labeled callout would get `1` from Q1's
  own `crossref_callouts()` (`customnodes/callout.lua:469-482`, which calls
  `add_crossref(label, type, title)` at `:477`) if suppression were reverted, so `1` reads
  identically across the states T4.1 exists to distinguish. `3` keeps this row usable as a
  secondary discriminator for Task 1's hunk as well as for H2b.

---

## Task 3: `crossref.custom` passthrough — regression test only, no production change

**Scope.** P6 Finding 2, narrowed by the design doc's corrected §4 C from "build a Q2→Q1 registry
export" to "confirm a passthrough with a test." Two assertions: (a) the merged metadata's
`crossref.custom` subtree survives into the wire output's Pandoc `Meta`; (b) Q1's own
`initialize_custom_crossref_categories(meta)` reads it from there and the custom category's
caption prefix renders.

**Files.** **No production file changes — this task is a test only.** That is the honest answer
and P6's plan states it outright ("no new export code, just the test"). Test files:
- `crates/quarto-core/tests/integration/crossref_custom_passthrough.rs` (new) +
  `pub mod crossref_custom_passthrough;` in `crates/quarto-core/tests/integration/main.rs`
  (alphabetized, per `.claude/rules/integration-tests.md`).
- Production code merely *read*, not changed: `metadata_merge.rs:460`;
  `crates/quarto-core/src/crossref/metadata.rs:96` (`read`, whose doc comment at `:14-16` states
  the verbatim mapping); the serializer `write_config_value_as_meta` on the streaming path
  (`crates/pampa/src/writers/json.rs`, reached from `stream_write_pandoc:4238`'s `meta` key).

**Acceptance criterion.**
1. A fixture with front matter `crossref: {custom: [{key: dia, reference-prefix: Diagram}]}` run
   through the merge + wire-serialization path yields a `meta` object whose
   `crossref.custom[0]` carries `key: "dia"` and `reference-prefix: "Diagram"` **verbatim** (the
   same two fields `crossref/metadata.rs:136-153` requires and `crossref/custom.lua:6-67` reads).
2. A Q1-side render of that `Meta`, with a `#dia-1`-labeled float carrying an injected order,
   produces the caption prefix `Diagram`+NBSP+`1:` — plus the caption body text (**D4**).
3. The test's module doc names, in one sentence, that this is a *negative-space guard on
   `metadata_merge.rs:460`*, not a binding of any P6 hunk — so a future reader does not mistake
   its green for evidence P6 implemented something.

**Prerequisite.** T3.1 requires nothing beyond **P2 Task 5's `Meta`-carriage harness** (reuse it;
do not build a second one). T3.2 additionally requires **P4 Task 2's materialized tree +
Task 3/4's blob**, **P5's Route-R `FloatRefTarget`** (to inject an order at all), and
**Task 1's param**.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | I | `MetadataMergeStage`'s real key-retention logic + `write_config_value_as_meta` | merge a fixture declaring `crossref.custom` → `pampa::writers::json::write` → assert `meta.crossref.custom[0].{key, reference-prefix}` present and verbatim | `ProjectContext`/`DocumentInfo`/`Format` fixtures, as P2 Task 5 already does | **cross-plan / negative-space**: `metadata_merge.rs:460`'s `retain` predicate |
| T3.2 | L | Q1's real `quarto_meta_init` → `initialize_custom_crossref_categories(meta)` → `add_crossref_category` → `setup_crossref_category_indices` → `float_title_prefix` | external-mode render of the same fixture with a `#dia-1` float carrying injected `order` → assert `Diagram`+NBSP+`1:` **and** the caption body text | environment only | **cross-plan**: the same `retain` predicate, **or** P2's `meta` emission |
| T3.3 | I | `crossref/metadata.rs::read` + `RefTypeRegistry::register_custom` | assert the Q2 side registers `dia` with `source == RefTypeSource::CustomFromMetadata` | none | `metadata.rs`'s `register_custom` call at `:161` |

**Revert hunks, stated exactly:**

- **T3.1** — Revert ⟨`crates/quarto-core/src/stage/stages/metadata_merge.rs:460`,
  `entries.retain(|e| e.key != "format")`, widened to
  `entries.retain(|e| e.key != "format" && e.key != "crossref")`⟩ → ⟨T3.1's
  `meta.crossref.custom[0].key` assertion⟩ **RED**. **This is not a hunk P6 adds.** It is a
  *mutation* of a pre-existing line, and naming it is the honest form of "this test guards a
  passthrough": the revert that reddens it is a hypothetical future regression, not a
  reversal of P6 work. Per the section above: negative-space guard, not a binding.
- **T3.1 (second, cross-plan)** — Revert ⟨P2 Task 5's `meta` emission on the streaming wire path
  (`json.rs:4238`'s `w.key("meta")`)⟩ → ⟨the same assertion⟩ **RED**. Legitimate cross-plan guard.
- **T3.2** — Revert ⟨either hunk above⟩ → Q1's `initialize_custom_crossref_categories` finds no
  `meta.crossref.custom`, `by_ref_type["dia"]` stays nil, and `float_title_prefix`
  (`crossref/tables.lua:226`) hits its unknown-category `fail()` → the render **aborts** →
  ⟨T3.2's `Diagram`+NBSP+`1:` assertion⟩ **RED**. Note the RED is an **abort**. That used to be
  "the same shape as T2.1's"; as of 2026-09-18 it no longer is — A7 turned T2.1's revert into a
  warn-and-skip, while **T3.2's abort is unaffected**, because A7 adds only the *order*-nil guard
  and T3.2 reddens through the *category*-nil `fail()`, which A7 deliberately leaves in place.
  So the two Callout-adjacent tests now have **different** RED shapes, and that difference is
  informative rather than incidental: an abort means "Q1 doesn't know this category", a
  warn-and-skip means "Q2 didn't inject an order".
- **T3.3** — Revert ⟨the `registry.register_custom(ref_type, kind, Some(src))` call at
  `crates/quarto-core/src/crossref/metadata.rs:161`⟩ → ⟨T3.3's
  `assert_eq!(def.source, RefTypeSource::CustomFromMetadata)`⟩ **RED**. **This one is a genuine
  local binding** — but of *pre-existing* Q2 code, and `metadata.rs`'s own tests
  (`:319-345`) already cover it. **T3.3 is therefore a duplicate; do not write it.** It is listed
  only so the implementer does not "add the missing coverage" and finds the existing test instead.

### Refactor-induced vacuity check

**This is the task the vacuity check exists for, so the answer is stated without hedging.**

- **T3.1 and T3.2 assert "the custom category renders with its declared name and number," which
  is true both before and after P6, because the passthrough already works.** Verified on both
  sides: Q2 never strips `crossref` (the only `retain`-filtered key is `"format"`,
  `metadata_merge.rs:460`), and Q1 reads the block from `Meta` unconditionally — **twice**, in
  fact: `initialize_custom_crossref_categories(meta)` at `quarto-init/metainit.lua:10` and
  `init_crossref_options(meta)` → `readFilterOptions(meta, "crossref")` at
  `crossref/options.lua:5-6`, both inside `quarto_meta_init`, part of the always-run
  `quarto_init_filters`, upstream of P3's gate entirely.
- **There is no change P6 makes whose revert reddens these rows.** Saying so plainly is the
  finding, not an evasion. Both rows are **re-pointed** accordingly: their named hunks are the
  `metadata_merge.rs:460` predicate and P2's `meta` emission, both explicitly labeled cross-plan /
  negative-space, and the test's own module doc is required (acceptance criterion 3) to say so, so
  a future reader cannot mistake the green for P6 evidence.
- **The one thing T3.2 adds that T3.1 cannot:** it exercises the *Q1-side* half of the passthrough
  — that the wire `Meta` shape Q2 emits is one `readFilterOptions`/`custom.lua` can actually
  parse. A Meta encoding that serialized the list as, say, a `MetaString` would satisfy T3.1 and
  fail T3.2. That is a real, non-vacuous contract and it is the reason to keep T3.2 despite its
  cost — **not** "it confirms the passthrough," which T3.1 already does more cheaply.
- **The retitle from "seed" to "passthrough" (`5a872ed17`) is honored:** no row here asserts
  against a registration or export mechanism, and no row references
  `crossref-<type>-title`, which belongs to **P4 Task 5** ("Q2's registry becomes authoritative"),
  not P6. Finding 1's corrected text is the authority; P6's implementer must not build a
  display-name path.

---

## Task 4: The numbering-suppression wiring matrix — suppression fired, registration and decoration survived

**Scope.** P6's "In scope" item D, as a behavioral matrix. Two halves, and the **negative half is
the one that matters**: (a) prove Q1's numbering *did not run* — not merely that the final numbers
are right, which is a different claim and, per **D1**, an undiscriminating one; (b) prove category
registration and the render-decoration sites still run and consume `plain_data.order`, now for
Callout as well as FloatRefTarget and Theorem.

**Files.** **No production file changes — this task is a test only.** It discriminates Task 1's
hunk, Task 2's hunks, P3's patch, and P5's order assignment.
- `crates/quarto-core/tests/integration/crossref_external_mode_matrix.rs` (new) + its
  `pub mod` registration. **Do not** extend P3's `crossref_numbering_matrix.rs`: that file's rows
  deliberately run with **no shim and no injected order** (P3's Task 7 scope), and mixing the two
  fixture families in one module invites a later reader to assume the wrong prerequisite set.
- Fixtures (new, under the crate's test fixture tree):
  - `external-injected-order.qmd` — one labeled figure, wire `order = {order: 7, section: []}`.
  - `external-number-sections.qmd` — `number-sections: true`, two levels of headers, one labeled
    figure.
  - `external-all-three.qmd` — one `#fig-`, one `#thm-`, one `#nte-` (**D3**-compliant), each with
    an injected order.

**Acceptance criterion.** All rows below behave as tabulated; every "absent" row additionally
asserts its body text is present (**D4**); the `number-sections` row is committed as an `insta`
snapshot **labeled in its own name and header comment as an accepted divergence** (design doc
§11/§12, `bd-5aklrxgi`), so nobody later "fixes" it.

**Prerequisite.** Requires **Task 1's param**, **P3 Task 2's `assignCrossrefNumbers` predicate**
and **P3 Task 3's four `crossref_present()` sites** landed in the vendored copy, **P4 Task 2's
materialized tree** + **P4 Task 3/4's blob**, **P5's Route-R reconstruction with post-construction
order assignment** (for all of FloatRefTarget, Theorem, and — via Task 2 — Callout), and
**P4 Task 7's pandoc-version reconciliation**. Every row is L tier and none is schedulable before
those land; P6 sits after P3 and P5 in the graph precisely so that this task is bindable.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | L | the full external-mode chain: Task 1's param → P3's `assignCrossrefNumbers` → P5's Route-R `FloatRefTarget` + order assignment → `decorate_caption_with_crossref` → `float_title_prefix` → `titlePrefix` | `external-injected-order.qmd` → assert the caption prefix is `Figure`+NBSP+**`7`**+`:` and the caption body text is present | environment only | **Task 1's profile-gated insert** |
| T4.2 | L | Q1's `sections()` inside the suppressed group; Q2's header path (`visit_header`, which injects nothing) | `external-number-sections.qmd` → assert **no** section number precedes any header title; assert every header **title** is present; snapshot as a labeled accepted divergence | environment only | **Task 1's insert** (second, order-free discriminator) |
| T4.3 | L | Q1's unconditional category registration: `quarto_meta_init` → `initialize_custom_crossref_categories` → `setup_crossref_category_indices`; `theorem_types`; the `eq`/`sec` literals | under external mode, a probe asserts `crossref.categories.by_ref_type["fig"] ~= nil`, `theorem_types["thm"] ~= nil`, and `valid_ref_types()` contains `eq`, `sec`, `thm`, `nte` | environment only | **none in P6** — see below |
| T4.4 | L | P5's Route-R `Theorem` + `theorem.lua:278`'s implicit `order == nil` gate + `captionPrefix` | `external-all-three.qmd`'s `#thm-` with injected `order = 5` → assert `Theorem`+NBSP+`5` appears | environment only | **cross-plan**: P5's `tbl.order` assignment for Theorem |
| T4.5 | L | the three-type joint case | `external-all-three.qmd` → assert all three injected numbers (`fig`=7, `thm`=5, `nte`=3) appear together in one render | environment only | **Task 1's insert**, jointly; see the vacuity note |

**Revert hunks, stated exactly:**

- **T4.1 — this is the single most important seam in P6.** Revert ⟨Task 1's
  `crossref-numbering: "external"` insertion in P4's params-blob builder⟩ → `main.lua`'s
  `assignCrossrefNumbers` becomes true → `quarto_crossref_filters` runs →
  `crossref/figures.lua:27-37` overwrites the injected `order` with its own `indexNextOrder("fig")`
  result, which for the only figure is **`1`** → ⟨T4.1's `Figure`+NBSP+`7:` assertion⟩ **RED**.
  This is the row that asserts *Q1's numbering was actually suppressed*, as distinct from *the
  final numbers are right* — and per **D1** those two claims differ for exactly one reason: the
  injected `7`.
- **T4.2** — Revert ⟨Task 1's insertion⟩ → the assign-group runs → `sections()` runs → headers
  render as `1 Title` / `1.1 Sub` → ⟨T4.2's "no section number precedes any header title"
  assertion⟩ **RED**. **A second, fully independent discriminator for the same hunk, on a surface
  that involves no injected order at all** (**D2**). Keeping both means T4.1's RED cannot be
  explained away as an order-injection bug in P5.
- **T4.3** — **`accepted-untested` as a binding; kept as a characterization tripwire.** There is
  no hunk in P6 — or anywhere in this epic — whose revert reddens it: P3's audit established that
  registration is **already unconditional today** (`quarto-init/metainit.lua:10`, inside
  `quarto_init_filters`, upstream of the gate), and Finding 1 established that three of the four
  built-in mechanisms have **no registration function at all** (`theorem_types` is a static table;
  `eq`/`sec` are hardcoded literals). The only hypothetical revert is "move
  `initialize_custom_crossref_categories` inside `quarto_crossref_filters`," which nobody plans.
  Keep the row anyway — it is nearly free (one probe in a run the matrix already performs) and it
  is the **only** per-mechanism assertion in the epic that all four survive external mode. Label
  it in the test as a tripwire, not as coverage.
- **T4.4** — Revert ⟨P5's `tbl.order` post-construction assignment for **Theorem**⟩ →
  `theorem.lua:278`'s `if order == nil then return el end` early-returns → no caption prefix →
  ⟨T4.4's `Theorem`+NBSP+`5` assertion⟩ **RED**. **That hunk belongs to P5.** Note the asymmetry
  with Callout that P3's companion surfaced: Theorem's site is a clean `order == nil` guard, so
  its revert is a silent missing prefix; Callout's has **no guard** and its revert is a render
  abort. Two types, two RED shapes, one mechanism.
- **T4.4 (what it does *not* discriminate)** — Revert ⟨P3's **A2**,
  `floatreftarget.lua:196`, or **A6**, `modules/callouts.lua:22`⟩ → **no effect on any row in this
  task.** Under `enable-crossref` unset (⇒ true) with `crossref-numbering: external`, the old
  expression `not param("enable-crossref", true)` is already false, so the decoration runs either
  way. P3's companion proved this and pinned the only cell that discriminates A2 — its Task 7 row
  **M4** (`A=false, B=external`). **Stated explicitly so no row here pretends to bind A2/A3/A6.**
- **T4.5** — Revert ⟨Task 1's insertion⟩ → all three numbers revert to Q1's own counts
  (`fig`→1, `thm`→1, `nte`→1) → ⟨T4.5's three-number assertion⟩ **RED** three times over.

### Refactor-induced vacuity check

- **"Numbers are absent/correct under external mode" is satisfied by a document with nothing to
  number.** Addressed the way P3's companion addressed the identical trap: T4.2 asserts its header
  **titles** are present, and T4.1/T4.4/T4.5 assert **caption body text** alongside each prefix.
  An empty, crashed, or node-dropping render fails every row.
- **The number itself is the collapsed discriminator, and D1 is the fix.** A golden that extracts
  semantic text and compares Q1-vs-Q2, or HTML-vs-docx, is **non-discriminating for numbering
  suppression** whenever Q1's and Q2's counts coincide — which, verified, is *every* flat
  single-file fixture, because both sides keep a per-ref-type counter incrementing from 1 in
  document order (`crossref_index.rs:250-311` vs. `crossref/index.lua:47-57`). T4.1/T4.5 move the
  discriminator onto injected orders (`7`/`5`/`3`) that Q1 would never compute; T4.2 moves it onto
  a surface with no order at all. **The `1`-valued parity form is retained only as shape/gating —
  in Task 5.**
- **`Figure` vs `Figure`+NBSP+`7:`.** Assert the digit and the delimiter. The bare word `Figure`
  matches the caption body of a document whose prefix is absent — a fully collapsed discriminator,
  and the exact trap P3's companion calls out for its own matrix.
- **One `#[test]` per row, not one loop over five params.** A loop hides rows 2-5 behind row 1's
  failure; nextest runs each `#[test]` in its own process, so five functions over a shared helper
  costs nothing and attributes cleanly.
- **T4.3's expected values cannot go RED for any P6 change — and that is recorded above rather
  than dressed up.** A row whose assertion is invariant across every state the plan can produce is
  theater if presented as coverage; it is useful as a tripwire if labeled as one. Labeled.

---

## Task 5: Figure / theorem / callout number-parity goldens — schedulable without P7

**Scope.** P6's last open checklist item. Two things that must not be conflated: a **parity**
assertion (the docx leg's numbers equal the HTML leg's numbers for the same source) and a
**discrimination** assertion (Q1's numbering was suppressed). Per **D1** they cannot be the same
test. Task 4 owns discrimination. This task owns parity — as shape/gating — plus Finding 3's
Proof case and Finding 5's cross-reference.

**Files.** **No production file changes — this task is a test only.**
- `crates/quarto-core/tests/integration/crossref_number_parity.rs` (new) + its `pub mod`.
- Committed artifacts: `insta` snapshots of **extracted semantic text**, not raw OOXML — per the
  design doc §11's resolved golden strategy ("extract semantic text … to avoid Pandoc-version
  byte-noise, and are committed as insta snapshots").
- Fixture: `parity-all-types.qmd` — one `#fig-`, one `#thm-`, one `#nte-` (**D3**), one `.proof`,
  no injected-order overrides (natural document order).

**Acceptance criterion.**
1. Rendering `parity-all-types.qmd` twice — once through the HTML leg, once through the Pandoc
   `--to docx` leg — yields the **same number** for each of the three labeled targets.
2. The Proof renders with its `proof.lua` label and **no** number (**D4**'s companion assertion
   included).
3. Both snapshots are committed, and the file's module doc states that this task's rows are
   **shape/gating for suppression** and that the discriminating rows live in Task 4 — so a future
   reader does not treat a green parity snapshot as evidence suppression works.

**Prerequisite — and this is where `94f060ae9`'s unschedulable ordering gets resolved, not
reintroduced.** P6's plan offers two options; **take option (a)**, with one correction:

- The blocker is real: P7 owns `cargo xtask capture-pandoc-goldens`, and the epic runs P6 before
  P7, so P6 cannot inherit that harness.
- **The plan's option (a) says "assert at the wire-AST/pandoc-JSON level." Taken literally that
  does not work**, and an implementer would discover it the hard way: `pandoc -f json -t json -L
  main.lua` sets `FORMAT` to `json`, so `floatreftarget.lua`'s **docx** renderer (registration at
  `:666`) is never selected — the generic fallback at `:183` is, and the decoration path under
  test differs. Verified by reading the 11 `add_renderer("FloatRefTarget", …)` registrations P3's
  audit enumerates.
- **Corrected option (a):** render `--to docx` for real, and extract text with a **narrow,
  P6-local extractor** rather than P7's harness. Two dependency-free choices, in preference order:
  (i) a second `pandoc` invocation, `pandoc -f docx -t plain`, using the same binary the L gate
  already requires — zero new crate deps; (ii) unzip `word/document.xml` and collect `<w:t>` runs
  — which is the shape **P3's companion Task 7 already specs**, so if P3 lands a helper for it,
  **reuse that helper instead of writing a second one**. (`zip` is present in `Cargo.lock`
  transitively but is **not** a direct dependency of `quarto-core`; choice (i) avoids adding one.)
- **Register the fixtures for P7.** P6's snapshots stay P6's; P7's `capture-pandoc-goldens` should
  re-capture the same fixtures through its own harness when it lands. Name that hand-off in Task 6's
  bookkeeping so the two golden families are not silently duplicated.

Also requires everything Task 4 requires, plus **P1's HTML leg unchanged** (for the parity half).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T5.1 | L | the docx leg end-to-end: Task 1's param → P4's transport → P5's shim (all three types) → Q1's three decoration paths | `parity-all-types.qmd --to docx` → extract text → `insta` snapshot → assert `Figure`+NBSP+`1:`, `Theorem`+NBSP+`1`, `Note`+NBSP+`1:` | environment only | **shape/gating — see below** |
| T5.2 | I | Q2's native HTML leg (`render_document_to_file`, realistic config per CLAUDE.md's end-to-end rule) | same fixture → HTML → extract the three numbers → assert they equal T5.1's | none | `crossref_index.rs`'s `index_custom_target` order write |
| T5.3 | L+I | the two legs jointly | assert the **number triple** extracted from T5.1 equals the triple from T5.2, element-wise | environment only | P5's order assignment (cross-plan) |
| T5.4 | L | P5's Route-R `Proof` + `proof.lua`'s registered renderer | same render → assert the proof's `proof_types` label ("Proof") **and** its body text are present, and that **no digit** follows the label | environment only | **none — Finding 3 is a deliberate absence**; see below |
| T5.5 | — | Finding 5's Tabset-containing-subfloat nesting | **cross-reference only — no test here.** P5 owns this fixture (its round-4 traversal sub-finding names "a `FloatRefTarget` inside a `Callout`'s `content` slot, exactly P6 Finding 5's fixture"); P6's own nesting coverage is **T2.4**. | — | P5's traversal direction |

**Revert hunks, stated exactly:**

- **T5.1** — Revert ⟨Task 1's param insert⟩ → **no change to the snapshot**: Q1's own counters
  produce `1`/`1`/`1` for this fixture, identical to Q2's. **T5.1 has no suppression binding, by
  construction.** What it *does* bind: revert ⟨P5's order assignment for any of the three types⟩ →
  that type's prefix disappears (or, for Callout, the render aborts — P3's companion Finding #2) →
  T5.1 **RED**. So T5.1 is a **P5 cross-plan guard plus a P6 shape/gating row**, and it is
  labeled as such in the test's module doc (acceptance criterion 3).
- **T5.2** — Revert ⟨the `plain_data.order` write-back in
  `crates/quarto-core/src/transforms/crossref_index.rs:285-296` (the `obj.insert("order", …)`
  block)⟩ → the HTML leg's `CrossrefRender` has no order to consume → ⟨T5.2's number assertions⟩
  **RED**. A genuine local binding, of pre-existing Q2 code — a negative-space guard, same class
  as Task 3's.
- **T5.3** — Revert ⟨either leg's order source⟩ → the triples diverge → **RED**. This is the row
  that expresses the epic's Definition-of-done clause ("crossref/callout/theorem **numbers**
  computed once and identical across HTML and Pandoc formats") as an assertion. It is worth having
  for that reason alone, independent of its weak suppression discrimination.
- **T5.4** — **`accepted-untested` for the absence itself; bound for the exercise.** There is no
  hunk whose revert makes a Proof *acquire* a number: Q2 deliberately writes no `ref_type`
  (`transforms/proof.rs:148`, "Intentionally no `ref_type` / `kind`"), so
  `has_crossref_plain_data` returns false and the indexer skips it; and Q1's `proof.lua` renderer
  never reads `.order` even when `crossref_theorems` assigns one (P3's audit, independently
  confirmed). **Both sides agree, so "no number" is over-determined and unrevertable.** What *is*
  bound is **D4**'s companion: revert ⟨P5's `Proof` dispatch arm, or the `plain_data.type` field
  P5 filed with P2 (P2 Task 3)⟩ → either the unwrap path fires (no "Proof" label) or
  `proof.lua:81`'s `proof_types[proof_tbl.type:lower()]` crashes on nil → T5.4's label assertion
  **RED**. That is the "the path was actually exercised" half, and without it T5.4's no-digit
  assertion passes on a document that never rendered a Proof at all.

### Refactor-induced vacuity check

- **T5.1 is the worst case in this file and the answer is written into the test, not hedged in
  prose.** A parity golden extracting `Figure 1:` **survives the revert of Task 1's hunk**, for
  the reason **D1** establishes: Q1 and Q2 both count from 1 in document order. Per the
  discipline, the collapsed value is kept **only for shape/gating** and the discriminator is moved
  to a surface that still differs — Task 4's `7`/`5`/`3` injected orders (T4.1, T4.5) and Task 4's
  order-free section-number surface (T4.2). Acceptance criterion 3 requires the test file to say
  this in its own module doc, because a snapshot named `..._parity` reads like a suppression gate
  to anyone who has not read this section.
- **No fixture in P6's scope naturally discriminates suppression.** Checked and logged under
  **D1**: `number-offset` is decided out (design §11), appendix/chapters numbering has no live Q2
  input (`in_appendix: false, // deferred`), Proof is over-determined, and the duplicate-id case
  would make a diagnostic path the discriminator. **The discriminating fixture is synthetic by
  necessity** — an injected order Q1 would not compute — and that is T4.1's, not T5.1's.
- **HTML-vs-docx parity is a weaker claim than it sounds, and the epic already knows it.** Design
  doc §12 and the epic's Definition of done both narrowed the promise to *numbers*, not
  presentation (`bd-wqdi1pd2`): the two legs legitimately differ on prefixes, `title-delim`, and
  `ref-hyperlink`, because Q2's `crossref_render.rs:28-31` hard-codes English defaults while Q1's
  `crossrefOption()` honors the user's metadata. **T5.3 must therefore compare extracted
  *numbers*, not extracted prefix strings** — comparing `"Figure 1:"` to `"Figure 1:"` would pass
  today and redden the moment any user sets `fig-prefix`, on a divergence the epic has frozen as
  accepted. Extract the digits.

---

## Task 6: Plan + design-doc bookkeeping (grouped — one task, not five)

**Scope.** Reconcile P6's checklist against what actually landed, correct the citation drift this
pass found, and record the two hand-offs (P5's Callout implementation; P7's golden re-capture).
Documentation only. Grouped per the instruction to group same-shape mechanical items.

**Files** (all q2, all `claude-notes/`):
- `claude-notes/plans/2026-08-20-pandoc-hybrid-P6-numbering-wiring.md`:
  - Check off the four open checklist items as their tasks land (Callout-to-R → Task 2;
    `crossref.custom` passthrough test → Task 3; `crossref-numbering: external` → Task 1;
    number-parity golden → Tasks 4+5).
  - **Citation corrections** (drift found 2026-09-18, this pass; see the anchor inventory):
    `crossref_callouts()` is `customnodes/callout.lua:469-482`, not `467-480`; the Theorem
    renderer's `local order = thm.order` is `customnodes/theorem.lua:222`, not `:220` (P5 already
    corrected its own copy, P6's was not updated); `initialize_custom_crossref_categories` is
    `crossref/custom.lua:6-157` (the file is 157 lines, so `6-158` overruns EOF);
    `indexNextChapter` is `crossref/index.lua:31-43`, not `31-41`.
  - Record the schedulability resolution for the number-parity golden — **option (a), corrected**:
    a real `--to docx` render with a P6-local text extractor, *not* a `-t json` round-trip (see
    Task 5's Prerequisite for why `-t json` selects the wrong renderer). `94f060ae9`'s ordering
    problem is resolved, not deferred.
  - **Re-scope Finding 5's subfloat paragraph to "dormant" (added 2026-09-18, decided with
    Gordon).** Finding 5 currently claims nested subfloats inside a Route-R Tabset "get the same
    treatment for free" because `prependSubrefNumber` reads `.order` generically. That branch is
    reachable only for a float Q1 recognizes as a *subfloat*, which requires a `parent`, and Q2
    has no subfloat producer: `CrossrefEntry.parent` (`crossref/index.rs:97`) is only ever
    assigned `None` (`crossref_index.rs:304`) and has zero readers. Rewrite the paragraph to the
    same verdict Finding 5 already reaches for `in_appendix` one paragraph earlier — **confirmed
    dormant, no live bug exists** — and reference **`bd-plcqhfcn`** (filed 2026-09-18, parented to
    the crossref epic `bd-jsbg`) plus the pre-existing deferral in
    `claude-notes/plans/2026-04-15-crossref-design.md` (its lines 77 and 405). **Do not** add
    subfloat support to P6's checklist; it is out-of-plan work per CLAUDE.md's braid-vs-plans
    rule. Keep Finding 5's *request* for the Tabset-containing-figure fixture — T2.4 still
    asserts top-level numbering of a nested figure, which remains worth asserting; only the
    "subfloat lettering" reading of it is withdrawn.
- `claude-notes/plans/2026-08-20-pandoc-hybrid-P5-lua-shim.md`: P5's checklist already carries
  "Route-R reconstruction … for R-types (Callout, …)"; add a pointer that Task 2 of this file is
  the Callout half and that P5's `fail()`-fallback warning is its companion.
- **Do not edit the design doc.** Every §3/§4/§9 correction P6 was responsible for has already
  landed (`8e9e546b4`, `cee1a6a31`, `5a872ed17`), and the §12 Callout-unregistered-category bullet
  landed in round 4. Nothing in this pass reopens a frozen decision.

**Acceptance criterion.** `grep -n 'callout.lua:467\|theorem.lua:220\|custom.lua:6-158\|index.lua:31-41'
claude-notes/plans/2026-08-20-pandoc-hybrid-P6-numbering-wiring.md` returns nothing; each of P6's
four open `- [ ]` items is either `- [x]` with a commit reference or annotated with the task that
owns it; the plan states the golden's extraction mechanism in one sentence.

**Prerequisite.** None for the citation half (schedulable immediately). The checkbox half follows
Tasks 1-5.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T6.1 | — | (none) | — | — | `accepted-untested` — see below |

**Revert hunks, stated exactly:**

- **T6.1** — **`accepted-untested`: plan-file prose has no machine-checkable surface in this
  repo.** The rationale is not "docs don't need tests": the *purpose* of correcting a citation is
  to keep a future reader pointed at the right line, and the behavior those lines describe is
  bound elsewhere — `crossref_callouts()` by **T2.2** (its absence under external mode is why an
  unlabeled callout stays unnumbered), `theorem.lua:222`'s order read by **T4.4**,
  `custom.lua`'s registration by **T3.2**, and `indexNextChapter`/`startAppendix` by nothing at
  all, deliberately (Finding 5: no live Q2 input). Adding a `cargo xtask lint` rule for plan-file
  citations is out of P6's scope and is not proposed here. This is the same verdict P3's
  companion reached for its own Task 1.

---

## Missing-test pass

Every load-bearing branch and structural contract in P6, with either a bound seam or an explicit
`accepted-untested: <rationale>`. Silent omission would read as "covered."

1. **The numbering-suppression gate's negative case — "Q1's numbering is actually suppressed," as
   distinct from "the final numbers are right."** **Bound, twice, and this is the most important
   verdict in this file.** These two claims come apart for exactly one reason (**D1**: both sides
   count per-ref-type from 1 in document order, so they agree on every flat fixture), and P6 must
   not leave the distinction implicit. Bound at **T4.1** (injected `order = 7` → `Figure 7:` under
   external; `Figure 1:` if Task 1's hunk is reverted) and independently at **T4.2** (the
   order-free surface: `number-sections: true` yields *no* section numbers under external mode,
   and numbered headers if the hunk is reverted). Two discriminators, one hunk, two unrelated
   surfaces — so a RED is attributable.

2. **`sections.lua`'s collateral suppression (design doc §11).** **Bound at T4.2, as a labeled
   accepted-divergence golden — and this is a deliberate divergence from P3's companion, which
   logged the same item `accepted-untested`.** P3's three reasons were: (a) it needs P4's transport,
   which P3 cannot reach; (b) it would pin Q1-Lua behavior Q2's own renderer does not match,
   inviting the wrong fix; (c) P7 owns the fixture set. For P6 the calculus differs on all three:
   (a) P6 runs after P4/P5, so transport exists; (b) the golden is *labeled as a divergence*, the
   pattern P7 already uses for mermaid, and its header comment names `bd-5aklrxgi` so the wrong
   fix is signposted against; (c) P6 needs the surface anyway as its suppression discriminator, and
   Task 6 hands the fixture to P7 for re-capture. **The frozen decision not to fix §11 is
   untouched** — capturing a loss is not fixing it. If Gordon prefers P3's verdict to stand
   epic-wide, T4.2 drops to `accepted-untested` and T4.1 becomes P6's only suppression
   discriminator, which is thinner but not unworkable.

3. **Finding 2's Q2-only-category / `Promised`-prefix gap.** **`accepted-untested` in P6; the
   warning is P5's, and the limitation is a design-doc entry, not a golden.** Working through the
   parent's question: the *warning* belongs to **P5** — its round-4 compounding finding resolves
   the fallback to "keep it, but make it loud," emits a `Q-<pandoc>-*` warning at the fallback
   through P4 Task 10's stderr channel, and names the fixture (`callout-foreign-category.qmd`) in
   P5's own Q2-only error-path test tier. P6 must not duplicate it, and per **D3** must keep that
   `ref_type` shape *out* of its numbering fixtures, because a prefix-absent assertion there is a
   false RED that reads as a false confirmation. The *limitation* is design doc §12's own bullet
   (added round 4), and is **not** asserted as a labeled golden here: the dangling-reference half
   of the consequence ("a `@nte-setup` reference renders `Note 3` while the callout is unnumbered")
   is produced by P5's Route-N `CrossrefResolvedRef` path and its fixture is P5's. **Rationale for
   the `accepted-untested`:** the only P6-side surface would be a golden of a known-wrong output,
   duplicating P5's error-path tier, and P6 Finding 2 explicitly closed this as "noting it here
   only so a future reader doesn't mistake it for something Problem C needs to solve."

4. **The four built-in-category mechanisms, per mechanism.** **Bound at T4.3 as a tripwire, not as
   coverage — and labeled that way in the test.** Per-mechanism verdicts:
   `crossref.categories.all` (the only one with a registration function, `add_crossref_category`
   at `mainstateinit.lua:133`) — asserted via `by_ref_type["fig"]`; `theorem_types`
   (`customnodes/theorem.lua:7-53`, static table, **no registration function**) — asserted via
   `theorem_types["thm"]`; bespoke `eq` and bespoke `sec` (hardcoded literals in
   `valid_ref_types()`, `crossref/refs.lua:209-210`) — asserted via `valid_ref_types()` membership.
   **No mechanism has a revertable hunk**, because P3 established registration is already
   unconditional and Finding 1 established three of the four have nothing to register. T4.3 is
   nearly free and is the only per-mechanism assertion anywhere in the epic; keeping it labeled is
   the honest middle between deleting it and overselling it.

5. **Finding 3's deliberately-unnumbered Proof.** **Bound at T5.4 — but only its
   *exercised* half.** The no-number assertion is over-determined (Q2 writes no `ref_type`,
   `proof.rs:148`; Q1's `proof.lua` renderer never reads `.order`) so **no revert makes a Proof
   acquire a number** — `accepted-untested` for the absence, explicitly. The **D4** companion
   assertion (the `proof_types` label and the body text are present) *is* bound, and reddens on
   P5's Proof dispatch arm or on the missing `plain_data.type` field (P2 Task 3) — the latter via
   `proof.lua:81`'s unguarded `proof_types[proof_tbl.type:lower()]`, a crash P5 flagged as a
   confirmed, not conditional, extension request.

6. **Finding 5's Tabset-containing-subfloat fixture — who owns it, and can it exercise what it
   claims?** **One artifact, owned by P5; P6's own nesting coverage is T2.4; and the fixture cannot
   exercise subfloat numbering at all today** — see Findings for Gordon #2. P5's round-4 traversal
   sub-finding already names the fixture in P5's terms ("a `FloatRefTarget` inside a `Callout`'s
   `content` slot, exactly P6 Finding 5's fixture") and P5's Layer-2 goldens own it. **Do not
   create a second one.** T2.4 is P6's contribution: it asserts both the outer Callout's and the
   inner float's numbers appear in one render, which is the property P6 Finding 5 actually cares
   about (that the shim assigns `.order` uniformly at every nesting depth).

7. **A Q2-only ref-type reaching a *non-Callout* Route-R type under external mode.** P5's
   `fail()`-guard fallback is specified for Callout only; `demo` (Q2's own built-in,
   `registry.rs:104`, absent from all four Q1 mechanisms) could reach a `FloatRefTarget` or a
   Theorem. **`accepted-untested`, and deliberately not opened here:** `demo` nodes are
   `ExampleEmbed`-adjacent, and P1 resolved `ExampleEmbed` entirely upstream in Rust
   (format-parameterized B1) so no `CustomNode("ExampleEmbed")` reaches the shim for Pandoc
   targets. Whether a `#demo-…` id can land on some *other* custom node and reach a Route-R
   constructor is a P5 shim-mechanics question, and P5's own bidirectional-totality Layer-1
   assertion is the mechanical guard for the shape. Recorded so it reads as considered-and-assigned
   rather than missed.

8. **P3's A2 / A3 / A6 render-decoration hunks.** **Not bound by P6, and stated so under T4.4.**
   Under `enable-crossref` unset + `crossref-numbering: external`, the pre-patch expression
   `not param("enable-crossref", true)` is already false, so reverting those hunks changes nothing
   P6 renders. Their only discriminating cell is `A=false, B=external`, which is **P3 Task 7's
   row M4**. Cross-referenced, not duplicated.

9. **Task 1's param reaching the real Pandoc invocation (as opposed to the builder function).**
   **Bound at T1.3**, and required rather than optional: CLAUDE.md's end-to-end rule, and the
   2026-04-20 `CodeHighlightStage` incident (every test green, no rendered document affected),
   make a builder-only test insufficient for a wiring task.

10. **An unknown `crossref-numbering` value on the *Q2* side.** There is no branch to test:
    Task 1's builder emits the literal or omits the key, and P3's Lua compares against
    `"external"` with `"quarto"` as the `param()` default, so any third value behaves as
    `"quarto"` with no diagnostic anywhere. **`accepted-untested`** — adding a diagnostic is new
    functionality and contradicts the epic's governing principle; P3's companion already pinned
    the frozen behavior behaviorally at its M5 row. Recorded so T1.1's green is not read as
    evidence that a typo is caught.

---

## Findings for Gordon

Three items. Each is a place where a test cannot be written the way the plan implies, or where an
expected value cannot discriminate what it exists to guard. **All three were decided by Gordon on
2026-09-18 and are applied** — the callout `order == nil` guard is folded into P3's PR as anchor
A7 (which changes T2.1's RED shape from a crash to a warn-and-skip), P3's checklist item 3 has
formally moved to this plan's Task 4, and Finding 5's subfloat claim is re-scoped to dormant with
the gap tracked as `bd-plcqhfcn`. Nothing on this plan is open.

1. **P6's Callout Route-R work makes a Q1 render *crash* where P3's plan says it warns — and the
   crash is the RED of P6's own primary Callout test.** P3's companion found this first (its
   Finding #2) and logged the callout gate `accepted-untested` pending your call; it lands squarely
   in P6's lap because P6 owns both the reclassification (Finding 4) and the external-mode wiring.
   Verified again this pass: `modules/callouts.lua:17` passes `callout.order` straight into
   `titlePrefix` → `numberOption` → `formatNumberOption`'s `local num = order.order`
   (`crossref/format.lua:124,140`) with **no nil guard on that path** — unlike the float side,
   which guards explicitly at `crossref/tables.lua:229-231`, and unlike Theorem, which guards at
   `customnodes/theorem.lua:278`. So under `crossref-numbering: external`, a `#nte-`-labeled
   callout whose `order` P5's shim failed to inject **fails the render**, and P6's T2.1 reddens as
   an abort rather than a missing string. Two consequences: (a) P6's plan's own Finding 4 framing
   ("silently skips decorating the title, since it's written as a nil-guarded early return, not an
   error") is true of the **gate** at `:22` but false of the **prefix function** at `:6-17`, and
   P6's implementer will read the former and be surprised by the latter; (b) whether that crash is
   acceptable as a "P6 always injects an order" invariant, or whether P3's PR should add the
   missing `order == nil` guard to `callout_title_prefix` — arguably the same
   generally-useful-and-backward-compatible shape as the rest of P3's patch, since the site is
   unreachable today — is your call, not mine. **Question:** accept the crash as an invariant (and
   correct Finding 4's framing for this site), or fold the guard into P3's PR?

   **RESOLVED 2026-09-18, decided with Gordon: fold the guard into P3's PR.** It is anchor **A7**
   there, mirroring `float_title_prefix`'s order-nil guard line for line. Consequences applied in
   this file:
   - **P6's plan's Finding 4 framing needed no weakening — A7 makes it true.** Finding 4 says the
     degradation is "a nil-guarded early return, not an error." That was true of the *gate* at
     `modules/callouts.lua:22` and false of the *prefix function* at `:6-18`; with A7 inserted at
     `:12-15`, it is now true of both, which is why consequence (a) above is resolved by the code
     change rather than by a documentation correction. **A P6 implementer reading Finding 4 will
     no longer be surprised** — the thing Finding 4 describes is what the patched Lua does.
   - **T2.1's revert shape changed** from a render crash to pandoc-exit-0 plus
     `field 'order' is missing from callout` on stderr plus an absent prefix. Its entry above is
     updated, including the `exit == 0` assertion that keeps the RED attributable and would catch
     A7 being dropped on a re-pin.
   - **A7 is a named dependency on P3 Task 3**, not just on "P3's external mode" — stated in
     T2.1's note so the coupling is visible.

2. **P6 Finding 5's conclusion that "nested subfloats inside a Route-R Tabset get the same
   treatment for free" describes a path with no live input — so the fixture it asks for cannot
   exercise the behavior it exists to guard.** Finding 5's argument rests on
   `prependSubrefNumber(caption_content, float.order)`
   (`customnodes/floatreftarget.lua:221`) reading `.order` "generically off whatever table it's
   handed, with no special-casing for nesting depth." Read the surrounding branch and it is
   reached only for a float Q1 recognizes as a *subfloat* — which requires a `parent`. Q2
   hardcodes `parent: None, // subfloats deferred`
   (`crates/quarto-core/src/transforms/crossref_index.rs:304`), exactly as Finding 5 itself
   observes for `in_appendix` two paragraphs earlier. **So no Q2 wire node can carry the
   parent link that selects the subfloat branch, and any Tabset-containing-subfloat golden — P5's
   or P6's — asserts top-level numbering of a nested figure, not subfloat lettering.** That is
   still a worthwhile assertion (it is what T2.4 does), but it is not what Finding 5 says it is.
   This is the same shape as Finding 5's *own* appendix verdict ("confirmed dormant, no live bug
   exists"), and the honest fix is to state the subfloat half as dormant too.

   **RESOLVED 2026-09-18, decided with Gordon: dormant, same as appendix. Tracked as
   `bd-plcqhfcn`.** There is no subfloat producer — confirmed by a third check beyond the two
   above, which is what makes "dormant" the right word rather than "unreachable by accident":
   `CrossrefEntry.parent` is *documented* infrastructure (`crossref/index.rs:97`, "For subfloats:
   the identifier of the parent float. `None` for top-level"), is only ever assigned `None`
   (`crossref_index.rs:304`), and has **zero readers** — grepping `.parent` across
   `crates/quarto-core/src/crossref/` and the crossref transforms returns no consumers at all. So
   the struct anticipates subfloats and nothing populates or reads the field, exactly like the
   `in_appendix: false, // deferred` on the adjacent line. Task 6's plan edit re-scopes Finding
   5's subfloat paragraph accordingly.

   **Prior art, checked before filing anything:** the deferral is already documented — 
   `claude-notes/plans/2026-04-15-crossref-design.md` has two "Subfloats deferred" paragraphs
   (its lines 77 and 405) naming parent/child id assignment, nested numbering ("Figure 1a") and
   `fig.subplots`-style engine output, with `parsefiguredivs.lua:41-60` as the Q1 reference, and
   saying the work "needs its own plan." That plan was never written and no strand existed, so
   `bd-plcqhfcn` is it, parented to **`bd-jsbg`** (the open crossref epic that owns that design
   plan). Also worth knowing for whoever picks it up:
   `claude-notes/designs/float-layout-class-taxonomy.md` already specifies the intended HTML
   surface (its lines 55-56, 74 and 131 — `quarto-subfloat-<ref>`, `quarto-layout-cell-subref`,
   `data-qf-subfloat` keyed on `float.parent_id ~= nil`), so the *presentation* contract is
   designed and it is the *index/numbering* half that is missing.

   **Not this epic's work**, per CLAUDE.md's braid-vs-plans rule: subfloat support is a feature
   none of the eight plans implements, and the Pandoc leg inherits it for free once the wire
   format carries a `parent` (Q1's Lua already handles subfloats). Deliberately **not** added to
   P6's or any plan's checklist. What remains true for P6 is the narrower statement: T2.4's
   Tabset-containing-figure golden asserts **top-level** numbering of a nested figure, which is
   still worth asserting — it just is not the subfloat-lettering assertion Finding 5's wording
   implied.

3. **P3's checklist item 3 has no P3-owned revert hunk and P3's companion recommends moving it to
   P6 — this file assumes that move without it being decided.** P3's companion Finding #3 works it
   through: under `enable-crossref=true, crossref-numbering=external` with an injected order,
   reverting A2 changes nothing and reverting the assign-numbers conjunct yields the *same* number
   for a single-figure fixture. What that golden certifies is P5's passthrough and P6's param —
   i.e. it is P6's acceptance evidence, not P3's binding — and P3's companion asks whether the item
   should move to P6 "which owns the wiring and already lists figure/theorem/callout-number parity
   as its review target." **This file's Task 4 is written as though the answer is yes**: T4.1 is
   exactly the injected-order variant P3's companion specs (`order = 7`, prefix
   `Figure`+NBSP+`7:`), and it is bound to **Task 1's** hunk, not to any P3 hunk. If the item stays
   with P3, T4.1 and P3's Task 8 are the same test written twice against different owners.

   **RESOLVED 2026-09-18: moved to P6, and this file's assumption is now the decided state.**
   P3's Task 8 has been replaced by a pointer section naming **P6 Task 4** as the single owner;
   nothing here needed to change. The deciding reason was the discipline's own rule rather than
   either file's preference — a test belongs where the hunk whose revert reddens it lives, and
   every hunk this golden discriminates is in P5 or P6. Two supporting reasons: P3 is scheduled
   before P4 and could never have run an `L`-tier golden at all, and the epic's Plans table
   already assigns "figure/theorem/callout-number parity" to P6. P6 Task 4 is also the stronger
   specification — its `order = 7`-against-Q1's-`1` fixture discriminates "Q2's number" from "a
   number", which P3's single-figure fixture could not.

**Not findings, recorded so they are not rediscovered:**
- The quarto-cli checkout has moved to `v1.11.5-1-g83d48d8e8`, but `git diff v1.11.3..HEAD` is
  **empty** for all thirteen files P6 cites, so every line number in this document is valid for
  both the pinned tag and today's upstream HEAD.
- `resources/pandoc-filters/` **does not exist yet** (`ls resources/` shows `pandoc/`, which is
  `highlight-styles` only). Every L row here is inert until P4 Task 1/2 lands.
- The `zip` crate is in `Cargo.lock` transitively but is **not** a direct dependency of
  `quarto-core`; Task 5 prefers a `pandoc -f docx -t plain` round-trip precisely to avoid adding
  one, and defers to P3's Task 7 helper if that lands first.
- P5's companion file (`2026-09-18-pandoc-hybrid-P5-implementation.md`) was not on disk when this
  file was first written, so its prerequisites were named by capability only. **Resolved
  2026-09-18:** the relevant P5 tasks are **Task 3** (Route R for Callout and Tabset) and
  **Task 2** (the shared post-construction `order` assignment, `tbl.order = …` on the
  constructor's second return value — P6's hunk **H2b** is the Callout instance of it). P5's
  **Task 1** owns the shim body and the `L`-tier capture harness every `L` row here runs through.
