# P6 — Category passthrough + numbering suppression

**Date:** 2026-08-20  **Updated:** 2026-09-18 (round 4 review) — cross-referenced P5's corrected
Callout `fail()`-fallback warning against this plan's own Finding 4 argument (both address the
same silent-number-loss risk, from different angles); fixed a stale `valid_ref_types()` line
citation.
**2026-09-17 (two passes)** — an epic-wide Opus review found this
plan's own H1 and Finding 1's closing paragraph both went stale after later plans overturned
them: retitled "seed" to "passthrough" (matching the design doc's §4 C rewrite, which this plan's
own Finding 1 drove but which never actually landed there until now), and corrected Finding 1's
claim that "Q1 always uses its own hardcoded English defaults regardless of what Q2's
`RefTypeRegistry` says" — P4's later "Q2 owns presentation defaults" finding says the opposite
via the `crossref-<type>-title` param. Also retracted a stale `ExampleEmbed`/Route N citation
(P1 resolved `ExampleEmbed` entirely upstream after this plan's Finding 3 was written). (First
pass, same day: full claim-by-claim audit against Q2's real crossref code and Q1's real vendored
Lua — previous update was the shallow first-pass rework, not a full audit. Found the prior
draft's framing of Problem C was wrong in a load-bearing way (there is no single "categories
bootstrap" to seed), closed the default-category-set drift check with a concrete table, found
and — with Gordon — resolved a real numbering gap in the frozen Route-L/R table (Callout), and
found the "combined subfloat/panel" checklist item is currently dormant (the Q2 feature it
depends on doesn't exist yet), not blocked.)
**Status:** Shape draft
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`  |  Depends on: P3, P5
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P6-implementation.md`](2026-09-18-pandoc-hybrid-P6-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Goal
The Q2-side wiring that makes Route-R crossref render correctly: **make sure Q1's category
mechanisms recognize Q2's ref-types** and **activate external numbering** (P3's mode) so Q1
renders using Q2's numbers without re-assigning. This is the residual "crux" (design note §4,
sub-problems C + D) — narrower in practice than the design doc's prose suggests (see Finding 1).

## Finding 1 — Problem C's premise is wrong: there is no single "categories bootstrap" to seed

The design doc's §4 Problem C says "seed Q1's `crossref.categories` by feeding it as metadata so
Q1's `custom.lua` rebuilds it," and the prior draft of this plan asked to compare
`RefTypeRegistry` against "Q1's `custom.lua` categories bootstrap." **Both are wrong about what
`custom.lua` is.** Reading `/Users/gordon/src/quarto-cli/src/resources/filters/crossref/custom.lua`
and `mainstateinit.lua` directly shows Q1 has **four independent, structurally different
built-in-category mechanisms**, and `custom.lua` is not one of them for built-ins — it only
handles user-declared extensions:

| Mechanism | File | Extensible via metadata? | Prefixes |
|---|---|---|---|
| `crossref.categories.all` (floats + blocks) | `mainstateinit.lua:32-119` | Yes, via `add_crossref_category` — but the table is a **static Lua literal**, not built from metadata at load time | `fig,tbl,lst` (float); `nte,wrn,cau,tip,imp,prf,rem,sol` (Block) |
| `theorem_types` | `customnodes/theorem.lua:7-53` | **No** — static table, no registration function exists at all | `thm,lem,cor,prp,cnj,def,exm,exr,alg` |
| bespoke `eq` handling | `crossref/equations.lua` | No — hardcoded string, native LaTeX/Typst numbering (`\begin{equation}`, `#math.equation`), no display-name table entry at all | `eq` |
| bespoke `sec` handling | `crossref/sections.lua` | No — hardcoded string, native Pandoc header numbering (`1.2 Title`), no display-name table entry at all | `sec` |
| `crossref.custom` (**this is what `custom.lua` actually is**) | `crossref/custom.lua:6-157` | Yes — `initialize_custom_crossref_categories(meta)` parses `meta.crossref.custom` directly and calls `add_crossref_category` | user-declared only |

Confirmed authoritative via `valid_ref_types()` (`crossref/refs.lua:198-212`, corrected
2026-09-18 from `198-210`):
```lua
function valid_ref_types()
  local types = tkeys(theorem_types)
  for k, _ in pairs(crossref.categories.by_ref_type) do table.insert(types, k) end
  table.insert(types, "eq")
  table.insert(types, "sec")
  return types
end
```
This is the exact union of the four mechanisms above — nothing else contributes to it.

