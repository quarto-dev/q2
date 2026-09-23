# P2 — Implementation tasks & Test Seam Spec

**Date:** 2026-09-18
**Plan (authoritative scope):** [`2026-08-20-pandoc-hybrid-P2-wire-schema.md`](2026-08-20-pandoc-hybrid-P2-wire-schema.md)
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)
**Epic:** [`2026-08-20-pandoc-hybrid-epic.md`](2026-08-20-pandoc-hybrid-epic.md)
**Depends on:** (per the epic's graph) **nothing** — P1, P2, P3 are parallelizable immediately.
P2 *produces* for P4 (serialization step), P5 (the frozen schema the shim reads) and P7 (`Meta`
carriage + the sideband-map carriage fact for code-block decorations).
**Status:** Ready for subagent-driven execution. **No blockers remain** — the three findings that
parked a seam (3: `cite_prefix`'s shape; 4: the TS interface-diff mechanism; 5: `ExampleEmbed`'s
`route`) were decided by Gordon on 2026-09-18 and are applied, so **T3.4, T4.3 and T1.1 are all
bound** and no `seam deferred until … Gordon` marker remains. **Seven tasks** — Task 7 is new,
carrying `meta.quarto_pandoc_reader_opts`, reassigned here from P4 (its Findings item 3) because
design §8 makes `Meta` carriage P2's. **P4's transport smoke depends on Task 7**, so it is the one
task in this file with a downstream plan waiting on it. Findings 1 and 7 remain open but park
nothing (both are "add a checklist item, or accept the risk explicitly" scope questions).

This file adds nothing to P2's scope — it converts P2's Coarse checklist into `## Task N` units
`superpowers:subagent-driven-development` can dispatch, and binds every test P2 needs to a named
production seam and revert hunk before any code is written (the `/prevalidating-test-seams`
discipline). The Spec is P2 + the design doc; where this file and the plan disagree, the plan wins.

Every code anchor below was opened and verified against the worktree at
`feature/pandoc-writer-hybrid` on 2026-09-18. Where a plan or a source comment cites a line that
has drifted, this file cites what is actually there and records the drift under
[Findings for Gordon](#findings-for-gordon) — it does not silently propagate or silently repair
either one.

---

## Tiers used in this file

| Tier | Meaning | How it runs |
|---|---|---|
| **U** | Rust unit test — `#[test]` in a `mod tests` inside the crate under test | `cargo nextest run -p <crate>` |
| **I** | Rust integration test — `crates/<crate>/tests/integration/<name>.rs`, registered as `pub mod <name>;` in that crate's `tests/integration/main.rs`. **Never** a top-level `tests/<name>.rs` (`.claude/rules/integration-tests.md`) | `cargo nextest run -p <crate>` |
| **T** | TypeScript/vitest test in an npm-workspace package | `npm test -w ts-packages/preview-renderer` |
| **L** | Lua/pandoc integration — real `pandoc --lua-filter` against the vendored Q1 tree | **not used in P2** (the Lua consumer is P5) |
| **G** | dev-only golden capture needing a real Q1 `quarto` binary / `external-sources/` | **not used in P2** |

**CI-gating note (`ci-test-suite-unwired`).** `ts-packages/preview-renderer` is **already wired
into the merge gate**: `.github/workflows/ts-test-suite.yml:287` runs
`npm test -w ts-packages/preview-renderer` (the default `vitest run` tier) and `:225` runs its
`test:integration` tier. So a new `*.test.ts` added to that package's default tier needs **no new
CI step**. Two consequences P2's Task 4 must respect:
- Put the Layer-1 test in the **default** tier (a plain `src/**/*.test.ts`), not in
  `test:integration` (which requires the WASM build).
- `typecheck` / `typecheck:tests` appear **nowhere** in `.github/workflows/` (grepped). Anything
  that is only a `tsc` type-level assertion is **outside CI** — see Finding 4.

**End-to-end verification (CLAUDE.md).** P2 ships no user-visible behavior of its own: the wire
format already ships, and the schema artifact plus its three conformance tests are internal
contracts. The user-visible consumer of this work is P5/P7's Pandoc leg, which does not exist
yet. Task 6 therefore records the honest status — "in-process tests green; the real Pandoc render
path does not exist until P4/P5/P7" — rather than claiming an end-to-end pass. The one real
binary-level check P2 *can* make is that `q2 preview`'s serialized AST still carries the schema's
`data-custom-data` for a live document (Task 6, T6.2).

---

## Task 1: Author the canonical schema artifact + its Rust loader and shape gate

**Scope.** Create `crates/quarto-pandoc-types/resources/custom-node-schema.json` — the single
source of truth `{version, types: {type_name → {route, slots, plain_data}}}` — populated for all
**8 real types**, with each type's `plain_data` field set **read as observed at the wire-format
cut** (i.e. after `crossref-index`/`crossref-resolve`), not at the construction site. Add a
`pub mod custom_node_schema` to `quarto-pandoc-types` that `include_str!`s the artifact and
deserializes it into typed structs, plus unit tests that gate the artifact's *shape* and its
*per-type route assignment*.

This merges P2's checklist items 1 ("record complete `plain_data`/slot field lists for all 8 real
types") and 4 ("author the canonical schema doc") — the inventory is the artifact's content, and
splitting them would produce one task with no checkable deliverable.

**Files.**
- NEW `crates/quarto-pandoc-types/resources/custom-node-schema.json` (the directory does not
  exist yet — `crates/quarto-pandoc-types/` currently holds only `Cargo.toml`,
  `proptest-regressions/`, `src/`, and there is **zero** existing `include_str!` in that crate).
- NEW `crates/quarto-pandoc-types/src/custom_node_schema.rs` (loader + `mod tests`).
- `crates/quarto-pandoc-types/src/lib.rs` — add the module and re-export, next to the existing
  `pub use atomic_custom_nodes::{ATOMIC_CUSTOM_NODES, is_atomic_custom_node};` at `lib.rs:27`.

**Source of truth for the field sets** (all verified 2026-09-18):

| `type_name` | Construction site | `plain_data` at construction | `+ order`? | Slots |
|---|---|---|---|---|
| `Callout` | `crates/quarto-core/src/transforms/callout.rs:299` | `type, appearance, collapse, collapse_starts_collapsed, icon` (`:277-283`) + `ref_type, kind, identifier` under the crossref-eligible-id guard (`:288-296`) | yes, iff eligible | `title` (Inlines), `content` (Blocks) — `:303-306` |
| `Tabset` | `transforms/panel_tabset.rs:285` | `level, tab_count, actives` (`:275-278`) + `group` iff the Div has a `group=` attr (`:272,:279-283`) | no | `title-{i}` (Inlines) / `content-{i}` (Blocks), `i` in `0..tab_count` — `:288-289` |
| `FloatRefTarget` | `transforms/float_ref_target.rs:346` and `:377` (two sites: Div-shaped and Figure-shaped) | `ref_type, kind, identifier` (`:347-351`, `:378-382`) | yes | `content` (Blocks) always; `caption_long` (Blocks) iff non-empty; `caption_short` (Inlines) iff present — `:352-362` |
| `Theorem` | `transforms/theorem.rs:292` | `ref_type, kind, identifier` (`:293-297`) | yes | `content` (Blocks) always; `title` (Inlines) iff non-empty — `:298-304` |
| `Proof` | `transforms/proof.rs:147` | `kind: "Proof"` only (`:150-152`); **deliberately no `ref_type`** (`:148-149`) | **no** — `crossref_index.rs:256-259` early-returns on the missing `ref_type` | `content` (Blocks) always; `title` (Inlines) iff non-empty — `:153-159` |
| `Equation` | `transforms/equation_label.rs:215` | `ref_type: "eq", kind: "Equation", identifier` (`:218-222`) | **yes** — see Finding 2 | `content` (Inlines, holding the `Math`) — `:225-226` |
| `ExampleEmbed` | `transforms/example_embed.rs:228` | **all conditional**: `file` iff the file validates (`:188-190`), `height`, `title` iff non-empty (`:191-208`), + `ref_type, kind, identifier` iff `valid_file && is_demo_id` (`:212-216`) | **yes**, under the same condition — see Finding 2 | `snippet` (Blocks) iff the body starts with a CodeBlock; `body` (Blocks) always — `:236-240` |
| `CrossrefResolvedRef` | `transforms/crossref_resolve.rs:296` | `identifier, ref_type, kind, resolved, kind_source` — all **unconditional** (`:297-313`; note `resolved` and `kind_source` are not enumerated anywhere in P2's prose) | yes, iff an index entry exists (`:314-319`) | `suffix` (Inlines) iff the original `Cite`'s suffix is non-empty (`:327-332`) |

`order`'s shape is `{"section": [int], "order": int}`, written by
`crates/quarto-core/src/transforms/crossref_index.rs:287-295` (P2 cites `:283-295`; `283` is the
comment) and by `crossref_resolve.rs:315-318` (P2 cites `:316-317`).

`slots` values are one of `Block | Blocks | Inline | Inlines` — the same four tags the wire
envelope's own `data-custom-slots` carries (`crates/pampa/src/writers/json.rs:3694-3699`,
mirrored in TS at `ts-packages/preview-renderer/src/framework/customNode.ts:44`).

**Acceptance criterion.**
1. `crates/quarto-pandoc-types/resources/custom-node-schema.json` exists, is valid JSON, has a
   top-level `"version": 1`, and has exactly 8 keys under `"types"`: the eight names in the table
   above.
2. Every type entry has `route`, `slots`, `plain_data`. Every `slots` value is one of the four
   tags. Every `plain_data` entry has a boolean `required`; entries with `required: false` carry
   a `"when"` string; the `order` entries additionally carry
   `"producer": "crossref-index (post-construction)"` (or `"crossref-resolve
   (post-construction)"` for `CrossrefResolvedRef`) and the `"shape"` string.
3. `cargo nextest run -p quarto-pandoc-types` green; `cargo clippy -p quarto-pandoc-types
   --all-targets -- -D warnings` clean.
4. The per-type `route` map matches design §3's frozen table exactly: `Callout=R`, `Tabset=R`,
   `FloatRefTarget=R`, `Theorem=R`, `Proof=R`, `Equation=N`, `CrossrefResolvedRef=N`.
5. **`ExampleEmbed` is deliberately absent from the artifact — RESOLVED 2026-09-18 with Gordon.**
   The artifact declares **7** types, not 8: `Callout`, `Tabset`, `FloatRefTarget`, `Theorem`,
   `Proof`, `Equation`, `CrossrefResolvedRef`. `ExampleEmbed` is excluded because design §3
   deliberately gives it no route — it "is not a Route N case in practice — there is no
   `CustomNode("ExampleEmbed")` left for the shim to route", since `example-embed-render` resolves
   it entirely in Rust upstream of the wire-format cut (reclassified B4 → format-parameterized B1,
   P1 Task 4). Every available `route` value would have been wrong (`N` contradicts §3's explicit
   sentence; `L`/`R` contradict it harder), and inventing a fourth value or making `route` optional
   would weaken the shape gate for the seven types that *do* have one. **The artifact must carry a
   prose note saying so** — a top-level `"$comment"` naming `ExampleEmbed`, the §3 sentence, and
   `example_embed.rs:274` as the transform that destroys it — so the omission reads as deliberate
   rather than as an oversight to be "fixed" by a later contributor.

**Prerequisite.** None. The artifact's field sets are derived by reading the Rust above; nothing
from a later task or another plan is required. (Authoring `Tabset`'s entry has **nothing to
cross-check it against on the TS side** — it does not reach q2-preview as a live CustomNode; that
is P2's own finding, and the compensating check is Task 2's T2.4, which observes it in the Rust
harness. `ExampleEmbed` had the same problem and is now simply out of the artifact — see
acceptance item 5.)

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T1.1 | U | `quarto_pandoc_types::custom_node_schema::load()` + the committed artifact | call `load()` → assert `Ok`, `version == 1`, and that the `types` key set **equals** the **7**-name literal set (two-way, not `contains`) — and, as the negative half, that it does **not** contain `"ExampleEmbed"` | none — `include_str!` is compile-time; no fs, no network | the `"Tabset"` object in `custom-node-schema.json` |
| T1.2 | U | same loader + artifact | call `load()` → assert the **per-type route map** equals the **7** `(name, route)` pairs from design §3 | none | `"route": "N"` on the artifact's `Equation` entry |
| T1.5 | U | the artifact's deliberate-omission note | assert the top-level `"$comment"` exists and mentions `ExampleEmbed` | none | the `"$comment"` field |
| T1.3 | U | the `Route` / `SlotKind` deserializers in `custom_node_schema.rs` | `serde_json::from_str` two malformed inline literals (`"route": "Q"`; `"slots": {"x": "Chunk"}`) → assert both are `Err` | none | `Route`'s enum `Deserialize` derive |
| T1.4 | U | same loader + artifact | call `load()` → assert `types["Proof"].plain_data` has **no** `order` key, and that every `order` entry elsewhere has `required: false` + a `producer` | none | the absence of an `"order"` entry under `Proof` |

**Revert hunks, stated exactly:**
- **T1.1** — Revert ⟨delete the `"Tabset": { … }` object from
  `crates/quarto-pandoc-types/resources/custom-node-schema.json`⟩ → ⟨T1.1's
  `assert_eq!(schema.types.keys().collect::<BTreeSet<_>>(), EXPECTED_EIGHT)`⟩ RED.
- **T1.2** — Revert ⟨change `"route": "N"` to `"route": "R"` on the artifact's `Equation`
  entry⟩ → ⟨T1.2's per-type route-map `assert_eq!`⟩ RED.
- **T1.3** — Revert ⟨replace `Route`'s `#[derive(Deserialize)]` enum with a plain `String`
  field⟩ → ⟨T1.3's `assert!(from_str::<Schema>(BAD_ROUTE).is_err())`⟩ RED.
- **T1.4** — Revert ⟨add an `"order": { "required": false, … }` entry under the artifact's
  `Proof` object⟩ → ⟨T1.4's `assert!(!proof.plain_data.contains_key("order"))`⟩ RED.

### Refactor-induced vacuity check

- **The Route-N admission fix (`cef8c674c`).** That commit widened the schema's *definition* from
  `route (L/R)` to `route (L/R/N)`. The trap it creates is exactly the one my brief names: a
  validity test written as `assert!(matches!(route, L | R | N))` passes for **every** type
  regardless of which route it actually got — including a `Tabset` mislabelled `L` (the route that
  design §3 step 3 says "silently loses data if wrong") or an `Equation` mislabelled `R` (for
  which no Q1 constructor exists at all). **The membership assertion is therefore reserved for
  T1.3 (shape/gating only, on deliberately-malformed literals) and the discriminator is moved to
  T1.2's per-type equality map.** Verified: flipping `Equation.route` to `"R"` leaves a
  membership assertion green and turns T1.2 red.
- **The schema-artifact worked example (`c56dbdbeb`).** That commit's contribution is the
  artifact's concrete form — JSON, at a named path, with a literal `Callout` entry. The trap: if
  a test's expected value *is* the committed artifact, reverting the **producer** (any
  `plain_data`-writing hunk in `quarto-core`) leaves the artifact untouched and the test green.
  **T1.1–T1.4 are deliberately artifact-only** — they discriminate artifact shape and route
  assignment, and are stated here as *not* binding any producer. Every producer hunk is bound in
  **Task 2**, whose expected value is the schema and whose observed value comes from running the
  real transforms. Naming this split explicitly is the point: Task 1 alone would be a
  self-consistent tautology.
- **`plain_data.order` (`6ec06d95d`).** Task 1's only `order` assertion is the *negative* one
  (T1.4: Proof has none), which is discriminating because it differs across the two states —
  `Proof` with `order` vs. without. The *positive* `order` assertions belong to Task 2 for the
  reason spelled out under that task's vacuity check.

---

## Task 2: Rust wire-cut conformance test — observed `plain_data` vs. the schema

**Scope.** Add one Rust integration test that, for a corpus of qmd fixtures, runs the **real**
transform chain through `crossref-index`/`crossref-resolve`, serializes the result with
`pampa::writers::json::write` (the production streaming writer), walks the output for
`__quarto_custom_node` wrappers, and asserts each wrapper's `data-custom-data` key set equals the
schema's declared set for that type **on that fixture's conditional branch**. This is P2's
checklist item 5, and it is the only thing in P2 that binds the schema to real production code.

**Files.**
- NEW `crates/quarto-core/tests/integration/custom_node_schema_conformance.rs`.
- `crates/quarto-core/tests/integration/main.rs` — add `pub mod custom_node_schema_conformance;`
  keeping the list alphabetized (96 `pub mod` entries today).
- Pattern to copy: `crates/quarto-core/tests/integration/crossref_fixtures.rs:29-129`'s
  `run_crossref(qmd)` helper, which already does parse → `RefTypeRegistry::builtin()` +
  `metadata::read` → `codeblock_shorthand::desugar_blocks` → `CalloutTransform` →
  `ExampleEmbedTransform` → `TheoremSugarTransform` → `ProofSugarTransform` →
  `FloatRefTargetSugarTransform` → `EquationLabelTransform` → `CrossrefIndexTransform` →
  `CrossrefResolveTransform`, against a minimal `ProjectContext`/`DocumentInfo`/`Format::html()`/
  `BinaryDependencies`.

**Two deltas from `run_crossref` this task must make:**
1. **Add `PanelTabsetTransform`** to the chain — `run_crossref` does not run it, so `Tabset` is
   unobservable without it. (It belongs in the Normalization run; `panel-tabset` is B1 per design
   §6 and stays enabled for Pandoc per P1.)
2. **Keep the `ASTContext`** that `crossref_fixtures.rs:39` currently discards as `_ast_ctx` —
   `pampa::writers::json::write(&ast, &ast_context, &mut buf)` needs it.

**Serialize through the production entry point, not the convenience one.** Call
`pampa::writers::json::write` / `write_with_config` (`crates/pampa/src/writers/json.rs:1892` /
`:1881`). Those route to `stream_write_pandoc` (`:1888`) → **`stream_write_custom_block`
(`:3684`)** / **`stream_write_custom_inline` (`:3795`)`**. Do **not** bind this test to
`write_custom_block` (`:1466`): that function is on the `pub(crate) write_pandoc` (`:1778`) path,
reached only by the HTML writer's source-map builder (`writers/html.rs:1909`, `:1998`,
`writers/html_source.rs:464`), and is **not** the path any wire-format consumer uses. See
Finding 1.

**Fixture corpus** (one per type, plus one per conditional branch that the schema's `"when"`
clauses describe):

| Fixture | Exercises |
|---|---|
| `::: {.callout-note}` … | `Callout`, no-crossref branch (5 keys only) |
| `::: {.callout-note #tip-foo}` … | `Callout`, crossref-eligible branch (5 + `ref_type`,`kind`,`identifier`,`order`) |
| `::: {.panel-tabset}` + `## A` / `## B` | `Tabset`, no-`group` branch |
| `::: {.panel-tabset group="lang"}` … | `Tabset`, `group` branch |
| `::: {#fig-alpha}` + `![](x.png)` + caption | `FloatRefTarget` (Div shape, `float_ref_target.rs:346`), `caption_long` present |
| a `#fig-`-id'd `Figure` | `FloatRefTarget` (Figure shape, `:377`) — the second construction site |
| `::: {.theorem #thm-a name="P"}` | `Theorem`, `title` slot present |
| `::: {.theorem #thm-b}` | `Theorem`, `title` slot absent |
| `::: {.proof}` | `Proof` (must have **no** `order`) |
| `$$ … $$ {#eq-a}` | `Equation` (and its `order` — Finding 2) |
| `See @fig-alpha` / `@Fig-alpha` / `[-@fig-alpha]` | `CrossrefResolvedRef` (Task 3 extends these) |
| two `::: {#fig-alpha}` blocks | the duplicate-id early return |

**Acceptance criterion.**
1. `cargo nextest run -p quarto-core -E 'binary(integration) & test(custom_node_schema_conformance)'`
   green; `cargo clippy -p quarto-core --all-targets -- -D warnings` clean.
2. For every fixture, the observed `data-custom-data` key set **equals** (not "is a subset of")
   the schema's key set for that type, filtered to the branch the fixture selects. A key present
   in the schema but absent from the observation, or vice versa, fails with both sets printed.
3. The union of observed `data-custom-type` values across the corpus **equals** the schema's **7**
   type keys, both directions (T2.4).
4. Every observed wrapper carries a `data-custom-data` attribute (T2.3 — the
   "path-was-actually-exercised" assertion).

**Prerequisite.** Task 1 (the artifact and its loader must exist to assert against).
**The harness observes a point *upstream* of the real Pandoc cut**, and this is a deliberate,
documented limitation, not an oversight: the real cut for `Pandoc(fmt)` is after
Finalization-minus-Navigation, where `panel-tabset-resolve` / `callout-resolve` /
`crossref-render` are excluded. For all **seven** schema types nothing between `crossref-resolve`
and the cut writes `plain_data`, so the observation is faithful. **A version of this test that
observes the literal `Pandoc(fmt)` cut is `seam deferred until P1's `Pandoc`-kind exclude-list +
P4's `PandocWriteStage` serialization step`** — name that in the test file's module doc so the next
reader does not mistake the upstream observation for the cut itself.

**The two `ExampleEmbed` fixtures were removed from the corpus 2026-09-18**, as a direct
consequence of Gordon's decision to drop `ExampleEmbed` from the artifact (Task 1 acceptance
item 5). They had to go: T2.4 asserts two-way equality between observed types and schema types, so
a corpus that still produced `ExampleEmbed` against a schema that no longer declares it would be
**RED against correct code**. Removing them also removes the one type for which this harness's
upstream-observation caveat was *materially* untrue rather than merely conservative — the real
Pandoc cut never sees an `ExampleEmbed` node at all, because `example-embed-render`
(`example_embed.rs:274`) has destroyed it by then. The remaining seven are all genuinely present at
the cut. **Cost, stated rather than absorbed:** these two fixtures were the binding for
`example_embed.rs:175-185`'s invalid-`file` degradation (see the Missing-test pass) — that binding
is gone and the path is now `accepted-untested` **in P2**, with its natural home being **P1 Task 4**,
which owns `ExampleEmbedRenderTransform`'s format-parameterization.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T2.1 | I | `CrossrefIndexTransform::index_custom_target` (`crossref_index.rs:250-311`) | run the chain on the `#fig-alpha` / `#thm-a` / `#eq-a` / `#tip-foo` fixtures → `json::write` → parse `data-custom-data` → assert each observed key set **equals** the schema's, i.e. **includes `order`** | `ProjectContext`/`DocumentInfo`/`Format::html()`/`BinaryDependencies` test fixtures (genuine env deps; no engine, no fs, no pandoc) | `crossref_index.rs:287-295` |
| T2.2 | I | `CalloutTransform`'s crossref-eligible guard (`callout.rs:287-296`) | run the chain on both Callout fixtures → assert the plain one's set is **exactly** `{type, appearance, collapse, collapse_starts_collapsed, icon}` and the id'd one's is that + `{ref_type, kind, identifier, order}` | same | `callout.rs:288-296` |
| T2.3 | I | `stream_write_custom_block` / `stream_write_custom_inline` (`json.rs:3684`, `:3795`) | after `json::write`, assert every `__quarto_custom_node` wrapper in the output has a `data-custom-data` kv | same | `json.rs:3710-3715` |
| T2.4 | I | the whole harness + the artifact | assert `observed_type_names == schema.types.keys()`, both directions | same | `PanelTabsetTransform` in the harness chain |
| T2.5 | I | the duplicate-id early return (`crossref_index.rs:262-270`) | two `#fig-alpha` fixtures → assert the **first** wrapper has `order` and the **second** does **not**, and that a duplicate-id diagnostic was collected | same | `crossref_index.rs:262-270` |

**Revert hunks, stated exactly:**
- **T2.1** — Revert ⟨delete the `if let Some(obj) = node.plain_data.as_object_mut() { obj.insert("order".into(), json!({…})) }` block at `crates/quarto-core/src/transforms/crossref_index.rs:287-295`⟩ → ⟨T2.1's observed-equals-schema key-set assertion on the `#fig-alpha`, `#thm-a`, `#eq-a` and `#tip-foo` fixtures⟩ RED (observed set loses `order`).
- **T2.2** — Revert ⟨replace the `if !identifier.is_empty() && let Some(reg) = registry && …` guard at `crates/quarto-core/src/transforms/callout.rs:288-296` with an unconditional three-key insert⟩ → ⟨T2.2's "plain Callout's key set is exactly the five unconditional keys"⟩ RED.
- **T2.3** — Revert ⟨delete the `if !custom.plain_data.is_null() { wrapper_attr_kvs.insert("data-custom-data", …) }` block at `crates/pampa/src/writers/json.rs:3710-3715`⟩ → ⟨T2.3's "every wrapper carries `data-custom-data`"⟩ RED.
- **T2.4** — Revert ⟨remove `PanelTabsetTransform::new().transform(…)` from the harness's chain⟩ → ⟨T2.4's `observed_type_names == schema.types.keys()`⟩ RED (`Tabset` missing from the observed side). And, from the other direction: Revert ⟨delete the `"Tabset"` entry from the artifact⟩ → ⟨the same assertion⟩ RED.
- **T2.5** — Revert ⟨delete the `if self.index.entries.contains_key(&identifier) { … return; }` block at `crates/quarto-core/src/transforms/crossref_index.rs:262-270`⟩ → ⟨T2.5's "the second `#fig-alpha` wrapper has no `order` key"⟩ RED.

### Refactor-induced vacuity check

- **`plain_data.order` (`6ec06d95d`) — the structural trap my brief names, confirmed present in
  this tree.** `crates/pampa/tests/integration/test_raw_json_roundtrip.rs:309-330`
  (`test_raw_json_roundtrip_custom_nodes`) is exactly the vacuous shape: it builds a synthetic
  `CustomNode("Callout")` with `with_data(json!({"type":"warning","appearance":"default"}))` and
  calls `assert_roundtrip_identity`. That assertion — "serialize then deserialize yields an equal
  value" — holds for **any** `plain_data` content, so adding `order` to the schema, removing
  `order` from the producer, or both, cannot move it. **Therefore `order` must never be bound by
  a round-trip assertion.** T2.1's discriminating surface is the *observed key set at the wire
  cut vs. the schema's declared key set* — two independently-sourced sets that differ exactly
  when the producer stops writing `order`. Verified by construction: deleting
  `crossref_index.rs:287-295` changes the observed set and not the schema.
- **The schema-artifact worked example (`c56dbdbeb`).** T2.1–T2.5's expected value is the
  committed artifact, so — taken alone — reverting a producer *and* the artifact together would
  keep them green. That is intended and correct (a same-commit schema+producer change is the
  documented bump policy). The discriminator is that **the producer hunks named above are in
  `quarto-core`/`pampa` and the artifact is in `quarto-pandoc-types`**: each named revert touches
  exactly one side and turns the test red. What this pair of tasks does *not* catch is a
  deliberate both-sides edit; nothing can, and the lockstep-by-review that P2's "hand-mirror +
  cross-consumer test" decision accepts is the answer.
- **Set-equality vs. per-type name mapping — verified against the real Q1 Lua, and P5's warning
  is confirmed.** `customnodes/theorem.lua:70,85` declares `ast_name = "Theorem"` with
  `slots = { "div", "name" }`; `customnodes/proof.lua:37,52` declares `ast_name = "Proof"` with
  `slots = { "div", "name" }` — against Q2's `content`/`title` (`theorem.rs:298-304`,
  `proof.rs:153-159`). `customnodes/panel-tabset.lua:138-143` declares `ast_name = "Tabset"` and
  **no `slots` key at all** (its `constructor(params)` at `:147` takes `params.level`,
  `params.attr`, `params.tabs`). Only `customnodes/callout.lua:39,77` (`{ "title", "content" }`)
  and `customnodes/floatreftarget.lua:79,94` (`{ "content", "caption_long", "caption_short" }`)
  align with Q2. **Consequence for this file: no seam anywhere in P2 may assert slot-name
  *set-equality across consumers*.** It is wrong for 3 of the 5 Route-R types and
  non-discriminating for the 2 it happens to fit. P2's slot assertions are therefore
  *intra-consumer* only (observed Rust slots == schema-declared Rust slots, T2.1's sibling
  assertion); the Q2↔Q1 slot *mapping* is P5's shim contract and is `seam deferred until P5's
  per-type Route-R field map`.
  **Correction (final review, 2026-09-18):** no such sibling assertion exists in the committed
  test file beyond `cite_prefix` (T3.4) — see the module doc note in
  `custom_node_schema_conformance.rs` for the accepted-untested scope.
- **The "wrong thing exercised" sibling trap — live in this tree.** Binding T2.3 to
  `write_custom_block` (`json.rs:1489-1494`) instead of `stream_write_custom_block`
  (`:3710-3715`) would produce a test that passes whether or not the production writer emits the
  envelope, because the production entry point never calls the former. T2.3 is bound to the
  streaming twin for exactly this reason, and the revert line above says so: deleting
  `json.rs:1489-1494` leaves T2.3 **green**, which is the diagnostic that the binding is on the
  right function. See Finding 1 for the parity gap this exposes.

---

## Task 3: Grouped `plain_data` extensions — `Proof.type`, `CrossrefResolvedRef.{cite_mode, label_upper}`, and the `cite_prefix` **slot**

**Scope.** Land the confirmed extension requests as one grouped mechanical task: add
`plain_data.type` to `Proof`'s producer + schema entry, add `cite_mode` + `label_upper` to
`CrossrefResolvedRef`'s producer + schema entry, and add **`cite_prefix` as a `slots` entry, not a
`plain_data` field** (decided 2026-09-18 with Gordon — see Finding 3). Extend Task 2's fixture
corpus so each new field is exercised on a branch that discriminates it.

**Files.**
- `crates/quarto-core/src/transforms/proof.rs:150-152` — add `"type"` to the `json!`.
- `crates/quarto-core/src/transforms/crossref_resolve.rs:297-320` — add `cite_mode` and
  `label_upper` to `build_resolved_ref`'s `data` map. The inputs are available: the function
  already receives `original: &Cite` (`:292`), whose `citations[0].mode` and `.id` carry
  everything needed.
- `crates/quarto-core/src/transforms/crossref_resolve.rs:327-332` — the `cite_prefix` **slot**,
  added immediately beside the existing `suffix` slot, whose line this is:
  `Slot::Inlines(original.citations[0].suffix.clone())`. `cite_prefix` is
  `Slot::Inlines(original.citations[0].prefix.clone())` — `prefix` is `Inlines`
  (`quarto-pandoc-types/src/inline.rs:285`, `pub prefix: Inlines`), which is exactly why it cannot
  be a `plain_data` field: `plain_data` is contractually AST-free
  (`quarto-pandoc-types/src/custom.rs:72-75`, "Plain JSON data that **doesn't contain AST
  elements**"). The code had already solved this for the sibling field; this follows it.
- `crates/quarto-pandoc-types/resources/custom-node-schema.json` — the three entries (two
  `plain_data` fields on `CrossrefResolvedRef`, one `plain_data` field on `Proof`) plus
  `cite_prefix` under `CrossrefResolvedRef`'s **`slots`** map, typed `Inlines`.
- `crates/quarto-core/tests/integration/custom_node_schema_conformance.rs` — new fixtures.
- `ts-packages/preview-renderer/src/q2-preview/custom/CrossrefResolvedRef.tsx:39-46` — the TS
  mirror gains the two optional fields (type-only; see the vacuity note).

**Grounding for the requests** (verified against the pinned Q1 tree at
`/Users/gordon/src/quarto-cli/src/resources/filters`, tag `v1.11.3`):
- **`Proof.type`.** `customnodes/proof.lua:54-61`'s `constructor(tbl)` returns
  `{name, div, identifier, type}` and its own comment says "proofs can be unnumbered and lack an
  identifier; we need to know the type explicitly". The renderer at `proof.lua:81` does
  `proof_types[proof_tbl.type:lower()]` **unconditionally** — a `nil` `type` raises
  `attempt to index a nil value`. Q2's current `plain_data` is `{"kind": "Proof"}` only
  (`proof.rs:150-152`). Confirmed gap.
- **`cite_mode` / `label_upper`.** `crossref/refs.lua:31` computes
  `local upper = not not string.match(cite.id, "^[A-Z]")` and threads it into
  `refPrefix(prefixType, upper)` (`:72`); `:56` branches on
  `cite.mode ~= pandoc.SuppressAuthor`; `:95-96` inserts the `ref-noprefix` class when
  `#cite.prefix > 0 or cite.mode == pandoc.SuppressAuthor`. Q2's `build_resolved_ref`
  (`crossref_resolve.rs:287-334`) records neither the mode nor the label's original case.
  Confirmed gap.
