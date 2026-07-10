# P3 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-18
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P3-upstream-crossref.md`](2026-08-20-pandoc-hybrid-P3-upstream-crossref.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md) (§7)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Depends on:** (per the epic's graph) nothing — P1/P2/P3 are parallel immediately, and that is
**unchanged** by the 2026-09-18 additions (Task 8 is upstream-only, `Q` tier). **One caveat:**
Task 7, the q2-side `L`-tier matrix, is blocked on capabilities P4 owns — see
`## Findings for Gordon` #1.
**Status:** Ready for subagent-driven execution, with Task 7 explicitly parked behind named
prerequisites. **Eight dispatchable tasks, Tasks 1–8** — note Task 8 is ordered *before* Task 5
(which opens the PR carrying it); numbers here are identifiers, the `Prerequisite` fields are the
ordering. **Updated 2026-09-18:** Gordon's two decisions are applied — the missing callout
`order == nil` guard is folded into the upstream patch as anchor **A7** (Task 3), which also
unblocks Task 7's labeled-callout fixture; and P3's checklist item 3 (the positive external-mode
golden) has **moved to P6 Task 4** (its old section is now a non-dispatchable pointer, and the
number 8 has been reused). **Also updated the same day:** Gordon's third decision adds **Task 8**,
the TypeScript half of the upstream patch (`crossref: numbering:` key, schema entry, params read,
two smoke tests), carried by Task 5's PR — resolving Findings #1.

This file adds nothing to P3's scope — it converts P3's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P3 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P3 + the design doc; where this file and the plan disagree, the plan wins.

---

## Tiers used in this file

| Tier | What it is | How it runs |
|---|---|---|
| **U** | Rust unit test — `#[test]` in a `mod tests` inside the crate under test | `cargo nextest run -p <crate>` |
| **I** | Rust integration test — `crates/<crate>/tests/integration/<name>.rs`, registered as `pub mod <name>;` in that crate's `tests/integration/main.rs`. **Never** a top-level `crates/<crate>/tests/<name>.rs` (`.claude/rules/integration-tests.md`) | `cargo nextest run -p <crate>` |
| **L** | Lua/pandoc integration test — a real `pandoc --data-dir … -L main.lua` invocation against the materialized vendored Q1 tree | `cargo nextest run -p quarto-core`, hard-gated on pandoc (see "L-tier gate policy" below) |
| **Q** | quarto-cli's **own** suite — `tests/unit-lua/*.test.lua` via `tests/smoke/lua-unit/lua-unit.test.ts`, and `tests/smoke/**/*.test.ts` via `tests/run-fast-tests.sh` / `run-parallel-tests.sh`. Runs **in the quarto-cli repo**, by the P3 implementer and by upstream CI on the PR. **Not** part of `cargo xtask verify` and not run by any q2 CI leg. |
| **G** | Dev-only golden capture — needs a real Q1 `quarto` binary and/or `external-sources/`. Local/dev gate only (CLAUDE.md External Sources Policy: not in CI). Committed artifacts are `insta` snapshots. |

### L-tier gate policy (stated once; referenced by every L row)

**No silent skip.** This repo already has the precedent and it is a *hard-fail*, not a skip:
`crates/pampa/tests/integration/test.rs:160-180`'s `assert_good_pandoc_version()` panics with an
actionable message when the local `pandoc` is outside a calibrated window
(`PANDOC_ORACLE_MIN_VERSION = (3, 6)` / `PANDOC_ORACLE_MAX_VERSION = (3, 10)`,
`test.rs:117-118`), with a single deliberate escape hatch
(`PAMPA_PANDOC_ORACLE_BYPASS_VERSION_GATE=1`, `test.rs:144-156`) that exists only so
`cargo xtask pandoc-check` can recalibrate. Every L row below adopts that shape: **absent or
out-of-range pandoc ⇒ the test fails loudly**, because a silently-skipping test is a vacuous test.

**Version floor, verified 2026-09-18.** Quarto `v1.11.3` (the epic's vendoring pin) declares
`export PANDOC=3.10` (`git show v1.11.3:configuration`); this repo's CI installs **3.8.3**
(`.github/workflows/test-suite.yml:18`, `ts-test-suite.yml:18`); `cargo xtask dev-setup`'s
`check_pandoc` floors at **3.6** (`crates/xtask/src/dev_setup.rs:282-305`); the dev machine used
for this pass has **3.8.1**. Reconciling that is **P4's checklist item**, not P3's — but every L
row here is inert until it is, so each L row names it as a prerequisite rather than assuming it.

**Precedent trap to copy:** pampa's gate ledger notes that `ORACLE_TEST_NAMES` in
`crates/xtask/src/pandoc_check.rs` is hand-maintained, so a newly-added gated test silently isn't
covered by `cargo xtask pandoc-check` until someone edits that list. Any new L-tier test added by
Tasks 7–8 must be registered in whatever equivalent list P4's pandoc preflight introduces, in the
same commit.

---

## Which tree does a hunk live in?

P3 is the only plan in this epic whose production hunks mostly land **outside this repo**. Three
distinct trees are in play, and a revert-hunk claim is meaningless without naming which:

1. **Upstream quarto-cli** — `/Users/gordon/src/quarto-cli/src/resources/filters/…`, the PR's
   target. Tests that redden on a revert here are **Q tier**, run in that repo.
2. **The vendored copy inside q2** — P4's `resources/pandoc-filters/` (path per P4's plan;
   `include_dir!` for storage, `ResourceBundle` to materialize to disk at invocation time, because
   `main.lua` needs real files). **This directory does not exist today** — verified
   2026-09-18: `ls resources/` shows no `pandoc-filters`. Tests that redden on a revert here are
   **I** (file-content tripwire) or **L** (behavioral), and run in q2.
3. **q2's own Rust** — nothing in P3 changes q2 Rust. P3's Rust-side surface is test code only.

