# P2 — Custom-node wire format: versioned shared schema

**Date:** 2026-08-20  **Updated:** 2026-09-18 (two passes) — see `git log --oneline -- claude-notes/plans/2026-08-20-pandoc-hybrid-P2-wire-schema.md`
for the full correction history. Latest (round 4 review, Reviewer A): the worked `Callout` schema
entry omitted `plain_data.order` — the field the entire epic turns on — because the field-discovery
instruction pointed at the construction site, where `order` genuinely doesn't exist yet (it's
injected later by `crossref-index`/`crossref-resolve`). Corrected the instruction to "as observed
at the wire-format cut" and added `order` to the four types that carry it. Also filed a confirmed
extension request for `CrossrefResolvedRef` (`cite_prefix`/`cite_mode`/`label_upper`, Reviewer D) —
without them P5's "Q1 is normative" Route-N decision can't actually be implemented as stated.
(Prior pass, same day, implementation-feasibility review: the canonical schema artifact — the
highest-divergence-risk single decision in this plan, per that review — was "decided" (hand-mirror
+ cross-consumer test) with no concrete form. Closed: **JSON**, at
`crates/quarto-pandoc-types/resources/custom-node-schema.json`, with a fully-worked `Callout`
entry in the plan text so an implementer copies the literal syntax rather than inventing it.
Also corrected the schema-version bullet's motivation (the "already-deployed Automerge sessions"
skew scenario didn't survive verification — the wire format isn't actually persisted in
`quarto-automerge-schema`) and specified the Rust construction-site test must drive the real
transform with per-conditional-branch fixtures, not a bare `CustomNode` literal.)
**Status:** Shape draft
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P2-implementation.md`](2026-09-18-pandoc-hybrid-P2-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Goal
Promote the existing `__quarto_custom_node` wire format from a private preview contract to a
**versioned, single-sourced shared contract** consumed by three runtimes (Rust producer, TS/React,
Lua/Q1). Today the per-type schema is hand-mirrored (Rust producer ↔ the TS registry) for **6 of
the 8 real types** (see inventory below) — a third hand-mirror in Lua multiplies that drift, and
the remaining 2 types have **no existing TS mirror at all** to promote from.

## The real type inventory (verified against code, not the plan's prior guess)

The plan previously claimed "~12 known types (callout, float, theorem, proof, tabset,
panel-layout, decorated-code-block, example-embed, crossref-resolved-ref, …)". That list was
wrong on both count and membership. Grepping every non-test `CustomNode::new(...)` call site
(`crates/quarto-core/src/transforms/*.rs`, `crates/quarto-core/src/crossref/*.rs`) gives exactly
**8 real types**:

| `type_name` | Constructed at | Constant |
|---|---|---|
| `Callout` | `transforms/callout.rs:299` | (literal) |
| `Tabset` | `transforms/panel_tabset.rs:285` | (literal — **not** `PanelTabset`) |
| `FloatRefTarget` | `transforms/float_ref_target.rs:346,377` | `crossref/mod.rs:60` |
| `Theorem` | `transforms/theorem.rs:292` | `crossref/mod.rs:68` |
| `Proof` | `transforms/proof.rs:147` | `crossref/mod.rs:77` |
| `Equation` | `transforms/equation_label.rs:215` | `crossref/mod.rs:84` |
| `ExampleEmbed` | `transforms/example_embed.rs:228` | `example_embed.rs:92` |
| `CrossrefResolvedRef` | `transforms/crossref_resolve.rs:296` | `crossref/mod.rs:97` |

**Caveat (added 2026-09-17):** a literal grep for `CustomNode::new(` actually turns up a 9th
non-test call site, `crates/quarto-core/src/transforms/crossref_render.rs:240`, constructing a
`"_placeholder"`-typed node inside `take_custom_node`. It's not a real semantic wire type — a
transient `std::mem::replace` ownership-swap sentinel used so rendering can take ownership of a
node without cloning, never serialized to the wire format — so it doesn't belong in the 8-type
inventory above, but a future re-run of this exact grep should expect 9 hits, not 8, and needs
to recognize and exclude this one rather than treat the count mismatch as new drift.

Two names in the old "~12" list are **fictional** — grepped and confirmed absent from the
codebase entirely: `PanelLayout`/`panel-layout` (no hits) and `DecoratedCodeBlock` (no
`CustomNode::new` site; the only hit is a comment at `crates/quarto-core/src/render.rs:364`
documenting that a `DecoratedCodeBlock` CustomNode was **considered and explicitly rejected** —
code-block decorations are carried in a sideband map
(`RenderContext::code_block_decorations`, keyed by `CodeBlockDecorationKey`) specifically "to
avoid the nested-CustomNode complexity Q1 ran into," per
`claude-notes/plans/2026-05-19-code-block-features.md`). **This is a real design decision already
made elsewhere, not an open question** — P2/P7 need to carry it forward as: code-block
decorations reach the Pandoc tail via the sideband map, not the wire format, so whatever bridges
to Q1's `decoratedcodeblock.lua` filter reads that map, not a CustomNode.

## The real TS-side registry (not `framework/customNode.ts`)

The plan previously guessed the TS consumer registry might be named `customNode.ts`. That file
exists (`ts-packages/preview-renderer/src/framework/customNode.ts`) but **it is not a per-type
registry** — it's the generic, type-name-agnostic wire ↔ JS-native codec (unwraps/rewraps the
`__quarto_custom_node` Div/Span wrapper into `CustomBlockNode`/`CustomInlineNode`, with zero
per-type knowledge; that's the "wire format" itself, already correctly described at design note
§2).

The actual **per-type** hand-mirror lives at `ts-packages/preview-renderer/src/q2-preview/`:
- `custom/index.ts` re-exports one component per registered type.
- `registry.ts`'s `previewRegistry` merges those in; `dispatchers.tsx`'s `CustomBlock`/
  `CustomInline` look a node up by `node.type_name`, falling back to `__fallback__`
  (`Fallback.tsx`) on miss.
- `registry.test.ts` locks the **exhaustive** registered set: `Callout`, `Theorem`, `Proof`,
  `FloatRefTarget`, `Equation`, `CrossrefResolvedRef` (plus synthetic `Fallback` /
  `PreviewTitleBlock`).

**Two of the 8 real types — `Tabset` and `ExampleEmbed` — have no registry entry at all today, and
neither actually reaches q2-preview as a live CustomNode — corrected twice now** (the prior pass
of this plan wrongly claimed both hit `Fallback`; a closer look at `ExampleEmbed` shows that
claim was wrong too, for a third, distinct reason):

- **`Tabset`**: excluded pre-creation. `Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1594-1642`)
  excludes **both halves of the tabset pair**, `"panel-tabset"` (the sugar transform itself) and
  `"panel-tabset-resolve"` — a `.panel-tabset` Div is never converted into a `Tabset` CustomNode
  for preview at all; it stays raw and renders as plain stacked headings. Tracked as its own
  follow-up strand per the exclusion comment.
- **`ExampleEmbed`**: created, then destroyed before the cut, by a *different* transform than
  its own sugar. `example-embed-render` (`example_embed.rs:274`, Finalization phase) **is not**
  in `Q2_PREVIEW_TRANSFORM_EXCLUDED`, so it runs unconditionally and replaces
  `Block::Custom(node)` with a plain `Block::Div` containing an `<iframe>` and a caption
  (`example_embed.rs:307-330`) — **before AST serialization, so no `ExampleEmbed` CustomNode
  ever reaches the wire format for preview either.** It renders as a generic Div with a raw
  HTML iframe, not a `Fallback` dashed box (there's nothing left with `type_name: "ExampleEmbed"`
  to dispatch through `Fallback` by the time serialization happens).

Both corrections point at the same root cause: I inferred "not in the registry ⇒ hits Fallback"
without checking whether the CustomNode actually survives to the wire cut in the first place.
Neither does, for two different reasons (exclusion vs. an un-excluded render transform).

This still changes what "promote the existing hand-mirror" means for P2, just not by the
mechanism previously claimed — for **both** `Tabset` and `ExampleEmbed`, there is no existing
mirror *and* no CustomNode reaching q2-preview to test against in the first place. Their schemas
(needed regardless, for the Pandoc tail, where the render transforms that destroy them —
`panel-tabset-resolve`, `example-embed-render` — get excluded per P1's `Pandoc`-kind list) have
nothing in q2-preview to cross-check once authored, for either type.

**Cross-plan resolution:** P1 (2026-09-16 rework) now has a concrete `Pandoc`-kind exclude-list
derived from the design doc's §6 bucket table, which excludes `panel-tabset-resolve`,
`example-embed-render`, `callout-resolve`, and the rest of B2/B4/Navigation, while *keeping*
`panel-tabset`'s sugar half enabled (Pandoc needs the raw `Tabset` CustomNode P5's shim routes,
unlike Preview). See P1 for the full list and reasoning; not re-derived here.

## Corrections owed to the design doc (§3 Route table) — flag only, fix during execution

Per the working pattern established for P1 (design doc is frozen for plan decomposition; factual
bugs found during review are flagged and checklisted, not silently patched mid-review):

- `PanelTabset` → rename to `Tabset` (that's the real `type_name`; verified above).
- Drop the `DecoratedCodeBlock` row entirely — no such CustomNode exists (verified above); the
  decision it was gesturing at (sideband map, not CustomNode) is already made and belongs in
  prose, not the Route table.
- Add a row for `Equation` — missing from the table entirely, and **the design doc's L/R
  dichotomy doesn't cover it: it's Route N, not R.** Confirmed by reading Q1's
  `crossref/equations.lua` directly — it has **no `_quarto.ast.add_handler`, no `ast_name`, no
  custom node at all** for equations; it's a plain Pandoc filter over `Para`/`Plain` blocks that
  detects `DisplayMath` inline content and does format-specific `RawInline` injection (LaTeX
  `\label{}`, Typst `#math.equation(numbering:...)`) directly on the existing `Math` inline —
  there is no constructor to route into. Add `Tabset`'s row too, reclassified from `PanelTabset`/L
  to `Tabset`/**R** (see P5 for the evidence — its wire shape matches Q1's `constructor`, not its
  `parse`).

## Field-map note (minor — scope for the schema inventory, not a design-doc bug)

Design note §3's Callout field-map example (`plain_data.{type,appearance,icon}`) is illustrative,
not exhaustive: the actual `plain_data` (`transforms/callout.rs:276-282`) unconditionally carries
5 fields (`type, appearance, collapse, collapse_starts_collapsed, icon`) plus 3 conditional
crossref fields (`ref_type, kind, identifier`) when the callout has a crossref-eligible id. The
full-inventory task below needs the complete field list per type, not the abbreviated one.

## Decided in this rework (2026-09-16): hand-mirror + cross-consumer test, not full codegen

Scoped as the plan required — "what would a generator need to emit for Rust/TS/Lua" — before
deciding, not as a preference call:

- **Rust has nothing typed to generate from or to.** `CustomNode.plain_data` is
  `serde_json::Value`, built ad hoc via `json!({...})` across 8 files, with conditional logic
  (e.g. Callout only adds `ref_type`/`kind`/`identifier` when the div has a crossref-eligible
  id). A generator can't derive that *conditional business logic* from a type — a human declares
  it either way, so Rust-side codegen buys nothing a test doesn't already buy.
- **`schemars` is a declared workspace dep (`Cargo.toml:49`) with zero actual usage** in
  `crates/` — codegen would mean wiring a new pipeline from scratch, not extending something
  live.
- **`ts-packages/quarto-automerge-schema` is precedent for "one versioned shared package,"** but
  it's pure hand-written TS with no Rust producer — it doesn't establish a Rust→TS codegen
  precedent, only the "single source, version const" shape P2 already wants.
- **The drift this plan worries about already exists, with only 2 consumers, before Lua exists.**
  `Callout.tsx`'s hand-written `CalloutPlainData` interface declares
  `type, appearance, collapse, collapse_starts_collapsed, icon, ref_type` — but the real Rust
  `plain_data` (`callout.rs:276-294`) also sets `kind` and `identifier` when crossref-eligible.
  Neither field is in the TS interface today. **The demonstrated bug is "no test," not "no
  types"** — a generated TS type would not by itself have caught this without a runtime/test-time
  check against real constructed values.

**Decision (confirmed with Gordon):** one canonical hand-authored schema doc (`type_name →
{slots, plain_data fields (incl. which are conditional), route}`), versioned. Rust, TS, and Lua
(P5) each keep their own hand-written representation — no generator, no new build step — but each
is checked by a test that diffs its real, constructed values against the canonical doc:
- **Rust:** a test per type that constructs it (or drives the real transform) and asserts the
  resulting `plain_data` keys match the schema's declared field set exactly (including the
  conditional ones), catching exactly the Callout drift found above.
- **TS:** the Layer-1 introspection test already scoped below, extended to diff the hand-written
  interfaces (e.g. `CalloutPlainData`) against the schema, not just registry coverage.
- **Lua (P5):** a runtime-validated table built from the same schema, checked in P5's contract
  test.

Full codegen (schemars-derived JSON Schema → generated TS types + generated Lua table) stays a
documented future option if the type count grows much past 8 or drift recurs after this test
lands — not a v1 requirement.

### The artifact's concrete form (added 2026-09-18)

An implementation-feasibility review found the decision above names *what* the artifact contains
but not *what it is* — no file format, no path, no load path in any of the three consuming
languages — leaving three implementers free to pick a Markdown table (unparseable by the tests
the decision itself requires), a Rust `const` (forces a Lua/TS export path nobody scoped), or a
JSON file (works everywhere with zero new dependencies). **Decided: JSON**, since `plain_data`
already *is* JSON at the wire format, and all three consumers can load it with what they already
have — `serde_json` (Rust, already a dependency), `JSON.parse` (TS, native), and Q1's own vendored
`_json.lua` decoder (`src/resources/pandoc/datadir/_json.lua` — P4 already traces this tree for
`init.lua`'s dependencies, so it costs P5's Lua contract test nothing new).

- **Path:** `crates/quarto-pandoc-types/resources/custom-node-schema.json`, loaded via
  `include_str!` in Rust (both the producer-side test and any future consumer), fetched/embedded
  the same way TS already embeds shared JSON resources, and read via `_json.lua` in P5's Layer-1
  Lua probe.
- **Shape** (one fully-worked entry, `Callout`, per the review's own recommendation — this is the
  literal syntax an implementer should copy for the other 7 types, not just the abstract shape):
  ```json
  {
    "version": 1,
    "types": {
      "Callout": {
        "route": "R",
        "slots": { "title": "Inlines", "content": "Blocks" },
        "plain_data": {
          "type":                      { "required": true },
          "appearance":                { "required": true },
          "collapse":                  { "required": true },
          "collapse_starts_collapsed": { "required": true },
          "icon":                      { "required": true },
          "ref_type":                  { "required": false, "when": "crossref-eligible id" },
          "kind":                      { "required": false, "when": "crossref-eligible id" },
          "identifier":                { "required": false, "when": "crossref-eligible id" },
          "order":                     { "required": false, "when": "labeled + crossref-indexed",
                                          "shape": "{ order: int, section: [int] }",
                                          "producer": "crossref-index (post-construction)" }
        }
      }
    }
  }
  ```
  (`slots` values are one of `Block|Blocks|Inline|Inlines`, matching the wire envelope's own
  `data-custom-slots` type tags — see P5's envelope-decoding finding.)

  **Correction (2026-09-18, round 4 review, Reviewer A) — `order` was missing from this worked
  example, and it is the field the entire epic turns on.** Design §3 describes Route R as calling
  constructors "with Q2's already-resolved fields (incl. `plain_data.order`)"; §4.D names number
  injection as "the real design problem"; P5's Theorem worked example is literally
  `tbl.order = { order = n, section = section }`. But no construction site writes `order` —
  `transforms/callout.rs:276-296` (and `theorem.rs`/`proof.rs`/`float_ref_target.rs`) build
  `plain_data` with no `order` key at all. **`order` is injected by a *later* transform**:
  `transforms/crossref_index.rs:283-295` writes `{"section": [...], "order": n}` into
  `node.plain_data` for every node passing `has_crossref_plain_data` (FloatRefTarget, Theorem,
  and crossref-id'd Callout); `crossref_resolve.rs:316-317` does the same for
  `CrossrefResolvedRef`. Add the `order` field (per the shape above) to `Callout`,
  `FloatRefTarget`, `Theorem`, and `CrossrefResolvedRef`'s schema entries. `Proof` correctly gets
  none — design §3 says it "deliberately carries no `ref_type`," and `crossref_index.rs:255`
  early-returns without one.
  **This also means the schema needs a producer-stage notion, not just `required`/`when`:**
  `order` is absent at every construction site and present at the wire-format cut for labeled
  nodes — the two points are different, and P2's field-discovery instruction below previously
  pointed only at the first one, which cannot see `order` at all.
- **Version placement/policy, closing a separate open item below:** `version` is a top-level
  integer in this same JSON file (not a wire-format attribute — see the deferred-question fix
  below for why). Bump policy: **reject on mismatch is deferred past v1** — for a pre-1.0 wire
  format with no external consumers of the artifact itself (only this repo's own three tests read
  it), a version bump is a normal same-commit change to producer + all three tests; the field
  exists so a *future* out-of-band consumer (e.g. a hub-client session running stale WASM against
  a newer server) has something to check, not because anything checks it yet.

## In scope
- **Inventory is done** (see table above) — no further "confirm a registry exists" step needed.
  What's left: for each of the 8 types, record the complete `plain_data` field set (not just the
  illustrative subset) and slot names. **Corrected 2026-09-18 (round 4 review, Reviewer A): read
  the field set as observed at the wire-format cut (after `crossref-index`/`crossref-resolve`
  have run), not "from the actual construction site" as this line previously said.** `order` is
  the concrete field that instruction would miss entirely — see "The artifact's concrete form"
  above for the full finding. Every construction-site transform (`callout.rs`, `theorem.rs`,
  `proof.rs`, `float_ref_target.rs`) builds `plain_data` before `order` is ever written; reading
  only the construction site guarantees a schema that omits the epic's load-bearing field.
- **Confirmed extension request (2026-09-18, round 4 review, Reviewer D): `CrossrefResolvedRef`
  needs `cite_prefix`, `cite_mode`, and `label_upper` fields**, following the same "no longer a
  maybe" pattern as the Proof `type` request below. P5's Route-N decision states Q1's `refs.lua`
  behavior is normative for the Pandoc leg specifically *because* it is cite-mode aware
  (`cite.prefix`, `SuppressAuthor` → `ref-noprefix`) and derives an `upper` flag from the
  original label's capitalization (`@Fig-1` → "Figure 1" vs. `@fig-1` → "fig. 1") — but
  `crossref_resolve.rs:325-335` explicitly drops the citation's `prefix` at construction (comment:
  "The prefix is dropped because crossref references usually don't carry a leading textual
  prefix"), and `build_resolved_ref` (`crossref_resolve.rs:286-320`) never records citation mode
  or the label's original case. Without these three fields, the "Q1 is normative" decision cannot
  actually be implemented as stated — add them to `CrossrefResolvedRef`'s schema entry (`required:
  false`, since HTML's own resolution has no matching concept for any of the three today).
- Define a **single source of truth** for the per-type schema: `type_name → { slots, plain_data
  fields, route (L/R/N) }` — **corrected 2026-09-17** (was `(L/R)`, which can't express the two
  types that are actually Route N; found by an epic-wide review) — using the corrected 8-type
  inventory and the Route corrections above.
- Add a **schema version** — a top-level integer field in the canonical schema artifact itself
  (see "The artifact's concrete form" above for the exact placement and shape), **not** a wire-
  format attribute. **Corrected 2026-09-18** (an implementation-feasibility review found the
  original motivation unverifiable): the AST/wire format is not actually persisted in
  `ts-packages/quarto-automerge-schema` (grepped — that schema carries project/file metadata, not
  ASTs), so "already-deployed hub-client sessions reading this format live via Automerge sync"
  doesn't name a real skew scenario. The field exists for a *future* out-of-band consumer (e.g. a
  hub-client session running stale WASM against a newer server, if the wire format is ever
  embedded in synced state) to have something to check — bump policy (reject/warn/ignore-unknown)
  is deferred past v1, since today's only three consumers are this repo's own tests, updated in
  the same commit as any schema change.
- **Handler-introspection test (Layer 1)** on the TS consumer: enumerate `previewRegistry` (not
  `framework/customNode.ts`) and diff declared slots/type-coverage against the schema. **Neither
  `Tabset` nor `ExampleEmbed` has a CustomNode reaching preview at all** (see finding above —
  excluded pre-creation vs. destroyed by an un-excluded render transform), so neither has
  anything to introspect there; both schemas still need authoring for the Pandoc tail, just not
  checked against React.
- **Author the canonical schema doc** (`type_name → {slots, plain_data fields incl. conditional
  ones, route}`) per the "Decided" section above, then write the three cross-consumer tests
  (Rust construction-site test, TS Layer-1 diff, Lua contract test in P5) against it.
- **Deciding whether to give `Tabset`/`ExampleEmbed` a real preview UI is out of scope for this
  epic** (both gaps predate it, and reviving either is bigger than a registry entry — tracked as
  its own follow-up per the `Tabset` exclusion comment) — P2 only needs their schemas authored
  for the Pandoc tail, not their preview UX fixed.

## Out of scope
- The Lua consumer/shim (P5). Emitting at the cut (P1). Per-format Meta mapping (P7). Giving
  `Tabset` or `ExampleEmbed` a real preview UI (both pre-existing gaps, orthogonal to this
  epic).

## Consumes / Produces (seams)
- **Consumes:** P1's neutral-core cut (the thing being serialized).
- **Produces for P5:** the frozen schema the shim reads; a channel for **`plain_data` extension
  requests** (P5's Route-R field audit may find fields Q1's render needs that React ignored).
- **Produces for P7:** confirmation the AST `Meta` block carries normalized doc metadata; and the
  sideband-map (not CustomNode) carriage fact for code-block decorations, so P7's invocation
  builder knows not to expect a `DecoratedCodeBlock` wire node.

## Coarse checklist
- [x] Record complete `plain_data`/slot field lists for all 8 real types, **read at the
  wire-format cut (after `crossref-index`/`crossref-resolve`), not at the construction site**
  (corrected 2026-09-18 — see the field-discovery correction above; the construction-site table
  further up gives construction sites for the base fields, but `order` for `Callout`,
  `FloatRefTarget`, `Theorem`, `CrossrefResolvedRef` is added later and must be recorded too).
- [x] Fix the design doc §3 Route table: `PanelTabset`→`Tabset` (reclassified **R**, decided),
  drop `DecoratedCodeBlock`, add `Equation` (**Route N** — confirmed no Q1 handler exists at
  all, not R). The `Tabset` rename + `Equation` addition landed in commit `8e9e546b4`
  (2026-09-17); the `DecoratedCodeBlock` row was flagged here but never actually dropped until
  a later pass caught it still sitting in the table (same never-landed-fix pattern as the
  `CalloutResolve` bucket-table miss) — removed now, alongside a stale `ExampleEmbed` Route-N
  row the same table had never updated after P1 resolved it entirely upstream.
- [x] Decide codegen vs. hand-mirror — **decided: hand-mirror + cross-consumer test** (see
  "Decided" section above).
- [x] Author the canonical schema doc — **JSON, at `crates/quarto-pandoc-types/resources/
  custom-node-schema.json`, per "The artifact's concrete form" above** (corrected 2026-09-18 from
  an unplaced "schema doc"); version field per the same section (bump policy deferred past v1,
  not "in-flight Automerge documents" — that motivation didn't hold up).
- [x] Rust construction-site test: assert each type's real `plain_data` keys (incl. conditional
  ones) match the schema exactly. **Drive the real transform, not a bare `CustomNode` literal**
  (2026-09-18 — a bare-construction test can't exercise the conditional-field logic the schema's
  `"when"` clauses describe, e.g. Callout's crossref-eligible-id branch); **one fixture per
  conditional branch** (e.g. Callout needs both a plain and a crossref-eligible fixture) so
  "including the conditional ones" is actually exercised, not just declared. **For the four
  `order`-bearing types, this test must run the full pipeline through `crossref-index`/
  `crossref-resolve`, not just the sugar transform** (2026-09-18, round 4 review) — asserting
  against the sugar transform's output alone would pass while still matching a schema that omits
  `order`, since `order` genuinely isn't present until that later stage runs.
- [x] File the `CrossrefResolvedRef` extension request (`cite_prefix`, `cite_mode`, `label_upper`
  — see the confirmed-extension-request note above) alongside the existing Proof `type` request.
- [x] Layer-1 introspection test (TS side) against `previewRegistry` *and* against the
  hand-written per-type interfaces (e.g. `CalloutPlainData`); `Tabset`/`ExampleEmbed` both have
  nothing to introspect against React (neither reaches preview). Fix the demonstrated
  `CalloutPlainData` drift (missing `kind`, `identifier`) as part of this.
- [x] Round-trip + preview-parity gate green.

## Deferred in-plan questions
- ~~Which extra `plain_data` fields Q1 needs~~ — **partially resolved by P5's audit (2026-09-16):**
  - **`Proof` needs a new field: `type`** (e.g. `"proof"`). Q1's constructor
    (`proof.lua:54-61`) requires it and render indexes it unconditionally
    (`proof_types[proof_tbl.type:lower()]`, `proof.lua:81`) — a missing `type` crashes Q1's
    render. Q2's current Proof `plain_data` is just `{"kind": "Proof"}` (`proof.rs:145-148`) —
    confirmed real gap, not a maybe.
  - `Theorem` needs no new field — `div`/`name` come from existing slots (`content`/`title`),
    not `plain_data`; `kind` may be *redundant* (Q1 derives its own default title), not missing.
  - `FloatRefTarget` needs nothing — its Q1 constructor is unfiltered passthrough
    (`floatreftarget.lua:96-107`); confirmed mechanical.
  - **`Tabset` (Route R, decided) needs no new field either.** Audited against
    `panel-tabset.lua`'s real `constructor(params)` (`params.level`, `params.attr`,
    `params.tabs` — a list of `{title, content, active}`): Q2's wire shape
    (`{level, tab_count, actives}` + `title-{i}`/`content-{i}` slots) zips cleanly into that
    list with no missing field. The extra `plain_data.group` Q2 sometimes sets is an HTML-only
    grouped-tab-sync signal (`TabsetsJsStage`) with no Q1 counterpart — harmless unused data for
    the Pandoc tail, not a gap.
  - **`Equation` is resolved to Route N, not R — no field audit applies.** Confirmed by reading
    `crossref/equations.lua`: Q1 has no custom node for equations at all, so there is no
    constructor to compare `plain_data` against. See the Route-table correction above.
- ~~Equation's Route (L or R)~~ — **resolved: Route N.** `crossref/equations.lua` has no
  `_quarto.ast.add_handler`/`ast_name` — it's a plain filter over `Para`/`Plain` blocks doing
  format-specific `RawInline` injection (LaTeX `\label{}`, Typst `#math.equation(numbering:...)`)
  directly on the existing `Math` inline. The shim should unwrap `Equation` back to a plain
  `Span`/`Math` and handle numbering the same way — no Q1 constructor to route into.