- **`Proof`'s value domain, for honesty about what the test can prove.** Q1's `proof_types`
  (`proof.lua:8-21`) has three keys — `proof`, `remark`, `solution`. Q2's sugar transform handles
  **`.proof` only** (`proof.rs:21`: "Scope: `.proof` only. `.remark` and `.solution` have
  ref-types [elsewhere]"). So `plain_data.type` is single-valued today; see the vacuity check.

**Acceptance criterion.**
1. Task 2's conformance test passes with the three new keys declared in the schema and produced
   by the transforms.
2. `@Fig-alpha` yields `label_upper: true`; `@fig-alpha` yields `label_upper: false`.
3. `[-@fig-alpha]` (SuppressAuthor) yields a `cite_mode` distinguishable from `@fig-alpha`'s.
4. A `::: {.proof}` fixture yields `type: "proof"` and its key set equals the schema's
   `{kind, type}`.
5. `cargo nextest run --workspace` green (this touches `quarto-core` producers that
   `crossref_fixtures.rs`, `crossref_render.rs`'s unit tests, and the preview snapshot tests all
   observe — expect churn there and account for it rather than blanket-accepting).
6. **`cite_prefix` is a `slots` entry, never a `plain_data` key.** The schema's
   `CrossrefResolvedRef.plain_data` must **not** contain `cite_prefix`, and its `slots` must —
   asserted in both directions by T3.4, because the failure mode this decision exists to prevent is
   someone "simplifying" it back into `plain_data`, where it would either serialize an AST into a
   field contractually free of AST or (more likely) get stringified and silently lose markup.

**Prerequisite.** Tasks 1 and 2. **No longer blocked** — Finding 3's shape question was decided
2026-09-18 (a slot, mirroring `suffix`), so T3.4 is bound.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T3.1 | I | `proof.rs::convert_div` (`proof.rs:137-161`) | `::: {.proof}` fixture through Task 2's harness → assert the observed `Proof` key set **equals** `{kind, type}` and `type == "proof"` | as Task 2 | the new `"type"` entry in `proof.rs:150-152` |
| T3.2 | I | `build_resolved_ref` (`crossref_resolve.rs:287-334`) | `@fig-alpha` **and** `@Fig-alpha` fixtures → assert `label_upper` is `false` and `true` respectively | as Task 2 | the new `data.insert("label_upper", …)` line |
| T3.3 | I | same | `@fig-alpha` **and** `[-@fig-alpha]` fixtures → assert `cite_mode` differs between them and equals the `SuppressAuthor` discriminant on the second | as Task 2 | the new `data.insert("cite_mode", …)` line |
| T3.4 | I | `build_resolved_ref`'s slot construction (`crossref_resolve.rs:327-332`) + the schema | fixture `[see @fig-alpha]` (a cite with a real prefix) through Task 2's harness → assert a `cite_prefix` **slot** is present and carries the prefix inlines; assert the schema declares `cite_prefix` under `slots` and **not** under `plain_data` | as Task 2 | the new `Slot::Inlines(original.citations[0].prefix.clone())` line |

**Revert hunks, stated exactly:**
- **T3.1** — Revert ⟨remove the `"type": …` entry from the `json!` at `crates/quarto-core/src/transforms/proof.rs:150-152`⟩ → ⟨T3.1's `assert_eq!(observed_keys, {"kind","type"})`⟩ RED (observed set loses `type` while the schema still declares it required).
- **T3.2** — Revert ⟨delete the `data.insert("label_upper".into(), json!(…))` line added to `build_resolved_ref` in `crates/quarto-core/src/transforms/crossref_resolve.rs`⟩ → ⟨T3.2's `assert_eq!(upper_fixture.label_upper, true)`⟩ RED.
- **T3.3** — Revert ⟨delete the `data.insert("cite_mode".into(), json!(…))` line added to `build_resolved_ref`⟩ → ⟨T3.3's `assert_ne!(normal.cite_mode, suppressed.cite_mode)`⟩ RED.
- **T3.4** — Revert ⟨delete the `cite_prefix` `Slot::Inlines(...)` line added beside `suffix` in
  `crates/quarto-core/src/transforms/crossref_resolve.rs`⟩ → ⟨T3.4's
  `assert!(slots.contains_key("cite_prefix"))`⟩ RED. **The fixture must carry a non-empty prefix**
  (`[see @fig-alpha]`, not a bare `@fig-alpha`) — a bare cite has an empty `prefix`, so the slot
  would be present-but-empty and an assertion on mere presence could not distinguish "the slot was
  built" from "the slot was built from nothing". Assert the prefix *content*, not just the key.
  The schema half of the row (in `slots`, not in `plain_data`) is what guards against the
  simplify-it-back regression named in acceptance item 6.

### Refactor-induced vacuity check

- **`label_upper` is the one field in this task where a single fixture would be vacuous.** A test
  asserting `label_upper == false` on `@fig-alpha` alone passes against a hardcoded `false`, a
  dropped field defaulting to `false`, and the correct derivation — three states it cannot
  distinguish. The `@Fig-alpha` / `@fig-alpha` **pair** is the discriminator, and T3.2 is
  specified as a pair for exactly this reason. Same argument, same shape, for `cite_mode`:
  T3.3 is a pair (`@fig-alpha` vs `[-@fig-alpha]`), not a single SuppressAuthor fixture.
- **`Proof.type` is a collapsed discriminator today, and the spec says so rather than pretending
  otherwise.** Because Q2's sugar handles `.proof` only (`proof.rs:21`), `type` can only ever be
  `"proof"`, so `assert_eq!(type, "proof")` cannot distinguish "derived from the class" from
  "hardcoded" — and hardcoding is *correct* today. **T3.1's discriminator is therefore the
  key-set equality (presence), not the value**; the value assertion is retained only as a shape
  check. The behavior the field actually protects — Q1's `proof.lua:81` `nil:lower()` crash — is
  `seam deferred until P5's Route-R Proof construction + P4's vendored `main.lua` tree` (it needs
  a real `pandoc --lua-filter` run, tier `L`, which P2 does not own). If `.remark`/`.solution`
  ever enter Q2's proof sugar, T3.1 must gain value cases; note that in the test's module doc.
- **The TS mirror edit in `CrossrefResolvedRef.tsx` is a type-only change with no runtime
  effect**, so no vitest assertion can bind it. It is logged as `accepted-untested` in the
  missing-test pass rather than dressed up with a test that would pass either way.

---

## Task 4: TS Layer-1 introspection test + the `CalloutPlainData` drift fix

**Scope.** Add the cross-consumer test on the TS side: load the canonical schema JSON and diff it
against the real `previewRegistry` / `Custom.*` exports, in **both** directions. Fix the
demonstrated `CalloutPlainData` drift. This is P2's checklist item 7.

**Files.**
- NEW `ts-packages/preview-renderer/src/q2-preview/schemaConformance.test.ts` (default vitest
  tier — already CI-gated, see the tier note above).
- `ts-packages/preview-renderer/src/q2-preview/custom/Callout.tsx:71-83` — the drift fix.
- Reads: `registry.ts:39-61` (`previewRegistry`), `custom/index.ts:20-27` (the per-type exports),
  `dispatchers.tsx:668`/`:683` (the `?? registry['__fallback__']` miss path).

**The drift, re-verified and larger than P2 states.** `CalloutPlainData` (`Callout.tsx:71-83`)
declares `type, appearance, collapse, collapse_starts_collapsed, icon, ref_type`. The real
producer also sets **`kind`**, **`identifier`** (`callout.rs:293-295`) **and `order`**
(`crossref_index.rs:287-295`). P2 names two missing fields; there are **three**. For contrast,
`Equation.tsx:36-41`, `FloatRefTarget.tsx:43-48`, `Theorem.tsx:52-57` and
`CrossrefResolvedRef.tsx:39-46` all already declare `order` — Callout is the outlier.

**What the existing TS test does and does not lock.** P2 describes `registry.test.ts` as locking
"the **exhaustive** registered set". It does not: `registry.test.ts:81-95` is a one-directional
`for (const name of [...8 literals]) expect(customExportNames.has(name)).toBe(true)` — a
**subset** assertion. A 9th type added to `custom/index.ts` passes it; a 9th type added to the
*schema* with no TS component passes it too. Supplying the missing direction (schema → registry
set equality) is precisely what T4.1 adds.

**Acceptance criterion.**
1. `npm test -w ts-packages/preview-renderer` green.
2. `cd hub-client && npm run build:all` succeeds (CLAUDE.md: the production `tsc -b && vite
   build` is stricter than `tsc --noEmit` and `vitest`).
3. The new test is in the **default** `vitest run` tier, so no new CI step and no
   `ci-test-suite-unwired` excuse is needed (`ts-test-suite.yml:287` already runs it). State this
   explicitly in the PR body so the reviewer does not go looking for a workflow change.
4. `CalloutPlainData` declares `kind`, `identifier` and `order` (three fields, not two), with
   `order` typed as `{ section?: number[]; order?: number }` to match its four siblings.
5. The schema's **preview-reachable** type set — the 6 types with a `Custom.*` component — is
   asserted as a two-way equality, with **`Tabset` the sole** entry in an inline
   `EXPECTED_UNREACHABLE` list carrying its reason (excluded pre-creation by
   `Q2_PREVIEW_TRANSFORM_EXCLUDED`, `pipeline.rs:1637-1638`). **Updated 2026-09-18:**
   `ExampleEmbed` is no longer an `EXPECTED_UNREACHABLE` entry because it is no longer in the
   schema at all (Task 1 acceptance item 5) — 7 schema types − 1 unreachable = the 6 with
   components, which makes this arithmetic exact rather than coincidental.

6. **The `plain_data` interfaces are derived from exported `as const` key arrays, not hand-written
   — RESOLVED 2026-09-18 with Gordon (option b).** For each of the schema's `plain_data`-bearing
   types, the component module exports a key array and derives its interface from it, e.g.
   ```ts
   export const CALLOUT_PLAIN_DATA_KEYS = [
     'type', 'appearance', 'collapse', 'collapse_starts_collapsed',
     'icon', 'ref_type', 'kind', 'identifier', 'order',
   ] as const;
   type CalloutPlainData = { [K in typeof CALLOUT_PLAIN_DATA_KEYS[number]]?: unknown };
   ```
   so the interface **cannot drift from the array by construction**, and a `vitest` test compares
   the array to the schema at runtime. This is why option (b) was chosen over the type-level
   alternative: it needs **no CI change** (the type-level form would have required wiring
   `typecheck:tests` into `ts-test-suite.yml`, which per CLAUDE.md must land with its
   `cargo xtask verify` counterpart in the same commit), and it converts a compile-time-only
   assertion into one the existing `npm test -w ts-packages/preview-renderer` step actually runs.
   The cost, stated: the interfaces become `{ [K in …]?: unknown }` rather than per-field types, so
   **field-level type checking inside the components is lost** — acceptable here because these
   interfaces describe a wire payload that arrives as `unknown` from JSON anyway, and the real
   guard on its shape is the schema.

**Prerequisite.** Task 1 (the artifact must exist for the test to read). **No longer blocked** —
Finding 4's mechanism question was decided 2026-09-18 (option b, above), so T4.3 is bound.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T4.1 | T | `previewRegistry` (`registry.ts:39-61`) + the `Custom.*` barrel (`custom/index.ts:20-27`) | `readFileSync` the schema JSON (path resolved from `import.meta.url`) → compute `schemaTypes − EXPECTED_UNREACHABLE` → assert that set **equals** `Object.keys(Custom) ∩ schemaTypes`, and that each is a function on `previewRegistry` | the fs read of the repo-relative schema (a genuine environment dep; **test-only** — nothing at runtime may import from `crates/`) | `export { Equation } from './Equation';` in `custom/index.ts:24` |
| T4.2 | T | `CustomBlock`'s registry lookup (`dispatchers.tsx:668`) | render a `CustomBlockNode` with `type_name: "Tabset"` (a real schema type with no registry entry) → assert the `Fallback` component renders and nothing throws | jsdom + `@testing-library/react` (the package's standard test env) | the `?? registry['__fallback__']` in `dispatchers.tsx:668` |
| T4.3 | T | `CALLOUT_PLAIN_DATA_KEYS` (and its siblings) vs. the schema's `plain_data` key sets | `readFileSync` the schema → for each `plain_data`-bearing type, assert the exported key array **equals** the schema's key set for that type, both directions | the same test-only fs read as T4.1 | the `'order'` entry in `CALLOUT_PLAIN_DATA_KEYS` |