**A q2 test guarding an upstream hunk via the vendored tree is legitimate and is the whole point**
— P3's own plan states the philosophy: "a behavioral test (render with `crossref-numbering:
external` and assert the caption prefix appears) fails immediately if a future re-vendor silently
overwrites the edited files, without needing bespoke diff tracking." Two consequences to be
precise about:

- **On a re-pin, the vendored copy is overwritten wholesale.** The patch is not a `.patch` file
  applied by tooling — P4 vendors by directory copy, and P3's edits are made directly in that copy
  with `QUARTO-PATCH(upstream PR …)` markers. So a re-vendor drops the patch *silently* unless a
  test asserts its presence. Task 6 owns that tripwire.
- **After upstream merges, the markers are removed but the code stays.** A tripwire keyed only to
  the marker comment would then fail for the right reason in the wrong direction. Task 6's spec
  separates the two assertions for exactly this reason (marker inventory vs. behavioral anchor).

---

## Verified anchor inventory (2026-09-18, read against real code)

Cited by **anchor text first, line number second** — per P3's own checklist item, because P4's
`main.lua` splice inserts a line between `main.lua:712` and `:713` and silently renumbers the
assignment gate from `718` to `719`.

| # | Anchor text (stable) | File (upstream tree) | Line today | Class |
|---|---|---|---|---|
| A0 | `local enableCrossRef = param("enable-crossref", true)` | `src/resources/filters/main.lua` | 227 | param read, not a gate |
| A1 | `if enableCrossRef then` (guarding `tappend(quarto_filter_list, quarto_crossref_filters)`) | `src/resources/filters/main.lua` | 718 (append at 719) | **assign-numbers**, opposite polarity |
| A2 | `if not param("enable-crossref", true) then` inside `function decorate_caption_with_crossref(float)` | `src/resources/filters/customnodes/floatreftarget.lua` | 196 (fn at 195) | present-numbers |
| A3 | `if not param("enable-crossref", true) then` inside `function full_caption_prefix(float, subfloat)` | `src/resources/filters/customnodes/floatreftarget.lua` | 242 (fn at 241) | present-numbers |
| A4 | `return _quarto.format.isIpynbOutput() and param("enable-crossref", true)` | `src/resources/filters/customnodes/floatreftarget.lua` | 965 (registration 964) | present-numbers (pair) |
| A5 | `return _quarto.format.isIpynbOutput() and not param("enable-crossref", true)` | `src/resources/filters/customnodes/floatreftarget.lua` | 982 (registration 981) | present-numbers (pair) |
| A6 | `if not param("enable-crossref", true) then` inside `local function decorate_callout_title_with_crossref(callout)` | `src/resources/filters/modules/callouts.lua` | 22 (fn at 20) | present-numbers |
| — | `layout/ipynb.lua:121`/`126` PanelLayout pair | `src/resources/filters/layout/ipynb.lua` | 121, 126 | **verified no-op again** — both `add_renderer` calls pass the identical `render_ipynb_layout` callback (read 2026-09-18, lines 120-127). No change. P3's audit is right and the external claim that it "misses" these is wrong. |
| — | `if order == nil then return el end` | `src/resources/filters/customnodes/theorem.lua` | 278 | already correct, no patch |

**Two facts the implementer needs and neither the plan nor the design doc states:**

- **`v1.11.3 == current HEAD` for every file P3 touches.** `git diff --stat v1.11.3..HEAD` over
  `main.lua`, `floatreftarget.lua`, `modules/callouts.lua`, `layout/ipynb.lua`,
  `customnodes/theorem.lua` is **empty** (checkout at `v1.11.5-1-g83d48d8e8`). So the line numbers
  above are simultaneously valid for today's upstream HEAD and for the pinned vendoring tag — a
  re-pin to a newer tag does not currently move any of them.
- **`enable-crossref` is not a user-facing YAML key.** The only writer is
  `src/command/render/filters.ts:534`, `params[kEnableCrossRef] = false` inside
  `ipynbFilterParams`, guarded by `options.format.render[kIpynbProduceSourceNotebook]`. So
  `enable-crossref: false` is reachable upstream **only** through the manuscript
  "produce source notebook" ipynb path, and **`grep -rln 'enable-crossref' tests/` in quarto-cli
  returns nothing** — there is no existing upstream test that names the flag. This reshapes
  P3's "Q1 tests bit-for-bit on default **and** on `enable-crossref: false`" checklist item into
  something concrete; see Task 4.

---

## Task 1: Re-anchor the patch-site citations to anchor text

**Scope.** Replace bare line-number citations of P3's patch sites with anchor text (plus a
line number as a secondary hint) in P3's own plan and in the epic doc, and record the
two-marker-category note P3's checklist calls for. Documentation only; no code.

**Files.**
- `claude-notes/plans/2026-08-20-pandoc-hybrid-P3-upstream-crossref.md` (q2) — the `main.lua:718`
  citations in "The change", the audit table, and "Fallback policy".
- `claude-notes/plans/2026-08-20-pandoc-hybrid-epic.md` (q2) — the single `main.lua:718` citation
  in "What the deep pass changed".
- The vendoring-README half of the item belongs to **P4's** `resources/pandoc-filters/README.md`
  and is a hand-off, not an edit here.

**Acceptance criterion.** `grep -n 'main.lua:718' claude-notes/plans/2026-08-20-pandoc-hybrid-*.md`
returns only occurrences that also carry the anchor text `if enableCrossRef then` on the same or
adjacent line; the two marker categories (`QUARTO-PATCH(upstream PR …)` = P3, upstreamable;
`Q2-local, permanent` = P4's `quarto_pandoc_shim_filters` splice) are named in P3's Fallback
policy section with an explicit note that P4's README must carry the same inventory.

**Prerequisite.** None. Schedulable immediately, in parallel with P1/P2.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T1.1 | — | (none) | — | — | `accepted-untested` — see below |

**Revert hunks, stated exactly:**

- T1.1 — **`accepted-untested`: plan-file prose has no machine-checkable surface in this repo.**
  The rationale is not "docs don't need tests": it is that the *purpose* of the anchor-text
  discipline — catching a site that moved — is bound elsewhere, by **T6.1b/T6.1c**, which assert
  the anchor text against the vendored tree and redden if it moves or is overwritten. Adding a
  `cargo xtask lint` rule for plan-file citations is out of P3's scope and is not proposed here.

---

## Task 2: Add the two predicates; convert the assign-numbers gate

**Scope.** In the upstream quarto-cli tree, extend the `enable-crossref` param read to also read
`crossref-numbering`, introduce the two predicates (`crossref_present()` and the
assign-numbers predicate), and convert the single assignment-group gate (anchor A1) to the
**opposite-polarity** predicate. Do not touch the render-decoration sites (Task 3).

**Files** (all **upstream quarto-cli**, not q2):
- `src/resources/filters/main.lua` — anchor A0 (param read, line 227), anchor A1
  (`if enableCrossRef then`, line 718; the append it guards is at 719).
- The predicate **definitions** must live somewhere `require`-able from a luaunit test with a
  mocked `param()` — the existing convention that satisfies this is a module under
  `src/resources/filters/modules/` returning a table (cf. `modules/callouts.lua`, which returns
  its table at line 347 and is consumed as `_quarto.modules.callouts.*`). A bare file-scope
  global in `crossref/format.lua` would *work* at runtime but is exactly the "undefended bare
  global" shape P3's own round-4 Finding argues against, and it is not unit-testable. **Naming
  and placement are the implementer's call; requirability-with-a-mocked-`param()` is an
  acceptance constraint**, because T2.1 depends on it.

**Acceptance criterion.**
1. `grep -c 'if enableCrossRef then' src/resources/filters/main.lua` is `0`; the append at 719 is
   guarded by the new assign-numbers predicate.
2. The predicates are exactly, per the plan (frozen): `crossref_present() = enableCrossRef or
   param("crossref-numbering", "quarto") == "external"`; assign-numbers `= enableCrossRef and
   param("crossref-numbering", "quarto") ~= "external"`.
3. T2.1 and T2.2 green; `tests/smoke/crossref/` green in the quarto-cli repo.

**Prerequisite.** A quarto-cli working tree and its Deno test toolchain
(`tests/configure-test-env.sh`). No q2-side prerequisite.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | Q | the two real predicate functions, `require`d from their module | new `quarto-cli/tests/unit-lua/crossref-numbering.test.lua`, registered in `LUA_TESTS` in `tests/smoke/lua-unit/lua-unit.test.ts:29-33`; drives the 2×2 cross-product plus an unknown-value row → asserts each predicate's boolean | `param()` only (the luaunit harness's documented convention: "Mocks any filter-runtime globals it needs (param, tcontains, …)"). The predicates themselves are real. | the `or param("crossref-numbering","quarto") == "external"` disjunct; the `and param(…) ~= "external"` conjunct |
| T2.2 | Q | `main.lua`'s real filter-list construction + the whole crossref assignment group, through a real `quarto render … --to docx` | **existing** `quarto-cli/tests/smoke/crossref/docx.test.ts` over `tests/docs/crossrefs/all-docx.qmd` → `ensureDocxRegexMatches` (`tests/verify.ts:1072`) asserts `>Figure 1: Elephant<` and `>Table 1: My Caption<` in `word/document.xml` | none — real `quarto`, real pandoc, real docx | anchor A1's polarity |

**Revert hunks, stated exactly:**

- T2.1 — Revert the `or param("crossref-numbering", "quarto") == "external"` disjunct from
  `crossref_present()` → T2.1's row `(enable-crossref=false, crossref-numbering="external")
  ⇒ crossref_present() == true` **RED**.
- T2.1 — Revert the `and param("crossref-numbering", "quarto") ~= "external"` conjunct from the
  assign-numbers predicate → T2.1's row `(enable-crossref=true, crossref-numbering="external")
  ⇒ assign_crossref_numbers() == false` **RED**.
- T2.1 — Replace `== "external"` with `~= "quarto"` (and/or `~= "external"` with `== "quarto"`)
  → T2.1's row `(enable-crossref=true, crossref-numbering="bogus") ⇒ present == true and assign
  == true` **RED**. This is the unknown-value discriminator; see the Missing-test pass.
- T2.2 — Invert anchor A1 (`if assign_crossref_numbers() then` → `if not
  assign_crossref_numbers() then`) → the crossref assignment group never runs in default mode →
  `float_title_prefix` (`crossref/tables.lua:223`) hits `if float.order == nil then warn(…); return
  {} end` at `:229-231` → no caption prefix → T2.2's `>Figure 1: Elephant<` regex **RED**.

### Refactor-induced vacuity check

- **The opposite-polarity gate (anchor A1) — the trap the parent instruction names.** A test that
  asserts "numbers are absent under external mode" is satisfied by a document with no numberable
  content at all, and T2.1 asserts only the *predicate's value*, which stays green under an
  inverted gate. **Both traps are addressed, in different places.** T2.2 is the upstream
  polarity discriminator and it works in the *default* direction: an inverted A1 loses
  `Figure 1:` from `all-docx.qmd`, which has real figures and tables. The *external* direction's
  polarity discriminator is **M3/M4 in Task 7**, whose "the path was actually exercised"
  assertion is the presence of the string `field 'order' is missing from float` on stderr — a
  warning only `crossref/tables.lua:230` can emit, and only when `decorate_caption_with_crossref`
  has passed its gate on a real float. An empty fixture cannot satisfy it.
- **"Default is bit-for-bit current behavior" is not tautological here, and the reason is
  specific.** By construction the *output* of a correct default-mode render is identical before
  and after; what makes T2.2 discriminating is that it is sensitive to A1's polarity, which the
  patch newly puts at risk — the pre-patch code had no polarity to get wrong. What T2.2 does
  **not** discriminate is the `"quarto"` default literal in `param("crossref-numbering",
  "quarto")`: omitting it makes `param()` return `nil`, and `nil ~= "external"` is true while
  `nil == "external"` is false, so both predicates behave identically to the `"quarto"` case.
  That is logged as `accepted-untested` in the Missing-test pass with that reasoning, not left
  implied.
- **The two-term disjunction needs the cross-product, and one cell is only reachable from q2.**
  A test run only in external mode cannot distinguish `enableCrossRef or external` from
  `external`; a test run only with `enable-crossref` true cannot distinguish it from `true`. The
  four cells and what each pins: `(T, quarto)` ⇒ present T, distinguishes from `external` alone;
  `(F, quarto)` ⇒ present F, distinguishes from `true`; `(T, external)` ⇒ assign F, the real
  external case; `(F, external)` ⇒ present T, **the only cell that distinguishes the disjunction
  from `enableCrossRef` alone**. T2.1 covers all four at the predicate level. At the *behavioral*
  level, cell `(F, external)` is unreachable in the quarto-cli repo — upstream never sets
  `crossref-numbering` at all (no TS writer; see Findings for Gordon #1) and sets
  `enable-crossref: false` only on the ipynb-source-notebook path. It is reachable **only** in
  q2's L tier, where the test builds `QUARTO_FILTER_PARAMS` itself. That is M4 in Task 7, and it
  is the single most load-bearing row in this whole document.

---

## Task 3: Convert the four present-numbers render-decoration gate sites

**Scope.** Mechanical, same-shape conversion of the four real render-decoration gates to
`crossref_present()` — one task, not four, per `superpowers:subagent-driven-development`'s
grouping preference. The ipynb pair becomes a `crossref_present()` / `not crossref_present()`
pair. `layout/ipynb.lua:121,126` is **not** touched (verified no-op, both branches dispatch the
identical callback).

**Files** (all **upstream quarto-cli**):
- `src/resources/filters/customnodes/floatreftarget.lua` — anchors A2 (`:196`), A3 (`:242`),
  A4 (`:965`), A5 (`:982`).
- `src/resources/filters/modules/callouts.lua` — anchor A6 (`:22`), **and anchor A7, the new
  `order == nil` guard in `callout_title_prefix` (decided with Gordon, 2026-09-18 — see below).**

**Anchor A7 — the missing `order == nil` guard (added to this task 2026-09-18).** Gordon's
decision on Findings for Gordon #2: fold the guard into this PR rather than accept the crash as a
"P6 always injects an order" invariant. It is the same generally-useful, backward-compatible shape
as the rest of the patch — the site is **unreachable today** (upstream, `enable-crossref: false`
early-returns at A6 and default mode always assigns an order), so the guard cannot change any
existing behavior; it only makes the state this PR newly creates degrade instead of abort.

The mirror is exact. `float_title_prefix` (`crossref/tables.lua:223-235`) and
`callout_title_prefix` (`modules/callouts.lua:6-18`) are the same function shape with the same
guard sequence, except the callout one is missing the middle guard:

```lua
-- crossref/tables.lua:226-231, the float side (present)
  if category == nil then
    fail("unknown float type '" .. float.type .. "'")
    return
  end
  if float.order == nil then                       -- :229-231
    warn("field 'order' is missing from float. Cannot determine title prefix for crossref.")
    return {}
  end

-- modules/callouts.lua:7-11, the callout side (guard absent) — A7 inserts the same three lines
  local category = crossref.categories.by_ref_type[refType(callout.attr.identifier)]
  if category == nil then
    fail("unknown callout prefix '" .. refType(callout.attr.identifier) .. "'")
    return
  end
