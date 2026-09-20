# P5 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-18
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P5-lua-shim.md`](2026-08-20-pandoc-hybrid-P5-lua-shim.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md) (§3, §4, §12)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Sibling companions:** [`2026-09-18-pandoc-hybrid-P4-implementation.md`](2026-09-18-pandoc-hybrid-P4-implementation.md) (the shim's loading
mechanism, the `L` tier, `run_main_lua`), [`2026-09-18-pandoc-hybrid-P2-implementation.md`](2026-09-18-pandoc-hybrid-P2-implementation.md)
(the schema artifact this file's field-maps and totality test consume)
**Depends on:** P1, P2, P4 (per the epic's graph)
**Status:** **Complete, including post-review fixes.** All eight tasks implemented and
independently verified on `braid/pandoc-hybrid-p5-lua-shim` (commits `1b4596c72`, `79acc955e`,
`6b4e018ac`, `3cc1870a3`, `2328a638e`, `9118e64d1`, `afb28cbf6`, `fe93e21bc` — Tasks 1–8
respectively). A whole-branch `/code-review` against `MERGE_BASE 8e29267e7` then found 9 findings
across Tasks 1–8, all fixed with the same TDD discipline and committed in 6 groups: `9fc901fc4`
(two crash bugs: duplicate-label Equation, foreign-ref-type Theorem), `4b6d5831a` (Theorem/Proof
attribute preservation), `70bb4c41d` (Callout `collapse` translation), `2a11e0c0c` (resolved-ref
`cite_prefix`), `127a82dfd` (test-integrity: goldens leak check, a missing positive assertion, and
a new zero-wire-node regression test), `55ed915c2` (derive the shim's `routes` table from
`route_handlers` instead of hand-maintaining both). Per-task and per-finding details, corrections,
and full verification records live in the SDD ledger at
`.superpowers/sdd/2026-09-18-pandoc-hybrid-P5-implementation/progress.md` (gitignored, session
scratch); the shape plan's own Coarse checklist has been reconciled against this file's Task 1–8
completion. Full `cargo nextest run --workspace` at this phase boundary: 14102/14102 pass, 0
failures. Branch not yet pushed — awaiting explicit push approval per repo `GIT PUSH POLICY`.

This file converts P5's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P5 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P5 + the design doc; where this file and the plan disagree, this file wins.

**Anchors below.** Lua citations are against the materialized `quarto-cli` tag **`v1.11.3`** tree
(`git archive v1.11.3 src/resources/filters src/resources/pandoc/datadir`). The local `quarto-cli`
worktree is `v1.11.5-1-g83d48d8e8`, so `v1.11.3` must be read via `git show`/`git archive`, **not**
from the worktree. Rust citations are against this q2 worktree. Claims marked **(measured)** were
reproduced by running `pandoc 3.8.1`.

---

## Tiers used in this file

| Tier | What it is | Where it lives | How it runs |
|---|---|---|---|
| **`U`** | Rust unit test | `#[test]` in a `mod tests` inside the crate under test | `cargo nextest run -p <crate>` |
| **`I`** | Rust integration test, in-process, no external binary | `crates/<crate>/tests/integration/<name>.rs`, registered `pub mod <name>;` in that crate's `tests/integration/main.rs` | `cargo nextest run -p quarto-core` |
| **`L`** | Lua/pandoc integration test — a **real** `pandoc` subprocess. Two shapes, both real: **`L`(chain)** = `pandoc -f json -t docx --data-dir … -L main.lua [-L probe.lua]` against the materialized `v1.11.3` tree; **`L`(lua)** = `pandoc lua <script>` running the shim file's own pure helpers in pandoc's Lua 5.4 with the real `pandoc` module | same file layout as `I`; gated (see below) | `cargo nextest run -p quarto-core` |
| **`G`** | Dev-only golden capture needing a real Q1 `quarto` binary and/or `external-sources/` | not in CI (CLAUDE.md External Sources Policy) | local/dev only |

**Per `.claude/rules/integration-tests.md`, never add a top-level `crates/<crate>/tests/<name>.rs`.**
One `integration` binary per crate. `crates/quarto-core/tests/integration/main.rs` has 96 `pub mod`
entries today; P5's two new files are appended alphabetically between
`page_navigation_pipeline` (`main.rs:68`) and `pass1_engine_resolution_pipeline` (`main.rs:69`),
next to P4's `pandoc_transport`: `pandoc_shim`, `pandoc_shim_goldens`, `pandoc_transport`.

### `L`-tier gate policy (inherited from P4, and its skipping stays visible)

P5 **reuses P4 Task 2's gate wholesale** rather than inventing a second one: `assert_pandoc_available()`
**panics** (it does not return `false`), and the version gate below P4's floor also panics naming
the floor. There is deliberately no `QUARTO_TEST_PANDOC=1`-style opt-in, because an opt-in gate is
a skip by default.

**The one thing P5 must actively do:** P4 T2.6 asserts the number of `L`-marked `#[test]` functions
equals a hardcoded `L_TIER_TEST_COUNT`. **Every task below that adds an `L` test must bump that
constant in the same commit**, and the acceptance criterion of each such task says so. That census
is the mechanism by which an `L` test silently converted to a skip-shaped body, or deleted, reddens
— and it is the reason a silently-skipping P5 test cannot read as green.

---

## What P5 inherits, and what it must not re-assert

P4 ships **before** P5 and already binds the shim's *position and shape*; P5 must not duplicate
those assertions, and must not break them:

- **P4 T8.1** — `init_idx < shim_idx < normalize_idx` on the embedded, patched `main.lua`
  (`main.lua:712` / the spliced `tappend` / `main.lua:713`). This is the *entire* mitigation for the
  class-keyed-dispatcher collision; P5 adds only its **effect** (Task 6 T6.6/T6.7), which P4
  explicitly deferred (`seam deferred until P5's shim implementation`, P4 missing-test item 10).
- **P4 T8.2** — the `import("./quarto2-shim.lua")` line sits after `import("./ast/customnodes.lua")`
  (`main.lua:18`).
- **P4 T8.3** — the shim file contains no `traverse = 'topdown'` and the group carries `name`,
  `filter`, `traverser = 'jog'`. **P5 replaces the file's body and must keep this test green.**
- **P4 T1.4 / T1.5 / T8.6** — the repo-level `vendored-pandoc-filters` lint rule asserting every
  file listed under `resources/pandoc-filters/README.md`'s `## Ours vs. pinned` exists. `quarto2-shim.lua`
  is on that list, so P5's file is already protected against a delete-and-recopy re-vendor.
- **P4 Task 2 `run_main_lua(ast_json, to_format, params_blob_json, out) -> PandocRunOutcome`** — the
  `L`-tier transport helper. P5 extends it (Task 1) rather than writing a second one.
- **P4 Task 9 `render_qmd_to_pandoc`** — the production entry point P5's one end-to-end test drives.
- **P4 Task 10 `classify_pandoc_stderr` + verbatim nonzero-exit passthrough + temp-JSON retention** —
  the channel through which *all three* of P5's error paths surface. P5 owns the fixtures; P4 owns
  the channel.
- **P2 Task 1** — `crates/quarto-pandoc-types/resources/custom-node-schema.json` +
  `crates/quarto-pandoc-types/src/custom_node_schema.rs`. Task 7's bidirectional-totality test reads
  this artifact; without it there is nothing to be total against.
- **P2 Task 3** — lands `Proof.plain_data.type` and `CrossrefResolvedRef.plain_data.{cite_mode,
  label_upper}`. **P5's checklist items "file the confirmed Proof extension request with P2" and
  "file the confirmed `CrossrefResolvedRef` extension request with P2" are therefore already
  discharged** — P2 Task 3 exists and names both. They are bookkeeping, not tasks here. `cite_prefix`
  is *not* landed (P2 Finding 3: a citation prefix is `Inlines` and `plain_data` is contractually
  AST-free) — see Task 4's scope note.

---

## Discriminating fixture values (the vacuity-critical ones, stated once)

Every value here was verified against the `v1.11.3` tree today. Tasks reference this section by
number rather than restating it.

### D1 — The Callout `fail()`-guard fixture **must** be `::: {#thm-x .callout-note}`

The guard is `crossref.categories.by_ref_type[ref_type] ~= nil`, not `valid_ref_types()`.
Verified sets:

- `by_ref_type` keys (`mainstateinit.lua`, the `ref_type =` entries at `:40,48,56,64,70,76,82,88,96,102,108`,
  indexed at `:128`): `fig, tbl, lst, nte, wrn, cau, tip, imp, prf, rem, sol` — **11**.
- `valid_ref_types()` (`crossref/refs.lua:198-212`) = `tkeys(theorem_types)` ∪ `keys(by_ref_type)`
  ∪ `{"eq", "sec"}`. `theorem_types` (`customnodes/theorem.lua:7-53`) = `thm, lem, cor, prp, cnj,
  def, exm, exr, alg` — **9**.
- Superset-only difference: **`{thm, lem, cor, prp, cnj, def, exm, exr, alg, eq, sec}`**.

**Stronger than the plan states, and it makes the fixture mandatory rather than merely preferable.**
`decorate_callout_title_with_crossref` (`modules/callouts.lua:20-49`) **already** filters on
`is_valid_ref_type(refType(callout.attr.identifier))` and early-returns at `:33-35` when it is
false; `callout_title_prefix` (`:6-18`) then `fail()`s at `:9` only when
`by_ref_type[refType(id)]` is nil. So the set of `ref_type`s that can reach `fail()` **is exactly
the superset-only difference above**. Consequences:

- A fixture in the intersection (`::: {#nte-x .callout-note}`) never reaches `fail()` under either
  predicate — it renders identically whether the guard is right or wrong. **Non-discriminating.**
- A fixture whose `ref_type` is outside `valid_ref_types()` entirely (a Q2-only category, a
  `crossref.ids` Promised prefix) early-returns at `:33-35` — also never reaches `fail()`, also
  **non-discriminating** for the guard (though it *is* the design §12 silent-unnumbering case the
  fallback warning exists for, bound separately at T6.4).
- `::: {#thm-x .callout-note}` is reachable for the reason P5 states: Q2's classifier splits on the
  first hyphen with no regard for the div's classes (`crossref/registry.rs:178-181`,
  `classify_cite_id`), so the node carries `ref_type: "thm"`, which is in `valid_ref_types()` and
  not in `by_ref_type`. **The only discriminating shape.**

### D2 — Splice position is the discriminator, and it is two-sided

The wrapper retains its classes: the live wire-format producer for the Pandoc leg is
`stream_write_custom_block` (`crates/pampa/src/writers/json.rs:3684`), which at `:3717-3718` does
`classes.insert(0, "__quarto_custom_node")` — a **prepend** onto `custom.attr.1`, the original
Div's own class list (`callout.rs:37` doc comment: "`attr`: Original Div attributes";
`CustomNode::new("Callout", div.attr.clone(), …)` at `callout.rs:299`). `json.rs:1466`
(`write_custom_block`) is a non-live twin reached only from the HTML writer's source-map path;
both twins prepend identically.

Both sides of the position are bindable, and both fail *loudly*:

- **Move the shim after `quarto_normalize_filters`** → `parse_extended_nodes()`
  (`normalize/astpipeline.lua:331`, inside the `normalize-combined-1` entry `:327-346`) dispatches on
  the class list (`ast/parse.lua:6-15`, first match wins) and `Callout.parse()`
  (`customnodes/callout.lua:36,46-70`) consumes the wire Div, rebuilding a Q1 Callout with **no**
  `order`. For a *numbered* callout the render then crashes: `callout_title_prefix` →
  `titlePrefix` (`crossref/format.lua:17-33`) → `numberOption("nte", nil)` (`:124-135`) →
  `formatNumberOption` → `local num = order.order` (`:140`) on nil. **Pandoc exit 83.** For an
  *unnumbered* callout it is a silent mis-render (the `data-slot-name` slot Divs surface as
  content). Three registrations collide this way — `Callout` (`callout.lua:36`), `Tabset`
  (`panel-tabset.lua:140`), `ConditionalBlock` (`content-hidden.lua:27`); the other four Route-R/N
  types declare `class_name = {}` (`theorem.lua:67`, `proof.lua:34`, `floatreftarget.lua:76`) or
  have no handler at all.
- **Move the shim before `quarto_init_filters`** → `crossref.options` is still nil
  (`init_crossref_options` is called from `quarto-init/metainit.lua:9`, inside
  `quarto_meta_init()`, the first entry of `quarto_init_filters` at `main.lua:230`), so any Route-N
  call through `crossrefOption` (`crossref/options.lua:17-19`) hits `readOption(nil, …)` →
  `options[name]` on nil (`common/options.lua:15`). **Pandoc exit 83.**

Confirmed there is no params-level backstop: `active-filters.normalization = false` gates only the
single `normalize` entry (`main.lua:263-273`), while `tappend(quarto_normalize_filters,
quarto_ast_pipeline())` (`main.lua:281`) and the `normalize-capture-reader-state` sibling
(`main.lua:275-278`) are unconditional.

### D3 — The `equations.lua` fixture must target docx (or any non-latex/non-typst), never `--to latex`

`renderEquation(eq, label, alt, order)` at `crossref/equations.lua:102-147`:

| branch | lines | reads `order`? |
|---|---|---|
| `isLatexOutput()` | `:105-113` | **no** — native `\begin{equation}…\end{equation}` |
| `isTypstOutput()` | `:115-131` | **no** — `#math.equation(numbering: equation-numbering…)` |
| fallback (docx, pptx, HTML) | `:133-144` | **yes** — `eq.text = eq.text .. " " .. eqNumber(inlinesToString(numberOption("eq", order)))` at `:142`, wrapped `pandoc.Span(eq, pandoc.Attr(label))` at `:143` |

The branch is chosen by pandoc's own `FORMAT` global, not by a param (`_format.lua:34-49`:
`isLatexOutput()` is `FORMAT == "latex" or "beamer" or "pdf"`; `isDocxOutput()` is
`FORMAT == "docx"`). **A golden captured with `-t latex` is vacuous for the `order` behaviour by
construction.** For docx the sub-branch is `eqQquad` (`:134`, `:153-155`) because
`isHtmlOutput()` is false, so the expected value is the Math text with `" \qquad(1)"` appended.

### D4 — `.order` goes on the **second** return value, and the per-type failure shape differs

`quarto.<AstName>(params)` is built at `ast/customnodes.lua:449-459`. `handler.constructor` returns
`(tbl, need_emulation)`:

- `need_emulation ~= false` → `:453` `create_emulated_node(...)` → `:257-264`, whose
  `return result, custom_node_data[id]` is at **`:263`**.
- `need_emulation == false` → `:455-457` `return tbl.__quarto_custom_node, tbl`.

**The `:457` branch *is* taken — by Tabset.** `panel-tabset.lua`'s constructor ends `return custom_data, false`
(`:243`), so **Tabset takes `:457`**. Callout (`callout.lua:112-120`), Theorem
(`theorem.lua:88-92`), Proof (`proof.lua:55-60`) and FloatRefTarget (`floatreftarget.lua:96-108`)
all return a single value and take `:453`. Both branches return two values and `.order` goes on the
second either way, so the *decision* is unaffected — but a test anchored only at `:263` does not
cover Tabset's path, and Tabset's second value carries a metatable whose `__newindex` special-cases
`"tabs"` (`panel-tabset.lua:230-239`).

**What happens when `.order` is missing — i.e. when it was assigned to the scaffold node instead of
the data table. Verified per type, and the three answers are different:**

| Route-R type | nil `order` behaviour | Discriminating assertion |
|---|---|---|
| **Callout** | **hard crash** — `titlePrefix` → `numberOption` → `formatNumberOption`, `order.order` on nil (`crossref/format.lua:140`); reached for docx because `calloutDocx` calls `decorate_callout_title_with_crossref` at `quarto-post/docx.lua:193` | render exits 0 **and** the title carries `Note\u{a0}1:` |
| **Theorem** | **silent, no prefix** — explicit `if order == nil then return el end` at `customnodes/theorem.lua:278-280` | the caption text contains `Theorem 1`. **"the render succeeded" is vacuous here** |
| **FloatRefTarget** | **warn + empty prefix** — `float_title_prefix` (`crossref/tables.lua:223-236`) `warn("field 'order' is missing from float…")` and `return {}` at `:229-232` | the caption text contains `Figure\u{a0}1:` **and** stderr has no `field 'order' is missing` |
| **Proof** | n/a — `proof.lua`'s renderer never reads `order` | nothing to assert; do not assign |
| **Tabset** | n/a — unnumbered | nothing to assert; do not assign |

### D5 — Route N's expected value is `Figure\u{a0}1` exactly, and three separate collapses are live

Construction of the expected string, from `resolveRefs`'s own body (`crossref/refs.lua:8-145`):

1. `refPrefix("fig", upper)` (`crossref/format.lua:66-96`) → reads
   `param("crossref-fig-prefix")` first (P4 Task 5 supplies it), then
   `by_ref_type["fig"].prefix` (`mainstateinit.lua:38` = `"Figure"`), then the bare `type .. "."`.
2. `add_ref_prefix`'s nbsp (`refs.lua:13-19`) — a `local function` **inside** the `Cite` callback,
   therefore **not reachable** from a spliced-in filter and must be reproduced inline. It appends
   `nbspString()` = `pandoc.Str '\u{a0}'` (`common/pandoc.lua:125-127`) unless the category sets
   `space_before_numbering == false` or the target is Typst.
3. `refNumberOption("fig", entry)` (`format.lua:106-121`, called by `resolveRefs` at `refs.lua:112`)
   → `formatNumberOption` → the arabic branch `resolve(tostring(num))` at `format.lua:184`.
4. `refHyperlink()` (`format.lua:102-104`) → wrap in `pandoc.Link(ref, "#"..label, "",
   Attr("", {'quarto-xref'}))` (`refs.lua:117-119`).

The collapses:

- **`"Figure"` does not discriminate a missing number.** The original three-function list
  (`refPrefix`/`crossrefOption`/`refHyperlink`) produces exactly the prefix and nothing else, so a
  `contains("Figure")` assertion is green under the pre-round-4 bug. Assert the **full**
  `Figure\u{a0}1`.
- **`"Figure 1"` with a regular space does not discriminate the missing inlined nbsp.** Assert the
  literal U+00A0 codepoint; do not normalize whitespace, and do not assert through a
  `pandoc -f docx -t plain` round-trip (which may fold it). The nbsp is asserted on the captured
  post-filter AST (Task 8's probe), which preserves codepoints.
- **`contains("Figure\u{a0}1")` does not discriminate the JSON-decoder float trap** (measured,
  pandoc 3.8.1 / Lua 5.4): `pandoc.json.decode('{"order":1}').order` is a **float** `1.0`
  (`math.type` → `"float"`), and `format.lua:184` does `tostring(num)`, so the rendered text becomes
  `Figure\u{a0}1.0` — **which contains `Figure\u{a0}1`**. Q1's own decoder, exposed in `main.lua`'s
  state as `quarto.json.decode` (`init.lua:151` `local json = require '_json'`, published at
  `init.lua:994` `json = json,`), returns an **integer** `1` and renders `1`. So: the shim must
  decode `data-custom-data` with `quarto.json.decode` (or coerce with `math.tointeger`), and the
  assertion must be **exact equality of the stringified link text**, or carry a companion
  `assert(not text:find("1.0", 1, true))`.

Theorem's caption prefix uses a *regular* space, not nbsp: `captionPrefix(name, type, theoremType,
order)` (`crossref/theorems.lua:79-90`) does `title(...)`, `table.insert(prefix, pandoc.Space())`,
then `numberOption` — so its expected value is `Theorem 1`, and `name` appears only as a
parenthetical suffix (`:83-88`), never as a replacement for the label. Callout and FloatRefTarget go
through `titlePrefix` (`format.lua:17-33`), which inserts `nbspString()` and then `titleDelim()`
(default `":"`, `format.lua:35-37`) — expected `Note\u{a0}1:` / `Figure\u{a0}1:`.

### D6 — Layer-1 asserts a per-type **name mapping**, never set-equality

Verified both sides today:

| Q2 `type_name` | Q2 slots (producer) | Q1 `slots` | mapping |
|---|---|---|---|
| `Callout` | `title` (Inlines), `content` (Blocks) — `callout.rs:303,306` | `{ "title", "content" }` — `callout.lua:77` | identity |
| `FloatRefTarget` | `content` (Blocks), `caption_long` (Blocks, opt.), `caption_short` (Inlines, opt.) — `float_ref_target.rs:352-362` | `{ "content", "caption_long", "caption_short" }` — `floatreftarget.lua:94` | identity |
| `Theorem` | `content`, `title` (opt.) — `theorem.rs:298-304` | `{ "div", "name" }` — `theorem.lua:85` | `content`→`div`, `title`→`name` |
| `Proof` | `content`, `title` (opt.) — `proof.rs:153-159` | `{ "div", "name" }` — `proof.lua:52` | `content`→`div`, `title`→`name` |
| `Tabset` | `title-{i}` (Inlines), `content-{i}` (Blocks) — `panel_tabset.rs:288-289` | **no `slots` key at all** — `panel-tabset.lua:138-244` | explicit **N/A**; the constructor takes `params.tabs` |
| `Equation` | `content` (Inlines, the Math) — `equation_label.rs:225-226` | no handler | Route N |
| `CrossrefResolvedRef` | `suffix` (Inlines, opt.) — `crossref_resolve.rs:328-331` | no handler | Route N |

So only Callout and FloatRefTarget match by name ✓, and a set-equality assertion would be RED on
four of seven types while *also* failing to detect a swap (`div`↔`name`).

### D7 — The golden harness cannot detect a 9th wire type, so Task 7's totality test is the only guard

Task 6(a)'s unrecognized-`type_name` path unwraps the scaffold to its slot content and drops the
wrapper. For a text-extracting golden that is a **zero-byte diff**: the words are all still there,
only the semantics (numbering, environment, callout chrome) are gone. So a 9th type shipping
without shim support is invisible to every golden. The bidirectional assertion in Task 7 (T7.3) is
the only mechanical guard, and it must not be deferred.

---

## Task 1: Replace P4's placeholder shim body — recognizer, attr sanitizer, slot collector, route table, and the `L`-tier capture harness

**Scope.** Turn `quarto2-shim.lua` from P4's placeholder into the real dispatcher skeleton: match a
`Div`/`Span` whose class list contains `__quarto_custom_node`, decode its three `data-custom-*`
attributes, collect its named slots, sanitize the attr it hands downstream, and dispatch through a
route table with exactly seven entries (five R, two N) and **no Route-L branch**. Route bodies are
stubs that Tasks 2-5 fill; Task 6 fills the fallback's warning. Also extend P4's harness with the
post-filter AST capture the later tasks assert on.

**Files:**
- `resources/pandoc-filters/filters/quarto2-shim.lua` — **ours, inside the vendored tree** (P4
  Task 8 created the placeholder; the inside-the-tree placement is forced because `import()`
  resolves relative to `PANDOC_SCRIPT_FILE`'s own directory, `main.lua:8-11`). Already listed under
  `resources/pandoc-filters/README.md`'s `## Ours vs. pinned`, so P4's `vendored-pandoc-filters`
  lint rule protects it from a delete-and-recopy re-vendor — **P5's checklist item "confirm the
  shim's own file lives as a sibling to `customnodes/*.lua` under our own tree" is discharged by
  re-running P4's T1.5/T8.6, not by new code.**
- `crates/quarto-core/src/pandoc_filters/harness.rs` — extend (ours, `cfg(any(test, feature =
  "test-harness"))`). Add `run_main_lua_capturing_ast(ast_json, to_format, params_blob_json, out)
  -> (PandocRunOutcome, serde_json::Value)`, which appends one **observer** `-L` probe after
  `main.lua`. Measured (pandoc 3.8.1): a second `-L` sees the first filter's output AST, can write
  it with `pandoc.write(doc, "json")` + `io.open`, and `-t docx` still produces the docx — so
  `FORMAT == "docx"` is preserved for `main.lua` while the post-filter AST stays inspectable. The
  probe is an observer, not a substitute: nothing about the unit under test is mocked.
- `resources/pandoc-filters/filters/quarto2-shim-probe.lua` — **ours**, the observer probe. Must
  also be added to the README's `## Ours vs. pinned` list (it lives in the vendored tree so the
  harness can find it by the same `share_path()` join).
- `crates/quarto-core/tests/integration/pandoc_shim.rs` — new; registered `pub mod pandoc_shim;` in
  `crates/quarto-core/tests/integration/main.rs` (between `page_navigation_pipeline` `:68` and
  `pass1_engine_resolution_pipeline` `:69`).
- `crates/quarto-core/tests/fixtures/pandoc_shim/` — new fixture directory.

**The shim's decoding contract, stated so the tests can bind it:**
- Recognizer: `Div`/`Span` with `"__quarto_custom_node"` in `attr.classes`. Q1's own scaffold is a
  bare `pandoc.Div({})` carrying `__quarto_custom{,_type,_context,_id}` **attributes** and **no
  classes** (`create_custom_node_scaffold`, `ast/customnodes.lua:236-255`), so the two shapes cannot
  be confused in either direction.
- Payload: `data-custom-type` (string), `data-custom-data` (JSON), `data-custom-slots` (JSON map
  name→`"Block"|"Inline"|"Blocks"|"Inlines"`) — emitted at `json.rs:3705-3714`.
- **Decode with `quarto.json.decode`, not `pandoc.json.decode`** — see D5's float trap.
- Slots: each direct child is a `Div` whose `data-slot-name` attribute names the slot
  (`json.rs:3735`); `Inline`/`Inlines` slots are wrapped in a `Plain` (`json.rs:3747-3758`) which
  must be unwrapped back to Inlines.
- Attr sanitizer: before handing `attr` to **any** constructor, remove the `__quarto_custom_node`
  class and the three `data-custom-*` keys, preserving the identifier, every other class and every
  other attribute. (An implementation that unwraps but preserves the wrapper's attributes onto the
  surviving content would re-arm the collision — the sanitizer is the single place that strips
  them.)
- Route table: seven keys, values `"R"` or `"N"`. No `"L"` value, no raw-Div reconstruction helper —
  design §3 keeps Route L defined for future presentation-only types, and P5's checklist says not to
  build it speculatively. (Task 6's Callout fallback is a *narrow, Callout-only* raw reconstruction,
  not general Route-L machinery.)