**Revert hunks, stated exactly:**
- **T4.1** — Revert ⟨delete `export { Equation } from './Equation';` at `ts-packages/preview-renderer/src/q2-preview/custom/index.ts:24`⟩ → ⟨T4.1's `expect(reachableFromSchema).toEqual(exportedAndInSchema)`⟩ RED. And, from the other direction: Revert ⟨add a 9th type to `custom-node-schema.json` without a `Custom.*` component and without an `EXPECTED_UNREACHABLE` entry⟩ → ⟨the same assertion⟩ RED. (`registry.test.ts:81-95` goes red on the first revert too but **not** on the second — that missing direction is T4.1's whole contribution.)
- **T4.2** — Revert ⟨remove the `?? registry['__fallback__']` from `CustomBlock`'s lookup at `ts-packages/preview-renderer/src/q2-preview/dispatchers.tsx:668`⟩ → ⟨T4.2's `expect(screen.getByTestId('custom-fallback')).toBeInTheDocument()` (or the Fallback's actual marker)⟩ RED.
- **T4.3** — Revert ⟨delete `'order'` from `CALLOUT_PLAIN_DATA_KEYS` in
  `ts-packages/preview-renderer/src/q2-preview/custom/Callout.tsx`⟩ → ⟨T4.3's
  `expect([...CALLOUT_PLAIN_DATA_KEYS].sort()).toEqual([...schemaKeys].sort())`⟩ RED. **`order` is
  the right field to name** rather than `kind` or `identifier`: it is the one of the three missing
  fields that the *epic* turns on (it is what carries Q2's numbers across the cut), and it is the
  field Callout is the sole outlier on — its four siblings already declare it. **Note what this row
  does not do:** it binds the array against the schema, not the *component's use* of the field.
  Deriving the interface from the array is what makes those two the same thing, which is why the
  derivation is an acceptance item and not just a suggestion — hand-writing the interface alongside
  the array would leave T4.3 green while the component ignored `order`.

### Refactor-induced vacuity check

- **Set-equality vs. per-type name mapping, TS edition.** T4.1 is deliberately a *type-name* set
  equality, **not** a slot-name or field-name comparison. A slot-name comparison against the TS
  side would be non-discriminating (the TS components read slots by literal string inside JSX,
  with no runtime-enumerable declaration) and a field-name comparison is blocked by type erasure
  (Finding 4). Naming what T4.1 does *not* check is the point: it closes the registry-coverage
  direction and nothing else.
- **The `CalloutPlainData` fix is invisible to every runtime test.** Adding three optional fields
  to a TS interface changes no emitted JavaScript. There is no assertion, at any tier available
  to P2, whose result differs before and after that edit. It is therefore logged as
  `accepted-untested` in the missing-test pass rather than paired with a test that would pass
  either way — the exact failure mode Check 2 warns about.
- **The `EXPECTED_UNREACHABLE` list is the one place T4.1 can go quietly vacuous.** If a future
  type is added to `EXPECTED_UNREACHABLE` to silence a failure, the set equality still passes
  while coverage has shrunk. Mitigation specified: each entry must carry a code citation in a
  comment, and T4.1 additionally asserts `EXPECTED_UNREACHABLE.length === 2`, so growing the list
  is itself a test failure that forces a deliberate edit.

---

## Task 5: Confirm the wire output's `Meta` carries normalized document metadata (produces for P7)

**Scope.** Design §8's Meta-block contract assigns P2 the *carriage* of normalized document
metadata (title / date / authors) in the wire output's Pandoc `Meta`, and P7 the per-format
`Meta`→template mapping. P2's "Consumes / Produces" section states this as
"confirmation the AST `Meta` block carries normalized doc metadata" — checklist-shaped work that
appears **only** in that seams section and in no checklist item. This task is that confirmation,
as a test.

**Files.**
- `crates/quarto-core/tests/integration/custom_node_schema_conformance.rs` (a second test module
  section, or a sibling file registered the same way — keep it in the same file so the harness is
  shared).
- Producers under test: `crates/quarto-core/src/transforms/metadata_normalize.rs:57`
  (`"metadata-normalize"`), `date_normalize.rs:75` (`"date-normalize"`),
  `authors_normalize.rs:87` (`"authors-normalize"`).
- Serializer: `write_config_value_as_meta` on the streaming path
  (`crates/pampa/src/writers/json.rs`, reached from `stream_write_pandoc:4215`).

**Acceptance criterion.**
1. A fixture with front-matter `title`, `date` and `author` is run through the three normalize
   transforms and serialized with `pampa::writers::json::write`; the resulting top-level `meta`
   object is asserted to contain the normalized `title`, `date` and author entries, with the
   *normalized* values (not the raw front-matter strings).
2. The test's module doc states, in one sentence, what P7 may rely on and what it may not — so
   P7's implementer reads a contract rather than re-deriving it.
3. `cargo nextest run -p quarto-core` green.

**Prerequisite.** None beyond Task 2's harness. **This task does not touch the schema artifact** —
`Meta` is not a CustomNode and has no entry in `custom-node-schema.json`; conflating the two
would be a scope error.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T5.1 | I | `MetadataNormalizeTransform` + `DateNormalizeTransform` + `AuthorsNormalizeTransform` and `write_config_value_as_meta` | parse a fixture with `title`/`date: 2026-01-02`/`author` front matter → run the three transforms → `json::write` → assert the serialized `meta` carries all three, with the normalized date shape | `ProjectContext`/`DocumentInfo`/`Format::html()`/`BinaryDependencies` fixtures | `date_normalize.rs`'s normalization write-back |

**Revert hunks, stated exactly:**
- **T5.1** — Revert ⟨the `meta` write-back in `crates/quarto-core/src/transforms/date_normalize.rs` (the hunk where the normalized date replaces the raw front-matter value)⟩ → ⟨T5.1's `assert_eq!(meta_json["date"], <normalized shape>)`⟩ RED (the raw front-matter string survives instead).

### Refactor-induced vacuity check

- **The trap here is asserting *presence* only.** `title` survives to `meta` with or without
  `metadata-normalize` running at all, so `assert!(meta.contains_key("title"))` is vacuous for
  the normalization behavior. T5.1's discriminator is the **normalized date value** — the one of
  the three whose serialized shape differs between "normalize ran" and "normalize did not run".
  If the implementer finds that `title`/`author` also change shape under normalization, add those
  as value assertions too; presence alone is not accepted for any of the three.

---

## Task 6: Round-trip + preview-parity gate; honest status record

**Scope.** P2's final checklist item ("Round-trip + preview-parity gate green") and the epic's
review criterion for P2 ("Reviewed against round-trip + preview parity"). Run the full gate,
account for its delta, and record the end-to-end status honestly — including what could *not* be
verified end-to-end because the Pandoc leg does not exist yet.

**Files.** No new source files. Touches, at most, existing q2-preview snapshot artifacts if
Task 3's new `plain_data` keys move them (they will: `cite_mode`/`label_upper` land in every
`CrossrefResolvedRef`'s `data-custom-data`).

**Acceptance criterion.**
1. `cargo nextest run --workspace` green, with its pass/skip counts reported as a **delta against
   the live baseline measured on this branch at the start of P2**, not a figure copied from an
   older document. Account for every new test and every moved snapshot.
2. `cargo xtask verify` (full, not `--skip-hub-build`) green — `quarto-pandoc-types` and
   `quarto-core` both changed, and both are in `wasm-quarto-hub-client`'s dependency closure.
3. If any `.snap` files changed: report the count, summarize what changed, and list the affected
   files (CLAUDE.md's snapshot-change rule). `cite_mode`/`label_upper` appearing in
   `data-custom-data` is the expected cause; anything else is flagged for review.
4. **Status recorded honestly**: "in-process Rust + TS conformance tests green; `q2 preview`
   spot-checked (T6.2); the real Pandoc render path does not exist until P4/P5/P7, so no
   docx/pptx end-to-end verification was possible." Per CLAUDE.md, "tests pass, I did not verify
   the real render path" is a valid status; claiming otherwise is not.

**Prerequisite.** Tasks 1–5. **Genuine end-to-end verification through a Pandoc target is
`seam deferred until P4's vendored `main.lua` tree + P7's per-format invocation builder`** — there
is no binary today that consumes the schema for a Pandoc format.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T6.1 | I | the whole workspace suite | `cargo nextest run --workspace` → assert green, delta accounted | none | any producer hunk from Tasks 2/3 |
| T6.2 | manual (binary-level) | `q2 preview` on a fixture with a callout, a figure, a theorem and an `@fig-` ref | run `cargo run --bin q2 -- preview <fixture>.qmd`, capture the served AST JSON, grep for `data-custom-data` and confirm the schema's keys (incl. `order`) are present in the live payload; inspect the rendered preview | the browser session (a genuine environment dep) | `json.rs:3710-3715` |

**Revert hunks, stated exactly:**
- **T6.1** — Revert ⟨any producer hunk named in Tasks 2 and 3 (e.g. `crossref_index.rs:287-295`)⟩ → ⟨the workspace run⟩ RED via the Task-2/3 assertions it contains. T6.1 adds no new discriminator of its own; it is the aggregation gate.
- **T6.2** — Revert ⟨delete the `data-custom-data` insert at `crates/pampa/src/writers/json.rs:3710-3715`⟩ → ⟨the grep for `data-custom-data` in the live `q2 preview` payload finds nothing⟩ RED. This is the only P2 check that runs through a real binary. **Note the rebuild chain**: per CLAUDE.md, a plain `cargo build --bin q2` will *not* pick up `quarto-core`/`quarto-pandoc-types` changes in the preview iframe — run `cd hub-client && npm run build:wasm`, then `cargo xtask build-q2-preview-spa`, then `cargo build --bin q2`, or T6.2 will silently verify pre-change code.

### Refactor-induced vacuity check

- **T6.1 is an aggregation gate, not a discriminator, and is labelled as such.** Its only
  independent value is the pass/skip-count delta, which catches a stray or duplicated test — the
  reason CLAUDE.md requires the workspace run at each phase boundary. Treating a green
  `--workspace` as evidence for any specific behavior in this plan would be exactly the
  "revert cannot find an absent test" error; the per-behavior bindings are all in Tasks 1–5.
- **T6.2's trap is the stale-WASM one, and it has bitten this repo before** (CLAUDE.md,
  2026-05-20). A T6.2 run against a stale embedded SPA would pass whether or not the producer
  change landed — the "path was actually exercised" evidence is the rebuild chain above plus a
  positive grep for one of Task 3's *new* keys (`label_upper`), which cannot appear in a
  pre-change payload.


## Task 7: Emit `meta.quarto_pandoc_reader_opts` (an empty `MetaMap`) — the Meta-contract key the vendored Lua indexes unconditionally

**Scope.** Added 2026-09-18: **Gordon assigned P4's Findings item 3 to P2.** Emit a
`quarto_pandoc_reader_opts` key, as an **empty `MetaMap`**, into the wire format's Pandoc `Meta`.
Without it the vendored Q1 Lua aborts before producing any document, so every `L`-tier test in
P4–P7 depends on this key existing.

**Why this is P2's and not P4's.** Design doc §8's Meta-block contract is explicit: *"the wire
output's Pandoc `Meta` carries normalized document metadata … **P2 owns the carriage**, P7 owns
per-format Meta→template mapping."* This key is carriage — it is a `Meta` entry the wire format
must contain — not invocation wiring. P4's companion had specified the seam (its Missing-test pass
item 12) but assigned it to neither plan; it is now P2's, and P4's finding points here.

**The failure it prevents, reproduced rather than reasoned** (P4's companion, verified against
`quarto-cli` tag `v1.11.3` with `pandoc 3.8.1`): `normalize/capturereaderstate.lua:9` calls
`readqmd.meta_to_options(meta.quarto_pandoc_reader_opts)` **unconditionally**, and
`readqmd.lua:280-286` indexes its argument — so a missing key yields
`readqmd.lua:283: attempt to index a nil value (local 'meta')` and **pandoc exit 83**, with no
document produced. An **empty `MetaMap` suffices**: every `reader_option_keys` entry then reads nil
and `pandoc.ReaderOptions({})` takes pandoc's defaults, which is the correct behaviour for a
document Q2 has already parsed.

**Why `active-filters` cannot substitute** (checked, because it is the obvious escape hatch):
`active-filters.normalization = false` gates only the single `normalize` filter-list entry
(`main.lua:263-273`), while `normalize-capture-reader-state` is a **separate, ungated sibling** at
`main.lua:275-278`. There is no params-level lever that switches this off.

**Files.**
- `crates/pampa/src/writers/json.rs` — the `meta` emission on the **streaming** path
  (`stream_write_pandoc`, the `w.key("meta")` site at `:4238`, reached via `write`/`write_with_config`).
  **Not** the legacy `write_pandoc(...) -> Value` twin — see the note below.
- `crates/quarto-pandoc-types/resources/custom-node-schema.json` — if the schema documents
  top-level `meta` keys at all, add this one with a comment naming it a Lua-compatibility key
  rather than document metadata. If it does not, add nothing and say so in the test's module doc.

**Acceptance criterion.**
1. `pampa::writers::json::write` output contains a top-level `meta.quarto_pandoc_reader_opts` whose
   value is an empty `MetaMap` (Pandoc JSON: `{"t":"MetaMap","c":{}}`), for **every** document,
   including one with no front matter at all (T7.1/T7.2).
2. The key is emitted on the **streaming** path specifically — the one `write`/`write_with_config`
   actually take (T7.1 asserts through the public `write`, not through the legacy twin).
3. The test's module doc states that this key exists **for the vendored Q1 Lua's benefit, not as
   document metadata**, so a future reader does not "clean up" an apparently-empty map.
4. `cargo nextest run -p pampa` green; `cargo clippy -p pampa --all-targets -- -D warnings` clean.

**Prerequisite.** None. This is a producer-side emission with no dependency on P4's vendored tree —
which is the point of moving it to P2: **P4's transport smoke depends on this task**, so it must be
landable before P4 runs.

### Test Seam Spec

| # | Tier | Real unit exercised | Seam (invoked → asserted) | Mock boundary | Named revert hunk |
|---|---|---|---|---|---|
| T7.1 | U (`pampa`) | the streaming `meta` emission in `stream_write_pandoc` | call the public `pampa::writers::json::write` on a document **with** front matter → parse the JSON → assert `meta.quarto_pandoc_reader_opts == {"t":"MetaMap","c":{}}` | none | the new `quarto_pandoc_reader_opts` emission at the `w.key("meta")` site |
| T7.2 | U (`pampa`) | the same, on the no-front-matter path | call `write` on a document with **no** front matter → assert the key is still present and still an empty `MetaMap` | none | an `if !meta.is_empty()` guard wrapped around the emission |
| T7.3 | **L** | the real vendored Lua's tolerance of the emitted shape | `seam deferred until P4 Task 1/2's materialized vendored tree` — then: run `main.lua` over a wire document produced by `write` → assert pandoc **exits 0** rather than 83 | nothing mocked | the same emission |

**Revert hunks, stated exactly:**

- **T7.1** — Revert ⟨the `quarto_pandoc_reader_opts` emission⟩ → ⟨`assert_eq!(meta["quarto_pandoc_reader_opts"], json!({"t":"MetaMap","c":{}}))`⟩ RED.
- **T7.2** — Revert ⟨add an `if !meta.is_empty()` guard around the emission⟩ → ⟨T7.2's no-front-matter assertion⟩ RED, while **T7.1 stays green**. This is the row that matters: the natural implementation puts the key inside whatever block already emits document metadata, and that block is plausibly skipped for a document with no front matter — which is exactly the document most likely to appear in a minimal smoke test. T7.1 alone cannot see it.
- **T7.3** — deferred; no hunk may be named until P4's tree exists. Recorded rather than dropped because it is the only row that verifies the *shape* is what the Lua accepts, as opposed to what this plan believes it accepts.

### Refactor-induced vacuity check

- **An empty map is the most vacuity-prone expected value in this file.** `{}` is what you get from
  a missing key mis-parsed, from a serializer that emits an empty object for `None`, and from a
  test that reads the wrong path and finds nothing. So T7.1 asserts the **Pandoc-typed** form
  (`{"t":"MetaMap","c":{}}`), not `{}` — the `"t"` tag is what distinguishes "an empty `MetaMap` was
  deliberately emitted" from "nothing was emitted and the assertion is reading air".
- **Do not assert via a `serde_json::Value::get(...).is_some()`.** `is_some()` on a key whose value
  is an empty object is true for both the correct emission and a `null` emission, and `null` is
  precisely what a `Option<MetaMap>` serializer would produce — which the Lua would still index
  into and still crash on.
- **The legacy/streaming writer split is a live trap here**, the same one P4's Findings item 8 hit
  with `pandoc-api-version`: `json.rs` has two `meta`-emitting paths and only the streaming one is
  reached by `write`/`write_with_config`. A test bound to the legacy twin passes while the real
  output lacks the key. T7.1 goes through the **public** `write` for this reason.

---

## Missing-test pass

Behavior with no revert hunk to find, reasoned across P2's whole surface. Each item gets either a
bound seam or an explicit `accepted-untested: <rationale>`.

### 1. Schema-version mismatch handling, per consumer

**Rust.** `accepted-untested: nothing reads the version at runtime, by decision.** P2's
"Version placement/policy" bullet is explicit: "reject on mismatch is **deferred past v1**… the
field exists so a *future* out-of-band consumer has something to check, **not because anything
checks it yet**." A test asserting rejection behavior would be asserting behavior the plan
deliberately does not build. The one thing that *is* bound is that the field exists and is `1`
(T1.1) — which is all the decision supports.

**TS.** `accepted-untested: same rationale; the TS consumer reads the artifact only from a test
(T4.1), and a test reading a mismatched version is not a product behavior.`

**Lua.** `seam deferred until P5's Layer-1 Lua probe` — P5 specifies an unrecognized-`type_name`
unwrap-and-warn on the Lua side; whether that path also checks `version` is P5's call, not P2's.
**Bound on the TS side today**, though, is the structurally analogous *unrecognized-type_name*
behavior: **T4.2** asserts a schema type with no registry entry falls through to `__fallback__`
(`dispatchers.tsx:668`) rather than throwing. That is the closest existing analogue to P5's Lua
unwrap-and-warn, and it is the answer to "is the Rust/TS side's behavior bound": **TS yes
(T4.2), Rust no** — Rust has no consumer of the artifact at all outside its own tests, so there
is no Rust-side unrecognized-type path to bind. `accepted-untested (Rust side): no Rust consumer
of the schema exists outside the tests; the Rust *producer* cannot encounter an unrecognized
type_name because it writes them.`

### 2. Hand-mirror drift — what mechanically catches a stale mirror, and is two consumers enough?

**Bound, in the direction P2 owns.** Two of the three mirrors get a mechanical check:
Rust-producer ↔ schema via **T2.1/T2.2/T2.4**, and TS-registry ↔ schema via **T4.1**. The Lua
mirror is `seam deferred until P5's Lua contract test`.

**Two real drift surfaces remain unbound, and both are named rather than assumed covered:**
- **The TS *field-level* mirror is unbound** (the interfaces), blocked on Finding 4. Today that
  is a two-consumer test at the type-name level and a **one-consumer** test at the field level.
  `accepted-untested pending Finding 4: the TS per-type field mirror has no runtime
  representation to diff.`
- **A fourth, unnamed mirror exists inside Rust itself.** `crates/pampa/src/writers/json.rs`
  contains **four** near-duplicate implementations of the custom-node envelope —
  `write_custom_block:1466`, `write_custom_inline:1564`, `stream_write_custom_block:3684`,
  `stream_write_custom_inline:3795` — with structurally identical slot-meta, `data-custom-type`,
  `data-custom-slots` and `data-custom-data` logic, and **no test asserting they agree** (grepped;
  none). P2's framing counts three consumers and one producer; there are four producers.
  **Bound seam specified** (this is a real gap with a cheap binding, so it gets a seam rather
  than an acceptance): a tier-**I** test in `pampa` —
  `crates/pampa/tests/integration/custom_node_writer_parity.rs`, registered in that crate's
  `tests/integration/main.rs` — that builds one block `CustomNode` and one inline `CustomNode`
  with non-null `plain_data` and all four slot kinds, serializes via the streaming path
  (`json::write`) and via the non-streaming path (`blocks_to_source_free_json`, `json.rs:1918`,
  which routes through `write_blocks` → `write_custom_block`), and asserts the two wrapper
  attribute maps are equal after source-key stripping.
  **Revert:** ⟨delete the `if !custom.plain_data.is_null() { … "data-custom-data" … }` block at
  `crates/pampa/src/writers/json.rs:1489-1494` (the *non-streaming* twin)⟩ → ⟨the parity
  assertion⟩ RED. Note this is the one revert that T2.3 deliberately leaves green, which is
  exactly why this seam is needed. **Whether to add a checklist item for it is Finding 1.**

### 3. The 8-type inventory's totality — what catches a 9th type shipping later?

**Partially bound; the durable mechanism is not, and that is Finding 7.**

- **Bound within the fixture corpus:** **T2.4** asserts `observed_type_names ==
  schema.types.keys()`, both directions, across Task 2's corpus. A 9th type that any corpus
  fixture produces turns T2.4 red.
- **Not bound:** a 9th type introduced by a *new* transform with *no* fixture in Task 2's corpus
  is invisible to T2.4 — the observed set simply never contains it. T2.4 is a corpus-completeness
  check, not a codebase-completeness check.
- **P5's warning is confirmed accurate.** P5 asks for a bidirectional-totality assertion and
  warns the golden harness would not catch this, because an unhandled type's unwrap-and-drop path
  preserves slot content → zero-byte snapshot diff. Verified: the analogous TS behavior is
  `dispatchers.tsx:668`'s `?? registry['__fallback__']`, which renders rather than fails; and
  `registry.test.ts:81-95` is a hardcoded one-directional subset assertion, so it does not catch
  a 9th type either. **Nothing in the tree today catches a 9th wire type.**
- **The mechanism that would**, matching this repo's three existing precedents for exactly this
  shape of reconciliation (`error-docs-page-missing`, `error-docs-sidebar-unlisted`,
  `ci-test-suite-unwired`): a repo-level `cargo xtask lint` rule that greps non-test
  `CustomNode::new(` call sites across `crates/`, extracts each `type_name` literal or constant,
  and reconciles that set against `custom-node-schema.json`'s keys, anchoring violations at the
  offending `CustomNode::new(` line. Two exclusions the rule must encode, both verified:
  `crossref_render.rs:240`'s `"_placeholder"` transient (a `std::mem::replace` ownership-swap
  sentinel, never serialized) and the fact that `FloatRefTarget` has **two** call sites
  (`float_ref_target.rs:346` and `:377`), so the literal grep yields **10** non-test hits for
  **8** types — not the 9 P2's caveat predicts.
  **Revert (if the rule lands):** ⟨add a `CustomNode::new("Sidenote", …)` call site in
  `crates/quarto-core/src/transforms/` without a schema entry⟩ → ⟨`cargo xtask lint`'s
  `custom-node-schema-unlisted` check⟩ RED.
  **P2 has no checklist item for this and P5 depends on it — see Finding 7. Not decided here.**

