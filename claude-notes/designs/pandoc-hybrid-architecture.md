# Pandoc-writer hybrid — architecture & frozen decisions

**Date:** 2026-08-20  **Updated:** 2026-09-18 (two passes) — see the **Amendment log** (bottom of
this file) for the full history of corrections. Latest pass (round 4 review, four dispatched Opus
reviewers): sharpened two §12 bullets (crossref-presentation asymmetry and mermaid are both
silently-wrong for an explicit user, not merely "different wording"; verified against real code),
landed Gordon's decision to gate project-mode Pandoc rendering in P7 rather than leave it to a
separate strand, and added a new §12 bullet for the Callout unregistered-category fallback (P5) —
see the log for full detail. (Prior pass, same day: three Opus reviews — build-order/
implementation-feasibility × 2, user-facing/backward-compatibility × 1 — found real gaps, resolved
per a governing principle from Gordon — new §12 "Known limitations," §13 "Project-mode Pandoc
rendering," §14 "Multi-format render guardrail.")
**Status:** Design frozen for plan decomposition. Supersedes an earlier "Cut A / Cut B"
framing (2026-07-10 draft, since removed from the tree — see git history if needed).
**Companions:** `research/2026-07-09-q1-filter-catalog.md` (Lua),
`research/2026-07-13-q1-format-typescript.md` (TypeScript),
`designs/transform-pipeline-phases.md` (the phase contract this builds on).

This note is the single source of truth for the decisions the plans (P1–P8) depend on.
The plans reference **this file**, not the design conversation that produced it.

---

## 1. The core idea, restated

Q2 owns parsing + engines + Quarto's semantic model (crossref numbers, callouts,
theorems, floats). For every non-native format it hands a **Pandoc JSON AST** to
`pandoc` and reuses Quarto 1's Lua filters — largely verbatim — for format-specific
rendering. We do **not** port Pandoc's writers.

The breakthrough reframe (vs. the original plan): **this is not a new pipeline shape
plus a new serializer.** It is *adding a third renderer to a boundary Q2 already has.*

## 2. The renderer trinity — one semantic core, three renderers

Q2 already emits its CustomNodes through a **custom-node wire format** (built for the
q2-preview epic: `pampa/src/writers/json.rs::write_custom_block`, mirrored in TS by
`ts-packages/preview-renderer/src/framework/customNode.ts`). It encodes a `CustomNode`
as a Pandoc `Div`/`Span` with class `__quarto_custom_node` + `data-custom-{type,slots,data}`
and named-slot child wrappers. Q2's `CustomNode` (`quarto-pandoc-types/src/custom.rs:62`)
is **generic** — `{ type_name, slots, plain_data, attr }` — not a typed enum.

Three renderers consume the *same* semantic core, via that *same* wire format:

| Renderer | Consumes | Bucket-4 analog |
|---|---|---|
| Native HTML writer | CustomNodes in-process | `CrossrefRender` / `CodeBlockRender` / … (Rust) |
| React preview | wire format → JS-native | preview components |
| **Pandoc + Q1 Lua (this epic)** | **wire format → Q1 nodes** | **`customnodes/*.lua` render** |

So the "CustomNode → Q1 scaffold bridge" the original plan called *the crux* is **not a
new serializer** — the serializer exists and ships. This epic **adapts the existing wire
format from serving TS (React) to serving Lua (vendored Q1)**.

## 3. The bridge: Route L / Route R

Q1 custom nodes are bifurcated: a scaffold Div (attributes `__quarto_custom*`,
`__quarto_custom_id`) plus field data in an in-memory `custom_node_data[id]` side-table,
populated either by a handler's `parse(div)` (from a raw classed Div) or its
`constructor(tbl)` (`ast/customnodes.lua`).

There are two bridge routes, chosen **per node-type** by whether Q2 injected resolved
semantics:

- **Route L — lower to raw.** Reconstruct the raw classed Div (e.g. `::: {.callout-note}`)
  and let Q1's unmodified `parse`+`render` run. This is the *inverse of Q2's Normalization
  sugar*. Zero coupling to Q1 internals. **Decision: performed in the Lua shim** (New-3).
  Correct for **presentation-only** nodes with no numbering to lose. **No current wire-format
  type actually uses this route** — the two candidates that looked presentation-only at design
  time, Callout and Tabset, both turned out to carry real crossref numbering and were
  reclassified to R (see the amended table below); Route L stays defined for future
  presentation-only types, not populated today.
- **Route R — reconstruct into the Q1 node.** A vendored Lua shim reads the wire format and
  calls the handler's constructor with Q2's already-resolved fields (incl. `plain_data.order`),
  **bypassing Q1's numbering**. Needed for **semantics-bearing** nodes: Float, Theorem, Proof.
  This is where the residual crux — **number injection alone, not category-registry seeding**
  (corrected 2026-09-17, see §4 C below) — lives.

The field-map is mechanical and verified 1:1 for callout (`callout.rs` doc ↔ `callout.lua`'s
**constructor**, `quarto.Callout(...)` — `parse` itself calls the same constructor internally,
so the mapping holds for both the historical Route-L framing and the current Route-R one): Q2
`plain_data.{type,appearance,icon}` + `slots.{title,content}` → Q1
`quarto.Callout({type,appearance,icon,title,content})`.

### Route-L/R table (Decision 4 — frozen, amended 2026-09-17)

| Q2 CustomNode `type_name` | Route | Why |
|---|---|---|
| Callout | **R** (was L) | **amended** — a Callout with a crossref-eligible id gets `plain_data.order` from the same shared indexer as FloatRefTarget/Theorem; Route L discarded it and Q1's own numbering pass is suppressed under external mode, silently dropping the "Note N:" prefix. Decided with Gordon, P6 Finding 4. |
| Tabset (né PanelTabset) | **R** (was L) | **amended** — P5 found the wire shape matches Q1's *constructor*, not its `parse`; reclassified, decided with Gordon. |
| FloatRefTarget | R | carries `plain_data.order` (numbered) |
| Theorem | R | numbered environment |
| Proof | R | reconstructed via Q1's constructor for shape parity, but **not numbered** — `plain_data` deliberately carries no `ref_type` (P6 Finding 3); Q1's own `proof.lua` renderer doesn't consume `.order` either |
| CrossrefResolvedRef | **N** | P5: Q1 has no persisted node for a resolved `@ref` either — resolves directly to a plain Pandoc inline |
| Equation | **N** | P5: no Q1 `ast_name`/constructor at all — a plain filter that RawInline-wraps the existing `Math` inline |