- Traversal: bottom-up. P4 T8.3 already asserts the file declares no `traverse = 'topdown'`.

**Acceptance criterion.** `pandoc lua` exercises `quarto2_shim.decode_wire_node` and
`quarto2_shim.sanitize_attr` directly and both behave as specified; a nested-wire-node fixture
renders with *both* nodes converted; the captured post-filter AST contains **no**
`data-custom-type` attribute anywhere; P4's T8.3 and T1.5 are still green; `L_TIER_TEST_COUNT` is
bumped by the number of `L` tests this task adds.

**Prerequisite.** P4 Task 1 (vendored tree + README `## Ours vs. pinned`), **P4 Task 2**
(`run_main_lua`, the `L` tier), **P4 Task 8** (the marked `main.lua` splice + the placeholder whose
body this replaces), P4 Task 4/5 (a params blob that makes `main.lua` run to completion). The
*production* argv that reaches `pandoc` is P4 Task 9's; this task's harness is a test-side sibling.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T1.1 | **L**(lua) | `quarto2_shim.decode_wire_node` in real pandoc Lua | Build a wire `Div` with `pandoc.Attr("fig-x", {"__quarto_custom_node","quarto-float"}, {["data-custom-type"]="FloatRefTarget", ["data-custom-data"]='{"ref_type":"fig","kind":"Figure","order":{"section":[],"order":1}}', ["data-custom-slots"]='{"content":"Blocks"}'})` plus one `data-slot-name="content"` child → assert `type_name == "FloatRefTarget"`, `slots.content` is Blocks, and **`math.type(data.order.order) == "integer"`** | none — real `pandoc lua`, real `pandoc` module, real `_json` via the materialized datadir on `package.path` | the `quarto.json.decode` call in `decode_wire_node` (reverting it to `pandoc.json.decode`) |
| T1.2 | **L**(lua) | `quarto2_shim.sanitize_attr` | Feed the same attr → assert the returned attr's identifier is `"fig-x"`, its classes are exactly `{"quarto-float"}`, and none of `data-custom-type`/`-slots`/`-data` remain | none | the sanitizer's class-filter line, and separately its attribute-delete lines |
| T1.3 | **L**(lua) | `quarto2_shim.routes` | Assert the table's key set is exactly the seven type names, each value is `"R"` or `"N"`, and **no** value is `"L"` | none | adding an eighth entry, or an `"L"` value, or dropping one |
| T1.4 | **L**(chain) | the shim + real `main.lua`, bottom-up **effect** | Render `nested-float-in-callout.qmd` (a `#fig-inner` figure inside a `::: {#nte-outer .callout-note}`) through `run_main_lua_capturing_ast` to **docx** → assert the captured AST has no `data-custom-type` attribute, contains the inner float's caption prefix `Figure\u{a0}1:`, **and** the outer callout's title prefix `Note\u{a0}1:` | **nothing mocked** — real pandoc, real Lua, real vendored tree | setting `traverse = 'topdown'` on the shim's filter table in `quarto2-shim.lua` |
| T1.5 | **L**(chain) | the harness's own observer contract | Same render → assert the captured AST is non-empty, deserializes as Pandoc JSON, **and** the docx at `out_path` starts `PK\x03\x04` (the observer did not displace the writer) | none | the `-L <quarto2-shim-probe.lua>` argument in `run_main_lua_capturing_ast` |
| T1.6 | U | `pandoc_filters::FILTERS_DIR` (P4's `include_dir!`) | Assert `FILTERS_DIR.get_file("quarto2-shim-probe.lua").is_some()` and that the README's `## Ours vs. pinned` list names it | none (compiled-in bytes) | adding the probe file without listing it in the README |

**Revert hunks, stated exactly:**
- T1.1 — Revert `quarto.json.decode` to `pandoc.json.decode` in `decode_wire_node` →
  `assert(math.type(data.order.order) == "integer")` in `test_decode_wire_node_preserves_integers`
  RED (measured: the value becomes the float `1.0`). **This is the hunk whose revert would otherwise
  produce `Figure 1.0` in every document with no test moving** — see D5.
- T1.2 — Revert the sanitizer's `classes:filter(function(c) return c ~= "__quarto_custom_node" end)`
  line → `assert_classes_eq(attr, {"quarto-float"})` in `test_sanitize_attr_drops_wire_class` RED.
  Revert its `attributes["data-custom-type"] = nil` deletions → the companion assertion in the same
  test RED.
- T1.3 — Revert the route table by adding an `"L"` entry (or a raw-Div reconstruction helper wired
  to one) → `assert_no_route_l(routes)` in `test_route_table_has_no_route_l` RED. This is the bound
  form of P5's "don't build dead Route-L machinery speculatively".
- T1.4 — Revert the shim's filter table to `traverse = 'topdown'` →
  `assert!(ast_text.contains("Figure\u{a0}1:"))` in `test_nested_wire_nodes_convert_inner_first` RED:
  a topdown traversal hands `quarto.Callout{content = …}` the raw, unconverted inner wire Div, which
  the shim's own group has already walked past, so the inner float is never converted and no float
  caption prefix is produced. **The companion `no data-custom-type` assertion is what proves the
  inner node was reached at all** — without it, "the outer rendered" is non-discriminating.
- T1.5 — Revert the `-L <probe>` argument → the captured-AST assertion in
  `test_capture_harness_observes_post_filter_ast` RED, while the `PK\x03\x04` half stays GREEN (see
  the vacuity check).
- T1.6 — Revert (delete) the probe's `## Ours vs. pinned` README entry → P4's
  `test_real_tree_is_clean` (T1.5) RED.

### Refactor-induced vacuity check

- **T1.5's `PK\x03\x04` half is shape/gating only, not a guard.** P4's T2.2 already records that a
  docx produced with no `-L` at all also starts `PK`; adding an observer filter cannot change that.
  The discriminator in T1.5 is the *captured AST*, not the bytes. Keep the bytes assertion anyway —
  it is what would catch an observer probe that accidentally consumed the document (e.g. returning
  `pandoc.Pandoc({})`).
- **T1.4's "it rendered" is the canonical collapsed assertion for the traversal contract.** A
  topdown traversal still produces a document — the outer Callout is still converted and still
  numbered; only the *inner* node silently degrades. So `assert!(outcome.status.success())` survives
  the topdown revert, and so does any assertion about the outer callout alone. The discriminator is
  the **inner** float's caption prefix, and the "path was actually exercised" assertion is the
  absence of `data-custom-type` from the captured AST.
- **`no data-custom-type` is the right "the shim ran" assertion; `no __quarto_custom_node` is not.**
  For Callout and Tabset the class-keyed dispatcher would consume the wrapper even if the shim never
  ran (D2), so the class can disappear for the wrong reason. `data-custom-type` is emitted only by
  Q2's writer (`json.rs:3705`) and is never produced by any Q1 code path, so its absence is
  unambiguous. Use it uniformly in every `L`(chain) test below.
- **T1.3's key-set assertion must be a literal list of seven names, not `#routes == 7`.** A count
  survives a rename or a swap; the plan's whole point is per-type routing.

---

## Task 2: Route R for Theorem, Proof and FloatRefTarget — the three fixed-field-set constructors, plus the shared post-construction `order` assignment

**Scope.** Implement the shared order-assignment mechanism (assign onto the **second** return value
of `quarto.<AstName>(params)`, return the **first**, never call `render`) and the three field maps
whose Q1 constructors take a fixed field set. Grouped because all three are the same mechanical
shape: derive constructor args from `plain_data` + `attr` + slots, call the constructor, assign
`order`, return the scaffold.

**Files:**
- `resources/pandoc-filters/filters/quarto2-shim.lua` — ours (in the vendored tree).
- `crates/quarto-core/tests/integration/pandoc_shim.rs` — extend.
- `crates/quarto-core/tests/fixtures/pandoc_shim/{theorem-basic,proof-basic,float-basic}.qmd` — new.

**The three field maps, audited against the real constructors:**

- **Theorem** — `theorem.lua:87-93` takes `{ name, div, identifier }`. (Citation ✓: the constructor
  starts at `:87`, not `:88`.)
  - `identifier` ← `attr[1]` (the sanitized identifier). Note `plain_data.identifier`
    (`theorem.rs:296`) carries the same value; prefer `attr` so the sanitizer is the single source.
  - `div` ← `slots.content` (Blocks), wrapped in a `Div` if not already one — Q1's renderer does the
    same normalization itself at `theorem.lua:211-215`.
  - `name` ← `slots.title` (Inlines), **absent when the slot is absent**. `plain_data.kind` must
    **not** be routed here: `captionPrefix` (`crossref/theorems.lua:79-90`) uses `name` only as a
    parenthetical suffix at `:83-88`, so forcing `kind` through it renders `Theorem 1 (Theorem)`.
    `kind` reaches Q1 via P4 Task 5's `crossref-<type>-title` param instead.
  - `order` ← `plain_data.order`, assigned post-construction.
- **Proof** — `proof.lua:54-61` takes `{ name, div, identifier, type }`.
  - `type` ← `plain_data.type` (**P2 Task 3's extension**; `proof.rs:150-152` carries only
    `{"kind": "Proof"}` today). The renderer does `proof_types[proof_tbl.type:lower()]`
    unconditionally at `proof.lua:81`, so a nil `type` is `attempt to index a nil value`.
  - `div`/`name`/`identifier` as for Theorem. **No `order`** — `proof.lua`'s renderer never reads it.
- **FloatRefTarget** — `floatreftarget.lua:96-108` decomposes `tbl.attr` into
  `identifier`/`classes`/`attributes` and passes everything else through unfiltered.
  - `attr` ← the sanitized attr.
  - **`type` ← `plain_data.kind`** — this is a rename, not a pass-through. Q1's renderer reads
    `float.type` as a **display name** keyed into `crossref.categories.by_name`
    (`ref_type_from_float`, `common/refs.lua:44-55`;
    `float_title_prefix`, `crossref/tables.lua:224`), and `by_name` is built from `category.name`
    (`mainstateinit.lua:129`), i.e. `"Figure"`/`"Table"`/`"Listing"` — exactly the value space of
    Q2's `plain_data.kind` (`float_ref_target.rs:349`). Handing `plain_data` through verbatim leaves
    `float.type` nil and crashes on `"unknown float type '" .. nil .. "'"`.
  - `content` ← `slots.content`; `caption_long` ← `slots.caption_long`; `caption_short` ←
    `slots.caption_short` (identity names, D6).
  - `order` ← `plain_data.order`, post-construction.

**The order-assignment literal** (shape per `crossref/format.lua:137-145`, matching Q2's
`plain_data.order` verbatim — `crossref_index.rs:287-295` writes `{ "section": [...], "order": n }`):

```lua
local node, tbl = quarto.Theorem{ identifier = id, name = name, div = div }
tbl.order = { order = data.order.order, section = data.order.section }
return node   -- main.lua's own render pass dispatches to the registered renderer;
              -- the shim never calls render itself
```

**Acceptance criterion.** Each of the three fixtures renders to docx through `main.lua` with the
expected numbered prefix in the captured post-filter AST (`Theorem 1`, `Figure\u{a0}1:`), a Proof
renders without crashing and carries the `proof` environment, and no `data-custom-type` attribute
survives. `L_TIER_TEST_COUNT` bumped.

**Prerequisite.** Task 1. **P2 Task 3** for `Proof.plain_data.type` — until it lands, T2.4 is
`seam deferred until P2 Task 3's Proof extension`, and the Proof route must *not* be given a
default `"proof"` in the shim as a workaround (that would make T2.4 permanently vacuous and hide the
schema gap). P4 Task 5 for the `crossref-<type>-title` params Theorem's label depends on.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | **L**(chain) | the shim's Theorem route + real `theorem.lua` renderer | `theorem-basic.qmd` (`::: {#thm-p .theorem}`) → docx, captured AST → assert the caption paragraph contains `Theorem 1` **and** a `Span` with class `theorem-title`, and no `data-custom-type` | none | the `tbl.order = { … }` assignment line |
| T2.2 | **L**(chain) | the same, `name` mapping | Fixture with `name="My Special Title"` → assert the caption contains `Theorem 1 (My Special Title)` and **does not** contain `Theorem 1 (Theorem)` | none | routing `plain_data.kind` into the constructor's `name` field |
| T2.3 | **L**(chain) | the shim's FloatRefTarget route + `crossref/tables.lua:223-236` | `float-basic.qmd` (`#fig-x` with a caption) → assert the caption contains `Figure\u{a0}1:` **and** stderr contains no `field 'order' is missing from float` | none | the `type = data.kind` mapping line; and separately the `tbl.order` assignment |
| T2.4 | **L**(chain) | the shim's Proof route + `proof.lua:81` | `proof-basic.qmd` (`::: {.proof}`) → assert exit 0 and the rendered div carries the `proof` class; assert stderr has no `attempt to index a nil value` | none | the `type = data.type` mapping line |
| T2.5 | **L**(chain) | the order-assignment target | A variant harness call that assigns `order` onto the **first** return value instead of the second → assert the Theorem fixture's caption **loses** `Theorem 1` entirely (silent), and the FloatRefTarget fixture emits `field 'order' is missing from float` on stderr | none — this is a deliberate wrong-target run of the real chain | the `local node, tbl = …; tbl.order = …` two-value destructuring |
| T2.6 | I | `render_qmd_to_pandoc` (P4 Task 9), production path | Render `theorem-basic.qmd` to `.docx`, then `pandoc -f docx -t plain` the result → assert the body contains `Theorem 1` | none | the shim's Theorem route body |

**Revert hunks, stated exactly:**
- T2.1 — Revert the `tbl.order = { … }` assignment → `assert!(caption.contains("Theorem 1"))` in
  `test_theorem_caption_is_numbered` RED. **Not** "the render fails" — `theorem.lua:278-280`
  explicitly `return el` on nil order, so the render still succeeds (D4).
- T2.2 — Revert `name = slots.title` to `name = data.kind` →
  `assert!(!caption.contains("Theorem 1 (Theorem)"))` in `test_theorem_name_is_user_title` RED.
- T2.3 — Revert `type = data.kind` → the run exits 83 with
  `attempt to concatenate a nil value` from `common/refs.lua:47` / `crossref/tables.lua:226`, so
  `assert!(outcome.status.success())` in `test_float_caption_is_numbered` RED. Revert the
  `tbl.order` assignment instead → `assert!(!stderr.contains("field 'order' is missing from float"))`
  in the same test RED (warn-and-skip, exit 0 — D4).
- T2.4 — Revert `type = data.type` → `assert!(outcome.status.success())` in
  `test_proof_renders_with_explicit_type` RED (`proof.lua:81`, `nil:lower()`).
- T2.5 — Revert the two-value destructuring to `local node = quarto.Theorem{…}; node.order = …` →
  `assert!(caption.contains("Theorem 1"))` in `test_order_goes_on_the_data_table` RED **for
  Theorem**, and `assert!(!stderr.contains("field 'order' is missing"))` RED **for
  FloatRefTarget**. Both polarities are needed; see the vacuity check.
- T2.6 — Revert the shim's Theorem route body to the unrecognized-type fallback →
  `assert!(plain.contains("Theorem 1"))` in `test_theorem_end_to_end_through_production_path` RED.

### Refactor-induced vacuity check

- **The `.order`-on-the-wrong-value test cannot be written once for all five types, because Q1's
  nil-order behaviour is three different things** (D4). A single "the render crashed" assertion is
  RED only for Callout; a single "the render succeeded" assertion is GREEN for Theorem and
  FloatRefTarget *and* for the bug. T2.5 therefore asserts a *different surface per type*: the
  caption text for Theorem (silent early return), the stderr warning for FloatRefTarget
  (warn-and-skip). Callout's crash polarity is bound separately in Task 3 (T3.2).
- **T2.1's `Theorem 1` is discriminating only because of the space character.** `captionPrefix`
  (`crossref/theorems.lua:81`) inserts `pandoc.Space()`, not `nbspString()` — unlike `titlePrefix`
  (`format.lua:17-33`), which callouts and floats use. Asserting `Theorem\u{a0}1` for a theorem
  would be RED against correct code; asserting `Figure 1:` (regular space) for a float likewise.
  Do not unify the expected strings across types.
- **The number must be asserted as `1`, not matched loosely.** Per D5, a `contains("Theorem 1")`
  assertion is GREEN when the JSON decoder yields a float (`Theorem 1.0` contains `Theorem 1`).
  T1.1 binds the decoder at the cheap tier; every caption assertion in this task should carry the
  companion `assert!(!text.contains("1.0"))` so the trap cannot re-enter through a later refactor of
  `decode_wire_node`.
- **T2.6 is the CLAUDE.md end-to-end leg and is deliberately coarser.** It asserts through
  `pandoc -f docx -t plain`, which may fold a non-breaking space into a regular one — so it uses
  the Theorem fixture (regular space, plain Pandoc inlines in the caption) and never asserts an
  nbsp. Every nbsp assertion lives on the captured AST.

---

## Task 3: Route R for Callout and Tabset — the numbered identity-mapped type and the tabs-list type

**Scope.** The two Route-R types whose shapes differ from Task 2's: Callout (identity slot names,
numbered, and the type whose nil-order failure is a crash), and Tabset (no Q1 `slots` declaration;
the constructor takes an already-split `params.tabs` list, and returns via the
`need_emulation == false` branch).