### 4. `plain_data` fields that exist for exactly one consumer

- **`Tabset.group`** — set at `panel_tabset.rs:272,279-283` from a `group=` Div attribute, consumed
  only by `TabsetsJsStage` (`crates/quarto-core/src/stage/stages/tabsets_js.rs:63`) for HTML
  grouped-tab sync. Q1's `panel-tabset.lua` `constructor(params)` (`:147-159`) reads
  `params.level`, `params.attr`, `params.tabs` and nothing else — `group` has no Q1 counterpart.
  P2's own audit calls it "harmless unused data for the Pandoc tail, not a gap."
  **Bound, and the binding is meaningful**: **T2.2**'s sibling Tabset pair (with and without
  `group=`) asserts the observed key set equals the schema's for both branches, so the schema must
  declare `group` as `required: false, when: "group= attribute present"`. The *harmlessness* to
  the Pandoc tail is **not** bound here: `seam deferred until P5's Route-R Tabset construction`
  — only a real `pandoc --lua-filter` run through `panel-tabset.lua`'s constructor can show that
  an extra table key is ignored rather than fatal. Do not assert harmlessness from Rust.
- **`CrossrefResolvedRef.kind_source`** — `crossref_resolve.rs:306-313`, values
  `builtin`/`custom`/`promised`; consumed by Q2's HTML renderer to distinguish "authored but
  undeclared". No Q1 counterpart (Q1 has no concept of a Q2-only ref-type category — design §12's
  last bullet). **Bound for presence** by T2.1's key-set equality (it is unconditional, so it is
  a `required: true` schema field). Its Pandoc-side irrelevance is
  `seam deferred until P5's Route-R CrossrefResolvedRef handling`.