**Consequence for Problem C: built-in ref-types cannot be "seeded" via metadata at all.** Three
of the four mechanisms have no registration function; the fourth (`crossref.categories`) is
pre-populated at file-load time, before any metadata is read. The only lever that exists for
built-ins is `crossref-<type>-title` (a **display-name override param**, read by `title()` in
`crossref/format.lua:4-7` — i18n, not registration) — and P4 already owns plumbing this family
(`languageFilterParams`, per P4's plan, "required — without them Q1's category init has nothing
to read for caption prefixes").

**Correction, 2026-09-17 (found by an epic-wide review pass): the paragraph below originally
said the opposite of what P4 later decided, and was never updated.** It read: "Built-in display
names are not a P6 concern: Q1 always uses its own hardcoded English defaults ... regardless of
what Q2's `RefTypeRegistry` says, for every built-in prefix" and treated `kind`-style fields as
"redundant... for every built-in Route-R type." **That's wrong as of P4's finding** ("Q2 owns
presentation defaults," decided with Gordon): `title()` checks the `crossref-<type>-title` param
**before** falling back to Q1's static default, so Q2's `RefTypeRegistry` *is* authoritative for
every registered type's display name via that param — P4's params-blob builder is the mechanism,
not passive inheritance from Q1's locale files. Q1's own static default is only ever load-bearing
for a prefix Q2 doesn't register at all. This still confirms this finding's original point about
*where* the field lives (not the constructor — `crossref-<type>-title` bypasses it entirely), so
Theorem's `kind` still never goes through `theorem.lua`'s constructor — but it is not "redundant"
in the sense of "Q2's opinion doesn't matter"; it matters via a different channel than the one
this plan originally assumed (P5's field-map).

## Finding 2 — Custom categories: passthrough already exists, no export mechanism needed

For **user-declared** `crossref.custom` entries (the one case Q1's `custom.lua` genuinely does
register from metadata), Q2 already has an independent, from-scratch reader of the exact same
YAML shape: `crossref/metadata.rs::read()` — its own doc comment states "we map this **verbatim**
to the Q1 schema." Two facts close this out without new code:

1. **`crossref.custom` is never stripped from the merged metadata `doc.ast.meta` map.** Audited
   `stage/stages/metadata_merge.rs:459-460` — the only key explicitly `retain`-filtered out of
   `meta` is `"format"`. `crossref` (and its `custom` child) survives into the Pandoc `Meta` Q2
   ultimately emits.
2. **`initialize_custom_crossref_categories(meta)` runs unconditionally today**, confirmed by
   P3's audit (called from `quarto-init/metainit.lua:10`, part of the always-run
   `quarto_init_filters`, upstream of the `enableCrossRef` gate entirely).

So: as long as the wire format's Pandoc `Meta` carries the document's original `crossref:`
metadata block through to the Q1 invocation (which nothing currently plans to strip — P2's
"Meta-block contract" only calls out title/date/authors as *required* carriage, it doesn't say
anything else is *removed*), **Q1's own `custom.lua` will register user-declared categories
itself, reading the same source YAML Q2 already reads.** Problem C reduces from "build a
Q2→Q1 registry export" to "confirm the passthrough holds and add a regression test for it" —
a much smaller item than the design doc's prose implies. Field-shape parity is also already
verified 1:1 by `crossref/metadata.rs`'s own tests (`key`→`ref_type`, `reference-prefix`→display
name), so there's no new mapping code to write, only a golden confirming the Pandoc leg doesn't
regress it.

**Promised ids** (`crossref.ids`, entries with no declared category — `RefTypeSource::Promised`)
have **no Q1 equivalent at all**: Q1 has no concept of "`crossref.ids`," so a Route-R node whose
`ref_type` came from a `Promised` source has nothing in `valid_ref_types()` to match against.
This was already true before this epic (it's a pre-existing Q2-only mechanism per the design
plan D6/D7 cited in `registry.rs`'s doc comment) and isn't new scope for P6 — noting it here only
so a future reader doesn't mistake it for something Problem C needs to solve.

## Finding 3 — Default-category-set drift: audited, closed, no unexplained gaps

Comparing Q2's `RefTypeRegistry::BUILTINS` (21 entries) against the union of Q1's three
built-in mechanisms (22 entries, Finding 1's table) turns up exactly two asymmetric entries,
both already explained by design, not bugs to fix here:

| Prefix | In Q1? | In Q2 BUILTINS? | Status |
|---|---|---|---|
| `prf` (Proof) | Yes — `crossref.categories.all`, kind=Block | **No** | **Confirmed intentional, not drift.** `transforms/proof.rs`'s doc comment: proofs are deliberately unnumbered — the sugar transform does not populate `plain_data.ref_type` at all, "the resulting `CustomNode("Proof")`... [is] skipped" by the indexer. P3's audit independently confirmed Q1's `proof.lua` renderer never reads `.order` either. Q1's `prf` category entry is real but its own renderer doesn't consume order from it — both sides agree Proof isn't numbered. No action. |
| `alg` (Algorithm) | Yes — `theorem_types` | **No** | **Confirmed real gap, already tracked — not new.** P5 found this independently: `theorem.rs`'s `THEOREM_CLASSES` (8 entries) has no `algorithm` entry. P5 also flagged a second, deeper issue alongside it (Q2 detects theorem-like divs by class name, Q1 by identifier prefix) and left the fix location as P5's own open item. **P6 does not re-open this — it's P5's checklist item, cross-referenced here so the drift audit reads as closed rather than silently dropped.** |
| `demo` (Demo) | **No** (absent from all three Q1 mechanisms) | Yes | **Confirmed intentional.** `demo` is a Q2-only feature (bd-t3cert81); nothing to reconcile against Q1. **Corrected 2026-09-17:** this row previously cited `ExampleEmbed`/Route N as the reason, retracted twice over since — P1 later resolved `ExampleEmbed` entirely upstream in Rust (format-parameterized B1, never reaching the shim for Pandoc targets at all), so it's neither Route N nor P5's item. The `demo` prefix asymmetry stands on its own regardless: it's a Q2-only ref-type category with no Q1 counterpart to reconcile, independent of how `ExampleEmbed` the CustomNode is handled. |

No other prefix mismatches exist in either direction. The checklist item "check default-category-
set drift" is complete as of this pass; it does not need to be re-run unless either side's
built-in list changes.

## Finding 4 — Callout needs Route R, not Route L (decided with Gordon)

**Reverses part of the frozen Route-L/R table (Decision 4).** A Callout with a crossref-eligible
identifier (e.g. `#nte-important`) is not presentation-only: it gets `plain_data.order` from the
*same* shared, pre-cut `CrossrefIndexTransform` that FloatRefTarget/Theorem use — confirmed by
reading `transforms/crossref_index.rs`'s generic `has_crossref_plain_data`/`index_custom_target`
path (matches any custom node carrying the `ref_type`+`kind`+non-empty-identifier triple,
regardless of type name), fed by `callout.rs`'s own construction step, which sets exactly that
triple when the id matches a registered ref-type (`test_callout_with_crossref_id_gets_plain_data_triple`).

Route L discards this: it reconstructs a **raw classed Div** (no `plain_data`, no wire fields at
all) and lets Q1's unmodified `callout.lua` parse+render fully re-derive everything from scratch —
including `.order`, normally assigned by `crossref_callouts()` (`customnodes/callout.lua:469-482`).
But `crossref_callouts` is part of `quarto_crossref_filters`, the exact filter group
`crossref-numbering: external` (P3) suppresses globally. So under external mode, a Route-L
callout with a crossref id gets **no order from either side** — not from Q2 (thrown away by
raw-Div reconstruction) and not from Q1 (its assignment pass is suppressed) — and
`decorate_callout_title_with_crossref` (`modules/callouts.lua:22`, one of P3's own 4 audited
render-decoration gate sites) silently skips decorating the title, since it's written as a nil-
guarded early return, not an error. Net effect: numbered callouts silently lose their "Note N:"
prefix in docx/pptx, with no error to flag it.

**Decided with Gordon: reclassify Callout to Route R**, uniformly (numbered or not) — mirrors
Tabset's reclassification in P5, and matches the design doc's own stated general rule for new
types ("numbered → R, else L"). Mechanically low-risk: the wire→constructor field-map was already
confirmed mechanical/1:1 in the design doc (`plain_data.{type,appearance,icon}` +
`slots.{title,content}` → `quarto.Callout({type,appearance,icon,title,content})`); Route R just
means the shim calls that same constructor directly and assigns `.order` onto the returned table
before invoking `render` — the identical post-construction-assignment mechanism P5 already
designed for Theorem (`theorem.lua:222`, constructor has no `order` field, renderer reads it off
the table directly). No new mechanism, no new field-map — just a routing-table change plus reuse
of an existing pattern.

**Follow-on for the design doc and P5 — done.** §3's Route-L/R table (Decision 4, frozen) needed
updating for **both** Callout (this finding) and Tabset (P5's reclassification). **Landed** on
2026-09-17 (commit `8e9e546b4`), both in the same pass, as anticipated above.

**Cross-reference, added 2026-09-18 (round 4 review, Reviewer D):** this Finding's whole argument
for reclassifying Callout to Route R was to stop the silent "Note N:" loss that Route L's raw-Div
reconstruction causes for a labeled callout. Round 4 review found P5's own error-handling fallback
(the Callout `fail()`-guard, for `ref_type`s Q1's category table doesn't know) routes exactly that
subset of callouts back down the Route-L-shaped path — for a different reason (avoiding a hard
crash, not presentation-only routing), but with the identical silent-loss consequence this Finding
argued against. P5 now emits a warning at that fallback and lists it in design doc §12; no change
needed here, cross-referenced so this Finding's own "no error to flag it" framing doesn't read as
fully closed by the R reclassification alone.

## Finding 5 — The "combined subfloat/panel" checklist item is dormant, not blocked

Traced the concrete failure mode the original checklist item worried about (a Tabset of figures,
or an appendix-relocated float, landing on the same numbering machinery) against both sides:

- **Appendix-aware numbering doesn't exist in Q2 yet.** `crossref/index.rs`'s `CrossrefEntry`
  has an `in_appendix` field, but `crossref_index.rs`'s builder hardcodes it to `false` with the
  comment "deferred" — Phase 1 is explicitly flat, single-file numbering only. Q1's own
  appendix-numbering machinery (`crossref.startAppendix`, set by `indexNextChapter` in
  `crossref/index.lua:31-43`, consumed by `format.lua:111-112,156-157` to render "A.1"-style
  labels) is **entirely inside** the same assign-group P3 suppresses under external mode — so
  even if it weren't dormant on the Q2 side, Q1 could never independently reconstruct it either
  once numbering goes external. **Since Q2 never produces an appendix-flagged order today, there
  is nothing for Route R to carry and nothing for Q1 to conflict with.** No live bug exists.
  **Forward note for whoever implements appendix-aware numbering in Q2 later:** that work will
  need to inject its own "A.1"-style formatting via `plain_data` (the same way flat order is
  injected today) rather than relying on Q1's `crossref.startAppendix`/`format.lua` path, since
  that path is unreachable under external mode. File that as new scope then, not now.
- **The Tabset/subfloat nesting case is dormant too, not "gets the same treatment for free"
  (corrected 2026-09-20, per P6 Task 6's implementation-companion review).** The original claim
  here was that `prependSubrefNumber(caption_content, float.order)`
  (`customnodes/floatreftarget.lua:221`) reads `.order` generically off whatever table it's
  handed, so a Tabset-containing-figure gets subfloat lettering "for free." Reading the
  surrounding branch shows that path is reached only for a float Q1 recognizes as a *subfloat*,
  which requires a `parent` link — and Q2 hardcodes `parent: None, // subfloats deferred`
  (`crossref_index.rs:304`), exactly like the `in_appendix: false` field one paragraph above.
  `CrossrefEntry.parent` (`crossref/index.rs:97`) is documented infrastructure with **zero
  readers** anywhere in `crates/quarto-core/src/crossref/` or the crossref transforms. So no Q2
  wire node can carry the parent link that selects the subfloat branch, and a
  Tabset-containing-figure fixture asserts **top-level** numbering of a nested figure, not
  subfloat lettering — the same verdict as the `in_appendix` bullet above: **confirmed dormant,
  no live bug exists.** Tracked as `bd-plcqhfcn` (parented to the crossref epic `bd-jsbg`), which
  is the first strand for a deferral already on record twice in
  `claude-notes/plans/2026-04-15-crossref-design.md` (lines 77 and 405) with no strand until now.
  P5's own round-4 traversal sub-finding already names the fixture this needs
  (`nested-float-in-callout.qmd`, a `FloatRefTarget` inside a `Callout`'s `content` slot) and P5's
  Layer-2 goldens own it; P6's own nesting coverage is its `test_all_three_injected_orders_*`
  pair in `crossref_external_mode_matrix.rs`, which asserts top-level numbers for all three types
  together, not subfloat lettering. No new checklist item needed — the gap is a feature request,
  not a P6 bug.

**This checklist item is closed for v1**, downgraded from "trace" to "confirmed dormant + one
cross-reference to P5's existing golden coverage." Re-open only when Q2 implements appendix-aware
numbering.

## In scope
- **Confirm the passthrough, not build a translator** (Finding 2): a regression test that
  `crossref.custom` metadata survives unmodified into the Pandoc `Meta` a `Pandoc(fmt)`-profile
  render emits, and a Q1-side golden showing `custom.lua` registers it and renders the custom
  category's caption prefix correctly from that same Meta.
- **No built-in registry export work** (Finding 1) — explicitly out of scope, not merely
  deferred: there is no metadata-time registration hook to feed for any of the three built-in
  mechanisms.
- **Reclassify Callout to Route R** (Finding 4, decided). The design doc §3 table update landed
  alongside P5's Tabset reclassification (2026-09-17, commit `8e9e546b4`); the shim implementation
  itself is still open (P5's file).
- **D — numbering suppression:** set `crossref-numbering: external` in the params blob for
  `Pandoc(fmt)` profiles (P4 plumbs the key generically once P6 supplies the value); confirm
  registration (unconditional, per P3) and render decoration (P3's `crossref_present()` sites)
  still run and consume `plain_data.order`, now covering Callout as well as FloatRefTarget/Theorem.
- Figure/theorem/**callout** number parity golden: docx "Figure N:"/"Theorem N."/"Note N:" ==
  HTML's numbers, for the same source document.

## Out of scope
- The upstream Q1 change itself (P3). The shim mechanics (P5) — P6 consumes P5's
  post-construction-order-assignment pattern, doesn't redesign it.
- Appendix-aware crossref numbering (doesn't exist in Q2 yet — Finding 5).
- `alg`/`algorithm` theorem-class gap (P5's item, cross-referenced not duplicated — Finding 3).
- `demo` Route N handling (moot — `ExampleEmbed` is resolved entirely upstream in P1's Rust
  core and never reaches the shim for Pandoc targets; corrected 2026-09-17, see Finding 3).

## Consumes / Produces (seams)
- **Consumes:** P3's external mode + `crossref_present()`/`assignCrossrefNumbers` predicates;
  P5's Route-R construction + post-construction order-assignment mechanism (now also Callout's
  mechanism, per Finding 4); P4's params-blob builder (P6 supplies the `crossref-numbering` value,
  P4 plumbs it).
- **Produces:** figure/theorem/callout numbers in Pandoc output that match the HTML path (the
  parity goal); the Callout routing decision, landed in the design doc's §3 table; confirmation
  that custom-category passthrough needs no new Rust code, only a regression test.

## Coarse checklist
- [x] Land Finding 1's conclusion in the design doc's §4 Problem C (frozen decisions section) —
      **done 2026-09-17.** Unlike Finding 4's §3 table fix, this had no checklist item and the
      design doc still specified the disproven "seed via metadata" mechanism until an epic-wide
      review caught it; fixed directly in the design doc (retitled §4 C, §9's P6 entry, and this
      plan's own H1 from "seed" to "passthrough").
- [x] Field-for-field comparison: `RefTypeRegistry` vs. Q1's built-in category mechanisms —
      done (Finding 1): there are four mechanisms, not one, and none are metadata-seedable for
      built-ins.
- [x] Check default-category-set parity (fig/tbl/eq/thm/…) between Q2 and Q1 — done (Finding 3):
      two asymmetric entries (`prf`, `alg`), both already explained/tracked, no unaddressed drift.
- [x] Decide Callout's route (Finding 4) — **Route R**, decided with Gordon.
- [x] Land the design-doc §3 table update for Callout, alongside Tabset's — done 2026-09-17
      (commit `8e9e546b4`).
- [x] Land the Callout-to-R change in the shim itself (P5's file — implementation, not the
      table update above) — **done in P5** (commit `4fb8b38e8`, "P5 Task 3: Route R for Callout
      and Tabset"); confirmed by reading `resources/pandoc-filters/filters/quarto2-shim.lua`'s
      `route_callout`, and by P5's own test suite (`test_numbered_callout_gets_prefix`,
      `test_unnumbered_callout_has_no_prefix`, `test_callout_order_must_be_on_the_data_table`,
      `test_nested_wire_nodes_convert_inner_first`).
- [x] Confirm `crossref.custom` passthrough into `Pandoc(fmt)` Meta with a regression test
      (Finding 2) — no new export code, just the test — **done**:
      `crossref_custom_passthrough.rs`'s `test_crossref_custom_survives_into_wire_meta_verbatim`
      (T3.1, negative-space guard on `metadata_merge.rs:460`) and
      `test_crossref_custom_category_renders_its_declared_prefix` (T3.2, real Q1-side render of
      the same fixture, asserts `Diagram\u{a0}1:`).
- [x] Set `crossref-numbering: external` for `Pandoc(fmt)` profiles in the params-blob builder
      (P6 supplies the value, P4's existing plumbing consumes it) — **done**: `params.rs`'s
      `insert_crossref_numbering_mode`, tested by `test_pandoc_profile_sets_external_crossref_numbering`
      / `test_non_pandoc_profile_omits_crossref_numbering`. Landing this surfaced (and fixed) a
      real collateral effect on four pre-existing P3/P4/P5 `L`-tier tests that relied on Q1's own
      `quarto_crossref_filters` group running against hand-built docx params
      (`crossref_numbering_matrix.rs`'s M1/M2, `pandoc_transport.rs`'s
      `test_thm_caption_uses_title_param`/`test_thm_reference_uses_prefix_param`/
      `test_real_q1_warn_call_is_surfaced`, `pandoc_shim.rs`'s
      `test_equation_latex_branch_ignores_order`) — each now explicitly strips or overrides
      `crossref-numbering` to restore the Q1-native-numbering path they were built to exercise.
- [x] Numbering-suppression discrimination matrix (design doc §7/§11, this plan's "In scope"
      item D) — **done**: `crossref_external_mode_matrix.rs`, 9 tests. Bound twice, on two
      independent surfaces per the plan's own discipline: injected orders (`fig`=7, `thm`=5,
      `nte`=3) Q1 would never independently compute, with `_when_reverted` companions proving
      attribution to Task 1's hunk; and the order-free `number-sections` collateral-suppression
      surface (design doc §11, a labeled accepted divergence, `bd-5aklrxgi`). Also a per-mechanism
      registration tripwire (all four built-in category mechanisms stay recognized under external
      mode — labeled as a tripwire, not coverage, since P3 already established registration is
      unconditional).
- [x] **Scope correction, 2026-09-20 (decided with Gordon):** figure/theorem number-parity golden
      — **not callout**. Verified empirically (`q2 render --to html` of a labeled, titled
      callout): Q2's native HTML crossref renderer never numbers `Callout` at all (no "Note N:"
      prefix anywhere), unlike Figure/Theorem, which both render correctly. This is a genuine
      **numbers** gap on the HTML side, not the presentation-only divergence design doc §12
      already documents — there is no callout number on the HTML leg to compare against the
      docx leg's. Out of scope for this epic (its own governing principle); filed as `bd-pk3gtn2i`
      and recorded as a new §12 bullet. **Done**: `crossref_number_parity.rs`, docx numbers ==
      HTML numbers for a fig+thm+proof fixture (the epic's DoD narrowed to *numbers*, not
      presentation, per design doc §12, and numbers are unaffected by that narrowing); the Proof
      renders unnumbered on both legs (Finding 3). **Ordering note resolved:** took option (a) —
      asserted against a real `--to docx` render with a P6-local `docx_to_plain` text extractor
      (a second `pandoc -f docx -t plain` subprocess call, no new dependency), not P7's
      not-yet-built `cargo xtask capture-pandoc-goldens` harness; P7 may re-capture the same
      fixture through its own harness when it lands, per this plan's own hand-off note.
      **Fixture note:** the figure uses the `Div(#fig-..)` authoring form, not the bare
      implicit-figure `![](){}` form — the latter's caption currently renders unnumbered in
      HTML on this pre-rebase branch due to the closed, already-fixed-on-`main` bug
      `bd-hb9a9ik8`/`bd-n3sark9b` (PR #690, merged 2026-09-17, one day after
      `feature/pandoc-writer-hybrid` was cut); decided with Gordon to keep working the plan now
      and let the eventual squash-rebase onto `main` pick up the fix, rather than cherry-pick it
      mid-plan.
- [x] Trace the combined subfloat/panel case — done (Finding 5): confirmed dormant for v1
      (Q2 has no appendix-aware numbering yet); nesting mechanics fold into P5's existing golden.