**Files:**
- `resources/pandoc-filters/filters/quarto2-shim.lua` — ours.
- `crates/quarto-core/tests/integration/pandoc_shim.rs` — extend.
- `crates/quarto-core/tests/fixtures/pandoc_shim/{callout-numbered,callout-plain,tabset-basic}.qmd` — new.

**Callout field map** (`callout.lua:79-121`, `slots = { "title", "content" }` at `:77`):
`type` ← `plain_data.type`; `appearance` ← `plain_data.appearance`; `icon` ← `plain_data.icon`;
`collapse` ← `plain_data.collapse`; `title` ← `slots.title`; `content` ← `slots.content`;
`attr` ← the sanitized attr; `order` ← `plain_data.order`, post-construction. Design §3 records this
map as verified 1:1 — it is the one type whose field-map prose lives in the design doc, not in P5.
Per the "Q2 owns presentation defaults" principle (P4/P5), `appearance` and
`icon` are already resolved before the cut, so Q1's own defaulting at `callout.lua:83-105` is dead
code for Route R; feed the resolved values and do not re-default.

**Tabset field map** (`panel-tabset.lua:147-244`): `level` ← `plain_data.level` (Q1 defaults to `2`
at `:156`); `attr` ← the sanitized attr (Q1 defaults to `Attr("", {"panel-tabset"})` at `:157`);
`tabs` ← one `quarto.Tab{ content = slots["content-"..i], title = slots["title-"..i], active =
plain_data.actives[i] }` per tab, `i` in `1..plain_data.tab_count`. `plain_data.group` has no Q1
counterpart (it is an HTML-only grouped-tab-sync signal) and is deliberately unused. **No `order`** —
Tabset is unnumbered. Note the constructor warns and substitutes an empty list when `params.tabs`
is nil (`:149-152`), so a shim bug that produces no tabs is a *warning on a zero-exit render*, not
a crash — which is why T3.4 binds the warning, not the exit code.

**Acceptance criterion.** A numbered callout renders with `Note\u{a0}1:` in its docx title; an
unnumbered callout renders with no numeric prefix and no warning; a two-tab tabset renders both tab
titles and both bodies with no `No tabs found in tabset` warning; no `data-custom-type` survives.
`L_TIER_TEST_COUNT` bumped.

**Prerequisite.** Task 1 and Task 2 (the shared order-assignment mechanism). **P1's `Pandoc`-kind
exclude list** must keep `panel-tabset` (the sugar half) enabled while excluding
`panel-tabset-resolve` — P1 companion Task 2 owns that list; without it there is no `Tabset` wire
node to route and T3.3/T3.4 have no input. P4 Task 5's `crossref-nte-title`/`-prefix` params for the
callout label.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | **L**(chain) | the shim's Callout route + `quarto-post/docx.lua:193` | `callout-numbered.qmd` (`::: {#nte-setup .callout-note}` with a title) → docx, captured AST → assert a `RawBlock("openxml", …)` whose text contains `Note\u{a0}1:`, and no `data-custom-type` | none | the Callout route's `tbl.order` assignment |
| T3.2 | **L**(chain) | the same, nil-order polarity | A wrong-target variant (order assigned to the first return value) → assert the run exits **83** and stderr contains `format.lua` | none | the Callout route's two-value destructuring |
| T3.3 | **L**(chain) | the shim's Callout route, unnumbered path | `callout-plain.qmd` (`::: {.callout-note}`, no id) → assert the title contains no digit-prefix and stderr contains no `unknown callout prefix` | none | the `attr` sanitizer (an unsanitized attr leaves `__quarto_custom_node` on the rendered Div) |
| T3.4 | **L**(chain) | the shim's Tabset route + `panel-tabset.lua:147-244` | `tabset-basic.qmd` (two `## ` tabs) → assert both tab titles and both bodies appear in the captured AST, **and** stderr contains no `No tabs found in tabset` | none | the `params.tabs` list construction (the per-tab `quarto.Tab{…}` loop) |
| T3.5 | **L**(chain) | Tabset's `need_emulation == false` return branch | Same fixture → assert the captured AST contains no `__quarto_custom_id`-attributed Div left unrendered (i.e. the scaffold returned from `:457` was picked up by `main.lua`'s render pass) | none | returning the *second* value from `quarto.Tabset` instead of the first |

**Revert hunks, stated exactly:**
- T3.1 — Revert the Callout route's `tbl.order = { … }` → the run exits 83
  (`crossref/format.lua:140`, `order.order` on nil, reached via `titlePrefix`), so
  `assert!(outcome.status.success())` in `test_numbered_callout_gets_prefix` RED. Keep the
  `Note\u{a0}1:` assertion too: it is what discriminates "order assigned, wrong value" from "order
  assigned correctly".
- T3.2 — Revert the two-value destructuring → `assert_eq!(outcome.status.code(), Some(83))` in
  `test_callout_order_must_be_on_the_data_table` RED (this test asserts the *failure* shape and so
  flips polarity relative to T3.1; see the vacuity check).
- T3.3 — Revert the attr sanitizer's class filter →
  `assert!(!ast_text.contains("__quarto_custom_node"))` in `test_unnumbered_callout_has_no_prefix`
  RED (the class rides through onto the rendered Div).
- T3.4 — Revert the `quarto.Tab{…}` loop to passing `params.tabs = nil` →
  `assert!(!stderr.contains("No tabs found in tabset"))` in `test_tabset_rebuilds_tabs` RED
  (`panel-tabset.lua:150`, a warning on a zero-exit render — `status.success()` stays GREEN).
- T3.5 — Revert `return node` to `return tbl` in the Tabset route →
  `assert!(!ast_text.contains("__quarto_custom_id"))` in `test_tabset_returns_the_scaffold` RED.

### Refactor-induced vacuity check

- **T3.2 partially survives its own revert and must be read as documentation of the failure shape,
  not as the guard.** It asserts exit 83; a *different* Lua crash anywhere in the chain also exits
  83. The `stderr.contains("format.lua")` half is the "the path was actually exercised" assertion
  and must not be dropped — P4's companion made the same call for its T4.4/T4.6.
- **Callout is the one type where "the render succeeded" IS a discriminator** (D4), which makes it
  tempting to write only T3.1 and skip the text assertion. Don't: a shim that assigns
  `tbl.order = data.order` where `data.order` is `{order = 1.0, …}` (the float trap) still succeeds
  and renders `Note\u{a0}1.0:`. The text assertion is what catches that.
- **T3.4's "both tab titles appear" is nearly non-discriminating on its own.** Q1's fallback for a
  nil `tabs` list is an *empty* tabset, which drops the bodies — so the title assertion does move.
  But a shim that builds the tabs in the *wrong order*, or drops `active`, produces output
  containing both titles. The warning assertion is the cheap discriminator; ordering is asserted by
  requiring the two titles to appear in source order in the captured AST.
- **T3.3 is a negative assertion ("no digit prefix"), which is weak alone.** It is paired with the
  positive `no __quarto_custom_node` assertion so the test cannot pass by the callout having failed
  to render at all.

---

## Task 4: Route N for `CrossrefResolvedRef` — call Q1's own functions, inline only `add_ref_prefix`'s nbsp logic