- **`Callout.collapse` / `collapse_starts_collapsed`** — HTML/JS-only accordion signals
  (`callout.rs:261-263`). Q1's `callout.lua` constructor does take `collapse`, so this is not
  single-consumer; no separate entry needed. **Bound** by T2.2's five-key equality.
- **`ExampleEmbed`'s whole entry** — every field is conditional and the node never reaches the
  Pandoc cut (destroyed by `example-embed-render`, `example_embed.rs:274`, which is B1 and runs
  for `Pandoc(fmt)`). **Bound** in Task 2's upstream harness (T2.4 observes it) but **not** at
  the real cut: `accepted-untested at the Pandoc cut: the node provably does not exist there;
  asserting its absence is P1/P5's exclude-list contract, not P2's schema contract.`

### 5. Load-bearing safety branches inventoried, with verdicts

| Branch | Verdict |
|---|---|
| `crossref_index.rs:262-270` duplicate-id early return (skips numbering, emits a diagnostic) | **bound — T2.5** |
| `crossref_index.rs:252-254` empty-identifier early return | `accepted-untested: unreachable from qmd. `has_crossref_plain_data` (`:319-321`) already returns false for an empty `attr.0`, so `index_custom_target` can never be called with one. Dead-defensive.` |
| `crossref_index.rs:256-259` missing-`ref_type` early return | `accepted-untested: unreachable from qmd for the same reason — `has_crossref_plain_data:322-326` requires a string `ref_type`. This is the branch that makes Proof order-free, and *that* consequence **is** bound (T1.4 from the schema side, T3.1's key-set equality from the producer side).` |
| `json.rs:3710` / `:1489` `plain_data.is_null()` guard — omits `data-custom-data` entirely | **bound — T2.3** (streaming) and the new parity seam in item 2 (non-streaming) |
| `json.rs:3708` / `:1487` `serde_json::to_string(...).unwrap_or_else(\|_\| "{}")` fallback | `accepted-untested: unreachable — `slot_meta` is a `Map<String, Value>` of string values, which cannot fail to serialize. Same for the `"null"` fallback at `:3713`/`:1492`.` |
| `example_embed.rs:175-185` invalid-`file` degradation (drops `file`, pushes a diagnostic) | **`accepted-untested` as of 2026-09-18 — the binding was lost, deliberately.** It *was* bound by Task 2's two `ExampleEmbed` fixtures (the numbered/unnumbered key sets differ, so the degradation path changed the observed set). Gordon's Finding-5 decision drops `ExampleEmbed` from the artifact, so those fixtures had to go (a corpus producing a type the schema does not declare is RED against correct code — T2.4 is two-way). Natural home: **P1 Task 4**, which owns `ExampleEmbedRenderTransform`'s format-parameterization and is where the node's Pandoc-leg behaviour is decided. Recorded rather than dropped so the loss is visible. |
| `callout.rs:270-274` `appearance="minimal"` → `("simple", false)` normalization | `accepted-untested by P2: this is value-level normalization, not field-set membership, and P2's contract is the field set. It is already covered by `callout_resolve.rs`'s existing unit tests (`:650-928`).` |
| `dispatchers.tsx:668`/`:683` `?? __fallback__` miss path | **bound — T4.2** (block form). `accepted-untested: the inline form at `:683` is the same one-line expression; a second test would assert the same hunk.` |
| Task 1's `include_str!` on a missing artifact | `accepted-untested: a missing file is a compile error, not a runtime branch — the strongest possible gate, and no test can be written for it.` |