+ if callout.order == nil then
+   warn("field 'order' is missing from callout. Cannot determine title prefix for crossref.")
+   return {}
+ end
```

**`return {}` is verified safe for this caller, not assumed.** The only caller is
`decorate_callout_title_with_crossref`, which does `tprepend(title, title_prefix)`
(`modules/callouts.lua:46`) and then `return callout`. With `{}` that prepend is a no-op and the
callout renders with no prefix — precisely the float path's warn-and-skip. (Today's `fail()` path
returns `nil`, but `fail()` aborts before the `tprepend` is reached, so no caller ever sees `nil`.)

**Note in passing, because it explains why the crash was reachable at all:** the caller already
guards on `is_valid_ref_type(refType(...))` (`:33`) — i.e. the `valid_ref_types()` *superset* —
which is exactly why the narrower `by_ref_type` nil case survives to `callout_title_prefix`. That
is the same superset/subset relationship round 4 corrected in P5's Callout `fail()`-guard
predicate, showing up a second time at a different site. A7 does not change that guard; P5's shim
still needs its own `by_ref_type` check for the unregistered-category case.

**Acceptance criterion.** In the upstream tree,
`grep -rn 'param("enable-crossref"' src/resources/filters/` returns **exactly these three
lines and no others**: `main.lua:227` (the param read, anchor A0) and `layout/ipynb.lua:121`
and `:126` (the two deliberately-unchanged PanelLayout predicates). State that expected *set*
explicitly in the PR description — a bare count is the kind of assertion that goes stale.
Plus: `tests/smoke/crossref/` and `tests/smoke/manuscript/` green in the quarto-cli repo.
**And for A7:** `callout_title_prefix` and `float_title_prefix` have the same guard sequence
(category-nil `fail()`, then order-nil `warn()`+`return {}`), and a crossref-labeled callout with
no `order` **warns and renders without a prefix instead of aborting** — asserted by T3.5, and by
Task 7's matrix, whose fixture A7 unblocks.

**Prerequisite.** Task 2 (the predicate must exist and be in scope at each of these three files).

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | Q | `decorate_caption_with_crossref` + `float_title_prefix` through a real docx render | **existing** `tests/smoke/crossref/docx.test.ts` → `>Figure 1: Elephant<` present | none | A2, but **only in the "gate removed entirely" direction** — see the vacuity note |
| T3.2 | Q | the `FloatRefTarget`-for-ipynb renderer **selection**, i.e. anchors A4/A5 as a pair | **existing** `tests/smoke/manuscript/render-manuscript.test.ts` → the `article.out.ipynb` / `notebooks/*.out.ipynb` outputs it already verifies (this is the only path on which `enable-crossref` is ever false, per `filters.ts:534`) | none | A5's `not` |
| T3.3 | L | A2 and A6 under external mode | `seam deferred until Task 7 of this plan` (which is itself `seam deferred until P4's materialized vendored tree + `--data-dir`/`QUARTO_FILTER_PARAMS` invocation`) | — | — |
| T3.4 | — | A3 (`full_caption_prefix`) | — | — | `accepted-untested` — dead branch, see below |
| T3.5 | L | **A7**, the new `order == nil` guard, via a crossref-labeled callout under external mode with no injected order | Render `#nte-setup` callout fixture at `enable-crossref=true, crossref-numbering=external` → assert pandoc **exits 0**, the stderr carries `field 'order' is missing from callout`, and the rendered callout title contains **no** `Note`+NBSP+number prefix | **nothing mocked** — real `main.lua`, real pandoc | **A7** (delete the three inserted lines) |
| T3.6 | Q | A7's backward-compatibility, i.e. that the guard is unreachable upstream | **existing** `tests/smoke/crossref/docx.test.ts` → `>Note 1: …<`-shaped callout prefixes unchanged, and stderr carries **no** `field 'order' is missing from callout` | none | A7's predicate **inverted** to `if callout.order ~= nil then` — deletion does **not** redden this row; see the vacuity note |

**Revert hunks, stated exactly:**

- T3.1 — Replace A2's guarded early return with an unconditional `return float` (i.e. delete the
  decoration outright) → T3.1's `>Figure 1: Elephant<` regex **RED**.
- T3.2 — Drop the `not` from A5 (`return _quarto.format.isIpynbOutput() and crossref_present()`)
  → under the source-notebook render both ipynb predicates read the same value; since
  `add_renderer` inserts at the **front** of the renderer list (`ast/customnodes.lua:483`,
  `table.insert(handler.renderers, 1, …)`), A5's registration is checked first, so with
  `enable-crossref: false` neither ipynb renderer matches, control falls through to the generic
  `add_renderer("FloatRefTarget", function(_) return true end, …)` at `floatreftarget.lua:183`,
  which `warn("Emitting a placeholder FloatRefTarget…")` and emits a scaffold → the manuscript
  test's `article.out.ipynb` figure content changes and the run emits the placeholder warning →
  T3.2 **RED**.
- T3.3 — deferred; no hunk may be named yet.
- T3.5 — Revert **A7** (delete the three inserted lines from `callout_title_prefix`) → the render
  aborts with `attempt to index a nil value` inside `formatNumberOption` instead of exiting 0, so
  **both** of T3.5's assertions go RED: the `exit == 0` assertion and the
  `field 'order' is missing from callout` stderr assertion. Note the exit-code assertion is the
  load-bearing one — a test that asserted only "no prefix appears in the output" would be
  satisfied by the crash too, since a crashed render produces no output at all.
- T3.6 — Revert by **inverting** A7's predicate to `if callout.order ~= nil then` →
  `tests/smoke/crossref/docx.test.ts`'s callout-prefix expectations RED (every labeled callout
  upstream *has* an order, so the inverted guard fires on all of them and strips every prefix).
  **Deleting A7 does not redden T3.6**, and that is the point of stating this hunk separately:
  A7's whole justification is that it is unreachable upstream, so "upstream is unchanged" is true
  before and after the guard **by construction** and cannot discriminate its presence. The
  inversion is the only mutation that proves T3.6 is testing the guard's *reachability* rather
  than restating a tautology.
- T3.4 — `accepted-untested`: **A3's gate is dead code in both polarity states.**
  `full_caption_prefix` has exactly two callers, both in `layout/lightbox.lua` (`:153`, `:268`),
  and `lightbox()` returns `{}` unless `quarto.doc.is_format("html:js")`
  (`layout/lightbox.lua:171`, `else return {} end` at `:424-425`). `enable-crossref` is only ever
  false on the ipynb source-notebook path, which is never `html:js`; and Q2 never routes HTML
  through this Lua path at all (it has its own writer — the same reason P3's audit classes
  `floatreftarget.lua:659`'s html renderer as irrelevant). So no reachable input distinguishes
  A3-patched from A3-unpatched. Patch it anyway for upstream consistency (the plan says so, and
  a future lightbox-for-typst would make it live); assert nothing.

### Refactor-induced vacuity check

- **How does the assertion discriminate per-site, across four sites?** It largely **does not**,
  and that is the central honest finding of this task. All four hunks are of the form
  `not param("enable-crossref", true)` → `not crossref_present()`. In every state reachable
  *upstream* — `(enable-crossref=true, numbering unset)` and `(enable-crossref=false, numbering
  unset)` — the old and new expressions evaluate **identically**, because with `numbering` unset
  the disjunct is false and `crossref_present() == enableCrossRef` exactly. **The four
  render-gate hunks are therefore undiscriminated by every default-mode and every
  `enable-crossref: false` test, by construction.** T3.1/T3.2 bind "the decoration still runs /
  the right renderer is still selected" — a necessary regression guard — not "the gate now reads
  `crossref_present()`". The discriminating state is `numbering == "external"`, which only
  M3/M4 in Task 7 reach. Per-site discrimination there:
  | Hunk | Discriminating run | Why that run and no other |
  |---|---|---|
  | A2 `decorate_caption_with_crossref` | **M4** (`enable-crossref=false`, `numbering=external`, `--to docx`) | in M3 (`enable-crossref=true`) old and new both read false and the decoration runs either way; only M4 flips old→early-return / new→run |
  | A3 `full_caption_prefix` | none | dead branch, see T3.4 |
  | A4/A5 ipynb pair | **M4 with `--to ipynb`** | with `enable-crossref=false` + external, unpatched A4 is false and unpatched A5 is true (the non-decorating renderer wins, front-inserted); patched, A4 is true and A5 is false. Distinguishable at the AST level via `-t json`: `Figure` vs `Para[Image]` |
  | A6 `decorate_callout_title_with_crossref` | **M4 with a `#nte-`-labeled callout** — unblocked 2026-09-18 by anchor **A7** (Gordon's decision on Findings #2). Before A7 this run aborted, so the M-matrix fixture had to exclude labeled callouts; with A7 it warns and renders prefix-less, which is an observable state, so A6 discriminates the same way A2 does: in M3 old and new both read false and the decoration runs either way, only M4 flips old→early-return / new→run |
- **A7's backward-compatibility claim is tautological unless the mutation is an inversion.** The
  guard's justification is that it is unreachable upstream, which means every upstream test
  produces byte-identical output with and without it — the same structural problem as the
  "default is bit-for-bit" claim elsewhere in this plan. Deleting A7 reddens only T3.5 (the
  external-mode run, where the guard is reachable); **inverting** it is what reddens T3.6 and
  therefore what proves T3.6 discriminates reachability rather than restating "nothing changed".
  Both hunks are named separately above for exactly this reason.
- **A7 also removes a false discriminator from T3.5's neighbourhood.** Asserting only "no
  `Note`+NBSP+number prefix appears" would be satisfied by the pre-A7 *crash*, which produces no
  document at all — so T3.5 asserts `exit == 0` first and the absent prefix second. This is the
  "the path was actually exercised" assertion the discipline calls for, in its most literal form.
- **Would a *set-of-sites* assertion go vacuous when upstream adds a fifth site?** Yes, if it
  were written as "these four sites contain `crossref_present()`". The fix, specified for T6.1c:
  assert the **exhaustive negative** — no occurrence of the substring `param("enable-crossref"`
  remains anywhere in the vendored filters tree except the three known-and-listed ones (A0 and the
  two PanelLayout predicates). A fifth gate arriving on a re-pin then reddens automatically,
  which a positive four-site list never would.

---

## Task 4: Pin the `enable-crossref: false` cell to the only path that reaches it

**Scope.** Establish the "bit-for-bit on `enable-crossref: false`" half of P3's checklist item 2.
The finding that makes this its own task: **no upstream test names `enable-crossref` at all**
(`grep -rln 'enable-crossref' tests/` in quarto-cli returns nothing), and the flag is not
user-settable — `src/command/render/filters.ts:534` sets it false only when
`format.render[kIpynbProduceSourceNotebook]` is true. So "bit-for-bit on `enable-crossref:
false`" means, concretely and only, "the manuscript ipynb-source-notebook outputs are unchanged."

**Files** (all **upstream quarto-cli**): test-side only —
`tests/smoke/manuscript/render-manuscript.test.ts` (existing; extend its assertions if it does not
already pin the figure shape), and the PR description, which must state this scoping so a reviewer
does not read "bit-for-bit on `enable-crossref: false`" as a broader claim than the tree can make.

**Acceptance criterion.** `tests/smoke/manuscript/` green; plus a recorded **G-tier** before/after
comparison (see T4.2) demonstrating byte-identity of the source-notebook outputs across the patch,
pasted into the PR description. The PR description states explicitly that `enable-crossref: false`
has no other reachable entry point.

**Prerequisite.** Tasks 2 and 3 (there must be a patch to compare against). No q2-side
prerequisite.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | Q | the ipynb renderer-selection pair A4/A5 and the source-notebook contributor (`src/render/notebook/notebook-contributor-ipynb.ts`) | **existing** `tests/smoke/manuscript/render-manuscript.test.ts` → its declared `article.out.ipynb` / `notebooks/*.out.ipynb` outputs | none | A5's `not` (same hunk as T3.2, different failure surface) |
| T4.2 | G | the whole default-and-false render path, byte level | run `quarto render tests/docs/crossrefs/all-docx.qmd --to docx` and the manuscript ipynb fixtures at the pre-patch commit and at the patched commit; compare **`word/document.xml` + `word/numbering.xml`** unzipped from each docx (not the `.docx` itself — a zip carries timestamps and is not byte-stable) and the `.ipynb`/`.tex` outputs directly | none; needs a real Q1 dev checkout, so dev-only, never CI (CLAUDE.md External Sources Policy) | A1's polarity; A2's presence |

**Revert hunks, stated exactly:**

- T4.1 — Drop the `not` from A5 → the generic placeholder renderer (`floatreftarget.lua:183-188`)
  handles the float in the source notebook → the manuscript test's `article.out.ipynb` content
  assertion **RED** (and a `Emitting a placeholder FloatRefTarget` warning appears).
- T4.2 — Invert A1's polarity → `word/document.xml` from the patched render lacks the
  `Figure 1:` run present in the pre-patch render → T4.2's XML diff is non-empty **RED**.
- T4.2 — Replace A2's guarded early return with an unconditional `return float` → same diff,
  **RED**.

### Refactor-induced vacuity check

- **A byte-identity assertion is the archetype of an assertion identical before and after by
  construction.** T4.2 is only meaningful because the *comparand* is an actual pre-patch render,
  produced from a different commit — not a committed snapshot that a `--accept` would silently
  refresh. Spec it as a two-commit procedure whose output is pasted into the PR, explicitly
  **not** as a committed `insta` snapshot: a snapshot committed at patch time records the
  post-patch state and can never again discriminate the transition it exists to certify.
- **"The path was actually exercised."** T4.2 must additionally assert the pre-patch render
  *contained* `Figure 1:` (and the manuscript fixture *contained* a figure). Comparing two
  empty outputs is byte-identical and proves nothing.

---

## Task 5: Expose the Route-N function set; open the upstream PR

**Scope.** Two asks, one PR. (a) Add sanctioned exports for the eight Lua functions P5's Route-N
shim calls by name, following the existing precedent at
`customnodes/floatreftarget.lua:239` (`quarto.doc.crossref.decorate_caption_with_crossref =
decorate_caption_with_crossref`, with its comment "we need to expose this function for use in the
docusaurus renderer, which is technically an extension that doesn't have access to the internal
filters namespace"). (b) Open the PR carrying Tasks 2–4, **Task 8's TypeScript plumb**, plus (a),
and record its number.

**Updated 2026-09-18 (Gordon's decision on Findings #1):** the PR is no longer Lua-only. **Task 8
must land before this task**, because its diff is part of this PR — the `crossref: numbering:`
YAML key, its schema entry, the `kCrossrefNumbering` constant, the `crossrefFilterParams` read,
and the two upstream smoke tests that drive them. Task 8 is numbered after this one but ordered
before it; see its own numbering note. Without it the PR's "front end supplies numbers" pitch is
not actually reachable by a quarto-cli user, only by a consumer that builds the params blob itself.

**Files** (all **upstream quarto-cli**), with the eight functions' real homes verified 2026-09-18:
- `src/resources/filters/crossref/format.lua` — `titlePrefix` (`:17`), `subrefNumber` (`:51`),
  `refPrefix` (`:66`), `refDelim` (`:98`), `refHyperlink` (`:102`), `refNumberOption` (`:106`).
- `src/resources/filters/crossref/options.lua` — `crossrefOption` (`:17`).
- `src/resources/filters/crossref/equations.lua` — `renderEquation` (`:102`).
- `src/resources/filters/common/pandoc.lua` — `nbspString` (`:125`).
- (Note: P3's plan lists eight names; `titlePrefix` is a ninth that `refPrefix`/`float_title_prefix`
  both route through. Whether to expose it is the PR author's call — it is not in P3's frozen list
  and this file does not add it.)

**Acceptance criterion.** The PR exists, its number is recorded in P3's plan, and every
`QUARTO-PATCH(...)` marker Task 6 writes cites that number. T5.1 green. Precedent-consistency:
each export sits adjacent to its definition with a one-line comment naming the consumer, matching
`floatreftarget.lua:237-239`'s shape.

**Prerequisite.** Tasks 2, 3, 4. Also relevant, not blocking: upstream's active
`_quarto.modules` migration (quarto-cli `#14702`) has not reached `crossref/` yet — this ask is
cheap now and expensive after.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T5.1 | Q | the real export statements in the four files above | extend `tests/unit-lua/crossref-numbering.test.lua` (or a sibling `crossref-exports.test.lua`, registered in `LUA_TESTS`): pre-seed `quarto = { doc = { crossref = {} }, utils = {} }`, `require` each file, then `assertNotNil` each of the eight names at its declared export address | `param()` and whatever filter-runtime globals the required files touch. The export statements themselves are real. | each individual `quarto.doc.crossref.<name> = <name>` / `quarto.utils.<name> = <name>` line |
| T5.2 | — | the PR itself | — | — | `accepted-untested` — see below |

**Revert hunks, stated exactly:**

- T5.1 — Revert any one export line (e.g. `quarto.doc.crossref.refPrefix = refPrefix`) → that
  name's `assertNotNil(quarto.doc.crossref.refPrefix)` **RED**. Eight independent reds, one per
  line — this is deliberately a per-name assertion, not a loop over a locally-defined list, so
  removing a name from both the exports and the list cannot pass.
- T5.2 — `accepted-untested`: "a PR was opened and a number recorded" is a process fact, not a
  behavior. Its downstream consequence *is* bound: T6.1a asserts every marker cites a concrete PR
  number rather than the literal placeholder `#<N>`.

### Refactor-induced vacuity check

One collapse risk, and it is a familiar one: if T5.1 is written as a loop over a list of names
defined **inside the test**, then a future rename that updates both the export and the list keeps
the test green while P5's shim breaks — the test would be asserting self-consistency, not a
contract. Spec it as eight literal `assertNotNil(quarto.doc.crossref.refPrefix)`-shaped
assertions, one per line, with the names spelled out. The list that matters lives in P5's shim;
duplicating it literally here is the point, not a smell.

**Implementation risk to surface at dispatch, not silently absorb.** `require`ing
`crossref/equations.lua` or `common/pandoc.lua` in isolation may pull in globals the luaunit
harness does not mock (the harness doc says the test "Mocks any filter-runtime globals it needs
(param, tcontains, format_typst_float, quarto.log.*, _quarto.*)", which is a per-test manual
effort, not a framework). If a file proves unrequirable in isolation, the fallback is **not** to
drop the assertion: P5's checklist already carries "add a Layer-1 assertion that each Route-N
function this shim calls is reachable," run inside a real pandoc-Lua probe. In that case record
T5.1 as `seam deferred until P5's Layer-1 introspection harness` and say so in the PR.

---

## Task 6: Carry the patch in q2's vendored tree, with markers and a re-vendor tripwire

**Scope.** Apply Tasks 2–3's edits to q2's vendored copy of the Q1 filters, each site carrying a
`-- QUARTO-PATCH(upstream PR quarto-dev/quarto-cli#<N>): …` comment, and add the tripwire test
that reddens when a re-pin silently overwrites them. Also record here (not in P4's plan) the
confirmation that P5's Layer-1/Layer-2 contract tests cover behavioral drift, per P3's checklist
item 6.

**Files.**
- `resources/pandoc-filters/main.lua`, `resources/pandoc-filters/customnodes/floatreftarget.lua`,
  `resources/pandoc-filters/modules/callouts.lua` (q2, **vendored copy** — paths per P4's plan;
  confirm the actual layout when P4 lands, and note that P4's own `main.lua` splice is a second,
  independent edit to the first of these files in the **`Q2-local, permanent`** marker category).
- `crates/quarto-core/tests/integration/pandoc_filters_patch.rs` (q2, new) +
  `pub mod pandoc_filters_patch;` in `crates/quarto-core/tests/integration/main.rs`
  (alphabetized). **Not** a top-level `crates/quarto-core/tests/*.rs`.

**Acceptance criterion.** T6.1 green. `cargo xtask lint` green (the vendored tree must contain no
`external-sources/` reference reachable from a compile-time macro). The three patched files each
carry a marker naming Task 5's real PR number.

**Prerequisite.** **Blocked on P4's vendored filters tree existing** — verified 2026-09-18 that
`resources/pandoc-filters/` does not exist in this worktree. Named by capability: *P4's vendored
Q1 filters tree (in-repo directory copy + `include_dir!`), pinned to `v1.11.3`, with its
`README.md` recording the pin and the marker-category inventory.* Nothing else is needed — this
task needs no pandoc, no transport, no shim.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T6.1a | I | the vendored tree's marker inventory | `crates/quarto-core/tests/integration/pandoc_filters_patch.rs` reads the three vendored files from disk → asserts each contains `QUARTO-PATCH(upstream PR quarto-dev/quarto-cli#` followed by digits (**not** the literal `#<N>`), and that the set of marker-bearing files equals a hard-coded expected list | none — real `std::fs` read of the real vendored tree | the `QUARTO-PATCH` comment at each of A1, A2, A3, A4/A5, A6 |
| T6.1b | I | the vendored `main.lua`'s assignment gate, structurally | same file → asserts `main.lua` contains the new predicate call at the gate **and** does **not** contain the bare line `if enableCrossRef then` | none | A1 |
| T6.1c | I | the vendored tree's *whole* gate surface, exhaustively | same file → walks every `.lua` under the vendored filters root; asserts `crossref_present()` appears in `floatreftarget.lua` and `modules/callouts.lua`, **and** that the complete set of occurrences of the substring `param("enable-crossref"` equals exactly the three allow-listed ones (`main.lua`'s param read; `layout/ipynb.lua` ×2) | none | A2, A4, A5, A6; and any *fifth* site a future re-pin introduces |
| T6.2 | L | behavioral drift after a re-pin | `seam deferred until P5's Layer-1/Layer-2 contract tests` — P3's checklist item 6 says "confirm P5's contract test covers drift detection (no new mechanism needed beyond P5's existing plan)", and P5's plan agrees ("the same behavioral-tripwire philosophy P3 already established for its own 3-file patch"). **P5 is scheduled after P3.** See the Missing-test pass for the verdict on that ordering. | — | — |

**Revert hunks, stated exactly:**

- T6.1a — Delete the `QUARTO-PATCH(…)` comment from `resources/pandoc-filters/modules/callouts.lua`
  → T6.1a's "every file in the expected list carries a marker" assertion **RED**.
- T6.1a — Leave a marker as the literal `QUARTO-PATCH(upstream PR #<N>)` placeholder →
  T6.1a's digits-after-`#` assertion **RED**. (This is what binds Task 5's "record the PR number".)
- T6.1b — Re-vendor `main.lua` from the pinned tag (overwriting the patch) → the file again
  contains `if enableCrossRef then` → T6.1b's negative assertion **RED**.
- T6.1c — Re-vendor `floatreftarget.lua` → three extra `param("enable-crossref"` occurrences
  appear → T6.1c's "the occurrence set equals the three-item allow-list" assertion **RED**.
- T6.1c — Upstream adds a fifth `param("enable-crossref"` gate, picked up by a re-pin →
  same assertion **RED**, with the new site named in the failure output.

### Refactor-induced vacuity check

- **The marker assertion goes vacuous the moment upstream merges.** After merge the markers are
  deliberately removed and the code stays — a tripwire keyed only to markers would then fail for
  the right reason in the wrong direction, and the pressure would be to delete it. That is why
  T6.1a (markers) and T6.1b/T6.1c (behavioral anchors) are **separate rows with separate
  assertions**: at merge time, T6.1a's expected list is edited to empty (a deliberate, reviewable
  act) while T6.1b/T6.1c continue to assert the gate shape unchanged. Do not collapse them into
  one "the patch is present" assertion.
- **A positive set-of-sites assertion is the vacuity trap here**, and T6.1c deliberately inverts
  it into an exhaustive negative for exactly the reason the parent instruction names: a positive
  list of four sites stays green when a fifth arrives. The allow-list of *permitted* occurrences
  is the discriminator that does not rot.
- **"The path was actually exercised."** T6.1's unit is a file read, so the failure mode is
  reading the wrong path and finding nothing (which a naive `!contains(...)` assertion would read
  as success). Every negative assertion in T6.1b/T6.1c must be preceded by a positive
  existence assertion on the same buffer — e.g. assert `main.lua` contains
  `tappend(quarto_filter_list, quarto_crossref_filters)` before asserting it does *not* contain
  `if enableCrossRef then`. Without that, a typo in the vendored path makes the whole test pass.

---

## Task 7: `crossref_present()` / assign-numbers cross-product matrix (q2, L tier)

**Scope.** The behavioral matrix that discriminates every hunk Tasks 2–3 introduce, in the one
place the full 2×2 is reachable: a q2-side `pandoc -L main.lua` run whose
`QUARTO_FILTER_PARAMS` blob the test builds itself. Five runs over one fixture; no shim, no Q2
pipeline, no injected `order`.

**Files.**
- `crates/quarto-core/tests/integration/crossref_numbering_matrix.rs` (q2, new) +
  `pub mod crossref_numbering_matrix;` in `crates/quarto-core/tests/integration/main.rs`.
- A fixture: one labeled figure div with a caption, **plus one `#nte-`-labeled callout
  (unblocked 2026-09-18 by anchor A7 — see Task 3)**. **Still deliberately no theorem** — see the
  Prerequisite and Findings for Gordon #3.

**The callout was added to this fixture, not given a separate one, deliberately.** A7 makes the
labeled-callout run *observable* rather than fatal, and the callout and float paths have
independent guards (`modules/callouts.lua` A7 vs. `crossref/tables.lua:229-231`) that emit
**distinguishable** warnings — `…missing from callout` vs. `…missing from float`. So one fixture
exercises both sites per run, and each run's stderr assertion names which warnings it expects,
which is strictly stronger than two fixtures that could each silently stop exercising its site.

**The five runs** (`--to docx` unless stated; `A` = `enable-crossref`, `B` = `crossref-numbering`):

| Run | A | B | Expected caption prefix | Expected callout prefix | Expected stderr |
|---|---|---|---|---|---|
| M1 | unset (⇒ true) | unset (⇒ `quarto`) | `Figure`+NBSP+`1:` present | `Note`+NBSP+`1` present | neither `missing from float` nor `missing from callout` |
| M2 | `false` | unset | absent | absent | neither |
| M3 | `true` | `external` | absent | absent | **both** `…missing from float` **and** `…missing from callout` |
| M4 | `false` | `external` | absent | absent | **both** |
| M5 | `true` | `bogus` | identical to M1 | identical to M1 | neither (identical to M1) |

M5 is identical to M1 by construction: an unrecognized `crossref-numbering` value makes both new
predicates read exactly as they did before the patch (`crossref_present()` short-circuits on
`enable-crossref` being true, and the assign-numbers conjunct tests `~= "external"`, which
`bogus` satisfies), so numbers are assigned and neither guard is reached.

**Acceptance criterion.** All five runs behave as tabulated, with the caption body text
(`A caption here`) **and** the callout body text asserted present in every run that produces
output — the shape assertion that prevents an empty document from satisfying the "prefix absent"
rows, and now also the assertion that distinguishes A7's warn-and-skip from the pre-A7 abort.

**Prerequisite — this is the load-bearing one.** Named by capability, not by plan-task number:
- *P4's materialized vendored filters tree + datadir* (`ResourceBundle` extraction to real files
  on disk; `include_dir!` alone is insufficient — `main.lua` needs real files).
- *P4's `QUARTO_FILTER_PARAMS` blob builder and runtime-environment contract.* This is not a
  formality. Verified empirically 2026-09-18 by driving `pandoc 3.8.1` against the real
  quarto-cli tree: `main.lua` fails outright without (i) `QUARTO_SHARE_PATH` (else
  `module '_format' not found`, from `datadir/init.lua:124-131`), (ii) `--data-dir` at the
  datadir root with the filters root at `<share>/filters` (`init.lua:257` extends `package.path`
  with `PANDOC_STATE.user_data_dir .. '/../../filters/?.lua'`), and (iii) a
  `QUARTO_FILTER_PARAMS` base64 blob — an **empty** blob crashes in
  `datadir/_base64.lua:123`, and successive additions were each required in turn:
  `quarto-filters.entryPoints` (`ast/emulatedfilter.lua:45` indexes it unconditionally),
  `results-file` (`quarto-pre/results.lua:6`), `active-filters` (`main.lua:475`), a populated
  `language` map (`modules/authors.lua:864`), and a `quarto_pandoc_reader_opts` Meta key
  (`normalize/capturereaderstate.lua:9`). The run reached the crossref filter chain but never
  produced a document.
- *P4's pandoc version reconciliation* (3.10 floor vs. CI's 3.8.3 vs. dev-setup's 3.6).
- **Not** required: P5's shim, P6's param wiring. Every row above is reachable with P4 alone,
  because `FloatRefTarget` nodes are created by `quarto-pre/parsefiguredivs.lua:121,180`, and
  `quarto_pre_filters` is appended at `main.lua:717` — **before** the crossref gate at `:718`.
  Verified 2026-09-18.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T7.1 | L | vendored `main.lua`'s real filter-list construction, real `quarto_crossref_filters`, real `decorate_caption_with_crossref`, real `float_title_prefix` | M1 → unzip `word/document.xml`, assert a run containing `Figure\u{a0}1:` and a run containing `A caption here` | environment only: `QUARTO_SHARE_PATH`, `--data-dir`, `QUARTO_FILTER_PARAMS` (built by the test). No Lua is mocked. | A1's polarity |
| T7.2 | L | same | M2 → assert **no** `Figure` run; assert `A caption here` present; assert stderr has no `field 'order' is missing` | same | A1's polarity in the `A=false` direction |
| T7.3 | L | same, plus `crossref/tables.lua`'s `float.order == nil` guard | M3 → assert no `Figure` run; assert `A caption here` present; **assert stderr contains `field 'order' is missing from float`** | same | A1's polarity; the assign-numbers conjunct |
| T7.4 | L | same | M4 → identical assertions to M3 | same | **A2**; the `crossref_present()` disjunct |
| T7.5 | L | same | M5 → assertions identical to M1 | same | the `== "external"` literal |

**Revert hunks, stated exactly:**

- T7.1 — Invert A1's polarity → the assignment group is skipped in default mode → `order` is nil
  → `float_title_prefix` warns and returns `{}` → T7.1's `Figure\u{a0}1:` assertion **RED**.
- T7.2 — Invert A1's polarity → the group *runs* under `A=false` → a `Figure 1:` prefix appears
  → T7.2's "no `Figure` run" assertion **RED**.
- T7.3 — Revert the `and param("crossref-numbering","quarto") ~= "external"` conjunct from the
  assign-numbers predicate → under `A=true, B=external` the group runs, `order` is assigned, no
  warning is emitted → T7.3's `field 'order' is missing from float` stderr assertion **RED**
  (and its "no `Figure` run" assertion also **RED**). Two independent reds from one revert.
- T7.4 — Revert **A2** to `if not param("enable-crossref", true) then` → under `A=false` the old
  expression is true → `decorate_caption_with_crossref` early-returns → `float_title_prefix` is
  never called → no warning → T7.4's stderr assertion **RED**. **M4 is the only run in this
  entire document that reddens on A2's revert**, and the only one that distinguishes
  `enableCrossRef or external` from `enableCrossRef` alone.
- T7.5 — Replace `~= "external"` with `== "quarto"` in the assign-numbers predicate → under
  `B="bogus"` the group is skipped → no prefix, plus a warning → T7.5's `Figure\u{a0}1:`
  assertion **RED**.

### Refactor-induced vacuity check

- **"Numbers are absent under external mode" is satisfied by a document with nothing to number.**
  Addressed twice: every "prefix absent" row (T7.2–T7.4) additionally asserts the caption body
  text `A caption here` is present — so an empty or failed render cannot pass — and T7.3/T7.4
  assert the *presence* of a warning string that only `crossref/tables.lua:230` emits, and only
  after `decorate_caption_with_crossref` has passed its gate on a real float with a nil `order`.
  That warning **is** the "the path was actually exercised" assertion, and it is not obtainable
  any other way.
- **Warning delivery is a dependency, not an assumption.** T7.3/T7.4 assert on the `pandoc`
  subprocess's **stderr**, captured by the test's own `Command` — not on a q2 diagnostic. P4's
  checklist separately owns re-emitting Q1 `[WARNING]` lines as q2 diagnostics; these rows
  deliberately do not depend on that, so they stay valid whichever way P4 resolves it.
- **`Figure` vs `Figure\u{a0}1:`.** Assert the NBSP form. Upstream's own docx test comments on
  this (`tests/smoke/crossref/docx.test.ts`: "These tests no longer pass after we started
  introducing non-breaking spaces… although upon cracking the file it seems like there are no
  non-breaking spaces"). Asserting the bare word `Figure` would match the caption body in a
  document whose prefix is absent — a collapsed discriminator. Assert the digit and the colon.
- **Five runs, one binary.** If the matrix is written as a single `#[test]` looping over five
  param blobs, a failure in run 1 hides runs 2–5. Write five `#[test]` functions sharing a
  helper; nextest runs each in its own process.

---

## (Moved out: the positive external-mode golden now lives in P6 — not a dispatchable task)

> **Note on the heading.** This section deliberately does **not** begin `## Task N:`.
> `superpowers:subagent-driven-development` dispatches one implementer per `## Task N` heading, so
> leaving a task-shaped heading here would have queued an implementer to do work that has moved to
> another plan. This was P3's Task 8 until 2026-09-18; the number has since been reused by the TS
> plumb task below, so refer to this work as "P6 Task 4", never as "P3 Task 8".

**P3's checklist item 3 (the positive external-mode "Figure N:" golden) is now owned by P6.**
Decided 2026-09-18 while applying Gordon's decisions, on this task's own recommendation
(Findings for Gordon #3) and P6's concurring one (its Findings #3). The golden itself is
unchanged; only its owner is. It lives in **P6's companion, Task 4** —
`2026-09-18-pandoc-hybrid-P6-implementation.md`.

**Why P6 and not P3**, in the order the reasons actually weigh:

1. **Its revert hunks are not P3's.** Under `enable-crossref=true, crossref-numbering=external`
   with an injected order, reverting **A2** changes nothing (the old expression already reads
   false) and reverting the assign-numbers conjunct yields the *same* rendered number for a
   single-figure fixture (Q1 would compute `1` too). What the golden discriminates is **P5's**
   `order` passthrough and **P6's** param insertion. Under `/prevalidating-test-seams` a test
   belongs where the hunk whose revert reddens it lives; this one's hunks are in P5 and P6.
2. **P6 can schedule it and P3 cannot.** It is an `L`-tier golden requiring P4's materialized
   tree and transport. P3 is scheduled *parallel with P1/P2 and before P4*; P6 runs after P3 and
   P5. Keeping it here would have left P3 holding a permanently-deferred seam.
3. **P6 already claims the review target.** The epic's Plans table lists P6's as
   "figure/theorem/callout-number parity" — this golden *is* that.
4. **It restores P3's parallel-schedulability**, which the epic's dependency graph asserts. With
   this moved, every remaining P3 task is bindable against P3's own hunks, except the deferrals
   Findings #1 records.

**P6's Task 4 is the stronger version of this test, not a copy.** It specifies the discriminating
fixture this task could only gesture at: an injected `order = 7` against a document where Q1's own
numbering would compute `1`, so the assertion (`Figure`+NBSP+`7:`) distinguishes "Q2's number" from
"a number" — which the single-figure fixture originally specified here could not do.

**Nothing was lost in the move.** The analysis that made this task worth writing — that A2's
revert is inert in this cell, and that the assign-numbers conjunct's revert is number-identical on
a one-figure fixture — is preserved in Findings for Gordon #3 below, and is what P6's Task 4 cites
as its rationale for the `order = 7` fixture.


## Task 8: Plumb `crossref-numbering` from metadata into `QUARTO_FILTER_PARAMS` (TS side) + its upstream smoke test

**Scope.** Decided with Gordon 2026-09-18, resolving **Findings for Gordon #1**: the upstream PR is
**not** deliberately Lua-only. Add the TypeScript half — a `crossref: numbering:` YAML key, its
schema entry, and the read that puts `crossref-numbering` into the params blob Q1's Lua reads via
`param()` — plus an upstream smoke test that drives it end to end. This is what gives the
*discriminating* direction of Tasks 2–3's five patched gates real **upstream** coverage instead of
only the "two pre-existing cells still behave as before" regression check.

**Numbering note (read this before dispatching).** This task is numbered 8 but must land
**before Task 5**, which opens the PR that carries it. Task numbers here are identifiers, not an
execution order — the `Prerequisite` fields are the ordering. Task 5's scope line has been updated
to name this task's diff as part of the PR.

**Files** (all **upstream quarto-cli**; every anchor below opened and confirmed at tag `v1.11.3`):

1. **`src/config/constants.ts`** — add `export const kCrossrefNumbering = "crossref-numbering";`.
   Place it with the existing crossref param constants (`kCrossrefFigTitle` … `kCrossrefDefTitle`,
   `:325-333`), **not** next to `kEnableCrossRef` (`:44`) — the latter is an internal flag with no
   YAML key, this is a user-facing metadata-backed param, same shape as the `crossref-*-title`
   family.
2. **`src/resources/schema/document-crossref.yml`** — add a `numbering:` property to the
   `crossref` object's `properties` map (the map opens at `:10`).
   **This edit is mandatory, not cosmetic: the object is declared `closed: true` (`:9`)**, so
   without a schema entry `crossref: {numbering: external}` is *rejected by validation* before any
   TS or Lua ever sees it. Shape, following the sibling `chapters` entry (`:48-51`) which is the
   closest precedent for a defaulted scalar:
   ```yaml
   numbering:
     enum: [quarto, external]
     default: quarto
     description: |
       Who assigns cross-reference numbers. "quarto" (the default) numbers
       cross-references in Quarto's own filters. "external" skips Quarto's
       numbering, index, and @ref-resolution passes and renders the numbers
       already present on the nodes — for front ends that compute their own.
   ```
3. **`src/command/render/crossref.ts`** — in `crossrefFilterParams` (`:31-`), emit the param.
   **Read it from `metadata`, not via `crossrefOption`** — verified: `crossrefOption`
   (`crossref.ts`, the `function crossrefOption` block) only consults `flags` and `defaults` and
   returns `undefined` otherwise; it never looks at metadata, so routing this key through it would
   silently always yield `undefined`. The correct precedent is one line below the
   `kCrossrefFilterParams` loop: `params[kNumberDepth] = metadata?.[kNumberDepth];` (`:66`), which
   reads `metadata` (bound at `:36`) directly. So:
   ```ts
   // crossref.numbering: who assigns numbers (see document-crossref.yml)
   const crossrefMeta = metadata?.crossref as Metadata | undefined;
   if (crossrefMeta?.numbering !== undefined) {
     params[kCrossrefNumbering] = crossrefMeta.numbering;
   }
   ```
   Emit only when set, so the absent case leaves the param unset and the Lua default
   (`param("crossref-numbering", "quarto")`, Task 2) supplies `"quarto"` — one default, in one
   place, rather than two that can drift.

**Interaction worth stating, because it bounds the feature.** `crossrefFilterActive`
(`crossref.ts`, immediately above `crossrefFilterParams`) returns
`options.format.metadata.crossref !== false`. So under `crossref: false` the whole crossref filter
group is inactive and `numbering:` is moot — it is a *sub-key of a block that can itself be
switched off*. The param is meaningful only when `crossref` is an object (or absent). Say so in
the PR description; do not add a diagnostic for the combination (that would be new functionality).

**Acceptance criterion.**
`grep -rn 'crossref-numbering' src/` in quarto-cli returns the constant definition, the
`crossref.ts` read, and nothing else; `crossref: {numbering: external}` passes schema validation
(it is rejected before this task by the `closed: true` object); and a `--to docx` render of a
fixture carrying that metadata reaches the Lua with `param("crossref-numbering")` equal to
`"external"` — asserted by T8.2, whose whole point is that the param actually traverses
TS → blob → `param()`.

**Prerequisite.** **Task 2** (the Lua side must read the param, or there is nothing for T8.2's
render to observe). Nothing from q2, and nothing from P4 — see the schedulability note below.

**Does this change P3's parallel-schedulability?** **No, and this is worth stating explicitly
rather than assuming.** Every file this task touches is in the upstream quarto-cli tree; it needs
no q2 crate, no materialized vendored tree, no `pandoc` invocation from q2, and no wire format.
Its test tier is `Q` (quarto-cli's own suite, run with a dev `quarto` binary in that repo), the
same tier Tasks 3 and 4 already use. So P3 remains parallel-schedulable with P1 and P2 exactly as
the epic's dependency graph asserts. What Findings #1 observed is unchanged and is a *different*
claim: **Task 7** (the q2-side `L`-tier matrix) is still blocked on P4. This task shrinks the
consequence of that — the discriminating direction now has upstream coverage too, so Task 7 is no
longer the *only* place any of it is exercised — but it does not unblock Task 7 and does not
change P3's position in the graph.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T8.1 | Q | the **schema**, i.e. that `closed: true` now admits the key | Validate a fixture with `crossref: {numbering: external}` in quarto-cli's own schema-validation test path → assert it passes; and assert `crossref: {numbering: bogus}` **fails** with an enum error naming `quarto`/`external` | none | the `numbering:` block in `document-crossref.yml` |
| T8.2 | Q | the full TS→blob→Lua traverse: `crossrefFilterParams` → `QUARTO_FILTER_PARAMS` → `param()` | Render a new fixture `tests/docs/crossrefs/numbering-external.qmd` (one `#fig-` div with a caption, `crossref: {numbering: external}`) at `--to docx` → assert via `ensureDocxRegexMatches` that the caption text contains the figure caption but **no** `Figure`+` `+digit prefix, **and** that stderr carries `field 'order' is missing from float` | none — real `quarto` binary, real pandoc, real Lua | the `params[kCrossrefNumbering] = …` read in `crossref.ts` |
| T8.3 | Q | the default path — absent key ⇒ `"quarto"` ⇒ current behavior | **existing** `tests/smoke/crossref/docx.test.ts` (which has no `numbering:` key) → its `text("Figure\\u00A01: Elephant")` expectations unchanged, and stderr carries **no** `field 'order' is missing` | none | the `!== undefined` guard around the read, i.e. emitting the param unconditionally |

**Revert hunks, stated exactly:**

- T8.1 — Revert the `numbering:` block from `document-crossref.yml` → the `closed: true` object
  rejects the key and T8.1's "passes validation" assertion goes **RED**. The paired
  `numbering: bogus` assertion is what keeps this row from degrading into "any key is accepted":
  without it, replacing the `enum` with a bare `string:` would leave T8.1 green.
- T8.2 — Revert the `params[kCrossrefNumbering] = …` read in `crossref.ts` → the param never
  reaches the blob, `param("crossref-numbering", "quarto")` returns its default, Q1 numbers the
  figure itself, and the caption regains its `Figure`+NBSP+`1:` prefix → T8.2's **absent**-prefix
  assertion goes **RED**, and so does its stderr assertion (no warning is emitted, because the
  order is no longer missing). **Two assertions, deliberately:** the prefix-absence half alone
  would also be satisfied by a render that failed to produce a document at all, and the stderr
  half alone would be satisfied by an unrelated warning. Together they say "the external path ran."
- T8.3 — Revert the `!== undefined` guard (emit `params[kCrossrefNumbering]` unconditionally, so
  an unset key sends `undefined` into the blob) → whether this reddens depends on how
  `param()` coerces a JSON `null`; **state the expected direction in the PR and assert it** rather
  than leaving it to chance. This is the row most at risk of being vacuous: if `undefined` is
  dropped during JSON encoding, the guard is unobservable and T8.3 should be re-pointed at the
  *schema default* instead (assert `"quarto"` is what the absent case resolves to), with the guard
  logged `accepted-untested`.

### Refactor-induced vacuity check

- **T8.3 is the "default is bit-for-bit current behavior" claim again**, in the same shape this
  plan already flagged twice (Task 4's `enable-crossref: false` cell, Task 3's A7-adjacent T3.6).
  An assertion that existing tests are unchanged is true before and after this task **by
  construction**, because the absent-key path is exactly the pre-task path. It is a legitimate
  regression guard, not a binding — and the *discriminating* mutation is the one named above
  (emit unconditionally), not deleting the feature.
- **T8.2's fixture must not reuse `all-docx.qmd`.** That fixture is the subject of
  `docx.test.ts`'s existing expectations; adding `numbering: external` to it would flip those
  expectations and couple two tests to one file. A new single-figure fixture keeps T8.2's RED
  attributable to this task's hunk.
- **T8.2 asserts the absence of a prefix, which is satisfiable by an empty render.** Paired with
  the stderr assertion and, per the acceptance criterion, with the caption body text being present
  — the "path was actually exercised" assertion in its literal form.
- **T8.1's enum is the discriminator, not the key's existence.** Verified the object is
  `closed: true` (`document-crossref.yml:9`), so key-acceptance alone is a real signal — but a
  future "fix" that widens `enum` to `string` would silently un-discriminate the bogus-value half.
  Both halves belong in one test.

---

## Missing-test pass

Every load-bearing safety branch and structural contract in P3, with either a bound seam or an
explicit `accepted-untested`. Silent omission would read as "covered."

1. **The `warn()`-and-skip degradation path (round-4 review named this; it must not pass
   unbound).** Located precisely: `crossref/tables.lua:223` `float_title_prefix` →
   `:229-231` `if float.order == nil then warn("field 'order' is missing from float. Cannot
   determine title prefix for crossref."); return {} end`; plus the sibling
   `warn("Subfloat without crossref information")` at `floatreftarget.lua:219` (inside
   `decorate_caption_with_crossref`) and `:273` (inside `full_caption_prefix`).
   **Bound — and it is the matrix's discriminator, not a footnote.** T7.3/T7.4 assert *both*
   halves the parent instruction asks for: that the warning **fires** (stderr contains the exact
   string) and that the skip is **survivable** (the render completes, `word/document.xml` exists,
   and the caption body `A caption here` is present). The two subfloat warnings at `:219`/`:273`
   are **`accepted-untested`**: `:273` is in the dead `full_caption_prefix` branch (T3.4), and
   `:219` requires a subfloat, which upstream's own `all-docx.qmd` has commented out
   (`_subFigRegexes` is dead in `tests/smoke/crossref/docx.test.ts`) — adding subfloat coverage
   is P7's fixture-set decision, not P3's.

2. **The *callout* degradation path is not a warn — it is a hard error, and the plan says
   otherwise.** `modules/callouts.lua:6-17` `callout_title_prefix` has **no `order == nil`
   guard**: it calls `titlePrefix(category.ref_type, default, callout.order, withDelimiter)`
   (`:17`) → `crossref/format.lua:27` `numberOption(ref_type, order)` → `:124` (type ~= "sec") →
   `formatNumberOption` → `local num = order.order`, which raises *attempt to index a nil value*.
   Callout nodes **do** exist under external mode (`callout.lua:36` declares real `class_name`s,
   so the node is parsed in the pre-gate AST pipeline) and `decorate_callout_title_with_crossref`
   proceeds past `is_valid_ref_type` for any `#nte-`/`#tip-` id. So under external mode a
   crossref-labeled callout with no injected `order` **fails the render**. P3's audit table states
   the opposite for this class of site ("silently drops the caption prefix with a warning rather
   than erroring").

   **RESOLVED 2026-09-18, decided with Gordon: add the missing guard to this PR** (anchor **A7**,
   Task 3). The audit table's framing becomes *true* rather than being corrected away — A7 gives
   `callout_title_prefix` the same order-nil `warn()`+`return {}` that
   `float_title_prefix` has had all along (`crossref/tables.lua:229-231`), so the callout site now
   degrades exactly like the float site. **Bound, no longer accepted-untested:** T3.5 (the guard
   fires: exit 0, `field 'order' is missing from callout` on stderr, no prefix), T3.6 (the guard
   is unreachable upstream, via an *inversion* mutation — deletion cannot discriminate a
   backward-compatibility claim), and Task 7's M3/M4 rows, whose fixture now carries a
   `#nte-`-labeled callout precisely because A7 made that run observable instead of fatal.

3. **The re-vendor tripwire.** **Bound at T6.1a/T6.1b/T6.1c** — a q2-side `I`-tier file-content
   test that needs no pandoc, no shim, and no transport. **Verdict on the ordering question the
   parent raises:** P3's plan and the design doc both delegate drift detection to P5 ("the P5
   contract test flags if a Q1 bump moves the gate"), and **P5 is scheduled after P3**, so P3 as
   written relies on a successor plan for its own patch's tripwire. That is a real but *bounded*
   exposure: nothing re-vendors between P3 and P5 (P4 performs the initial vendoring), so the
   window is P4-lands → P5-lands. T6.1 closes it anyway, at negligible cost, and P5's Layer-1/
   Layer-2 tests remain the *behavioral* tripwire (T6.2, deferred). T6.1 is structural and P5's
   is behavioral; keep both.

4. **An unknown `crossref-numbering` value.** There is **no branch**: both predicates compare
   against the string `"external"`, so `crossref-numbering: bogus` behaves exactly like
   `"quarto"` — Q1 numbers the document and no diagnostic is emitted anywhere. **Bound** at
   T2.1's unknown-value row (predicate level) and T7.5/M5 (behavioral level), which pin that
   frozen behavior.

   **Materially updated 2026-09-18 by Task 8, and this is the one missing-test verdict Gordon's TS
   decision actually changes.** The paragraph above was written when the param had no YAML key, so
   a "typo" could only arrive from a consumer hand-building the params blob, where nothing could
   plausibly validate it. Task 8 adds a real user-facing key backed by an `enum: [quarto,
   external]` in a `closed: true` schema object — so **for the YAML path a typo is now caught,
   loudly, by quarto-cli's own schema validation, before any filter runs**, and that is bound by
   **T8.1's `numbering: bogus` assertion**. What remains `accepted-untested` is strictly narrower
   than before: a bogus value injected *directly into the params blob* by a consumer that bypasses
   the YAML layer — which is exactly how q2 uses it. Nothing validates the blob, and adding
   validation there would be new functionality. So the honest split is: **YAML path bound (T8.1);
   blob path `accepted-untested`**, and q2 is on the blob path. Do not read T8.1's green as
   protection for q2's own call site.

5. **The TS plumb's own failure modes (added 2026-09-18 with Task 8).** Three, with verdicts:
   (a) **the key set but never reaching `param()`** — bound by T8.2, which is the whole point of
   that row; (b) **the key absent ⇒ `"quarto"`** — bound by T8.3 as a regression guard, with the
   honest caveat recorded in its own revert-hunk note that the discriminating mutation is
   "emit unconditionally", not "delete the feature"; (c) **`crossref: false` combined with
   `numbering: external`** — **`accepted-untested`.** `crossrefFilterActive` returns
   `metadata.crossref !== false`, so the entire crossref filter group is inactive and `numbering:`
   is inert. There is no branch to test and no diagnostic to assert, and adding one would be new
   functionality. Recorded because "I set `numbering: external` and nothing happened" is a
   plausible report whose cause is a sibling key three lines up.

6. **The `"quarto"` default literal in `param("crossref-numbering", "quarto")`.**
   **`accepted-untested`: behaviorally indistinguishable.** If the default is omitted, `param()`
   returns `nil`; `nil ~= "external"` is true and `nil == "external"` is false, so both predicates
   take exactly the same branch as with `"quarto"`. There is no input that discriminates the two
   spellings, so no test can be written for it. Keep the literal for readability.

7. **`sections.lua`'s suppression under external mode (design doc §11).** Under
   `crossref-numbering: external` the whole `quarto_crossref_filters` group is skipped, and
   `sections()` is inside it (`main.lua`'s `crossref-combineFilters` entry lists
   `file_metadata(), qmd(), sections(), crossref_figures(), equations(), crossref_theorems(),
   crossref_callouts()`) — so `number-sections: true` silently loses section numbers until
   `bd-5aklrxgi` lands. The decision not to fix it is frozen. **`accepted-untested` as a
   known-shape golden**, deliberately: the loss is *not* asserted anywhere, and it should not be
   asserted by P3. Reasons, in order: (a) the golden would have to be captured against P4's
   transport, which P3 cannot reach; (b) `number-sections` support is a *general* pre-existing Q2
   gap (absent for HTML too), so a golden here would pin Q1-Lua behavior that Q2's own renderer
   does not match, inviting the wrong fix; (c) P7 owns the fixture set and the
   labeled-accepted-divergence golden pattern (it already does exactly this for mermaid) — that
   is where a `number-sections` divergence golden belongs if one is ever wanted. Reference
   `bd-5aklrxgi` if it surfaces in a P7 golden diff.

8. **The upstream-PR-vs-carried-patch divergence.** If upstream merges a *different* shape (say,
   `crossref: numbering: external` as nested metadata, or a differently-named predicate), what
   catches it? **Partially bound, partially `accepted-untested`.** Bound: at the next re-pin,
   T6.1b/T6.1c redden (the merged upstream `main.lua` would not contain the vendored patch's exact
   predicate call, and/or the `param("enable-crossref"` allow-list would no longer match), and
   P5's Layer-2 golden would redden behaviorally. Unbound: **nothing detects the divergence
   between merge and re-pin** — the vendored tree keeps working on the carried patch and no test
   consults the upstream repo. `accepted-untested`: a test that queries GitHub for the PR's merged
   state is a network dependency in `cargo nextest`, which this repo does not do anywhere.
   The mitigation that *is* in scope is documentary: the `QUARTO-PATCH(upstream PR …#N)` marker
   names the PR, so whoever performs the re-pin has the pointer — which is precisely what
   T6.1a's digits-after-`#` assertion protects.

9. **`layout/ipynb.lua:121,126`'s deliberate no-change.** **`accepted-untested`.** Both
   `add_renderer("PanelLayout", …)` calls pass the identical `render_ipynb_layout` callback
   (re-verified 2026-09-18, `:120-127`), so no input distinguishes patched from unpatched and no
   test can be written. Worth a one-line comment at the site in the vendored copy explaining why
   these two `param("enable-crossref"` occurrences are *deliberately* left alone — otherwise
   T6.1c's allow-list reads as an unexplained exception, and a future implementer "completing the
   patch" would redden T6.1c for a change that alters nothing.

10. **`customnodes/theorem.lua:278`'s implicit gate (`if order == nil then return el end`).**
   **`accepted-untested` within P3.** The plan says it "auto-adapts once P6 wires `order` onto
   the node," which is right, but it is unreachable in Task 7's matrix: `theorem.lua:67` declares
   `class_name = {}`, so a raw `::: {#thm-x}` Div is **not** parsed into a `Theorem` node by the
   pre-gate AST pipeline — the node is created by `crossref_preprocess_theorems()`
   (`crossref/theorems.lua:5`), which is *inside* the suppressed group. So under external mode
   there is no `Theorem` node for this branch to guard until P5's shim constructs one. Verdict:
   this contract belongs to **P5/P6's** per-type goldens, not to P3; P3 asserts nothing and this
   file says so rather than leaving the plan's "no patch needed" reading as tested.

11. **`float_title_prefix`'s and `callout_title_prefix`'s `fail()` branches for an unknown
    category** (`crossref/tables.lua:226`, `modules/callouts.lua:9`). **Out of P3's scope, and
    already owned:** this is design doc §12's "a callout whose crossref category Q2 knows and Q1
    doesn't" bullet and P6 Finding 2 / P5's Callout `fail()`-fallback. Recorded here only so the
    reader knows it was considered and assigned, not missed.

---

## Findings for Gordon

Three items. Each is a place where a test cannot be written the way the plan describes because the
plan's own mechanism does not support it. **All three were decided by Gordon on 2026-09-18 and are
applied** — the TS plumb is now Task 8 (carried by Task 5's PR), the callout `order == nil` guard
is anchor A7 in Task 3, and the positive external-mode golden has moved to P6. Nothing on this
plan is open.

1. **The upstream PR, as scoped, is untestable end-to-end upstream — because nothing plumbs
   `crossref-numbering` into `QUARTO_FILTER_PARAMS` on the TS side.** P3 scopes the patch as
   "3 files, ~6 edited lines" of Lua. But the param is only ever read via `param()`, i.e. out of
   the `QUARTO_FILTER_PARAMS` blob that `src/command/render/filters.ts` builds, and **there is no
   TS writer for `crossref-numbering` and no YAML key**. Verified: the only `kEnableCrossRef`
   writer is `filters.ts:534`, and `grep -rn 'crossref-numbering' src/` in quarto-cli returns
   nothing. Consequences: (a) no upstream smoke test can reach `crossref-numbering: external`, so
   the *discriminating* direction of all five patched gates has **zero** upstream coverage — the
   Q-tier can only assert "the two pre-existing cells still behave as before"; (b) the PR's
   stated general-usefulness pitch ("front end supplies numbers", "other quarto-cli consumers may
   want this too") is not actually available to a quarto-cli user, only to a consumer that builds
   the params blob itself, as q2 does; (c) q2 is unaffected functionally — it builds the blob —
   but it means **every discriminating test of P3's patch lives in q2's L tier (Task 7) and is
   therefore blocked on P4**, in a plan the epic graph schedules as independent and parallel with
   P1/P2. Question: should the PR also include the TS plumb (a metadata→param read alongside
   `crossrefFilterParams`, plus an upstream smoke test that sets it), or is P3 deliberately a
   Lua-only patch whose only real test lives downstream in q2?

   **RESOLVED 2026-09-18, decided with Gordon: add the TS side and its tests.** Landed as
   **Task 8**, carried by Task 5's PR. Four files, all upstream, all anchors verified at
   `v1.11.3`: a `kCrossrefNumbering` constant in `config/constants.ts` (placed with the
   `crossref-*-title` family, not next to the YAML-less `kEnableCrossRef`), a `numbering:` property
   on the `crossref` schema object, the read in `crossrefFilterParams`, and two smoke tests.

   Three things the research changed about how that task had to be written, worth surfacing
   because each would have bitten an implementer working from the Findings text alone:
   - **The schema edit is mandatory, not cosmetic.** `document-crossref.yml:9` declares the
     `crossref` object `closed: true`, so `crossref: {numbering: external}` is *rejected by
     validation* before any TS or Lua sees it. A TS-only plumb would have looked correct and done
     nothing.
   - **The read must not go through `crossrefOption`.** That helper consults only `flags` and
     `defaults` and returns `undefined` otherwise — it never reads metadata, so routing this key
     through the obvious-looking path would always yield `undefined`. The right precedent is
     `params[kNumberDepth] = metadata?.[kNumberDepth]` one line below the option loop.
   - **`numbering:` is a sub-key of a block that can be switched off.** `crossrefFilterActive`
     returns `metadata.crossref !== false`, so under `crossref: false` the whole filter group is
     inactive and `numbering:` is moot. Stated as a bound on the feature in the PR description; no
     diagnostic for the combination, which would be new functionality.

   **Consequence (a) of Findings #1 is now resolved** — the discriminating direction of Tasks 2–3's
   gates has real upstream coverage via T8.2. **Consequence (b) is resolved** — the PR's
   general-usefulness pitch is now actually available to a quarto-cli user. **Consequence (c) is
   narrowed but not removed**: Task 7 is still blocked on P4, it is just no longer the *only* place
   the external direction is exercised. **P3's parallel-schedulability with P1/P2 is unchanged** —
   Task 8 touches only the upstream tree and runs in the `Q` tier, needing no q2 crate, no
   vendored tree and no pandoc invocation from q2. Task 8 states this explicitly rather than
   leaving it inferred.

2. **The plan's "degrades to a silent `warn()`, not an error" framing is false for the callout
   gate site, and it is the one site whose test I therefore could not bind.** P3's audit table
   classes the downstream `order == nil` nil-guards as "defensive `warn()`-and-skip" and builds
   the regression-check rationale on that ("a missing-`order` bug degrades to a silent `warn()`,
   not a failure, so the golden has to check the string"). That holds for floats —
   `crossref/tables.lua:229-231` guards `float.order == nil` explicitly. It does **not** hold for
   callouts: `modules/callouts.lua:17` passes `callout.order` straight into
   `titlePrefix` → `numberOption` → `formatNumberOption`'s `local num = order.order`
   (`crossref/format.lua:124,140`), with **no nil guard anywhere on that path**. Under
   `crossref-numbering: external`, a `#nte-`-labeled callout whose `order` Q2 has not injected
   therefore raises a Lua error and fails the render — and callout nodes *do* survive into
   external mode (real `class_name`s at `callout.lua:36` ⇒ parsed pre-gate). This is new
   reachability that P3's patch creates: today the site is unreachable with a nil order, because
   `enable-crossref: false` early-returns and default mode always assigns.

   **RESOLVED 2026-09-18, decided with Gordon: add the missing `order == nil` guard to the PR.**
   Landed as anchor **A7** in Task 3, mirroring `float_title_prefix`'s guard
   (`crossref/tables.lua:229-231`) line for line. Three consequences, all applied:
   - The plan's "degrades to a silent `warn()`, not an error" framing is now **true for this site
     too**, so it needed no correction — the guard makes the audit table right rather than the
     table being weakened to match the code.
   - Task 7's matrix fixture **now includes** a `#nte-`-labeled callout (the exclusion existed
     only because the run aborted), and its M3/M4 rows assert both the float and the callout
     warning, which are distinguishable strings.
   - T3.5/T3.6 bind the guard. Worth flagging one subtlety that fell out of binding it: the
     backward-compatibility half (T3.6) **cannot** be reddened by deleting A7, because A7 is
     unreachable upstream and "upstream unchanged" is therefore true with or without it. T3.6's
     named mutation is an **inversion** of the predicate, which is the only thing that proves the
     row tests reachability instead of restating a tautology.
   `return {}` was verified safe for the one caller rather than assumed — `tprepend(title, {})`
   (`modules/callouts.lua:46`) is a no-op, so the callout renders prefix-less exactly as a float
   does. Details in Task 3's A7 section.

3. **P3's checklist item 3 (the Q2 golden) has no P3-owned revert hunk — it discriminates P5's
   and P6's hunks, not P3's.** Working the binding through: under `enable-crossref=true,
   crossref-numbering=external` with an injected order, reverting A2 changes nothing (the old
   expression is already false), and reverting the assign-numbers conjunct yields the *same*
   rendered number for a single-figure fixture (Q1 would compute `1` too). What the golden
   actually certifies is that P5's shim passed `order` through and P6 set the param. It can be
   made to discriminate "Q2's number, not Q1's" only by constructing a fixture where the injected
   order differs from what Q1 would compute.

   **RESOLVED 2026-09-18: moved to P6.** P6's companion had independently reached the same
   conclusion and had already written its Task 4 assuming the move, so the two files agreed and
   the only thing outstanding was to pick one and delete the other. P3's Task 8 is now a pointer
   section explaining the move; **P6's Task 4 is the single owner**, and it is the stronger
   version — it specifies the `order = 7`-against-Q1's-`1` fixture that actually discriminates
   "Q2's number" from "a number", which the single-figure fixture specified here could not. The
   deciding reason was the discipline's own rule rather than convenience: a test belongs where the
   hunk whose revert reddens it lives, and every hunk this golden discriminates is in P5 or P6.
   Secondary but real: P3 is scheduled before P4, so it could never have run this golden at all.
   P3's remaining acceptance is Task 7's matrix, which binds P3's own hunks.

**Not findings, recorded so they are not rediscovered:** `floatreftarget.lua:242`
(`full_caption_prefix`) is a dead branch from both Q2's and upstream's perspective — its only
callers are in `layout/lightbox.lua`, which returns `{}` unless `quarto.doc.is_format("html:js")`,
a format where `enable-crossref` can never be false — so it is patched for consistency and
asserted nowhere (T3.4). And `git diff v1.11.3..HEAD` is **empty** for all five files P3 reads, so
every line number in this document is currently valid for both the pinned tag and upstream HEAD.