**Scope.** Resolve a `CrossrefResolvedRef` wire node directly to plain Pandoc inlines by calling
Q1's real globals with Q2's already-resolved `plain_data`, mirroring `resolveRefs`'s body. Q1's
behaviour is normative for the Pandoc leg (decided; design §12's accepted asymmetry).

**Files:**
- `resources/pandoc-filters/filters/quarto2-shim.lua` — ours.
- `crates/quarto-core/tests/integration/pandoc_shim.rs` — extend.
- `crates/quarto-core/tests/fixtures/pandoc_shim/{ref-figure,ref-unresolved}.qmd` — new.

**The seven callable globals, all verified present and reachable in `main.lua`'s state:**

| function | anchor | role |
|---|---|---|
| `refPrefix(type, upper)` | `crossref/format.lua:66-96` | the prefix half (`"Figure"`); reads `param("crossref-<t>-prefix")` first (P4 Task 5) |
| `refNumberOption(type, entry)` | `crossref/format.lua:106-121` | **the number**; called by `resolveRefs` at `refs.lua:112` |
| `subrefNumber(order)` | `crossref/format.lua:51-53` | the subfloat branch's `(a)` |
| `refHyperlink()` | `crossref/format.lua:102-104` | whether to wrap in a `Link` |
| `refDelim()` | `crossref/format.lua:98-100` | multi-ref join |
| `crossrefOption(name, default)` | `crossref/options.lua:17-19` | every option read above goes through it |
| `nbspString()` | `common/pandoc.lua:125-127` | `pandoc.Str '\u{a0}'` |

**`add_ref_prefix` must be reproduced inline**, not called: it is a `local function` declared inside
the `Cite` callback at `refs.lua:13-19`, so it is unreachable from a spliced-in filter. Its logic:
extend with the prefix, then append `nbspString()` unless
`crossref.categories.by_ref_type[ref_type].space_before_numbering == false` or
`_quarto.format.isTypstOutput()`.

**`refNumberOption` takes an index-entry-shaped table, not a raw order.** The shape is
`{ parent, order, caption, appendix }` (`crossref/index.lua:66-78`, `indexAddEntry`); `:111` reads
`entry.appendix` and `entry.order.section`. The shim synthesizes it from `plain_data`:
`{ order = data.order, parent = nil, caption = pandoc.Blocks({}), appendix = false }` — `parent` and
`appendix` are not in `plain_data` and default to nil/false for the flat, non-appendix, non-nested
case this epic's v1 targets.

**Deliberate v1 scope-outs, stated so they are not silently unaddressed:**
- **Subfloat refs** (`refs.lua:104-110`, `entry.parent ~= nil`) are unreachable in v1 because
  `plain_data` carries no `parent`. Not implemented; `subrefNumber` is still probed by Task 7's
  arity test so a signature change is loud.
- **Multi-ref joining** (`refs.lua:42-45`) is moot: Q2 emits one `CrossrefResolvedRef` per `@ref`
  and drops additional ids upstream of the cut (its own test documents this —
  `crossref_resolve.rs:540`, `multi_crossref_cite_resolved_to_first`). `refDelim` is likewise
  probed but not called.
- **The `#cite.prefix > 0` branch** (`refs.lua:54-55`) cannot be implemented in v1: it needs the
  citation's prefix `Inlines`, and P2 Finding 3 establishes that `cite_prefix` cannot be a
  `plain_data` field (it is AST, and `plain_data` is contractually AST-free —
  `quarto-pandoc-types/src/custom.rs:72-75`) with the slot-shaped alternative left undecided.
  `seam deferred until P2 Finding 3 is answered`.
- **`cite_mode` / `label_upper`** (`refs.lua:31`, `:56`, `:95-96`) **are** available once P2 Task 3
  lands; the `SuppressAuthor` branch and `refPrefix`'s `upper` argument are in scope.

**Unresolved refs** degrade the way Q1's own `resolveRefs` degrades them, not with Q2's `?id?`
convention: the `not resolve` path builds a `Span(stringToInlines(label), Attr("",
{"quarto-unresolved-ref"[, "ref-noprefix"]}))` (`refs.lua:93-102`), and the "no index entry" path
emits `warn("Unable to resolve crossref @" .. label)` plus `Strong(Str("?@" .. label))`
(`refs.lua:127-131`). The shim reproduces the first shape for `plain_data.resolved == false`.

**Acceptance criterion.** `ref-figure.qmd` renders a `Link` with class `quarto-xref` whose
stringified text is **exactly** `Figure\u{a0}1` (U+00A0, and `1` not `1.0`), href `#fig-x`;
`ref-unresolved.qmd` renders a `Span` with class `quarto-unresolved-ref` and does not crash. No
`data-custom-type` survives. `L_TIER_TEST_COUNT` bumped.

**Prerequisite.** Task 1. **P3's `crossref-numbering: external` mode** must be active or Q1's own
`resolveRefs` would also run — but note that under external mode `quarto_crossref_filters` is not
appended at all (`main.lua:718-720`), which is *why* the shim must do this work; the shim's own
calls are to functions defined at file-load time, not to the suppressed filter group. P4 Task 5 for
`crossref-fig-prefix`. **P4 Task 8's splice position is load-bearing here specifically**: every
function above funnels through `crossrefOption`, which indexes `crossref.options`, nil until
`quarto_init_filters` runs (D2).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | **L**(chain) | the shim's Route-N `CrossrefResolvedRef` + real `refPrefix`/`refNumberOption`/`refHyperlink` | `ref-figure.qmd` (`See @fig-x.` + a `#fig-x` figure) → docx, captured AST → assert a `Link` with class `quarto-xref`, target `#fig-x`, and `pandoc.utils.stringify(link.content) == "Figure\u{a0}1"` **exactly** | none | the `refNumberOption(...)` call |
| T4.2 | **L**(chain) | the inlined `add_ref_prefix` nbsp logic | Same capture → assert the link text's 7th byte sequence is U+00A0 (assert on the exact string, not a whitespace-normalized one) | none | the inlined `nbspString()` insertion |
| T4.3 | **L**(chain) | the integer coercion | Same capture → assert the link text does **not** contain `1.0` | none | `quarto.json.decode` → `pandoc.json.decode` in `decode_wire_node` (Task 1) |
| T4.4 | **L**(chain) | `refHyperlink()` honouring | Render with `crossref: {ref-hyperlink: false}` in the fixture's front matter → assert the captured AST has **no** `Link` with class `quarto-xref` but still contains the text `Figure\u{a0}1` | none | the `if refHyperlink() then` branch (reverting to an unconditional `pandoc.Link`) |
| T4.5 | **L**(chain) | the unresolved path | `ref-unresolved.qmd` (`@fig-missing` with no target) → assert exit 0 and a `Span` with class `quarto-unresolved-ref` | none | the `plain_data.resolved == false` branch |
| T4.6 | **L**(chain) | the splice-position dependency | A variant harness call using a `main.lua` patched to splice the shim **before** `quarto_init_filters` → assert exit **83** and `stderr.contains("options.lua")` | none | the splice position (P4 T8.1's hunk, asserted here by *effect* rather than by line order) |

**Revert hunks, stated exactly:**
- T4.1 — Revert the `refNumberOption(type, entry)` call (leaving only `refPrefix`) →
  `assert_eq!(link_text, "Figure\u{a0}1")` in `test_resolved_ref_text_is_prefix_and_number` RED,
  because the text collapses to `Figure\u{a0}`. **This is the round-4 bug's exact shape**: a
  `contains("Figure")` assertion would stay GREEN.
- T4.2 — Revert the inlined nbsp insertion → the same exact-equality assertion in
  `test_resolved_ref_uses_nbsp` RED (the text becomes `Figure1`). Asserting a normalized
  `"Figure 1"` would be GREEN for both.
- T4.3 — Revert Task 1's decoder choice → `assert!(!link_text.contains("1.0"))` in
  `test_resolved_ref_number_is_an_integer` RED (measured).
- T4.4 — Revert the `if refHyperlink()` guard to an unconditional `pandoc.Link` →
  `assert!(links.is_empty())` in `test_resolved_ref_honours_ref_hyperlink` RED.
- T4.5 — Revert the `resolved == false` branch → the run either crashes in `refNumberOption`
  (nil `entry.order`) or emits a bare label, so `assert!(span_has_class("quarto-unresolved-ref"))`
  in `test_unresolved_ref_degrades_like_q1` RED.
- T4.6 — Revert the splice to before `quarto_init_filters` →
  `assert_eq!(outcome.status.code(), Some(83))` in
  `test_route_n_requires_post_init_position` — this test asserts the *failure*, so read the
  vacuity note.

### Refactor-induced vacuity check

- **T4.6 survives its own revert and is shape/gating only.** It constructs the broken position
  itself, so it passes whether or not the shipped `main.lua` has the right splice. P4's T8.1 carries
  the line-order discriminator. T4.6's value is recording the exact diagnostic
  (`common/options.lua:15`, `attempt to index a nil value (local 'options')`) that a future
  implementer who moves the shim will actually see. Same treatment P4 gave its own T2.3.
- **`Figure` / `Figure 1` / `Figure\u{a0}1` are three different assertions with three different
  discriminating powers** (D5), and the round-4 history shows each of the weaker two was the
  plan's stated expectation at some point. T4.1 must use exact equality; if a future edit relaxes
  it to `contains`, T4.3's negative assertion is the only thing left standing between the suite and
  a `Figure 1.0` regression.
- **T4.4's expected value must not be `"Figure 1"` (the non-hyperlinked text).** Both the
  hyperlinked and non-hyperlinked states contain that text; the discriminator is the presence or
  absence of the `Link` node with class `quarto-xref`. Assert the node shape, and keep the text
  assertion only as the "the path was actually exercised" half.
- **Do not assert Q2's own `crossref_render.rs` text shape here.** `render_resolved_ref`
  (`crossref_render.rs:1114`) is the HTML leg's independent implementation and design §12 records
  that the two legs legitimately differ. A test that compared them would encode the asymmetry the
  design accepted as a bug.

---

## Task 5: Route N for `Equation` — call `renderEquation` with Q2's order, on the branch that reads it

**Scope.** Unwrap the `Equation` wire node back to its `Math` inline and call Q1's existing
`renderEquation(eq, label, alt, order)` with Q2's already-computed order, rather than reimplementing
the format-specific `RawInline` injection.

**Files:**
- `resources/pandoc-filters/filters/quarto2-shim.lua` — ours.
- `crates/quarto-core/tests/integration/pandoc_shim.rs` — extend.
- `crates/quarto-core/tests/fixtures/pandoc_shim/equation-numbered.qmd` — new.

**Field map.** `eq` ← the single `Math` inline in `slots.content` (`equation_label.rs:225-226` stores
it as `Slot::Inlines(vec![math_inline])`); `label` ← `plain_data.identifier`
(`equation_label.rs:221`); `alt` ← `nil` (Typst-only accessibility parameter, `equations.lua:101`,
out of scope for docx/pptx); `order` ← `plain_data.order`. **`Equation` does carry
`plain_data.order`** even though `equation_label.rs:218-222` does not set it: it sets the full
crossref triple (`ref_type: "eq"`), and `crossref_index.rs:287-295` injects `order` into any node
passing `has_crossref_plain_data` (`:318`). P2 Finding 2 records the same conclusion independently.

**Acceptance criterion.** `equation-numbered.qmd` rendered to **docx** produces a `Span` with
identifier `eq-x` whose `Math` text ends with `\qquad(1)`; the same fixture rendered to `latex`
produces `\begin{equation}` and no `\qquad`. No `data-custom-type` survives. `L_TIER_TEST_COUNT`
bumped.

**Prerequisite.** Task 1. P3's external-numbering mode (so Q1's own `equations()` filter, which
would otherwise index and renumber, does not run — it lives in `quarto_crossref_filters`, gated at
`main.lua:718`).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T5.1 | **L**(chain) | the shim's Equation route + `renderEquation`'s fallback branch (`equations.lua:133-144`) | `equation-numbered.qmd` → **docx**, captured AST → assert a `Span` with identifier `eq-x` containing a `Math` whose `text` ends with `\qquad(1)`, and no `data-custom-type` | none | the `order` argument in the `renderEquation(eq, label, nil, order)` call |
| T5.2 | **L**(chain) | the integer coercion, equation path | Same capture → assert the Math text contains `\qquad(1)` and **not** `\qquad(1.0)` | none | Task 1's `quarto.json.decode` choice |
| T5.3 | **L**(chain) | branch attribution, negative direction | Same fixture → **latex** → assert the captured AST contains `\begin{equation}` and **no** `\qquad` | none | none — this test exists to prove the latex target cannot bind `order` (see the vacuity check) |
| T5.4 | **L**(chain) | the unwrap | Same docx capture → assert the `Math` inline's `mathtype` is `DisplayMath` and there is exactly one `Math` node (the wire `Span`/`Plain` wrappers were unwrapped, not nested) | none | the slot-unwrap step that strips the `Plain` wrapper around an `Inlines` slot |

**Revert hunks, stated exactly:**
- T5.1 — Revert the `order` argument (pass `nil`) → `renderEquation`'s fallback does
  `numberOption("eq", nil)` → `formatNumberOption` → `order.order` on nil (`format.lua:140`) → exit
  83, so `assert!(outcome.status.success())` in `test_equation_is_numbered_for_docx` RED, and the
  `\qquad(1)` assertion RED with it.
- T5.2 — Revert Task 1's decoder → `assert!(!math_text.contains("\\qquad(1.0)"))` in
  `test_equation_number_is_an_integer` RED.
- T5.3 — No revert: **this row is a deliberate documentation test, not a guard.** See the vacuity
  check.
- T5.4 — Revert the `Plain`-unwrap for `Inlines` slots →
  `assert_eq!(math_nodes.len(), 1)` in `test_equation_slot_is_unwrapped` RED (a `Plain` block
  nested inside an inline position is either dropped or produces a second wrapper).

### Refactor-induced vacuity check

- **A golden captured with `--to latex` is vacuous for the `order` behaviour by construction**
  (D3): `equations.lua:105-113` never reads `order`, so T5.1's revert is GREEN under latex.
  **T5.1's fixture must target docx.** T5.3 exists to make the vacuity itself a committed,
  reviewable fact rather than a
  sentence in a plan — it asserts the latex branch's *shape* (`\begin{equation}`, no `\qquad`) so
  that a future reader who tries to move T5.1 to latex sees why it cannot work. It is explicitly
  labelled shape-only in the table above.
- **`\qquad(1)` vs `\tag{1}`.** The fallback picks `eqTag` instead of `eqQquad` when
  `isHtmlOutput()` and the math method is mathjax/katex (`equations.lua:139-141`). For docx
  `isHtmlOutput()` is false (`_format.lua:150`), so `eqQquad` is correct — but an implementer
  copying this expected value into an HTML-adjacent fixture would get `\tag{1}`. Stated so the
  value is not transplanted.
- **`\qquad(1)` is not discriminating against the float trap** for the same reason as D5:
  `"\\qquad(1.0)"` does not contain `"\\qquad(1)"`, so here the positive assertion *does*
  discriminate. T5.2 is kept anyway as the explicit negative, because a future relaxation of T5.1 to
  a looser match would reopen the hole.

---

## Task 6: The shim's error handling — the unrecognized type, the Callout `by_ref_type` guard with its warning, and the constructor-crash visibility

**Scope.** Implement P5's three error paths and their three named fixtures. Two of the three are
Q2-only by construction and cannot appear in any Q1-parity golden.

**Files:**
- `resources/pandoc-filters/filters/quarto2-shim.lua` — ours.
- `crates/quarto-error-catalog/error_catalog.json` + `docs/errors/pandoc/Q-18-<n>.qmd` +
  `docs/_quarto.yml` — extend P4 Task 6's `pandoc` subsystem with the two new warning codes
  (unrecognized wire type; callout with an unregistered crossref category). **Both lint rules
  require the page and the sidebar entry in the same commit** —
  `error-docs-page-missing` (`crates/xtask/src/lint/error_docs.rs:73`) and
  `error-docs-sidebar-unlisted` (`crates/xtask/src/lint/error_docs_sidebar.rs:81`), with entries
  **ascending by code number** inside the `- section: "pandoc"` block.
  `2026-09-20-pandoc-hybrid-P7-foundation-implementation.md`'s Task 1 also claims a `Q-18-*` code
  in this same catalog file and sidebar block. `Q-18-1` through `Q-18-4` are already taken (P4
  Task 6); P7-foundation's Task 1 claims `Q-18-5`, so **this task's two codes are `Q-18-6` and
  `Q-18-7`**. Whichever of these two tasks lands second must re-check the catalog for the current
  next-free number before adding its entries — the numbers above are a coordination default, not
  a reservation the CRDT/catalog enforces.
- `crates/quarto-core/tests/integration/pandoc_shim.rs` — extend.
- `crates/quarto-core/tests/fixtures/pandoc_shim/{unknown-wire-type,callout-foreign-category,proof-missing-type}.qmd` — new.
- `claude-notes/designs/pandoc-hybrid-architecture.md` — §12 already documents the Callout
  unregistered-category limitation. No edit needed; verify it still reads correctly when closing
  the task.

**Path 1 — unrecognized `type_name`.** Unwrap the scaffold to its concatenated slot content and drop
the wrapper; emit exactly one warning naming the type. **The wrapper's attributes must be dropped
with it, not re-applied to the surviving content** — re-applying them would put the retained
`.callout-*`-shaped classes back on a Div that the class-keyed dispatcher
(`ast/parse.lua:6-15`) still gets to see, re-arming the collision for the unsupported-type path
specifically (D2). Do **not** hard-fail the render, and do **not** let a
`__quarto_custom_node`-classed Div reach the writer.

**Path 2 — the Callout `fail()` guard.** Before calling `quarto.Callout`, check
`crossref.categories.by_ref_type[ref_type] ~= nil` — **the literal condition
`callout_title_prefix` tests** (`modules/callouts.lua:7-11`), not `valid_ref_types()`. When it is
nil, fall back to a narrow raw-Div reconstruction (no numbering, no crash) **and emit a warning**
naming the callout's id and its unregistered `ref_type`. The silent version re-creates exactly the
number loss Callout was reclassified L→R to prevent, plus a dangling `@ref` that cites a number
appearing nowhere (design §12, final bullet).

*Precision note, verified:* `callout_title_prefix` tests
`by_ref_type[refType(callout.attr.identifier)]`, i.e. it derives the ref-type from the identifier
rather than reading `plain_data.ref_type`. The two agree because Q2's `classify_cite_id` splits on
the first hyphen (`crossref/registry.rs:178-181`) — but the shim's guard should compute it the same
way Q1 does (from the identifier) so the two cannot drift.

**Path 3 — a Route-R constructor crash.** No Lua-side validation layer for v1: the crash *is* the
signal (`proof.lua:81` with a nil `type` is the template). P5 owns only the *fixture*; the
diagnostic's shape — stderr verbatim, temp-JSON retention — is **P4 Task 10's**, already bound
there by a P4-producible failure (its T10.4). This task adds `proof-missing-type.qmd` **alongside**
P4's fixture, never in place of it.

**Acceptance criterion.** `unknown-wire-type.qmd`: exit 0, the slot text survives, no
`__quarto_custom_node` and no `data-custom-*` attribute anywhere in the output, exactly one warning.
`callout-foreign-category.qmd` (`::: {#thm-x .callout-note}`): exit 0 — **not** `FATAL QUARTO
ERROR` — with the fallback warning naming `thm`. `proof-missing-type.qmd`: nonzero exit whose
diagnostic carries pandoc's stderr verbatim and whose temp JSON is retained. `cargo xtask lint`
green. `L_TIER_TEST_COUNT` bumped.

**Prerequisite.** Tasks 1-3. **P4 Task 6** (the `pandoc` error-catalog subsystem, number 18) and
**P4 Task 10** (the stderr channel and temp-JSON retention). `proof-missing-type.qmd` additionally
requires that P2 Task 3's `Proof.type` has landed *and* that the fixture can suppress it — if the
field is mandatory in the schema, construct the fixture's wire JSON by hand rather than through the
production writer, and say so in the test's doc comment.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T6.1 | **L**(chain) | the unrecognized-type fallback | A wire AST carrying `data-custom-type="FutureThing"` with a `content` slot of one paragraph → assert exit 0, the paragraph's text survives in the captured AST, and no node carries `__quarto_custom_node` or any `data-custom-*` attribute | none | the fallback's "return the concatenated slot content" body |
| T6.2 | **L**(chain) | the fallback's attribute-dropping specifically | Same input but with the wrapper also carrying `class="callout-note"` → assert the surviving content carries **neither** `callout-note` nor `__quarto_custom_node`, and that no BlockQuote/callout chrome was produced | none | re-applying the wrapper's attr onto the surviving content |
| T6.3 | **U** | `classify_pandoc_stderr` (P4 Task 10) + the shim's warning text | Feed the observed stderr from T6.1 → assert exactly **one** diagnostic, warning severity, message naming `FutureThing` and carrying the new `Q-18-*` code | injected stderr text | the shim's `warn(...)` call; and separately its de-duplication (one warning per node, not one per slot) |
| T6.4 | **L**(chain) | the Callout guard + `modules/callouts.lua:7-11` | `callout-foreign-category.qmd` (`::: {#thm-x .callout-note}`) → assert exit **0**, stderr contains no `unknown callout prefix`, stderr **does** contain the fallback warning naming `thm`, and the callout's body text survives | none | the `crossref.categories.by_ref_type[ref_type] ~= nil` guard |
| T6.5 | **L**(chain) | the guard's warning specifically | Same fixture → assert at least one warning-severity diagnostic mentions the id `thm-x`; assert the count of such warnings is exactly 1 | none | the `warn(...)` at the fallback (leaving the fallback itself intact) |
| T6.6 | **L**(chain) | the guard is not `valid_ref_types()` | Same fixture rendered against a shim whose guard is `is_valid_ref_type(ref_type)` → assert exit **83** with `stderr.contains("unknown callout prefix")` | none | replacing the guard predicate with `valid_ref_types()`/`is_valid_ref_type` |
| T6.7 | **L**(chain) | the intersection fixture is non-discriminating (documentation) | `::: {#nte-setup .callout-note}` (Task 3's `callout-numbered.qmd`) rendered against **both** guard predicates → assert both runs exit 0 with identical captured ASTs | none | none — shape-only; see the vacuity check |
| T6.8 | I | P4 Task 10's channel, P5's fixture | `proof-missing-type.qmd` through `render_qmd_to_pandoc` → assert the returned error's payload contains `proof.lua:81` **and** `attempt to index a nil value`, and that the temp JSON named in the message still exists | none | P4 Task 10's `.stderr(stderr_text)` field and its retention branch |
| T6.9 | **U** | the error-catalog data | Load the catalog → assert the two new codes exist with `subsystem == "pandoc"` and the canonical `docs_url` shape | none | either new catalog entry's `subsystem`/`docs_url` |

**Revert hunks, stated exactly:**
- T6.1 — Revert the fallback body (pass the wrapper Div through unchanged) →
  `assert!(!ast_text.contains("data-custom-type"))` in `test_unknown_wire_type_unwraps` RED.
- T6.2 — Revert by re-applying the wrapper's attr to the surviving content →
  `assert!(!surviving_classes.contains("callout-note"))` in
  `test_unknown_wire_type_drops_wrapper_attrs` RED. **This is the hunk P5's round-4 note warns
  about explicitly**, and nothing else in the suite covers it.
- T6.3 — Revert the shim's `warn(...)` for the unknown type →
  `assert_eq!(diags.len(), 1)` in `test_unknown_wire_type_warns_once` RED. Revert its
  de-duplication (warn per slot) → the same assertion RED from the other side.
- T6.4 — Revert the `by_ref_type` guard → `assert_eq!(outcome.status.code(), Some(0))` in
  `test_foreign_category_callout_does_not_abort` RED (`fail()` at `modules/callouts.lua:9`).
- T6.5 — Revert the fallback's `warn(...)` (keeping the fallback) →
  `assert_eq!(fallback_warnings, 1)` in `test_foreign_category_callout_warns` RED. **This is the
  compounding finding's own guard**: without it the fallback is a silent unnumbering plus a dangling
  reference.
- T6.6 — Revert the guard predicate to `is_valid_ref_type` →
  `assert_eq!(outcome.status.code(), Some(83))` in
  `test_valid_ref_types_guard_does_not_prevent_the_abort` RED. This is the round-4 correction's
  bound form.
- T6.7 — No revert: shape-only, see the vacuity check.
- T6.8 — Revert P4 Task 10's `.stderr(...)` field → `assert!(err.contains("proof.lua:81"))` in
  `test_proof_missing_type_surfaces_lua_traceback` RED. Revert its retention branch →
  `assert!(temp_json.exists())` in the same test RED.
- T6.9 — Revert a new catalog entry's `subsystem` to `lua` → `assert_eq!(entry.subsystem,
  "pandoc")` in `test_pandoc_shim_codes` RED.

### Refactor-induced vacuity check

- **This is the task the Callout guard lives in, and the collapse is exact.** The guard is
  `by_ref_type[…] ~= nil`, not `valid_ref_types()`. Because
  `decorate_callout_title_with_crossref` **already** filters on `is_valid_ref_type` at
  `modules/callouts.lua:33-35`, the wrong predicate is a *complete* no-op: it admits exactly the
  set Q1 has already admitted (D1). So a test whose fixture `ref_type` is in the intersection
  (`nte`, `fig`, …) passes under **both** predicates, and so does a test whose `ref_type` is outside
  `valid_ref_types()` entirely. **`#thm-x` is the only discriminating shape**, and T6.6 is the only
  test that distinguishes the two predicates. T6.7 records the non-discriminating case as a
  committed fact so a future edit cannot quietly swap the fixture to `#nte-x` and leave the suite
  green.
- **T6.4's `exit 0` is the discriminator; "the body text survives" is not.** Under the wrong guard
  the whole render aborts, so *no* output exists — meaning a text assertion is also RED. But a
  fallback that silently drops the callout's content would keep `exit 0` and lose the text, so both
  halves are needed and neither alone suffices.
- **T6.5 must count warnings, not merely find one.** The `L` chain emits other warnings (missing
  resources, etc.); `stderr.contains("thm")` is satisfied by the string `thm-x` appearing in an
  unrelated diagnostic. Count the warnings matching the specific `Q-18-*` code.
- **T6.8 asserts a *failure*, so it cannot discriminate P5's own code at all** — the shim has no
  code on this path by design. Its revert hunks are P4 Task 10's. Recorded here rather than dropped
  because P5's plan names the fixture and because T6.8 is the regression guard that the channel
  keeps working when P4's own `language`-omission fixture is eventually retired.
- **The `unknown-wire-type.qmd` fixture cannot be produced by Q2's own writer**, since Q2 emits only
  the eight real types. It must be a hand-written wire AST (or a fixture plus a one-line test-only
  string substitution on the serialized JSON). Say which in the test's doc comment — a fixture
  that silently fails to contain an unknown type would make T6.1/T6.2/T6.3 all vacuously green.

---

## Task 7: Layer-1 contract test — registry introspection, per-type name mapping, bidirectional totality, and the Route-N function arity probe

**Scope.** The Q1-version drift tripwire against the `v1.11.3` pin: introspect Q1's live handler
registry from inside `main.lua`'s own Lua state, assert a **per-type name mapping** (not
set-equality), assert totality in **both** directions against P2's schema artifact, and assert each
Route-N global the shim depends on exists and accepts the expected arity.