### 6. Structural contracts a successor plan depends on

- **P4 needs the schema to exist as a loadable JSON artifact before it builds `PandocWriteStage`'s
  serialization step.** **Bound — T1.1** (the artifact loads and has all **7** types; count
  corrected 2026-09-18 with Finding 5's resolution).
- **P5 needs the per-type `route` assignment frozen.** **Bound — T1.2** (per-type route map
  equality, not membership).
- **P5 needs `plain_data.order` to be present at the cut for the numbering injection the epic
  turns on.** **Bound — T2.1**, and deliberately *not* via a round-trip assertion (see Task 2's
  vacuity check).
- **P7 needs `Meta` carriage confirmed.** **Bound — T5.1.**
- **P7 needs the "code-block decorations travel in the `RenderContext::code_block_decorations`
  sideband, not the wire format" fact.** `accepted-untested: this is the *absence* of a wire type.
  T2.4's two-way set equality is the closest available binding — a `DecoratedCodeBlock` wire node
  appearing would turn it red — and that is sufficient; a dedicated "no such type" assertion would
  restate T2.4.`

---

## Findings for Gordon

Seven items. Each is a grounding correction or a place where a test cannot be written the way the
plan describes. None reopens a frozen design decision; none has been decided or worked around
here.

**1. The design doc and P2 both name the wrong wire-format producer, and there are four of them,
with no parity test.** Design §2 and P2's Goal cite
`pampa/src/writers/json.rs::write_custom_block` as *the* producer. Verified: that function
(`json.rs:1466`) sits on the `pub(crate) write_pandoc` path (`:1778`), which is reached **only**
by the HTML writer's source-map builder (`writers/html.rs:1909`, `:1998`,
`writers/html_source.rs:464`). Every actual wire-format consumer — q2-preview
(`pipeline.rs:999`, `:1039`) and whatever P4 builds — goes through the public
`write`/`write_with_config` (`:1892`/`:1881`) → `stream_write_pandoc` (`:1888`) →
**`stream_write_custom_block` (`:3684`) / `stream_write_custom_inline` (`:3795`)**. So a test
bound to the cited function exercises the wrong thing and would pass whether or not the
production writer emits the schema's envelope. Task 2's T2.3 is bound to the streaming twin
accordingly. Two open questions for you: (a) should the design doc §2 citation be corrected /
widened to name all four functions; (b) the four implementations are structurally identical
duplicated logic with **no** test asserting they agree (grepped — none) — I have specified a
bound parity seam in the missing-test pass (item 2) but P2 has no checklist item for it. Add one,
or accept the duplication risk explicitly?

**2. Six wire types carry `plain_data.order`, not the four P2's round-4 correction names.** The
correction in `6ec06d95d` says to add `order` to `Callout`, `FloatRefTarget`, `Theorem` and
`CrossrefResolvedRef`, and explains that `Proof` "correctly gets none." Two more types get it:
- **`Equation`.** `equation_label.rs:218-222` sets the full crossref triple
  (`ref_type: "eq"`, `kind`, `identifier`); `crossref_index.rs:193-206`'s `Inline::Custom` arm
  calls `index_custom_target` for any inline custom node passing `has_crossref_plain_data`. Three
  independent corroborations: `crossref/mod.rs:83`'s own doc comment for `EQUATION` states "the
  specific numbering is stored in `plain_data.order` (set by the indexer)"; and
  `ts-packages/preview-renderer/src/q2-preview/custom/Equation.tsx:36-41`'s `EquationPlainData`
  **already declares `order`**.
- **`ExampleEmbed`.** `example_embed.rs:212-216` sets the triple when
  `valid_file.is_some() && is_demo_id(&id)`, so a numbered `#demo-` embed passes
  `has_crossref_plain_data` and gets `order` injected — before `example-embed-render` destroys the
  node.
Task 1's field table records all six. Flagging rather than silently fixing, because the
correction's own reasoning ("Proof correctly gets none") reads as an exhaustive enumeration and a
future reader will treat it as one.

**3. `cite_prefix` cannot be a `plain_data` field — the plan's own mechanism doesn't support it.**
P2's confirmed extension request asks to add `cite_prefix` to `CrossrefResolvedRef`'s schema entry
as a `plain_data` field. But a citation prefix is `Inlines` (`quarto-pandoc-types/src/inline.rs:285`,
`pub prefix: Inlines`), and `plain_data` is contractually AST-free —
`quarto-pandoc-types/src/custom.rs:72-75`: "Plain JSON data that **doesn't contain AST
elements**." The code already solved this problem the other way for the sibling field: the
citation's **suffix** is carried as a **slot**, not in `plain_data`
(`crossref_resolve.rs:327-332`, `Slot::Inlines(original.citations[0].suffix.clone())`). So the
implementable shape is almost certainly "a `cite_prefix` **slot**, mirroring `suffix`" — but that
is a different schema surface (`slots`, not `plain_data`), so I have **not** decided it.

**RESOLVED 2026-09-18, decided with Gordon: a `cite_prefix` slot, mirroring `suffix`.** Landed in
Task 3 — the producer line goes beside the existing
`Slot::Inlines(original.citations[0].suffix.clone())` at `crossref_resolve.rs:327-332`, the schema
entry goes under `CrossrefResolvedRef.slots` typed `Inlines`, and **T3.4 is now bound** (the
`seam deferred` marker is gone). Two things added while binding it: the schema assertion is
**two-way** (in `slots`, *not* in `plain_data`), because the failure mode worth guarding is someone
"simplifying" it back into `plain_data` where it would either serialize an AST into a field
contractually free of AST or get stringified and silently lose markup; and the fixture must be
`[see @fig-alpha]` rather than a bare `@fig-alpha`, because a bare cite has an **empty** prefix, so
a presence-only assertion could not distinguish "the slot was built" from "the slot was built from
nothing". `cite_mode` and `label_upper` are plain scalars and were always unaffected.

**4. "Diff the hand-written TS interfaces against the schema" is not implementable as stated.**
P2's checklist item 7 requires the Layer-1 test to diff "the hand-written per-type interfaces
(e.g. `CalloutPlainData`) against the schema." Three verified obstacles: TS interfaces are
type-erased at runtime — `registry.test.ts:18-23` says exactly this about the sibling case
("`framework/types.ts` does not export a runtime-introspectable set — `BlockNode`/`InlineNode`
are TS unions, type-erased at runtime"); `CalloutPlainData` is **module-private**
(`Callout.tsx:71`, no `export`); and `typecheck` / `typecheck:tests` appear **nowhere** in
`.github/workflows/` (grepped), so a compile-time-only assertion would be outside the merge gate.
Two mechanisms would work, and picking between them is a real choice I have left to you:
(a) **type-level**: export the interfaces, import the schema JSON with `resolveJsonModule`, and
assert `keyof CalloutPlainData` equals the schema's literal key union in a file compiled by
`tsconfig.tests.json` — requires wiring `typecheck:tests` into `ts-test-suite.yml` to be
CI-gated (and per CLAUDE.md that wiring must land in the same commit as its `cargo xtask verify`
counterpart); or (b) **runtime**: replace each interface with one derived from an exported
`as const` key array (`type CalloutPlainData = { [K in typeof CALLOUT_KEYS[number]]?: … }`), so a
vitest test can compare the array to the schema at runtime and the interface cannot drift from it
by construction. (b) needs no CI change and gives a non-vacuous `vitest` assertion; (a) is a
smaller diff to the components. T4.3 is deferred pending your call. The `CalloutPlainData` field
*fix* itself (Task 4 acceptance item 4) is unaffected and should land either way — noting that it
is **three** missing fields (`kind`, `identifier`, **`order`**), not the two P2 names.

**RESOLVED 2026-09-18, decided with Gordon: option (b), runtime derivation from an exported
`as const` key array.** Landed as Task 4 acceptance item 6, and **T4.3 is now bound** against the
array-vs-schema comparison. Why (b) over (a), recorded because the tradeoff is real: (b) needs
**no CI change** — the type-level form would have required wiring `typecheck:tests` into
`ts-test-suite.yml`, which per CLAUDE.md must land with its `cargo xtask verify` counterpart in the
same commit — and it converts a compile-time-only assertion into one the existing
`npm test -w ts-packages/preview-renderer` step actually runs. **Its cost, stated plainly:** the
interfaces become `{ [K in …]?: unknown }`, so field-level type checking inside the components is
lost. Acceptable because these interfaces describe a wire payload that arrives as `unknown` from
JSON anyway and the real guard on its shape is the schema — but it *is* a loss, not a free win.
One non-obvious consequence folded into Task 4: **deriving the interface from the array is an
acceptance item, not a suggestion** — hand-writing the interface alongside the array would leave
T4.3 green while the component ignored the field. The three-missing-fields fix lands regardless, as
this finding already said.

**5. `ExampleEmbed` has no `route` value, but the schema's shape makes `route` mandatory.** Design
§3's Route table deliberately omits `ExampleEmbed` and states it "is not a Route N case in
practice — there is no `CustomNode("ExampleEmbed")` left for the shim to route." P2 nonetheless
requires its schema entry ("P2 only needs their schemas authored for the Pandoc tail"). An
implementer authoring that entry has to put *something* in `route` and every available value is
wrong: `N` contradicts design §3's explicit sentence, `L`/`R` contradict it harder, and omitting
the key breaks Task 1's shape gate. Options I can see: a fourth value
(`"resolved-upstream"` / `"none"`), `null` with the field made optional, or dropping
`ExampleEmbed` from the artifact entirely and recording it in prose (which would also shrink
T1.1/T2.4 from 8 types to 7).

**RESOLVED 2026-09-18, decided with Gordon: drop `ExampleEmbed` from the artifact entirely and
document the exclusion in prose.** The artifact now declares **7** types. Applied in four places,
which is more ripple than the original finding anticipated — worth listing because the last one is
a real loss:
1. **Task 1** — acceptance item 5 rewritten from "the `route` cell is blocked" to "the type is
   deliberately absent", with the artifact required to carry a top-level `"$comment"` naming
   `ExampleEmbed`, design §3's "not a Route N case in practice" sentence, and
   `example_embed.rs:274`. **T1.1 asserts the 7-name set and, as its negative half, that
   `"ExampleEmbed"` is absent**; T1.2's route map is 7 pairs; **new T1.5** asserts the `$comment`
   exists, so the note cannot be silently dropped.
2. **Task 2** — the **two `ExampleEmbed` fixtures are removed from the corpus.** They had to go:
   T2.4 asserts two-way equality between observed and schema types, so a corpus still producing
   `ExampleEmbed` against a schema no longer declaring it would be **RED against correct code**.
   Acceptance item 3 now says 7.
3. **Task 4** — `ExampleEmbed` is no longer an `EXPECTED_UNREACHABLE` entry (it is not in the
   schema to be unreachable *from*), leaving `Tabset` as the sole entry. Pleasant side effect: 7
   schema types − 1 unreachable = the 6 with components, so that arithmetic is now exact rather
   than coincidental.
4. **The cost.** Those two fixtures were the binding for `example_embed.rs:175-185`'s
   invalid-`file` degradation (Missing-test pass). That binding is **gone**; the path is now
   `accepted-untested` in P2, and its natural home is **P1 Task 4**, which owns
   `ExampleEmbedRenderTransform`'s format-parameterization. Recorded in the Missing-test pass
   rather than quietly dropped.

**6. `registry.test.ts` is not the exhaustive lock P2 describes it as.** P2's "real TS-side
registry" section says `registry.test.ts` "locks the **exhaustive** registered set." Verified:
`registry.test.ts:81-95` is a one-directional subset assertion
(`for (const name of [...]) expect(customExportNames.has(name)).toBe(true)`). A 9th `Custom.*`
export passes it; a 9th *schema* type with no component passes it. This is only a prose
correction — T4.1 is specified to supply the missing direction — but it matters because the
sentence is load-bearing for the totality question in Finding 7.

**7. Nothing in the tree catches a 9th wire type, P2 has no checklist item for it, and P5 depends
on it.** P5 independently asks for a bidirectional-totality assertion and correctly warns that
the golden harness would not catch it (an unhandled type's unwrap-and-drop path preserves slot
content → zero-byte snapshot diff). Verified: the TS side falls through to `__fallback__`
(`dispatchers.tsx:668`) and renders; `registry.test.ts` is one-directional (Finding 6); Task 2's
T2.4 catches only types that some corpus fixture produces. The mechanism that *would* work is a
repo-level `cargo xtask lint` rule reconciling non-test `CustomNode::new(` call sites against the
schema's keys — the same shape as the three precedents already in `CLAUDE.md`
(`error-docs-page-missing`, `error-docs-sidebar-unlisted`, `ci-test-suite-unwired`). I have
specified it as a bound seam in the missing-test pass (item 3, including the two exclusions it
needs) but have **not** added it as a task, because P2's checklist has no item for it and adding
one is a scope decision. Related grounding nit for whoever re-runs the grep: P2's caveat predicts
**9** literal `CustomNode::new(` non-test hits; the real count is **10**, because
`FloatRefTarget` has two construction sites (`float_ref_target.rs:346` and `:377`) in addition to
the `"_placeholder"` sentinel at `crossref_render.rs:240`.

### Minor citation drift (recorded, not propagated, not silently repaired)

None of these change any decision; they are listed so a future pass does not read them as new
drift. Plan-side: `crossref_index.rs:283-295` → the insert is `:287-295`;
`crossref_index.rs:255` (the "early-returns without a `ref_type`" cite) → `:256-259`;
`crossref_resolve.rs:316-317` → `:315-318`; `callout.rs:276-296` → `:277-296`;
`pipeline.rs:1594-1642` → the array is `:1594-1639`. Source-comment-side (pre-existing, unrelated
to this epic): `custom/index.ts:7` cites `callout.rs:233` → `:299`; `Callout.tsx:77-78` cites
`callout.rs:236-241` → `:288-296`; `FloatRefTarget.tsx:27` cites
`float_ref_target.rs:292-295` → `:347-351`/`:378-382`; `Theorem.tsx:36` cites
`theorem.rs:282` → `:293-297`; `Proof.tsx:31` cites `proof.rs:145` → `:150-152`;
`CrossrefResolvedRef.tsx:33` cites `crossref_resolve.rs:316` → `:297-320`;
`framework/customNode.ts:7-9` cites `write_custom_block:1297` / `write_custom_inline:1381` /
`read_custom_block_from_div:2220` / `read_custom_inline_from_span:2358` → `json.rs:1466` /
`:1564` / `readers/json.rs:2916` / `:3054` (and see Finding 1 — the *function* named there is
also the non-production one); `registry.test.ts:13` cites `pipeline.rs:1985` for the
`Q2_PREVIEW_TRANSFORM_EXCLUDED` validator → `:2839`/`:2864`.