**Route decision procedure for new types (corrected 2026-09-17 — the old one-line rule
"numbered → R, presentation-only → L" doesn't reproduce the table above):** Tabset is Route R
without being numbered, and Callout is Route R "uniformly (numbered or not)" per P6 Finding 4 —
so "numbered" is not the actual test. Use instead:
1. **Does Q1 have an `ast_name` for this type at all?** No → **N** (resolve/render directly,
   mirroring whatever Q1 does natively for the equivalent content).
2. **Does the wire shape match Q1's `constructor` (already decomposed), or does Q2 hold resolved
   semantics — `order`, a resolved `appearance`/`active` flag, anything a raw-Div reconstruction
   through `parse` would discard or have to re-derive?** Either condition → **R**.
3. Only a type that is both presentation-only *and* whose wire shape round-trips losslessly
   through Q1's `parse` (no reconstruction, no discarded resolved state) is **L**. Verify this
   against the handler before choosing it — it is the choice that silently loses data if wrong.

Zero of the current 8 types are L today (Tabset failed step 3 on shape; Callout failed it on
resolved state) — L remains defined for a genuinely presentation-only type whose author confirms
condition 3, not a default.

**Two of Q2's 8 real wire-format types are deliberately absent from this table**, for two
different reasons (both corrections landed here 2026-09-17, having been flagged by P2/P5/P1 but
never actually applied to this table until now):
- **`DecoratedCodeBlock` never belonged in this table at all.** P2's real 8-type inventory
  (grepped every non-test `CustomNode::new` call site) found no such CustomNode exists — code-block
  decorations are carried in a sideband map (`RenderContext::code_block_decorations`), not the
  wire format, "to avoid the nested-CustomNode complexity Q1 ran into." `decoratedcodeblock.lua`
  is a real Q1 `ast_name`, but this shim never sees a `DecoratedCodeBlock` wire node to route,
  L or R.
- **`ExampleEmbed` is a real wire-format CustomNode, but never reaches this shim for Pandoc
  targets.** P1 (2026-09-17, decided with Gordon) reclassified `example-embed-render` from B4 to
  format-parameterized B1: it already emits its iframe as a `RawBlock("html", ...)` (which
  non-HTML writers drop natively) and its snippet/caption content as plain portable Pandoc
  blocks, so it now runs for `Pandoc(fmt)` too, resolving `ExampleEmbed` entirely in Rust
  upstream of the wire-format cut. It is not a Route N case in practice — there is no
  `CustomNode("ExampleEmbed")` left for the shim to route by the time a Pandoc render reaches
  this bridge.

## 4. The crux, decomposed (scoped to Route R)

"The crux" = four sub-problems; only the last is hard:
- **A. scaffold representation** — mechanical (per-type; the wire format already provides it).
- **B. per-node payload** — mechanical (`plain_data` already carries resolved data).
- **C. category registry — corrected 2026-09-17, was disproven by P6 Finding 1.** There is no
  metadata-time registration hook to "seed" for built-in categories: Q1 has four independent
  built-in-category mechanisms, three with no registration function at all and the fourth
  (`crossref.categories`) a static Lua table populated at file-load time, before any metadata is
  read. `custom.lua`/`crossref.custom` — the only metadata-driven mechanism — registers **only
  user-declared** extensions, and Q2's `crossref.custom` metadata already survives unmodified
  into the Pandoc `Meta` Q1 reads, so that case needs only a regression test, not new code. The
  one lever that *does* exist for built-ins is a **display-name override**, not a registration
  one: `crossref-<type>-title` (read by `title()`, `crossref/format.lua:4-7`) lets Q2's
  `RefTypeRegistry` override Q1's static locale default per registered type — owned by P4's
  params-blob builder, decided with Gordon (see P4's "Q2 owns presentation defaults" finding).
  **Consequence: sub-problem C is not "build an export," it's "confirm a passthrough with a
  test" — a much smaller item than this paragraph previously implied.**
- **D. numbering suppression + number injection** — the real design problem: run Q1's
  category registration and render, but **not** its number-assignment pass, and make Q1's
  render consume Q2's numbers. Solved by the New-4 upstream change (below), not by disabling
  the whole crossref group.

**When the crux exists:** iff we fork *after* Q2 has numbered AND still reuse Q1's presentation
filters. We chose exactly that (runtime-reconstruction), so it exists — but only for Route R, and
only for sub-problem D above; sub-problem C turned out not to be hard at all.

## 5. The unified cut — formats = shared neutral core + per-format tail (Decision 2)

Every output target (HTML, revealjs, q2-preview, and all Pandoc formats) is a **format** with:
1. a **shared, format-neutral semantic core** (Normalization-neutral + Crossref), whose output
   — CustomNodes intact, numbers in `plain_data`, `@refs` resolved — **is the wire-format cut**;
2. a **per-format tail**.

The core is a shared **builder**, not a shared **artifact**: it is format-*parameterized*
(shortcodes and — later — content-hidden resolve per target `lua_format`), so each format
re-runs the core with its own format param. q2-preview's cut AST is not literally reusable for
docx; the *pipeline shape* is shared, the *instance* is not.

Tails:
- **HTML / revealjs:** Navigation → Finalization → native HTML/reveal writer.
- **q2-preview / q2-slides:** (chrome only) → wire format → React.
- **Pandoc formats:** **skip Navigation** → wire format → Pandoc + vendored Q1 Lua.

This replaces q2-preview's *subtractive* deny-list (`Q2_PREVIEW_TRANSFORM_EXCLUDED`) with an
*additive* `PipelineProfile { HtmlFull, Revealjs, Preview, Pandoc(fmt) }` that selects a tail.
**Superseded — flagged 2026-09-17, not yet landed here:** this four-variant shape is wrong (no
slot for the Revealjs-family/Preview-kind cell, i.e. q2-slides, which the runtime already
handles correctly today); P1 corrected it to the five-variant, two-axis shape
`{ HtmlRender, HtmlPreview, RevealjsRender, RevealjsPreview, Pandoc(fmt) }`. This shorthand is
retained here only for the tail-selection argument above, which holds either way; P1 owns
landing the corrected shape in this section (checklist item still open).

## 6. The four buckets + litmus (Decision 1 — frozen)

**Litmus:** a transform is format-specific (Bucket 2/4) if it bakes one format's *presentation*
of semantics captured neutrally elsewhere, re-expressed per-format downstream (Pandoc
template/writer).

- **B1 — format-neutral semantic core** (pre-cut, shared; = wire-format content).
- **B2 — format-family scaffolding** (pre-cut, per-family; must not touch semantics).
- **B3 — shared post-core service** (crosses the cut; runs for Pandoc too).
- **B4 — format presentation** (the tail: native writer / Q1 Lua / React).

| Transform(s) | Bucket | Notes |
|---|---|---|
| Callout (sugar only); ShortcodeResolve; Metadata/Date/Authors Normalize; CodeBlockGenerate; ExampleEmbed (sugar), **ExampleEmbedRender**, Theorem/Proof/FloatRefTarget sugar, EquationLabel; CrossrefIndex, CrossrefResolve | **B1** | ShortcodeResolve is format-*parameterized*; CodeBlockGenerate output is a RenderContext **sideband** (does NOT cross the cut). **`CalloutResolve` moved out of this row to B4** (P1 fix, see below) — it was misclassified here; only the *sugar* half (`callout`, builds the CustomNode) is B1. **`ExampleEmbedRender` reclassified from B4 2026-09-17** (decided with Gordon, P5): its iframe is already emitted as `Block::RawBlock(format: "html", ...)` — the identical Pandoc primitive a hand-authored raw-HTML iframe would use, which non-HTML writers already drop by convention — so it is format-*parameterized* like ShortcodeResolve: emit the iframe only for iframe-capable profiles (`HtmlRender`/`HtmlPreview`/`RevealjsRender`/`RevealjsPreview`); for `Pandoc(fmt)`, emit the `snippet`/numbered-caption content only, omitting just the iframe. Resolved entirely here — `ExampleEmbed` never reaches the Lua shim as a raw `CustomNode` for Pandoc targets, so it needs no Route N handling in P5. **CodeBlockGenerate's sideband, corrected 2026-09-17** (was "Q1 re-derives from attrs," which understated a real contradiction between this doc, P2, and P5 — resolved by reading `decoratedcodeblock.lua`/`code-filename.lua`/`foldcode.lua` directly, not the sideband map): no bridge from Q2's sideband to Q1 is needed for either currently-implemented decoration. `filename` is independently re-derived by Q1's own `code-filename.lua`, which reads the identical Pandoc `CodeBlock` attribute Q2's `CodeBlockGenerate` reads — two independent mechanisms that happen to converge on the same source data, not a Q2→Q1 handoff. `copy` (copy-to-clipboard) is an HTML/JS-only presentation feature with no docx/pptx equivalent to lose. **However, this surfaces a real, separate content-loss gap, not yet resolved:** `decoratedcodeblock.lua` has renderers for html/markdown/latex only; docx and pptx fall through to the generic default renderer, which drops the `filename` header entirely (confirmed by reading the renderer registrations) — so even though the *data* needs no bridge, the *filename header* is currently silently lost for docx/pptx regardless. This needs an explicit v1 scope decision (accept the loss vs. contribute a docx/pptx renderer upstream vs. some other approach), tracked as an open question, not assigned to any plan yet. |
| **Footnotes** | **SPLIT** | B1: `NoteRef`+`Def` → native Pandoc `Note`. B2/4: HTML `<section>`+backlinks |
| TitleBlock; Sectionize; TitleBanner; Website{TitlePrefix,Favicon,BootstrapIcons,CanonicalUrl} | **B2** | HTML-family (title-block via template from Meta; section-divs is HTML-only) |
| RevealColumns, RevealSlides, RevealFooterAlias, RevealFootnotes | **B2** | revealjs-family scaffolding |
| entire Navigation phase (toc/navbar/sidebar/pagenav/footer, listings, feeds, categories) | **B4** | HTML/website chrome — Pandoc skips wholesale. **Added 2026-09-17 (were missing from this row despite being real Navigation-phase transforms, per an independent verification pass):** `breadcrumbs-render`, `quarto-nav-js`, `repo-actions-render`. |
| CrossrefRender, **CalloutResolve**, Mermaid, CodeBlockRender, TableBootstrapClass, reveal auto-stretch, **AttributionRender**, **AttributionGenerate** | **B4** | HTML writer's copy of the renderer trinity. `CalloutResolve` moved here from the B1 row (P1 fix, 2026-09-17): it destroys the `Callout` CustomNode into Bootstrap-specific HTML DOM, already excluded from `Q2_PREVIEW_TRANSFORM_EXCLUDED` for exactly that reason (`pipeline.rs:1595`; confirmed by `Callout.tsx`'s own comment). **`AttributionRender`/`AttributionGenerate` added 2026-09-17** (were entirely unclassified, alongside the already-flagged `attribution-viewer`): both populate `ctx.format_options` fields consumed only by the HTML/JSON writers (per-node attribution records for hover badges) — no Pandoc consumer exists or is planned. |
| `panel-tabset` (sugar) | **B1** | **Added 2026-09-17** (was unclassified — P1's Pandoc exclude-list derivation depended on this row existing but it never did). Builds the `Tabset` CustomNode; must stay enabled for Pandoc so P5's Route-R shim has a node to route (unlike Preview, which excludes both halves — see P1). |
| `panel-tabset-resolve` | **B4** | **Added 2026-09-17.** Exactly parallel to `Callout`/`CalloutResolve`: destroys the `Tabset` CustomNode into presentation HTML; excluded for both Preview and Pandoc. |
| `ConditionalContentTransform` | **B1** | **Added 2026-09-17** (confirmed still missing by both P1 and P8's independent checks). Runs unconditionally, first in Normalization, before any HTML/reveal-family branching; format-parameterized via `lua_format_for()`; already correct for a genuine Pandoc `target_format` string with no further work. |
| ResourceCollector | **B3** | resource/mediabag staging — Pandoc needs it |
| LinkRewrite | **B3** | cross-doc/relative links; project/book-gated; no-op standalone. Resolved 2026-09-18 (was `B3?`) — unconditionally part of the B3 shared-services segment, decided with Gordon; see the Amendment log |
| AppendixStructure | **B3** | Resolved 2026-09-17 (P1, decided with Gordon): `appendix.rs` emits pure Pandoc primitives (no RawHTML), already runs unexcluded for q2-preview's non-HTML consumer today — same argument extends to Pandoc. Excluding it would silently drop user appendix/license/citation content from docx/pptx with no replacement. |

**This table's coverage is not yet asserted as total.** It was found, twice (2026-09-17, once by
P1's own audit and once by an independent verification pass), to have missed real transforms
that a mechanical derivation should have caught — see the additions above. Until a test asserts
every transform registered in `build_transform_pipeline` has exactly one bucket here (tracked as
a P1 recommendation, not yet a checklist item), treat this table as "believed complete," not
"proven complete."

## 7. Upstream Q1 contribution (Decision 5, New-4)

Contribute to quarto-cli so we keep the vendored sources **verbatim**: add
**`crossref-numbering: "quarto" | "external"`** (default `"quarto"`, bit-for-bit current
behavior). When `"external"`: skip the numbering/index/`@ref`-resolve group, but keep category
registration and render decoration (which read `order`/`parent_id` off the nodes). One-hunk,
backward-compatible, generally-useful ("front end supplies numbers"). The render gates change
from `if not param("enable-crossref")` to `if not crossref_present()` where `crossref_present =
enable-crossref OR crossref-numbering=="external"`. Until merged, carry as a marked patch with a
PR link; the P5 contract test flags if a Q1 bump moves the gate.

## 8. Cross-cutting decisions

- **No Pandoc bundling; enforce a minimum Pandoc version.** Non-pampa formats are
  **native-build only** (no WASM/preview); a future WASM-pandoc is out of scope.
- **User Lua filters don't run for the Pandoc leg — added 2026-09-17 (P4, epic-wide review
  I9).** `quarto-filters` (the param Q1 TS uses to splice user filters into `main.lua`) is N/A
  here: Q2 runs user filters itself, via `UserFiltersStage`, independent of Q1's Lua entirely.
  Consequence: for `--to docx`/`--to pptx`, a user filter's `UserFiltersStage::post()` sees the
  **pre-cut** AST (custom-node wrappers intact), not the rendered shape an HTML render or Q1
  itself would show it — different from both. Accepted as out of scope for v1 (the golden-parity
  fixtures are filter-free); revisit if a filter-bearing fixture is ever added to the parity set.
- **Wire format becomes a versioned, single-sourced shared contract** (New-2). It now serves
  Rust producer + TS (React) + Lua (Q1). `plain_data` may need extending for fields Q1's render
  reads that React ignored (per-Route-R audit in P5 → change request to P2).
- **CI parity story:** CI-runnable gates = wire-format round-trip + AST-at-cut snapshots +
  the handler-introspection contract test. Q1 byte-parity uses `external-sources` and is a
  **local/dev** gate (external-sources is not in CI).
- **Meta-block contract:** the wire output's Pandoc `Meta` carries normalized document metadata
  (title/date/authors); P2 owns the carriage, P7 owns per-format Meta→template mapping.
- **content-hidden / `when-format` gating** is **deferred to P8**, out of scope for docx/pptx v1.

## 9. Plan decomposition (P1–P8) + dependency graph

- **P1 — Neutral-core + `PipelineProfile`.** Split Footnotes; relocate the B2 transforms into
  per-family pre-cut segments; resolve the AppendixStructure cell; convert preview's deny-list
  to the additive profile. Reviewed against **HTML/reveal/preview byte-identity**. No Pandoc.
- **P2 — Wire-format schema v1.** Single-sourced type/route/slots/plain_data schema + version +
  handler-introspection test on the TS consumer. Reviewed against round-trip + preview parity.
- **P3 — Upstream Q1 crossref split.** `crossref-numbering: external` PR + vendored patch.
  Reviewed against Q1's suite + a Q2 golden.
- **P4 — Vendored-Q1 run machinery.** Vendor filters + `init.lua`; build `QUARTO_FILTER_PARAMS`;
  `--data-dir`; locate `pandoc`. Reviewed by a "run main.lua, get bytes" transport smoke.
- **P5 — Lua shim (Route R + N) + contract tests** (Layer-1 introspection, Layer-2 golden). No
  Route-L type exists in the current 8-type wire-format inventory (Tabset and Callout both
  amended L→R during the P1–P8 deep pass) — corrected 2026-09-17, this section previously still
  said "Route L + R."
- **P6 — Category passthrough + numbering-suppression wiring** (retitled 2026-09-17 from
  "Category-registry seed" — see §4 C above; there is nothing left to seed for built-ins).
  Consumes P3 **and P5** — P6 reuses P5's Route-R post-construction order-assignment mechanism
  for Callout, and the Callout reclassification itself lands in P5's shim file (corrected
  2026-09-17: this section previously omitted the P5 dependency, out of sync with the epic doc's
  own dependency graph and Plans table, which already stated "P6 after P3 and P5").
- **P7 — Per-format tail + invocation builder — docx first**, then pptx; latex a stub. Owns the
  fixture set, so also owns confirming whether any in-scope docx/pptx fixture exercises the
  `algorithm` theorem class (P5's `THEOREM_CLASSES` gap) and filing the follow-on if not.
- **P8 — content-hidden / `when-format` gating**, mostly already implemented elsewhere;
  remaining work is Pandoc-target verification. **Not fully independent of P7** (corrected
  2026-09-17): P7's checklist wants a content-visible/hidden smoke fixture once the Pandoc tail
  exists, which needs P8's verification first — so the fixture item belongs on **P8's**
  checklist, added once P7 lands, not on P7's own checklist as a P8-dependent item (the two
  plans previously each pointed at the other for this one item, a latent cycle with no plan
  actually holding it).

**Dependencies (corrected 2026-09-17 — the previous "P1–P5 parallelizable" line was
self-contradicted by this same document's Route/bucket findings and by the epic's own Plans
table, which already lists P5 as depending on P2 and P4):**
- **Parallel immediately:** P1, P2, P3 (no cross-dependencies once the frozen decisions in this
  doc land).
- **P4** — after P2 (needs the wire-format schema to build `PandocWriteStage`'s serialization
  step), though it can start against the *frozen schema decision* before P2's implementation
  fully lands.
- **P5** — after P1 (needs the `Pandoc`-kind exclude-list decision, specifically that
  `panel-tabset`'s sugar half stays enabled, for its Tabset field-map to be valid), P2 (schema),
  and P4 (transport, for its Layer-2 golden render).
- **P6** — after P3 (external-numbering mode) and P5 (Route-R construction + order-assignment
  mechanism, reused for Callout).
- **P7** — after P1, P2, P4, P5 (P6 too, for correct numbers in its golden).
- **P8** — independent to start; its docx/pptx smoke-fixture item is added once P7's tail exists
  (see above).

## 10. Seams the next session works

- P1↔P7: B3 shared-services output (staged resources, rewritten links) consumed by the Pandoc tail.
- P1↔P7: Footnotes B1-split → native `Note` consumed by Pandoc's per-format footnote writer.
- P2↔P5: wire schema ↔ shim; `plain_data` extension requests from the Route-R field audit.
- P2↔P7: AST `Meta` carriage ↔ per-format `Meta`→template mapping.
- P3↔P6: `crossref-numbering: external` param ↔ numbering-suppression wiring.
- P4↔P5↔P7: run machinery ↔ shim ↔ per-format invocation.

## 11. Open questions — all three resolved 2026-09-17

- **Resolved, decided with Gordon: the `filename` header content loss for docx/pptx is
  an accepted upstream Q1 limitation, not this epic's scope.** §6's B1 row explains the mechanism:
  Q1's `decoratedcodeblock.lua` has renderers for html/markdown/latex only, and docx/pptx fall
  through to the default renderer, which never reads `node.filename`. Q2's job is only to arrive
  at the shim boundary with correct data, which it already does with no bridge needed (see §6);
  a missing Q1-side renderer for one specific format pair is Q1's gap to fix, not something this
  epic should work around (e.g. by duplicating `decoratedcodeblock.lua`'s rendering logic
  Rust-side or patching the vendored Lua beyond the already-scoped crossref patch). Filed upstream:
  [quarto-dev/quarto-cli#14906](https://github.com/quarto-dev/quarto-cli/issues/14906). No plan
  needs a checklist item for this; if upstream fixes it, the fix arrives for free on the next
  vendored-source re-pin.
- **Confirmed a real, standalone gap and filed as a braid strand, not epic scope: `bd-5aklrxgi`,
  "Q2 has no number-sections / @sec- header-numbering implementation for any format" (run
  `braid show bd-5aklrxgi` for the full writeup).** Deeper investigation than the initial grep: Q2's
  `crossref_index.rs` does track a section-counter stack (`advance_sections`, driven by `Header`
  blocks), but its own module doc says this exists only to compute *compound numbers for other
  crossref targets* ("Figure 1.2.3") — `visit_header` never registers the header itself as an
  indexed target and never injects a visible number into it. `crossref_resolve.rs` has no
  handling at all for a `sec`-typed ref or for the `HeadingRecord` list `crossref_index.rs` does
  populate. This is a pre-existing, general Q2 gap (missing for HTML too, not just Pandoc
  targets) — out of this epic's scope to fix, per this repo's beads/strand policy. The epic's own
  risk remains real though: under `crossref-numbering: external`, Q1's own `sections.lua`
  (P3's audit, §7) is suppressed alongside the rest of `quarto_crossref_filters`, so `--to docx`
  with `number-sections: true` will silently lose section numbers until bd-5aklrxgi lands. No
  plan checklist item needed here; reference the strand if this surfaces in a golden diff.
  **Decided 2026-09-18, Gordon: do not forward `number-sections`/`number-offset` to pandoc's own
  defaults as a workaround.** A user-facing/backward-compat review found Q1 itself *deletes*
  these two keys from the pandoc defaults file specifically because its own Lua (`sections.lua`)
  handles numbering instead (`quarto-cli/src/command/render/pandoc.ts:1049-1062`, verbatim: "we
  are handling some things on behalf of pandoc"). Forwarding them under external mode would
  produce a **third** behavior matching neither Q1 nor "no numbers": pandoc's own numbering has
  no `number-depth` equivalent (ignores it, numbers every level) and a different level-1
  delimiter (a space, not Q1's `". "`). That's a new, un-audited behavior, not a wiring choice —
  out of scope. P4's `crossrefFilterParams`-derived `number-sections`/`number-offset`/
  `number-depth` params stay in the "required" list for API completeness (Q1's own
  `quartoFilterParams` always includes them) but are inert under external mode since the only Lua
  that reads them never runs; P4 should say so rather than implying the params make the feature
  work.
- **Resolved with a concrete proposal, see P7.** Golden captures reuse quarto-cli's own test
  fixtures, are produced by a dev-only `cargo xtask capture-pandoc-goldens` against a real
  pinned-release `quarto` binary, extract semantic text (not raw XML) to avoid Pandoc-version
  byte-noise, and are committed as **insta snapshots** — reusing this repo's existing snapshot
  infrastructure for both the Q1-parity check and Q2's own regression test in one assertion.
  Full detail in P7's "In scope" section.

## 12. Known limitations for v1 (added 2026-09-18)

Three fresh reviews (two build-order/implementation-feasibility simulations, one user-facing/
backward-compatibility audit against real Q1 documents and test fixtures) found real gaps this
epic does not close. **Governing principle, decided with Gordon: this epic wires up existing
Pandoc + vendored-Q1-Lua behavior; it does not implement new Q2 functionality to close gaps
between Q2's own HTML renderer and what Q1's Lua/TS already does.** Where the Pandoc leg ends up
*ahead* of Q2's native HTML leg on some feature, that is an accepted asymmetry for v1, not a bug
this epic must fix — but it must be listed here so nobody discovers it by surprise, and a strand
exists so the gap has continuity if someone picks it up later.

- **Crossref presentation options** (`crossref.title-delim`, `fig-prefix`/`tbl-prefix`/etc.,
  `ref-hyperlink`, `labels`, `chapters`-scoped numbering) are honored by the Pandoc leg (it's
  Q1's own Lua) but ignored by Q2's native HTML renderer, which hard-codes English defaults
  (`crossref_render.rs:28-31`). Same source document, different reader-visible caption/ref text
  per format. **This means the epic's crossref-identity promise is about *numbers*, not
  *presentation*** (see the Definition of done correction in the epic doc). Tracked as
  `bd-wqdi1pd2`. **Sharpened 2026-09-18 (round 4 review, Reviewer C): this is not just "different
  wording" — it's silently wrong for a user who is explicit.** Verified zero Q2 readers of
  `crossref.title-delim`/`fig-prefix`/`ref-hyperlink` anywhere in `crates/` (only
  `crossref_render.rs:29`'s own comment admits the gap), while on the Q1 side `crossrefOption()`
  reads the user's own metadata **ahead of** any param default. So a document with an *explicit*
  `crossref: {fig-prefix: "Abb.", title-delim: " —"}` override renders "Abb. 1 — caption" in docx
  and "Figure 1: caption" in HTML from the same source, **with no diagnostic in either leg** —
  the divergence widens exactly as the user becomes more explicit about what they want. Not being
  fixed (the governing principle and `bd-wqdi1pd2` are correct); recorded here so this bullet
  states the actual risk rather than reading as a cosmetic default-string mismatch.
- **Mermaid diagrams have no rendering story for docx/pptx.** Q1 renders them via Puppeteer/
  headless Chromium in TypeScript, not Lua — there is no vendored Lua counterpart to reuse. A
  ` ```mermaid ` block survives to the Pandoc leg as a plain `CodeBlock`, rendering as visible
  diagram *source text* in the output document. Tracked as `bd-h1ub8f8z`. No warning ships in
  v1. **Sharpened 2026-09-18 (round 4 review, Reviewers C and D): "revisit if the silent case
  proves confusing in practice" currently has no mechanism by which anyone would find out.** The
  self-gate (`mermaid.rs:175`), the Pandoc-kind exclude-list, and P7's golden-harness fixture
  selection (which now deliberately excludes mermaid fixtures) each independently make this case
  invisible — no transform runs, no warning is emitted, and no test would ever move. This repo has
  independently converged, three times recently, on treating "accepted-but-inert" as deserving a
  signal rather than silence (`Q-5-18`'s `project: type: book` warning; `warn_aliases_ignored`;
  the `website.llms-txt` warning, whose own code comment states the policy outright: "an
  accepted-but-inert key deserves a signal, not silence"). P7 now captures one mermaid fixture as
  a labeled accepted-divergence golden (see P7's corrected Finding 4) so the gap is at least a
  committed, reviewable artifact rather than purely a design-doc sentence; a runtime warning
  remains deferred, per the original decision.
- **A user Lua filter resolving to a `Post` entry point cannot see or traverse into any
  `CustomNode`'s content for a Pandoc-target render** (Callout, Tabset, Theorem, Proof,
  FloatRefTarget are all still standing at that point, unlike native Q1 or Q2's own HTML path,
  where nothing custom remains by then). A filter that would normally transform text anywhere in
  the document silently skips everything inside those constructs. The **default** `Pre`-position
  case (`filters: [f.lua]`, no `quarto` marker — the overwhelmingly common shape) is unaffected —
  verified independently in round 4 review (Reviewer C): `UserFiltersStage::pre()` runs
  immediately before `AstTransformsStage`, i.e. before any sugar transform creates a CustomNode,
  so a `Pre` filter sees the same raw Divs an HTML render would. Tracked as `bd-o90yz5mg`.
- **The docx/pptx code-block `filename` header is dropped** (accepted upstream Q1 limitation,
  filed as [quarto-dev/quarto-cli#14906](https://github.com/quarto-dev/quarto-cli/issues/14906)
  — see §6's B1 row).
- **Section numbering (`number-sections`, `@sec-` refs) is unsupported for the Pandoc leg** until
  `bd-5aklrxgi` lands (a general, pre-existing Q2 gap, not new to this epic — see §11).
- **Listing pages render with only their prose body** (`listing-generate`/`listing-render` are
  excluded for the Pandoc leg) — near-parity with Q1, which also doesn't produce real docx/pptx
  listings, but worth stating since "my listing page came out blank" is a plausible report.
- **Project-mode rendering (website/book/manuscript) to a Pandoc target is out of scope for v1
  — see §13, now with a containment gate landed in P7 (decided with Gordon, 2026-09-18) rather
  than left to a separate, unblocked strand.**
- **A callout whose crossref category Q2 knows and Q1 doesn't** (a Q2-only category, or a
  `crossref.ids` Promised prefix) **renders unnumbered, and any reference to it will cite a
  number that appears nowhere in the document** (added 2026-09-18, round 4 review, Reviewer D —
  P5's Callout `fail()`-fallback deliberately avoids crashing the whole render for this case, at
  the cost of the number; a warning is emitted at the fallback so this doesn't happen silently,
  but the underlying gap — Q1 has no concept of a Q2-only ref-type category — is not fixed and
  is not fixable within this epic's scope, per P6 Finding 2).
- **`content-visible` / `content-hidden` with `when-format`/`unless-format` works for a bare
  Pandoc writer name, but is not promised or documented for v1** (added 2026-09-18, P8 companion
  Finding F3). §8 defers the feature to P8 and calls it out of scope for docx/pptx v1; P8's own
  tests then confirm it *does* work for a plain `docx`/`pptx`/`latex` target, because
  `ConditionalContentTransform` runs pre-cut and format-parameterized. Both statements are true and
  neither is user-facing, so this bullet is the reconciliation: **the behavior is verified, the
  support is not claimed.** Do not draft release notes that promise it. It is additionally *broken*
  for an extension format (`acm-docx` and friends) — a pre-existing, non-Pandoc-specific gap
  tracked as `bd-n5hjadpr`, not fixed here.

## 13. Project-mode Pandoc rendering — preparing without implementing (added 2026-09-18)

Decided with Gordon: **Q2 has no book-project support at all yet** (independent of this epic —
see `Q-5-18`'s existing disclaimer, "Quarto 2 does not implement book projects yet"), and this
epic will not add project-mode Pandoc rendering as new functionality. But the epic should *not*
close off the future work, and should fill in the part of the design that's legitimately ours to
define now, even though nobody is implementing it yet.

**Why this is lower-urgency than it first looked.** A user-facing review flagged website-project
`--to docx` as producing bogus sitemap/redirect artifacts. Investigation (`bd-bgeet2mw`) found
this reads worse than reality: Q1's own `websiteProjectType.postRender` already filters its
`outputFiles` to HTML-only before running any sitemap/alias/feed logic — it is structurally
incapable of pointing a sitemap at a docx file, because non-HTML outputs never reach that code.
Q2's equivalent (`WebsiteProjectType::post_render`) has no such filter — a **pre-existing,
general Q2 project-orchestration gap** (not introduced by this epic; just newly reachable once
docx is a supported target), filed as its own strand, unrelated to any Pandoc-specific plan.

**Also worth noting: docx/pptx are not the natural book output formats even in Q1.** Q1's
"premium" book formats are **typst and latex/PDF** (this epic's own Tier-2, already-deferred
targets) — `docx: extensions: book: selfContainedOutput: true` exists but produces N separate
per-chapter `.docx` files by default, each restarting figure/table/theorem numbering at 1, no
merged book, no `part:` structure, no shared bibliography. So "book support for the Pandoc
hybrid" is really "book support for typst/latex," a later epic's problem, not this one's.

**What's ours to prepare now, without building it:**
- **v1 stance:** until `bd-bgeet2mw` lands, `q2 render <project> --to docx|pptx` for a
  website/book/manuscript project type should not silently proceed into project post-processing
  that assumes HTML output exists. The minimal containment (not full project-mode support): gate
  `WebsiteProjectType::post_render`'s hook sequence on `format.identifier.is_html_based()`, the
  same one-line fix `bd-bgeet2mw` already describes — this is squarely "match Q1's own
  established behavior," not new functionality, and removes the actual risk (bogus sitemap/
  redirect artifacts) without requiring anyone to design project-mode Pandoc rendering. **Whether
  to land that one-line gate as part of this epic (since it's this epic that makes it reachable)
  or wait for `bd-bgeet2mw` is Gordon's call — recorded here, not decided.**
- **The forward-compatible shape:** when book/website Pandoc rendering is eventually designed,
  it is a per-format-tail concern (P7's layer), not a neutral-core one — nothing in P1's
  `PipelineProfile`/exclude-list work needs to anticipate it, since `Pandoc(fmt)` already skips
  Navigation wholesale and the "project renders one document per chapter" shape Q1 already uses
  for docx generalizes cleanly to whatever P7-equivalent work happens later. No design debt to
  pay down now.

## 14. Multi-format render guardrail (added 2026-09-18)

A user-facing review found that relaxing `render.rs:680-684`'s format check (P7's own checklist
item, needed to admit docx/pptx at all) removes the *only* existing signal that Q2 renders one
format per invocation. Today, `format: {docx: default, html: default}` fails loudly ("Format
'docx' is not yet supported") — a side effect of the refusal, not its purpose, but a real
guardrail nonetheless. After P7's relaxation, the same document would silently render only
`mydoc.docx`, dropping the `html:` entry with no diagnostic.

**Decided with Gordon's framing (no new functionality, but this is a regression P7's own change
introduces, not a pre-existing gap):** P7 must add a **warning**, not new rendering capability —
when `format:` declares more than one key and only one is rendered, name which was used and
which were skipped. This restores the pre-existing signal at the cost of one diagnostic, not a
feature. See P7's checklist.

## Amendment log

Full history of corrections to this document, most recent first. Kept separate from the header
so the frozen decision text above stays readable while the correction trail remains traceable.

- **2026-09-18, test-seam prevalidation pass (P8 companion, Finding F3): added the
  content-hidden reconciliation bullet to §12.** §8 defers `content-visible`/`content-hidden`
  `when-format` gating to P8 and calls it out of scope for docx/pptx v1, while P8's companion then
  verified by direct read that it *does* work for a bare Pandoc writer name — two true statements
  with nothing user-facing adjudicating between them, so either reading was defensible from the
  artifacts and a release note could have gone both ways. **Decided with Gordon: one §12 bullet,
  stating the behavior is verified but the support is not claimed** — chosen over the Definition-of-
  done paragraph or the docs site, since §12 is where this epic's limitations already live and the
  DoD paragraph already draws from it. No code, scope or test change; §8's deferral and P8's tests
  both stand exactly as they were. The same bullet also records that the extension-format case
  (`acm-docx`) is genuinely broken rather than merely unclaimed, pointing at `bd-n5hjadpr`.
- **2026-09-18, test-seam prevalidation pass (P1 companion, Finding F6): closed §6's last
  question-marked bucket cell.** `LinkRewrite` read `**B3?**` while P1's own "Produces for P7"
  section already asserted "ResourceCollector, LinkRewrite, Appendix — all confirmed in, not
  conditional," and no checklist item owned reconciling the two. **Decided with Gordon: confirmed
  `B3`**, unconditionally part of the B3 shared-services segment. Supporting evidence, same
  argument that settled `AppendixStructure` in the first 2026-09-17 pass: `link-rewrite` is
  `TransformPhase::Finalization`, runs at `pipeline.rs:1465` immediately before
  `appendix-structure` at `:1466`, and is already unexcluded for q2-preview's non-HTML consumer
  today. This is the only edit to a frozen §6 cell made during the prevalidation pass, and it was
  taken with Gordon's explicit sign-off because a `?` in a frozen section reads as an open
  decision rather than a typo. **Also decided in the same pass, with no design-doc consequence:**
  the Footnotes §6 SPLIT row is implemented as **two registered transforms** (`footnotes` /
  `footnotes-resolve`, F1) — the row's existing prose is accurate for that shape and was
  deliberately left untouched; and §6's total-bucket-coverage guard is a Rust `#[test]` over a
  `const BUCKETS` in `pipeline.rs` rather than a lint rule over this table (F3), **which makes
  this table advisory documentation of intent, with no mechanical guard of its own** — the
  accepted cost of that decision, recorded here so a future reader does not mistake the table for
  the enforced artifact.
- **2026-09-18, round 4 (four dispatched Opus reviewers — ripple/bookkeeping audit,
  Q1-source verification, error-taxonomy/scope re-litigation, systemic ripple):** verified five
  Critical findings directly against real code before acting on them (`pptx` unresolvable at
  `Format::from_format_string`; `plain_data.order` missing from P2's schema; the
  `crossref-<type>-prefix`/`-title` param split; the Callout `fail()`-guard's wrong predicate; the
  pandoc-version mismatch between the vendored Lua's implied floor (3.10) and this repo's CI
  (3.8.3)/dev-setup (3.6)) — all confirmed accurate, fixed across P1–P7. **Landed Gordon's decision
  on the project-mode gate** (§13): P7 now gates `WebsiteProjectType::post_render` on
  `is_html_based()` directly, rather than leaving it to the separate `bd-bgeet2mw` strand. Sharpened
  two §12 bullets (crossref-presentation-options and mermaid are both silently-wrong-for-an-
  explicit-user, not merely differently-worded, and mermaid's golden-harness exclusion was found
  to be circular — three independent justifications each citing the other two); added a new §12
  bullet for the Callout unregistered-category fallback's dangling-reference consequence. All
  fixes are wiring/specification corrections to work already in scope, not new functionality —
  full detail in each affected P-plan's own commit history.
- **2026-09-18, round 3: three more Opus reviews (two implementation-feasibility/build-order simulations
  for P1-P4 and P5-P8; one user-facing/backward-compatibility audit against real Q1 documents and
  test fixtures) surfaced 13 Critical + ~29 Important findings — genuinely new failure modes, not
  drift.** Resolved per Gordon's governing framing: this epic wires up existing behavior, it does
  not implement new Q2 functionality. Added §12 (known limitations), §13 (project-mode — prepare,
  don't implement), §14 (multi-format guardrail); corrected the number-sections decision (§11,
  don't forward to pandoc defaults — would produce a third, unaudited behavior); filed five
  strands for accepted-but-tracked gaps (`bd-wqdi1pd2` crossref presentation options,
  `bd-h1ub8f8z` mermaid, `bd-bgeet2mw` website post-render format gate, `bd-mamqirfz`
  `contributes.format` alias, `bd-o90yz5mg` Lua-bridge custom-node walkability). Remaining
  findings (shim loading mechanism, `PipelineProfile` seam, self-gating transforms, binary output
  contract, error-catalog subsystem, `--reference-doc` forwarding, format-specific `execute`
  defaults, stage-level exclude list) are wiring/specification gaps, not new-feature questions —
  resolved directly in the affected P-plans, see their own commit history.
- **2026-09-17, third pass (two dispatched Opus reviewers — architecture/cross-document
  consistency, independent code-grounding):** found this doc still contained the epic's last two
  Critical defects. §4 Problem C still specified the category-registry "seed" mechanism P6
  Finding 1 disproved — rewritten to state the real finding (display-name override only, via
  `crossref-<type>-title`, owned by P4); retitled P6's §9 entry and P6's own H1 to "Category
  passthrough + numbering suppression." §6's B1 row asserted "Q1 re-derives from attrs" for
  code-block decorations while P2 and P5 separately asserted the opposite (a bridge is needed),
  with neither reading verified — resolved by reading `decoratedcodeblock.lua`/
  `code-filename.lua`/`foldcode.lua` directly: no bridge is needed for either currently-
  implemented decoration (two independent mechanisms converge on the same `filename` CodeBlock
  attribute; `copy` is HTML/JS-only), but this surfaced a real, separate content-loss gap (no
  docx/pptx renderer for the `filename` header) — recorded as an open question above, not
  silently dropped. Also: rewrote §3's Route-decision procedure (the old one-line rule didn't
  reproduce its own table's Tabset/Callout rows — replaced with a three-step procedure); added an
  inline staleness marker to §5's four-variant `PipelineProfile` shorthand; added four missing
  rows to §6's bucket table (the `panel-tabset` pair, `AttributionRender`/`AttributionGenerate`,
  `breadcrumbs-render`/`quarto-nav-js`/`repo-actions-render`, `ConditionalContentTransform`) with
  a note that this table's coverage is not yet asserted as total; corrected §9's dependency graph
  (the prior "P1–P5 parallelizable" line was self-contradicted by this same document's own Plans
  table); moved this changelog out of the file header per a reviewer's own recommendation about
  the header becoming unreadable.
- **2026-09-17, second pass (pan-epic adversarial re-audit):** found the §3 Route table itself
  hadn't fully caught up with its own header's claims: the `DecoratedCodeBlock` row (flagged for
  removal by P2, never actually dropped) and a stale `ExampleEmbed` Route-N row (the header prose
  already said this was "removed... entirely," but the table row survived a later edit pass
  untouched) — both fixed, with an explanatory note added below the table. Also fixed §9's
  plan-decomposition summary, which still said "P5 — Lua shim (Route L + R)" (no Route-L type
  remains in the inventory) and "P6 depends on P3" (omitting P5, already correctly listed in the
  epic doc's own dependency graph and Plans table).
- **2026-09-17, first pass:** landed two Route-L/R reclassifications (§3 table) decided during
  the P1–P8 deep-pass review: `Tabset` and `Callout` both move L→R (both carry real numbering
  that Route L would discard; see P5 and P6 for the evidence). Also resolved §6's last open cell:
  `AppendixStructure` classified **B3** (see P1). Reclassified `ExampleEmbedRender` from B4 to
  format-parameterized B1 (decided with Gordon, P5): its iframe is already a
  `RawBlock("html", ...)`, which non-HTML writers drop natively — the same node's snippet/caption
  content is portable and now emitted for Pandoc directly in Rust, removing `ExampleEmbed` from
  P5's Route N scope entirely.
- **2026-08-20:** initial freeze, decomposed into P1–P8.