**Files:**
- `resources/pandoc-filters/filters/quarto2-shim.lua` — ours; the env-gated introspection dump (see
  the mechanism finding).
- `crates/quarto-core/src/pandoc_filters/harness.rs` — extend; a `capture_layer1_introspection()`
  helper.
- `crates/quarto-core/tests/integration/pandoc_shim.rs` — extend.

**What must be introspected, and where it lives.** Q1's registry is
`quarto_global_state.extended_ast_handlers.handlers`, a table with three sub-tables —
`Inline` and `Block` (by class name) and `by_ast_name` (all handlers) — created by
`construct_extended_ast_handler_state()` (`ast/customnodes.lua:552-574`) and populated one entry
per `_quarto.ast.add_handler` call at `:462` (`state.handlers.by_ast_name[handler.ast_name] =
handler`). The public accessor is `_quarto.ast.resolve_handler(name, key)` (`:488-499`). Each
handler carries `ast_name`, `kind`, `class_name`, `constructor`, `parse`, and **optionally**
`slots` — `panel-tabset.lua` declares none at all (D6), and `customnodes.lua:438-447` treats a
missing `slots` as "no forwarder", warning only if it is present and not an array.

**Why this cannot be a plain sibling `--lua-filter`.** Measured
(pandoc 3.8.1): **each `--lua-filter` runs in its own Lua state.** A global set in one `-L` file
reads `nil` in the next. So a standalone probe script invoked as `pandoc -L main.lua -L probe.lua`
would see an **empty** `by_ast_name` and the whole Layer-1 test would be vacuously green. The probe
must therefore run *inside* `main.lua`'s state, which means it must be reachable from the vendored
tree's import graph.

**The probe is mechanism (a): an env-gated dump inside `quarto2-shim.lua` itself.** Rejected
alternative: (b) a second "ours" file in the vendored tree imported by P4 Task 8's marked patch —
it would keep the shim's production body free of test scaffolding, but costs an extra hunk in P4's
patch and an extra `## Ours vs. pinned` README entry, which (a) needs neither of. **P4 Task 8's
patch hunk is unchanged under (a)** — the census entry joins the shim's *own* filter group, which
that patch already splices in, so P4's companion needs no edit.

**The production hunk, concretely (call it H7).** One additional entry, appended last, in the
filter-group literal `quarto2-shim.lua` already returns for P4 Task 8's `tappend` call:

```lua
-- H7: Layer-1 registry census. Inert unless QUARTO2_LAYER1_DUMP names a path.
{
  name = "quarto2-layer1-census",
  traverser = 'jog',
  filter = {
    Pandoc = function(doc)
      local out = os.getenv("QUARTO2_LAYER1_DUMP")
      if out == nil then return nil end          -- inert: no AST change, no cost
      local census = {}
      for ast_name, h in pairs(
        quarto_global_state.extended_ast_handlers.handlers.by_ast_name
      ) do
        census[ast_name] = {
          kind       = h.kind,
          class_name = h.class_name,
          slots      = h.slots,                  -- nil stays nil; see T7.6
        }
      end
      local f = io.open(out, "w")
      if f then                                  -- warn, never abort: see below
        f:write(quarto.json.encode({
          handlers = census,
          routes   = quarto2_shim.routes,         -- the shim-facing direction, T7.3
        }))
        f:close()
      else
        warn("quarto2-layer1-census: cannot write " .. out)
      end
      return nil                                 -- document passes through unchanged
    end,
  },
}
```

**Verified against the pinned tree, not invented:** `quarto.json.encode` is a real global at
`v1.11.3` and this is very nearly Q1's own idiom — `crossref/index.lua:131-138` encodes a table
and writes it to a path with exactly this `io.open` / `write` / `close` / `warn`-on-failure shape.
`quarto_global_state.extended_ast_handlers` is the documented access path
(`ast/customnodes.lua:49`, `:414`, `:489`; assigned at `:562`).

Four properties the tests below depend on, so they are stated rather than left to the
implementer: it returns `nil` (not `doc`) on both paths, so the census is **never** able to
perturb the AST the Layer-2 goldens assert on; it emits `handlers` **and** `routes` in one file,
so T7.2 and T7.3 read the same artifact and cannot drift apart; it encodes a missing `slots`
as JSON `null` rather than `[]`, which is the whole of T7.6; and an unwritable path **warns rather
than aborts** (Q1's own idiom above), because this hunk ships in the production shim and a
mistyped `QUARTO2_LAYER1_DUMP` must not be able to fail a user's render. The cost of that choice
is that a silently-unwritten census would read as a *missing file* to the harness — T7.1's
`capture_layer1_introspection()` must therefore treat "file absent" as a hard error, not an empty
census, which is the same vacuity trap in a different coat.

**Why appended last.** The census must observe the registry *after* every `add_handler` call has
run. Placing it earlier in the shim's own group would still be after `quarto_init_filters` (P4
Task 8 splices the group there), but appending it last within the group also keeps it after the
shim's own route-table construction, which T7.3 reads.

**Rejected mechanisms, recorded so they are not re-proposed:** a standalone probe that `dofile`s
`main.lua` re-runs the whole document pipeline (`main.lua:737` `run_as_extended_ast(...)` executes
at load); a standalone probe that imports `ast/customnodes.lua` plus each `customnodes/*.lua`
by hand re-implements `main.lua`'s 170-line import block and would drift silently — which is the
exact failure Layer-1 exists to catch.

**The Route-N arity probe.** For each of `refPrefix`, `refNumberOption`, `subrefNumber`,
`refHyperlink`, `refDelim`, `crossrefOption`, `nbspString`: assert `type(_G[name]) == "function"`
**and** assert the declared parameter count via `debug.getinfo(fn, "u").nparams` against a recorded
literal (`refPrefix` 2, `refNumberOption` 2, `subrefNumber` 1, `refHyperlink` 0, `refDelim` 0,
`crossrefOption` 2, `nbspString` 0 — all read from the `v1.11.3` sources cited in Task 4). A rename
is *already* loud (`attempt to call a nil value` → pandoc nonzero exit → P4 Task 10's diagnostic);
the arity assertion is what converts a **signature change on the same name**, which would
otherwise silently produce wrong output, into a contract-test failure.

**Acceptance criterion.** H7 is present in `quarto2-shim.lua`'s filter group and writes a census
when `QUARTO2_LAYER1_DUMP` is set; the census contains an entry for each of the five Route-R
`ast_name`s with its `kind`, `class_name` and `slots` (or an explicit nil marker); the per-type
mapping table in D6 is asserted entry by entry; both totality directions hold against
`custom-node-schema.json`; all seven Route-N globals exist with the recorded arity; and the same
fixture renders to an identical AST with the env var set and unset (T7.7).
`L_TIER_TEST_COUNT` bumped.

**Prerequisite.** Tasks 1-5. **P2 Task 1** (the schema artifact + its Rust loader) — without it
there is nothing for T7.2/T7.3 to be total against. **P4 Task 8** (the `main.lua` splice that puts
the shim's filter group in the chain at all) — unchanged by mechanism (a). No open decision
remains; this task is dispatchable.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T7.1 | **L**(chain) | Q1's live `by_ast_name` registry, from inside `main.lua`'s state | Run `main.lua` with `QUARTO2_LAYER1_DUMP=<tmp>`, load the census → assert it contains all five Route-R `ast_name`s (`Callout`, `Tabset`, `Theorem`, `Proof`, `FloatRefTarget`) **and** that its total size is `>= 13` (the `ast_name` count in the pinned tree), so an empty or partial census cannot read as green; **and** that the file exists at all (absence is a hard error, not an empty census) | **nothing mocked** — the registry is the unit under test and must never be hand-written | **H7**, the `quarto2-layer1-census` filter-group entry in `quarto2-shim.lua` |
| T7.2 | I | the census × `custom_node_schema` — **Q1-facing** direction | For each of the five Route-R types in the schema, assert the census has a handler, and assert the Q1 `slots` value equals the literal recorded in D6 **per type** (`{div,name}` for Theorem/Proof, `{title,content}` for Callout, `{content,caption_long,caption_short}` for FloatRefTarget, **nil** for Tabset) | none | a mapping table entry (e.g. swapping Theorem's `div`↔`name`) |
| T7.3 | I | the shim's route table × the schema — **shim-facing** direction | Assert every `type_name` in the schema whose `route` is `R` or `N` has an entry in `quarto2_shim.routes`, and that `routes` has no entry absent from the schema | none | adding a ninth type to the schema without a shim route (the direction no golden can catch — D7) |
| T7.4 | **L**(chain) | the seven Route-N globals | For each name assert `type(fn) == "function"` **and** `debug.getinfo(fn,"u").nparams == <recorded>` | none | any recorded arity literal; and, upstream, a Q1 signature change on the same name |
| T7.5 | U | the recorded arity literals vs. the pin | Assert the arity table's length is 7 and that the module records `QUARTO_CLI_PIN` (P4 Task 1's constant) alongside it, so a re-vendor that changes the pin forces a re-read | none | the pin constant reference |
| T7.6 | I | Tabset's explicit N/A | Assert the mapping table's Tabset entry is an explicit "no `slots` declared" marker, **not** an empty list or a missing key | none | representing Tabset's N/A as `[]` |
| T7.7 | **L**(chain) | H7's inertness when unset | Run the **same** fixture twice through `run_main_lua_capturing_ast`, once with `QUARTO2_LAYER1_DUMP` set and once unset → assert the two ASTs are identical | **nothing mocked** | H7's `if out == nil then return nil end` early guard |

**Revert hunks, stated exactly:**
- T7.1 — Revert **H7** (delete the `quarto2-layer1-census` entry from `quarto2-shim.lua`'s
  filter-group literal) → `capture_layer1_introspection()` finds no census file and
  `test_q1_handler_registry_is_populated` RED at its file-exists precondition. **Two assertions
  are doing distinct work here**: the file-exists precondition catches H7 being absent or its
  write failing; `assert!(census.len() >= 13)` catches H7 present but observing an empty registry
  (the wrong-probe-mechanism failure, measured). An empty census would satisfy any "for each
  expected type, if present then …" formulation, and a missing file would satisfy any formulation
  that defaults to an empty census — so neither assertion is redundant.
- T7.7 — Revert H7's early `if out == nil then return nil end` guard (so the census body runs
  unconditionally) → `assert_eq!(ast_with_dump, ast_without_dump)` in
  `test_layer1_census_does_not_perturb_the_ast` RED. **Why this row exists:** nothing else binds
  the census's non-interference. The Layer-2 goldens (Task 8) never set `QUARTO2_LAYER1_DUMP`, so
  they exercise only the inert path and would stay green no matter what the active path did to the
  document. The seam has to run the *same* fixture twice — once with the env var set, once without
  — and compare the two ASTs, which is the only formulation that discriminates.
- T7.2 — Revert Theorem's mapping entry from `{div, name}` to `{content, title}` →
  `assert_eq!(mapping["Theorem"], ["div", "name"])` in `test_per_type_slot_mapping` RED. A
  set-equality formulation would be RED against *correct* code for four of seven types and would
  also fail to catch a `div`↔`name` swap.
- T7.3 — Revert by adding a ninth schema type with no shim route →
  `assert!(missing_routes.is_empty())` in `test_shim_routes_cover_the_schema` RED. **This is the
  only mechanical guard for that case** (D7) and must not be deferred.
- T7.4 — Revert `refNumberOption`'s recorded arity from 2 to 1 →
  `assert_eq!(nparams("refNumberOption"), 2)` in `test_route_n_globals_have_expected_arity` RED.
- T7.5 — Revert the `QUARTO_CLI_PIN` reference → `assert_eq!(recorded_pin, QUARTO_CLI_PIN)` in
  `test_arity_table_is_pinned` RED.
- T7.6 — Revert Tabset's N/A marker to `[]` →
  `assert!(matches!(mapping["Tabset"], SlotMapping::NotDeclared))` in
  `test_tabset_slots_are_explicitly_not_declared` RED.

### Refactor-induced vacuity check

- **Set-equality is both wrong and non-discriminating**, which is why the plan calls for a per-type
  mapping. Verified today (D6): `theorem.lua:85` and `proof.lua:52` declare `{ "div", "name" }`
  against Q2's `content`/`title`; `panel-tabset.lua` declares no `slots` key at all. A
  `assert_eq!(q1_slots.sorted(), q2_slots.sorted())` test would be RED on Theorem, Proof and Tabset
  against correct code — and, if "fixed" by sorting both sides into a union, would stop
  discriminating a swap.
- **The Layer-1 test's characteristic failure mode is an empty registry reading as green.** Every
  formulation of the shape "for each type I expect, if the census has it, check its slots" is
  vacuously true for an empty census — which is precisely what the wrong probe mechanism produces
  (measured). T7.1's `>= 13` size assertion is the guard, and it must come *before* the per-type
  assertions in the same test module so a mechanism regression is unambiguous. **Under the chosen
  mechanism (a) the trap has a second mouth:** H7 warns rather than aborts on an unwritable path,
  so a failed write yields *no file* rather than an empty one, and any harness that defaults a
  missing file to an empty census re-enters the same vacuity from the other side. Hence T7.1
  asserts file-existence as a hard precondition as well as the size — two assertions, two distinct
  failure modes, neither redundant.
- **`debug.getinfo(...).nparams` is the discriminator for a signature change; the name check is
  not.** A rename already produces a loud runtime failure; only a same-name arity change is silent.
  Note the limit honestly: `nparams` does not detect a change in a parameter's *meaning* (e.g.
  `refNumberOption(type, entry)` becoming `refNumberOption(type, order)`). That residual is
  logged in the **Missing-test pass**.
- **T7.3's assertion must be bidirectional in one test, not two.** Splitting it invites a later
  refactor that keeps the schema-facing half and drops the shim-facing half — and the shim-facing
  half is the one no golden can substitute for.

---

## Task 8: Layer-2 per-type goldens — a narrow post-filter-AST harness, committed as `insta` snapshots

**Scope.** P5's checklist offers a choice: build a narrower one-off harness for per-type Lua-level
assertions, or explicitly defer Layer-2 until after P7. **This task takes the first option and says
so**, resolving the scheduling contradiction the checklist leaves open.

**Rationale for not deferring.** P7's `cargo xtask capture-pandoc-goldens` harness needs a real Q1
`quarto` binary and `external-sources/` (a `G` tier, out of CI) and extracts *semantic text from
rendered docx*. P5's Layer-2 needs neither: Task 1's `run_main_lua_capturing_ast` already produces
a Pandoc-JSON AST from the real `main.lua` at the `L` tier, entirely in CI, and every per-type
assertion Tasks 2-5 need is expressible on it. So the one-off harness is not a parallel
implementation of P7's — it is a *different artifact at a different tier*, and P7's Q1-parity
capture remains the thing that needs P7.

**Files:**
- `crates/quarto-core/tests/integration/pandoc_shim_goldens.rs` — new; registered
  `pub mod pandoc_shim_goldens;` in `crates/quarto-core/tests/integration/main.rs`.
- `crates/quarto-core/tests/integration/snapshots/integration__pandoc_shim_goldens__*.snap` — the
  committed snapshots. **Per `.claude/rules/integration-tests.md`, insta filenames carry the
  `integration__` prefix** because `module_path!()` now starts with the binary name.
- `crates/quarto-core/tests/fixtures/pandoc_shim/` — reuse Tasks 2-6's fixtures; add none.

**The named fixture and expected-output shape P5 asks for before the harness is written.** Fixture:
`float-basic.qmd` (`#fig-x` with a caption), target **docx**. Expected shape: a normalized
projection of the captured post-filter AST — for each node, `(tag, identifier, classes, text)` with
raw-block payloads included verbatim — snapshotted. The projection, not the raw AST, is what is
committed: the raw AST carries `s:` source ids that churn on unrelated pampa changes, which would
make every snapshot a false positive.

**Acceptance criterion.** One snapshot per wire type routed by the shim that reaches a Pandoc target
(five R + two N = seven), each asserting the type's numbered/rendered shape; `cargo insta test`
green; every snapshot's diff reviewed and its content summarized in the commit message per
CLAUDE.md's snapshot-test rule (count added, what changed, anything surprising flagged).
`L_TIER_TEST_COUNT` bumped.

**Prerequisite.** Tasks 1-6. Deliberately **not** P7: `seam deferred until P7's
capture-pandoc-goldens` applies only to the *Q1-parity* comparison, which this task does not
attempt and must not be read as providing.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T8.1 | **L**(chain) | the whole shim, per type | For each of the seven types, render its fixture to docx via `run_main_lua_capturing_ast` and `insta::assert_snapshot!` the normalized projection | **nothing mocked** | any one route body in `quarto2-shim.lua` |
| T8.2 | U | the projection function | Feed a fixed AST containing two nodes with different `s:` ids but identical structure → assert the projection is identical (source ids are excluded) | none | including `s` in the projection |
| T8.3 | **L**(chain) | the projection's completeness | For each snapshot, assert the projected text is non-empty **and** contains no `data-custom-type` | none | a projection that drops raw-block payloads (under which the callout snapshot would be empty) |
| T8.4 | U | the `L`-tier census (P4 T2.6) | Assert `L_TIER_TEST_COUNT` equals the actual count after P5's additions | filesystem read of the crate's own test sources | forgetting to bump the constant |

**Revert hunks, stated exactly:**
- T8.1 — Revert the Theorem route body to the unrecognized-type fallback → the Theorem snapshot
  diff (the `theorem-title` Span and the `Theorem 1` prefix disappear, the body text stays) in
  `test_theorem_golden` RED. Revert the Callout route body → the Callout snapshot RED. And so on
  per type — **one named hunk per snapshot**; a snapshot with no such hunk should not be committed.
- T8.2 — Revert the projection to include `s` → `assert_eq!(proj_a, proj_b)` in
  `test_projection_excludes_source_ids` RED.
- T8.3 — Revert the projection to skip `RawBlock` payloads →
  `assert!(!projected.is_empty())` in `test_callout_golden_is_not_empty` RED (docx callouts are
  entirely raw openxml — `quarto-post/docx.lua:6-203`).
- T8.4 — Revert (omit) the census bump → `assert_eq!(found, L_TIER_TEST_COUNT)` in P4's
  `test_l_tier_census` RED.

### Refactor-induced vacuity check

- **A text-extracting golden is structurally blind to the failure this epic most cares about**
  (D7): the unrecognized-type fallback preserves slot content, so a fully semantics-stripped node
  produces a *zero-byte* snapshot diff. That is why T8.1's per-type revert hunks must be named
  individually, and why Task 7's T7.3 is not optional. **Do not let Task 8's existence be read as
  covering totality.**
- **The projection is where vacuity would enter silently.** If a future change drops raw-block
  payloads from the projection "to reduce noise", the docx callout, theorem-latex and float
  snapshots all collapse to their body text and stop discriminating their entire renderers — while
  staying green, because the snapshot would be re-accepted. T8.3 is the guard, and it must assert
  non-emptiness per snapshot rather than in aggregate.
- **`cargo insta review`'s accept-everything ergonomics is the standing risk here**, which is why
  the acceptance criterion invokes CLAUDE.md's snapshot rule explicitly: report the count, summarize
  what changed, flag anything unexpected, and list the affected `.snap` files after committing.
- **The Equation snapshot must be the docx one** (D3); a latex Equation snapshot would be stable
  across the presence and absence of the `order` argument entirely.

---

## Missing-test pass

Behaviour with no seam above, each given a bound seam or an explicit `accepted-untested`. The
mandated verdicts first.

**1. The Callout `fail()`-guard fallback — that the render *completes* and that the warning
*fires*.** **Both bound.** Completion: T6.4 (`exit 0`, no `unknown callout prefix` on stderr).
Warning: T6.5, counting diagnostics carrying the specific `Q-18-*` code — a `contains("thm")`
formulation would be satisfied by unrelated stderr. The predicate itself is bound separately by
T6.6, the only test that distinguishes `by_ref_type` from `valid_ref_types()` (D1). The dangling-
`@ref` consequence — Route N renders `Note 3` for a callout that renders unnumbered — is
**`accepted-untested`: it is the *designed* outcome of two independently-correct code paths, recorded
as a design §12 limitation with a warning at the fallback; a test asserting it would encode the
limitation as a contract and would have to be deleted rather than fixed if `bd`-tracked work ever
closes the gap.** The warning is the mitigation and it is bound.

**2. The unrecognized-`type_name` warning.** **Bound, in three parts, because P5's own note makes
this three assertions rather than one.** Slot content survives: T6.1. The wrapper is gone: T6.1's
`no data-custom-*` half. Exactly one warning: T6.3 (both polarities — missing warning, and a
warning per slot). And the part P5 flags specifically — an implementation that "unwraps but
preserves the wrapper's attributes onto the surviving content" would re-arm the class-keyed-handler
collision — is **T6.2**, whose revert hunk is exactly that re-application. Nothing else in the suite
covers T6.2's hunk.

**3. The Route-R constructor crash (`proof-missing-type.qmd`).** **Bound as a visibility assertion
only, and deliberately so.** The crash *is* the signal; P5 adds no code on this path. The seam is
therefore on P4 Task 10's diagnostic — stderr verbatim plus temp-JSON retention — asserted by T6.8
with P5's fixture, alongside P4's own T10.4 with its `language`-omission fixture. **Stated plainly:
T6.8 cannot discriminate any P5 hunk.** It is the regression guard that the channel keeps working
once P4's placeholder-era fixture is retired, and the record that P5's named fixture actually exists.

**4. The bidirectional-totality assertion.** **Bound, T7.3, and not deferred.** P5 is explicit that
the golden harness cannot catch a 9th wire type shipping without shim support (unwrap-and-drop
preserves slot content → zero-byte snapshot diff, D7), so this is the only mechanical guard. It is
one test with both directions, not two — splitting it invites dropping the load-bearing half.

**5. The bottom-up traversal contract.** **Bound, T1.4**, with the nested fixture P6 Finding 5 also
needs (a `FloatRefTarget` inside a `Callout`'s `content` slot). The discriminator is the **inner**
node's caption prefix, not "it rendered" — a topdown traversal still produces a numbered outer
callout and a syntactically valid document. The "path was actually exercised" assertion is the
absence of `data-custom-type` from the captured AST. P4 T8.3 binds the *field*
(no `traverse = 'topdown'`); T1.4 binds the *effect*, which P4 explicitly could not
(P4 missing-test item 10).

**6. The Layer-1 Route-N function existence/arity probe.** **Bound, T7.4/T7.5**, and the seam is
deliberately the **arity**, not the name: a rename is already loud (`attempt to call a nil value`
→ pandoc nonzero exit → P4 Task 10's diagnostic), so a name-only check cannot discriminate a silent
signature change at the same arity. **Residual, `accepted-untested`: `nparams` cannot detect a
change in a parameter's *meaning* at constant arity** — `refNumberOption(type, entry)` becoming
`refNumberOption(type, order)` would keep arity 2 and silently produce wrong numbers. The
behavioural backstop is T4.1's exact-equality assertion on `Figure\u{a0}1`, which *would* move; the
contract test cannot see it. Recorded rather than papered over.

**7. Layer-2's scheduling contradiction.** **Resolved, not left contradictory:** Task 8 builds the
narrower one-off harness (post-filter AST projection + `insta`) and runs in CI at the `L` tier;
P7's `capture-pandoc-goldens` remains the `G`-tier Q1-*parity* artifact and is not a prerequisite.
Stated in Task 8's rationale so a future reader does not read Task 8 as a duplicate of P7's.

**Further items this pass surfaced (not in the mandated list):**

**8. The JSON-decoder integer trap.** **Bound, T1.1 (`U`-cheap tier via `pandoc lua`) plus the
negative assertions T4.3 and T5.2.** Measured: `pandoc.json.decode` yields a float and
`format.lua:184`'s `tostring` renders `1.0`. Worth naming separately because the natural assertion
(`contains("Figure\u{a0}1")`) is GREEN under the bug.

**9. The attr sanitizer.** **Bound, T1.2 (both halves: the class filter and the attribute
deletions) and T3.3 (`no __quarto_custom_node` on a rendered callout).** Named separately because
every Route-R constructor receives the attr, so a sanitizer bug is a seven-type defect that no
single route's test would localize.

**10. `FloatRefTarget`'s `kind`→`type` rename.** **Bound, T2.3**, whose revert hunk is the mapping
line itself.

**11. Tabset's `need_emulation == false` return branch.** **Bound, T3.5** — a test anchored only
at `customnodes.lua:263` would miss Tabset's path, which takes `:455-457` instead.

**12. The `plain_data.group` field Tabset carries and Q1 ignores.** **`accepted-untested`: it has no
Q1 counterpart at all (an HTML-only grouped-tab-sync signal), so there is no observable difference
between reading it and ignoring it. A test would assert the absence of a code path.** Task 3's
field-map prose says it is deliberately unused, which is the record.

**13. Multi-ref joining and subfloat refs (Route N).** **`accepted-untested` by construction: Q2
emits one `CrossrefResolvedRef` per `@ref` and drops additional ids upstream of the cut
(`crossref_resolve.rs:540`'s own test documents this), and `plain_data` carries no `parent`, so
neither branch is reachable in v1.** `refDelim` and `subrefNumber` are still arity-probed (T7.4) so
an upstream signature change is loud when the branches do become reachable.

**14. The `#cite.prefix > 0` branch (Route N, cite-mode-aware half).**
`seam deferred until P2 Finding 3 is answered` — a citation prefix is `Inlines`, `plain_data` is
contractually AST-free, and the slot-shaped alternative is undecided. `cite_mode` and `label_upper`
are unaffected and in scope once P2 Task 3 lands.

**15. The end-to-end CLI verification CLAUDE.md requires.** `cargo run --bin q2 -- render x.qmd
--to docx` is still rejected by `crates/quarto/src/commands/render.rs:680-684`; relaxing it is
**P7-foundation's** checklist item (`2026-09-20-pandoc-hybrid-P7-foundation.md` Task 3). P5's
highest-fidelity entry point is therefore `render_qmd_to_pandoc` in-process plus a real `pandoc`
subprocess (T2.6, T6.8). Per CLAUDE.md's own instruction, P5's completion report must say so
explicitly: *"Tests pass, including real `pandoc` subprocesses against the vendored `v1.11.3` Lua
and the real shim; I did not verify through the `q2` binary, because the CLI format gate that
admits docx is P7-foundation's."* Same bar P4's companion sets for itself.

**16. Windows.** **`accepted-untested` on Windows, bound as portable logic elsewhere:
`test-suite.yml:28`'s matrix is `[ubuntu-latest, macos-latest]` — there is no Windows CI leg.**
Required mitigations: every path the harness builds uses `Path::join` (never a literal `/`), and the
Lua side needs no change (`init.lua:123-131` derives its separator from `package.config:sub(1,1)`).
The residual risk is the observer probe's `io.open` path on a Windows temp directory.

