# P5 — Lua shim: wire format → Q1 nodes (Route R + N)

**Date:** 2026-08-20  **Updated:** 2026-09-18 (two passes) — see `git log --oneline -- claude-notes/plans/2026-08-20-pandoc-hybrid-P5-lua-shim.md`
for the full correction history (moved out of this header once it grew past readability, same
pattern as the design doc's "Amendment log"). Latest (round 4 review, verified directly against
real Q1 Lua): the Callout `fail()`-fallback guard tested the wrong predicate
(`valid_ref_types()`, a superset) and would not have prevented the abort it was written to
prevent — fixed to `crossref.categories.by_ref_type[ref_type] ~= nil`, and the fallback now emits
a warning (it was independently found to re-create the exact silent number loss Callout was
reclassified to Route R to avoid). The "wire wrapper isn't shaped like anything Q1 recognizes"
reasoning was wrong — the wrapper *does* retain its original semantic classes, and three Q1
handlers key on them; the shim's splice position (not the wrapper's shape) is what actually averts
the collision. Extended Route N's function list (`refNumberOption`/`subrefNumber`/`refDelim`/
`nbspString` were missing — the original three functions could produce a prefix but not a
number) and corrected the `equations.lua` branch attribution (the branch that reads `order` is the
non-LaTeX/Typst fallback, not LaTeX). Added the shim's traversal-direction contract (bottom-up) and
a named Q2-only error-path test tier P7's Q1-parity harness cannot cover by construction. (Prior
pass, same day: an implementation-feasibility review found the shim had no specified loading
mechanism (resolved in P4), Route N was "mirror `refs.lua`" with no worked example or
normative-source decision (resolved: call Q1's own functions directly), no error handling anywhere
(three concrete cases resolved), and the order-assignment snippet mis-described
`quarto.<AstName>()`'s actual two-return-value API. All four of this plan's originally-flagged
open design questions were already closed as of that pass.)
**Status:** Shape draft
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`  |  Depends on: P2, P4
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P5-implementation.md`](2026-09-18-pandoc-hybrid-P5-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Goal
The one vendored-adjacent **Lua shim** that decodes the custom-node wire format into Q1 nodes,
so Q1's render handlers run. Colocated with the vendored `customnodes/*.lua` (per-type knowledge
lives in Lua, tracking Q1 upstream).

## The real type inventory (8 types — same as P2, verified against Q1's actual `ast_name` census)

Grepped every `ast_name = "..."` in `/Users/gordon/src/quarto-cli/src/resources/filters/customnodes/`
(13 total: `Theorem, Shortcode, LatexEnvironment, PanelLayout, DecoratedCodeBlock,
FloatRefTarget, Proof, HtmlTag, LatexInlineCommand, LatexBlockCommand, ConditionalBlock,
Callout, Tabset`) and cross-referenced against Q2's 8 real wire types (P2). **Five** of the eight
(corrected 2026-09-17 from "Four" — the table below always showed five: Callout, Tabset,
Equation, ExampleEmbed, CrossrefResolvedRef; found by an epic-wide review, M2)
don't fit the L/R dichotomy as cleanly as the frozen table assumes:

| Q2 `type_name` | Q1 `ast_name`? | Route (corrected) | Why |
|---|---|---|---|
| `Callout` | `Callout` | **R, not L** (reclassified — see P6 Finding 4, decided with Gordon 2026-09-17) | a Callout with a crossref-eligible id gets `plain_data.order` from the same shared indexer as FloatRefTarget/Theorem; Route L's raw-Div reconstruction would discard it and Q1's own numbering pass is suppressed under external mode, silently dropping the "Note N:" prefix |
| `Tabset` | `Tabset` | **R, not L** (decided — see finding below) | wire shape matches Q1's *constructor*, not its `parse`; field-map audited clean |
| `FloatRefTarget` | `FloatRefTarget` | **R** | numbered; unchanged, confirmed mechanical (audited below) |
| `Theorem` | `Theorem` | **R** | numbered; unchanged, field-map detailed below |
| `Proof` | `Proof` | **R** | numbered; unchanged, but field-map has a real gap (audited below) |
| `Equation` | *(none — no Q1 `ast_name`)* | **N, not R** (resolved — see finding below) | Q1 has no custom node for equations at all; plain filter + format-specific `RawInline` injection on the existing `Math` inline |
| `ExampleEmbed` | *(none)* | **resolved upstream, not this shim's problem** | Decided with Gordon (2026-09-17, see P1): `example-embed-render` reclassified B4→format-parameterized B1 — it emits snippet+numbered-caption content (portable Pandoc blocks) for `Pandoc(fmt)` and skips only its `RawBlock("html")` iframe. `ExampleEmbed` never reaches this shim as a raw `CustomNode` for Pandoc targets at all; no Route N handling needed here. |
| `CrossrefResolvedRef` | *(none)* | **N — no Q1 handler exists**; resolve directly, mirroring `refs.lua` | see finding below |

`DecoratedCodeBlock` is a real Q1 `ast_name` (`decoratedcodeblock.lua`, confirmed
`ported`/sideband in the filter catalog) — but it is **not part of the wire format at all**
(P2 finding: Q2 carries code-block decorations in a sideband map, never a CustomNode). **This
shim never sees a `DecoratedCodeBlock` wire node to route**, L or R — whatever bridges Q1's
`decoratedcodeblock.lua` to Q2's `code_block_decorations` sideband is a *different* mechanism,
owned by P7's invocation builder, not this plan.

## Finding: Tabset's frozen Route-L classification doesn't fit the current wire shape

Read `panel-tabset.lua` directly. It exposes **two** independent entry points, and they want very
different shapes:

- `parse(div)` → `parse_tabset_contents(div)`: expects a **raw Div with content interleaved as
  `Header, ...body..., Header, ...body...`** at a consistent heading level, and derives each
  tab's `active` flag from whether that Header's own `attr.classes` includes `"active"`. This is
  what Route L would reconstruct.
- `constructor(params)`: takes `params.tabs` — a list of **already-split** `{title, content,
  active}` triples — plus `params.level`/`params.attr`. This is Route R's shape.

Q2's actual wire format for `Tabset` is produced by the **sugar** transform,
`panel_tabset.rs:271-287` (`plain_data = {level, tab_count, actives[, group]}` + slots
`title-{i}` (Inlines) / `content-{i}` (Blocks) per tab) — **already split into per-tab
title/content/active**, i.e. **structurally the constructor's shape, not the raw-Div `parse()`
shape**. Reconstructing a raw Div for Route L would mean synthesizing `Header` blocks Q2 no
longer has (title is `Inlines`, not a `Header`) and faking the `active` class onto the right one
— throwing away structure the wire format already carries cleanly, only to have Q1's `parse`
re-derive it.

**This is the opposite of "presentation-only, therefore trivially Route L."** The frozen design
doc §3 table lists `PanelTabset` (sic — see P2's naming correction) as Route L; the evidence
above says Route R is the better fit for the *current* wire shape.

**Decided (confirmed with Gordon): reclassify `Tabset` as Route R.** This reverses the frozen
design doc §3 table and needs to land there in the same pass as P2's naming fix
(`PanelTabset`→`Tabset`) and its `DecoratedCodeBlock`/`Equation` corrections. **Landed** in the
design doc §3 table on 2026-09-17 (commit `8e9e546b4`), alongside Callout's reclassification
found independently in P6.

**Field-map audited, clean.** `panel-tabset.lua`'s `constructor(params)` takes `params.level`,
`params.attr`, and `params.tabs` (a list built by `quarto.Tab({content, title, active})`). Q2's
wire fields zip into that directly: `level`→`params.level`, `attr` (from the `CustomNode`'s own
attr)→`params.attr`, and `actives[i]`/`slots.title-{i}`/`slots.content-{i}`→one
`quarto.Tab(...)` call per tab. No missing field. The extra `plain_data.group` Q2 sometimes sets
is an HTML-only grouped-tab-sync signal (`TabsetsJsStage`) with no Q1 counterpart — harmless
unused data for the shim, not a gap to request from P2.

**Correction to this plan's own prior pass:** the type inventory table above previously implied
`Tabset` reaches q2-preview and falls to `Fallback` like `ExampleEmbed`. Checking
`Q2_PREVIEW_TRANSFORM_EXCLUDED` (`pipeline.rs:1594-1642`) directly shows **both halves of the
tabset pair — `"panel-tabset"` (the sugar transform itself, not just its resolve step) and
`"panel-tabset-resolve"` — are excluded from preview.** So a `Tabset` CustomNode is never even
*created* for q2-preview; the `.panel-tabset` Div stays raw and renders as plain stacked
headings. (Checking further, `ExampleEmbed`'s own "falls to Fallback" claim turned out wrong
too, for a different reason: at the time this was written, `example-embed-render` wasn't
excluded and destroyed the CustomNode before serialization; see P2's correction. **That's now
moot** — as of 2026-09-17, `example-embed-render` is deliberately kept running for `Pandoc(fmt)`
too, format-parameterized to skip only its iframe; see the updated `ExampleEmbed` row above.)
This doesn't change the Tabset Route decision — the wire shape audited above is what the
**Pandoc** tail's cut carries once P1's `Pandoc`-kind profile runs the sugar transforms but
excludes the render/resolve ones that would destroy them.

**Resolved with P1** (2026-09-16 rework, updated 2026-09-17): P1 has a concrete, name-verified
`Pandoc`-kind exclude-list derived from the design doc's §6 bucket table, covering
`panel-tabset-resolve`, `callout-resolve`, and the rest of B2/B4/Navigation — while explicitly
keeping `panel-tabset` (the sugar half) enabled, which the shape audit above depends on.
**`example-embed-render` is no longer on this exclude list** (reclassified B1, format-
parameterized, decided with Gordon — see the `ExampleEmbed` row above). See P1 for the full
list; not re-derived here.

## Finding: Equation has no Q1 handler either — Route N, not R

Read `crossref/equations.lua` directly. It has **no `_quarto.ast.add_handler`, no `ast_name`, no
custom node at all** — it's a plain Pandoc filter (`Para = process_equations, Plain =
process_equations`) that scans block content for `DisplayMath` inlines and does **format-specific
`RawInline` injection directly on the existing `Math` inline**: LaTeX gets
`\begin{equation}...\label{}...\end{equation}` wrapping (native LaTeX numbering, no literal
text), Typst gets `#math.equation(numbering: equation-numbering)`. There is no constructor to
call and nothing to reconstruct into — the previous "R, proposed" note in this plan (and in P2)
was wrong to assume Equation would pattern-match Theorem/Proof/FloatRefTarget just because it's
numbered; those three all have a real Q1 `ast_name`, Equation does not.

**Resolved: `Equation` is Route N.** The shim should unwrap the `Equation` CustomNode straight
back to a plain `Span`/`Math` inline and apply the same format-specific numbering treatment Q1's
`process_equations` does, rather than attempting any Route-R constructor call.

## Finding: one type has no Q1 handler at all — Route L/R is not exhaustive

`CrossrefResolvedRef` appears nowhere in Q1's 13 `ast_name`s. Not a gap in Q1 that got missed —
it's Q2-native:

- **`CrossrefResolvedRef`** is the resolved state of an `@ref` citation (`plain_data:
  {identifier, ref_type, kind, resolved, kind_source, order?}`, per
  `crossref_resolve.rs:296-320`). Q1's own model for this is `refs.lua`, which resolves a `Cite`
  **directly into final Inlines (a `Link`/`Str`) in place** — Q1 never persists a scaffold/custom
  node for a resolved ref at all. **Recommendation: the shim resolves `CrossrefResolvedRef`
  directly to a plain Pandoc inline (mirroring `refs.lua`'s own behavior), not via a
  nonexistent Q1 constructor.** Simpler than either Route L or R — no scaffold round-trip needed.

**`ExampleEmbed` no longer belongs in this finding — resolved 2026-09-17, out of this plan's
scope entirely (decided with Gordon, see P1).** It also has no Q1 `ast_name` (correcting this
plan's own earlier "zero Q1 precedent" framing: Q1's own docs *do* show the identical reading
experience — quarto-web's `docs/presentations/revealjs/index.qmd:13` is a hand-typed
`<iframe class="slide-deck" src="demo/">` — but that's authors hand-writing raw HTML with no
Quarto mechanism behind it at all, confirmed by grepping the whole `quarto-web` repo for any
supporting filter/shortcode; none exists). Rather than a bespoke Route-N design here, P1
reclassified `example-embed-render` (B4→format-parameterized B1): it already builds the iframe
as `Block::RawBlock(format: "html", ...)` — the exact same primitive Q1's hand-authored raw HTML
would produce, which non-HTML writers already drop by convention — and its `snippet`/caption
content is plain portable Pandoc blocks. So it now runs for `Pandoc(fmt)` too, emitting
everything except the iframe, entirely in Rust, upstream of the wire-format cut.
`ExampleEmbed` never reaches this shim as a raw `CustomNode` for Pandoc targets.

`CrossrefResolvedRef` needs a **third route** in the design doc's vocabulary (not "L" or "R" — call it
**Route N**, "no Q1 handler; resolve or render directly").

## Finding: the shim's loading mechanism is P4's, not this plan's — and it resolves what "mirror `refs.lua`" should mean for Route N (2026-09-18)

An implementation-feasibility review found this plan never specified *how* or *where* the shim
gets loaded into Q1's Lua interpreter — a real blocker, since Route R needs `quarto.Callout`/
`quarto.Theorem`, which only exist inside `main.lua`'s own Lua state (a separate
`pandoc --lua-filter` pass is a separate interpreter and cannot reach them). **Resolved in P4**
(which owns vendoring `main.lua` in the first place): a small, marked patch inserts the shim's
filter group into `quarto_filter_list` between `quarto_init_filters` and
`quarto_normalize_filters`, in the same spirit as P3's existing 3-file crossref patch. See P4's
"Finding — five wiring gaps closed" for the full mechanism and position rationale.

**This closes Route N's central open question too: since the shim runs inside `main.lua`'s own
Lua state, "mirror `refs.lua`"/"mirror `equations.lua`" means *call Q1's own functions*, not
reimplement their logic.** Concretely:

- **`CrossrefResolvedRef`.** Q1's `resolveRefs()` (`crossref/refs.lua:8-145`, corrected 2026-09-18
  from `8-130`) is a `{Cite = ...}`
  filter callback, not a general-purpose "resolve one ref" function — the shim can't call it
  directly as a helper. But `refs.lua`'s internal building blocks (`refPrefix()`, `crossrefOption`,
  `refHyperlink()`) **are** plain callable globals inside `main.lua`'s state, and this is the
  version to call: it's cite-mode aware (`cite.prefix`, `SuppressAuthor` → `ref-noprefix`), only
  emits a `Link` `if refHyperlink()`, and uses class `quarto-xref` — genuinely different from
  Q2's own `crossref_render.rs:1114-1193` (always a `Link`, `"{kind}\u{a0}{n}"` text, no
  SuppressAuthor handling at all). **Decided, consistent with "no new functionality" and the
  crossref-presentation-options limitation (design doc §12): Q1's behavior is normative for the
  Pandoc leg** — the shim builds the visible inline by calling Q1's real prefix/hyperlink
  functions with Q2's already-resolved `plain_data` (`identifier, ref_type, kind, order,
  kind_source`), not by porting `crossref_render.rs`'s logic to Lua. This is the same asymmetry
  already accepted for crossref presentation options generally — the Pandoc leg gets Q1's fuller
  behavior for free, Q2's HTML leg doesn't (yet) match it. **Worked example:**
  ```lua
  -- input wire fragment (plain_data): {identifier="fig-x", ref_type="fig", kind="Figure", order={order=1,section={}}}
  -- shim calls Q1's own refPrefix()/crossrefOption() with that data, honoring ref-hyperlink/title-delim
  -- output: a Link (if refHyperlink()) with text "Figure 1" / "fig. 1" per crossref.fig-prefix, class "quarto-xref"
  ```
  An unresolved ref (`kind_source: "promised"` with no real category, or `resolved: false`)
  should degrade the same way Q1's own `resolveRefs` degrades a broken `@ref` — check
  `refs.lua`'s own unresolved-citation path rather than inventing new placeholder text (e.g. Q2's
  own `?id?`/`quarto-unresolved-ref` convention).

  **Correction (2026-09-18, round 4 review, Reviewer B) — the function list and worked example
  above are incomplete: `refPrefix()`/`crossrefOption()`/`refHyperlink()` alone cannot produce
  "Figure 1", only the prefix half of it.** The **number** comes from a fourth function,
  `refNumberOption(type, entry)` (`crossref/format.lua:106-121`, called by `resolveRefs` at
  `refs.lua:112`), which takes an **index-entry-shaped table** — `{parent, order, caption,
  appendix}` (`crossref/index.lua:72-77`), not a raw `order` — and reads `entry.appendix` /
  `entry.order.section` before delegating to `formatNumberOption`. **The shim must synthesize this
  entry shape from `plain_data`** (it already has `order` in the right shape per P2's schema
  correction; `appendix`/`parent` are not currently in `plain_data` and default to
  false/nil for the flat, non-appendix, non-nested case this epic's v1 targets). Three more
  pieces of `resolveRefs`'s body are load-bearing and were previously unmentioned:
  - **`add_ref_prefix`'s nbsp logic must be inlined, not called** — it is a `local function`
    declared *inside* the `Cite` callback (`refs.lua:13-19`), so unlike the four globals above it
    is **not** reachable from a spliced-in filter. Its logic (append the prefix, then insert
    `nbspString()` unless the category sets `space_before_numbering == false` or the target is
    Typst) is six lines but silently wrong if omitted — the output would be missing the
    non-breaking space between "Figure" and "1".
  - **Subfloat refs take a different branch**: when `entry.parent ~= nil`, the text is the
    *parent's* number plus `subrefNumber(entry.order)` in parentheses (`refs.lua:104-110`,
    `subrefNumber` is a global, `format.lua:51-53` — callable, unlike `add_ref_prefix`).
  - **Multi-ref joining** inserts `refDelim()` between citations (`refs.lua:42-45`, also a
    global) — moot for v1 if Q2's own pre-existing multi-id drop bug
    (`crossref_resolve.rs:487`, one `CrossrefResolvedRef` per `@ref` with no multi-id support
    upstream of the cut) means the shim never actually sees more than one ref to join; state that
    explicitly rather than leaving it silently unaddressed.
  **Corrected function list for Route N's `CrossrefResolvedRef` case: `refPrefix`,
  `refNumberOption`, `subrefNumber`, `refHyperlink`, `refDelim`, `crossrefOption`, `nbspString` —
  all reachable plain globals except `add_ref_prefix`'s nbsp logic, which must be reproduced
  inline.**
- **`Equation`.** `renderEquation(eq, label, alt, order)` (`crossref/equations.lua`) **already
  takes `order` as a parameter** — so, exactly like Route R's post-construction `order`
  assignment, the shim's job for Route N is to call this existing function with Q2's already-
  computed order, not reimplement the format-specific `RawInline` injection. **Corrected
  2026-09-18 (round 4 review, Reviewer B) — the branch attribution below was wrong:**
  `crossref/equations.lua:105-113` (`isLatexOutput()`) and `:115-131` (`isTypstOutput()`) both
  **ignore `order` entirely** — LaTeX gets native `\begin{equation}...\end{equation}` numbering,
  Typst gets `#math.equation(numbering:...)`. **The branch that reads `order` and matters for this
  epic's shipping formats is the fallback at `:133-144`** (docx, pptx, and HTML — not LaTeX as
  previously stated), which does `eq.text = eq.text .. " " .. eqQquad(...)`, wrapped in
  `pandoc.Span(eq, Attr(label))`. The "already takes `order`, just call it" argument is sound only
  for this fallback branch — which is fortunately the one docx/pptx actually take — but state this
  precisely so an implementer doesn't expect `order` to matter for the (stubbed) latex target and
  get confused when it doesn't. **If the shim runs inside `main.lua`'s state (per the loading
  mechanism above), this is a direct function call, not a reimplementation** — an implementer
  writing this cold, without knowing `renderEquation` already accepts `order`, would very likely
  reimplement it and drift from Q1 as Q1 evolves.

## Finding: which Q1 filter groups actually see wire-format content, once you check what they match on (2026-09-18)

A review worried that handing the whole AST to `main.lua` means every Q1 filter group
(`quarto_normalize_filters`, `quarto_pre_filters`, layout/post/finalize) runs against content Q2
has already normalized, risking double-processing. Investigated concretely, not just in the
abstract — **most of the worry doesn't survive contact with what these filters actually match
on**:

- **Correction (2026-09-18, round 4 review, Reviewer B) — the claim below is factually wrong; the
  conclusion (no collision) survives for a different reason.** Original claim: "the wire format's
  scaffold Div carries class `__quarto_custom_node` plus `data-custom-{type,slots,data)}`
  attributes — not the semantic markers Q1's own content-detectors key off... so Q1's own
  `parse()`-keyed handlers never fire on the wrapper itself — it isn't shaped like anything they
  recognize." **The wire wrapper *does* retain its original semantic classes.**
  `write_custom_block` (`crates/pampa/src/writers/json.rs:1497-1500`) only *prepends*
  `__quarto_custom_node` to the node's own `attr.1` class list — it doesn't replace it — and
  `CustomNode.attr` is the original Div's attr verbatim (`callout.rs:303`, doc comment at
  `callout.rs:37`: "`attr`: Original Div attributes"). So a Callout's wire Div is *always*
  `["__quarto_custom_node", "callout", "callout-<type>", ...]`, and Q1's class-keyed dispatcher
  (`ast/parse.lua:6-13`, iterates the class list, takes the first match) **does** fire on it: at
  least three registrations collide — `Callout` (`customnodes/callout.lua:36`), `Tabset`
  (`customnodes/panel-tabset.lua:140`), `ConditionalBlock` (`customnodes/content-hidden.lua:27`,
  relevant if any of P8's content-hidden types ever reach the cut). **What actually prevents the
  collision is the shim's position, not the wrapper's shape**: because the shim splices in
  *before* `quarto_normalize_filters` (which is where `parse_extended_nodes`, the class-keyed
  dispatcher, actually runs — `normalize/astpipeline.lua:327-336`), the wire Div has already been
  replaced with a Q1 scaffold — a bare `pandoc.Div({})` with **no classes**, only
  `__quarto_custom*` attributes (`create_custom_node_scaffold`, `customnodes.lua:236-255`) — by
  the time that dispatcher runs, so it finds nothing to match. **This matters for two reasons
  beyond correcting the record:** (1) a future reader who believes "class-keyed handlers can't
  fire on the wrapper" has no reason not to move the shim later (e.g. "normalization is Q2's job,
  put the shim after it") — doing so would silently hand every callout wire Div to
  `Callout.parse()`, rebuilding a Q1 Callout whose externally-assigned `order` is gone, a quiet
  mis-render, not a crash; (2) the error-handling case below (unrecognized `type_name` → "unwrap
  the scaffold to its slot content and drop the wrapper") needs an explicit note that dropping the
  wrapper also discards the retained `.callout-*`-shaped classes — an implementation that
  "unwraps but preserves the wrapper's attributes onto the surviving content" would re-arm this
  exact collision for the unsupported-type path specifically. **Also worth stating as a backstop
  check: there is no params-level lever that disables this risk as a fallback.**
  `active-filters.normalization = false` only gates the single `normalize` filter-list entry
  (`main.lua:263-273`); the four `quarto_ast_pipeline()` groups containing the class-keyed
  dispatcher are appended **unconditionally** (`main.lua:281`), outside that gate — so correct
  shim placement is the *entire* mitigation, not one layer of several.
- **`content-hidden.lua`/`quarto-pre/hidden.lua`:** genuinely harmless, not just "probably fine."
  If `ConditionalContentTransform` already resolved (removed) every `.content-visible`/
  `.content-hidden` Div before the cut, there is nothing with those classes left for Q1's own
  pass to find — it runs, matches zero Divs, no-ops. Not a collision; a no-op over an empty set.
- **`code_filename()` re-deriving a `DecoratedCodeBlock` wrapper:** already resolved, see design
  doc §6's B1 row and the code-block-decorations finding — this is the intended, no-bridge-needed
  mechanism, not a collision. Q2's `CodeBlockGenerate` never wraps the `CodeBlock` itself, so Q1's
  own filter finds a plain `CodeBlock` with a `filename` attribute and wraps it exactly the way it
  would for a hand-authored document.
- **`parse_floatreftargets` (in `quarto_normalize_filters`) is a real hazard, and the shim's
  position resolves it** (**corrected 2026-09-18: "a real hazard," not "the one" — see the
  class-keyed-dispatcher correction above, which independently found at least three more
  collisions the shim's position also resolves**). This pass scans for a captioned image/figure
  shape and wraps it in Q1's *own* `FloatRefTarget` — if it ran on the wire node's still-raw slot
  content *before* the shim converts that wire node into Q1's real `FloatRefTarget` object, Q1
  would double-process and produce an unnumbered duplicate (numbering is external, so Q1's own
  pass wouldn't assign an order). This is exactly why Finding (above) puts the shim **before**
  `quarto_normalize_filters` in the chain: by the time `parse_floatreftargets` runs, the wire node
  has already become a real Q1 `FloatRefTarget` scaffold (a *different* AST shape than an
  unlabeled image+caption), so the detector's own predicate doesn't match it a second time.
  Verified independently in round 4 review (Reviewer B): the real predicate is `parse_float_div`
  (`quarto-pre/parsefiguredivs.lua:215-228`), which requires `refType(div.identifier)` to resolve
  *and* `crossref.categories.by_ref_type[ref].kind == "float"` — Q1's own `FloatRefTarget`
  scaffold is class-less with no identifier, so it does not re-match. Finding confirmed correct.

**No separate "filter-chain manifest" artifact is needed beyond this finding and P4's loading
mechanism** — the risk reduces to "does the shim run early enough," which is already answered.

**New sub-finding (2026-09-18, round 4 review, Reviewer B) — the shim's traversal direction is
unspecified, and nested wire nodes depend on it.** `run_emulated_filter_chain` calls
`run_emulated_filter(doc, v.filter, v.traverser)` (`ast/runemulation.lua:82`) — traversal is a
**per-entry** property of the filter-list entry, not a global setting, and nearly every entry in
`main.lua` sets `traverser = 'jog'` explicitly. Wire nodes nest: a `FloatRefTarget` inside a
`Callout`'s `content` slot serializes as a wire Div inside a slot Div inside a wire Div
(`json.rs:1502-1534` wraps each slot's content in a `data-slot-name` Div). A **bottom-up**
traversal converts the inner node first, so `quarto.Callout{content = ...}` receives an
already-real Q1 scaffold; a topdown traversal would hand the constructor raw, unconverted wire
Divs, which then never get converted, because the shim's own filter group has already passed that
subtree. P6 Finding 5 specifically asks for a Tabset-containing-subfloat fixture, so this case is
known to be in scope. **State the traversal contract explicitly: bottom-up (matching `jog`'s
default direction for this kind of pass), not topdown**, in the shim's filter-group definition —
this is P4's patch item to show concretely (the actual group literal, not just the `tappend`
line), cross-referenced here since it's this plan's shim code that the literal defines.

## Finding: error handling (2026-09-18)

Three concrete failure paths, resolved per the epic-wide `pandoc`-subsystem decision (P4 Finding
4) rather than invented per-plan:

1. **Unrecognized `type_name` reaching the shim** (a future Q2 wire type whose Lua support
   hasn't shipped yet, or a schema-version mismatch): **unwrap the scaffold to its slot content
   and drop the wrapper**, don't pass a `__quarto_custom_node`-classed Div through to the writer
   (a stray one would render as a bare, meaningless Div in the output) and don't hard-fail the
   whole render for one unsupported type. Log a `Q-<pandoc>-*` warning naming the unrecognized
   type.
2. **A Route-R constructor crash** (the already-known nil-`type` Proof crash, `proof.lua:81`, is
   a template for the general case — any required `plain_data` field missing crashes the
   corresponding Q1 constructor with a raw Lua error). This surfaces as the `pandoc`-nonzero-exit
   diagnostic P4 Finding 4 specifies (stderr passthrough) — no separate Lua-side validation layer
   is needed for v1; the crash *is* the signal, just make sure it's visible to the user instead of
   swallowed.
3. **A newly reachable Q1 `fail()` path under Route R:** verified one concretely —
   `callout_title_prefix` (`modules/callouts.lua:6-11`) calls `fail("unknown callout prefix …")`
   when `crossref.categories.by_ref_type[ref_type]` is nil. This is exactly the Promised-id /
   Q2-only-category case P6 Finding 2 already documents as having "no Q1 equivalent at all" — but
   Route R reconstruction makes it reachable as a **hard abort**, not a documented gap. A
   `#nte-foo` callout whose category Q2 knows and Q1 doesn't turns a whole render into a crash.
   **This is not new-functionality territory to fix broadly** (P6 Finding 2 already decided
   Promised ids have no Q1 counterpart, correctly), but the shim should catch this specific case:
   before calling a Route-R constructor for a Callout, check whether its `ref_type` is in Q1's
   `valid_ref_types()` (already read by P6 Finding 1); if not, fall back to Route L-style raw
   reconstruction (no numbering, but no crash either) rather than letting `fail()` abort the
   render. Small, contained, and consistent with "no new functionality" — it's choosing between
   two routes this plan already defines, not building new capability.

   **Critical correction (2026-09-18, round 4 review — found independently by two dispatched
   reviewers): the guard above is specified against the wrong predicate and does not prevent the
   abort it exists to prevent.** `callout_title_prefix`'s actual guard tests
   **`crossref.categories.by_ref_type[refType(id)]`**, not `valid_ref_types()`. Read directly:
   `valid_ref_types()` (`crossref/refs.lua:198-212`) is a strict **superset** of
   `by_ref_type`'s keys — it additionally contributes every theorem-family type
   (`tkeys(theorem_types)`: `thm, lem, cor, prp, cnj, def, exm, exr, alg`) plus the literals `"eq"`
   and `"sec"`. So any `ref_type` in that extra set **passes the guard above and still hits
   `fail()`** — reachable, not theoretical: Q2's classifier splits on the first hyphen with no
   regard for the div's own classes (`crossref/registry.rs:178-181`,
   `classify_cite_id`), so `::: {#thm-foo .callout-note}` gets `ref_type: "thm"` injected into
   `plain_data`, passes the proposed `valid_ref_types()` check, and still crashes at
   `by_ref_type["thm"]` being nil. **Fix: state the guard as
   `crossref.categories.by_ref_type[ref_type] ~= nil`** — the literal condition the `fail()`
   tests — not `valid_ref_types()`. (Theorem's own renderer has the *complementary* requirement,
   indexing `theorem_types[refType(...)]` directly — a Route-R Theorem needs the opposite guard if
   one is ever added there.)

   **Compounding finding (2026-09-18, round 4 review, Reviewer D): even with the corrected guard,
   the fallback re-creates precisely the silent number loss P6 Finding 4 reclassified Callout to
   Route R in order to prevent — and produces a dangling reference, not just a missing prefix.**
   P6 Finding 4's whole argument for moving Callout L→R was that Route L's raw-Div reconstruction
   discards `plain_data.order` and Q1's own assignment pass is suppressed under external mode, so
   "numbered callouts silently lose their 'Note N:' prefix in docx/pptx, with no error to flag
   it." The corrected fallback above routes exactly the callouts whose `ref_type` Q1 doesn't know
   — Q2-only categories, and every `crossref.ids` Promised prefix (P6 Finding 2: "no Q1 equivalent
   at all") — back down that same Route-L path, for the same silent-loss reason. Two things make
   this worse than a missing prefix alone: (a) it inverts user intent — `::: {.callout-note}`
   renders correctly, but `::: {.callout-note #nte-setup}` (labeled *specifically so it can be
   referenced*) renders silently unnumbered; (b) a `@nte-setup` reference elsewhere in the prose is
   still resolved by Q2 upstream of the cut into a `CrossrefResolvedRef` carrying a real `order`,
   so Route N renders something like "Note 3" — a visible reference to a number that appears
   nowhere in the document. **Resolution (a diagnostic, not new capability — same pattern as
   P7's multi-format-render warning, design doc §14): keep the fallback (not crashing is
   correct), but make it loud.** Emit a `Q-<pandoc>-*` warning at the fallback naming the callout's
   id and its unregistered `ref_type`, through the same unconditional-stderr channel P4 Finding 4
   now specifies, and add this case to design doc §12 as a listed limitation ("a callout whose
   crossref category Q2 knows and Q1 doesn't renders unnumbered, and references to it will cite a
   number that does not appear").

## Theorem field-map, audited against the real Q1 constructor (not assumed mechanical)

`theorem.lua`'s constructor starts at line **87**, not 88 (citation fix, round 4 review, Reviewer
B). It is `{name, div, identifier}` — **not** `{ref_type, kind, identifier}`
as Q2's `plain_data` carries. The shim has to derive, not pass through 1:1:
- `identifier` — direct pass-through, matches.
- `div` — reconstruct from `slots.content` (wrapped in a `Div` if not already one; Q1's render
  does exactly this reconstruction itself: `if el.t ~= "Div" then el = pandoc.Div(el) end`).
- `name` — Q1's **user-supplied title override**, drawn from `slots.title` (Inlines), not from
  `plain_data.kind`. **`kind` does not go through the constructor at all — resolved 2026-09-17,
  decided with Gordon** (see P4's finding, "Q2 owns presentation defaults"). Forcing `kind`
  through `name` would be wrong regardless of policy: reading `captionPrefix`
  (`crossref/theorems.lua:79-90`) shows `name` is an *optional parenthetical suffix* ("Theorem 1
  (My Special Title)"), never a replacement for the type label — passing `kind` through it would
  render "Theorem 1 (Theorem)". The actual label comes from `title(type, theoremType.title)`
  (`crossref/format.lua:4-7`), which checks a `crossref-<type>-title` **param** before falling
  back to Q1's static default. **P4's params-blob builder sets that param from Q2's
  `RefTypeRegistry`** for every registered type — no constructor change, no shim change, this
  plan's field-map is unaffected. This is also the general resolution to the "constructor
  default-passthrough policy" question below, not just a Theorem-specific fix.
- **`order` is set post-construction, not passed to the constructor at all** — confirmed by
  reading the renderer (`local order = thm.order`, `theorem.lua:222`, corrected 2026-09-18 from
  `:220`) against the constructor (`theorem.lua:87-93`, no `order` field). This is concrete
  evidence for design note §4.D's number-injection mechanism. **Corrected 2026-09-18 (an
  implementation-feasibility review found the prose below mis-described the API): `quarto.
  <AstName>(params)` returns *two* values, `(scaffold_node, data_table)`. **Citation fix (round 4
  review, Reviewer B): the two-value return for the branch every Route-R handler actually takes
  (`create_emulated_node`) is at `ast/customnodes.lua:263`, not `448-458`** — that range is
  `add_handler`'s dispatcher, and only its `need_emulation == false` branch (line 457, taken by
  none of the five Route-R handlers) returns two values directly; the branch actually taken
  (line 453, `return create_emulated_node(...)`) delegates to line 263's two-value return, which
  an implementer reading only 448-458 would not see. `.order` must be assigned onto the
  **second** return value (the data table), not "the returned table" singular. Also: the shim
  does not invoke `render` itself — it returns the scaffold node and lets `main.lua`'s own render
  pass (further down the filter chain) dispatch to the handler's registered renderer, the same way
  it would for any natively-parsed node.** Literal shape:
  ```lua
  local node, tbl = quarto.Theorem{ identifier = id, name = name, div = div }
  tbl.order = { order = n, section = section }  -- shape per crossref/format.lua:137-145,
                                                 -- matches Q2's plain_data.order verbatim
                                                 -- (crossref_index.rs:286-293)
  return node  -- main.lua's own render pass takes it from here; the shim never calls render
  ```

**`algorithm`/`alg` gap: confirmed real, fix location resolved, detection-mismatch worry resolved
as a non-issue (2026-09-17).** `theorem.rs`'s `THEOREM_CLASSES` (8 entries:
theorem/lemma/corollary/proposition/conjecture/definition/example/exercise) has no `algorithm`
entry, while Q1's `theorem_types` table (`theorem.lua:1-52`) does (`alg = {env="algorithm",
style="plain", title="Algorithm"}`).

Reading `theorem.rs` past its own module-doc summary (not just the first paragraph) shows the
earlier "Q2 detects by class, Q1 by identifier" framing was incomplete: **Q2 already does dual-
path detection**, exactly like Q1. A `Div` is theorem-like iff its class list names a flavor
*or* its id prefix classifies via the registry — the real function pair is `match_theorem_class`
(`theorem.rs:221`) and `match_theorem_id` (`theorem.rs:241`); `registry_classify` (`theorem.rs:258`)
is a separate helper used only by the cross-check diagnostic when the two disagree, not part of
the detection pair itself (corrected 2026-09-17 — verified against the real function names, not
guessed) — and there's an existing test proving id-prefix-only detection works with no class
present (`id_prefix_alone_triggers_theorem_sugar`, `theorem.rs:596`, with an inline "Q1 parity"
comment at line 598, not a test literally named that). **This fully resolves the "does the
mismatch affect the other 8 classes" worry: it doesn't** — both detection paths already exist
and are tested for all 8.

The real gap is narrower: `alg` is simply **absent from both places it needs to be** —
`THEOREM_CLASSES` (`theorem.rs:61-69`) for class-detection, and `RefTypeRegistry::BUILTINS`
(`crossref/registry.rs`) for id-prefix detection (since `registry_classify` filters against the
*same* `THEOREM_CLASSES` set via the registry). **Fix location: Q2's Rust core (`theorem.rs` +
`crossref/registry.rs`), not this shim.** The shim can't fix this on its own — if Q2's core
never recognizes `.algorithm`/`#alg-...`, no `CustomNode("Theorem")` with `ref_type: "alg"` is
ever produced for the shim to route. Once the wire carries that `ref_type`, the shim needs **no
changes at all** — Q1's own `theorem_types.alg` entry already exists and picks it up for free,
since Route-R reconstruction is already `ref_type`-agnostic (same mechanism as the other 8).

## Proof and FloatRefTarget field-maps, audited (closes part of P2's deferred question)

- **Proof — real gap found, not just risk.** Q1's constructor is `{name, div, identifier,
  type}` (`proof.lua:54-61`), and its render **indexes into `proof_types[proof_tbl.type:lower()]`
  unconditionally** (`proof.lua:81`) — a nil `type` crashes render (`nil:lower()`). Q2's current
  `plain_data` for Proof is deliberately just `{"kind": "Proof"}` (`proof.rs:145-148`; no
  `ref_type` on purpose, since `.proof` is unnumbered by design). **`type` does not exist in
  Q2's plain_data at all today.** This is a confirmed P2 extension request, not a maybe: add a
  `type` field (value `"proof"` today; extensible if `.remark`/`.solution` are ever ported, per
  the catalog's flagged gap). `identifier` isn't a plain_data concern — it lives on
  `CustomNode.attr`, which both Theorem's and Proof's shim mapping read directly, not from
  `plain_data`.
- **FloatRefTarget — the one type that actually is "mechanical."** Its constructor
  (`floatreftarget.lua:96-107`) just decomposes `tbl.attr` into `identifier`/`classes`/
  `attributes` and passes everything else in `tbl` through unfiltered — unlike Theorem/Proof, it
  doesn't hard-filter to a fixed field set. No missing-field risk found; the design doc's
  "mechanical" framing holds for this one type specifically, not as a general property of Route R.

## In scope
- The shim + a per-type field-map (wire `plain_data`/`slots` → Q1 constructor args / raw-Div
  attrs). **All field-maps are now audited** (Theorem, Proof, FloatRefTarget, Tabset above;
  **Callout's lives in the design doc's §3 prose, not in this plan** — corrected 2026-09-17, an
  epic-wide review found this line claimed a Callout audit section that doesn't exist here;
  `CrossrefResolvedRef`/`Equation` resolved to Route N, no constructor to map against).
  `ExampleEmbed` is **out of scope entirely** as of 2026-09-17 — resolved upstream in P1's Rust
  core, never reaches this shim for Pandoc targets (see finding above).
- **Tabset routes R** (decided — see finding). Landed the design-doc §3 change.
- **Decide Route N's mechanics** for `CrossrefResolvedRef` (direct-to-inline, proposed above) and
  `Equation` (direct-to-inline + format-specific numbering, proposed above). `ExampleEmbed` is no
  longer part of Route N — removed from this plan's scope (see finding above).
- ~~Decide Theorem's `algorithm`/`alg` fix location~~ **Resolved 2026-09-17** (see finding
  above): Q2's Rust core (`theorem.rs` + `crossref/registry.rs`), not this shim; the
  class-name-vs-identifier-prefix detection mismatch is a non-issue (dual-path detection already
  exists and is tested). Not this plan's implementation work — file as a small, separate fix
  against `quarto-core`, outside this epic's critical path (it's a pre-existing Q2 gap, not
  something the epic introduced).
- **File the confirmed Proof extension request with P2**: add `plain_data.type` (see field-map
  audit above) — this is no longer conditional, it's a known crash otherwise.
- ~~Design the Q1-version pinning/drift-detection mechanism~~ **Resolved 2026-09-17** (decided
  with Gordon): pin to the **release version**, not a dev commit — confirmed the local
  `quarto-cli` checkout (`dcffbfade8`) is `v1.11.3` + one doc-only commit (`git describe`:
  `v1.11.3-1-gdcffbfade`; the one commit ahead touches only `.claude/rules/filters/overview.md`,
  zero diff under `src/resources/filters/`), so **pin to tag `v1.11.3`** — byte-identical to
  what's actually vendored, no dev drift to absorb. Record this in
  `resources/pandoc-filters/README.md` (P4 owns creating it), mirroring
  `resources/scss/README.md`'s Source/Updating structure — Bootstrap pins a release number,
  this pins a release **tag**, since quarto-cli's Lua filters have no per-file version. The
  actual drift-detection mechanism doesn't need separate design: it's the Layer-1/Layer-2
  contract tests below, the same behavioral-tripwire philosophy P3 already established for its
  own 3-file patch ("a behavioral test... fails immediately if a future re-vendor silently
  overwrites"). A re-vendor becomes a deliberate act: bump the pinned tag, re-run Layer-1/2, see
  what breaks.
- **Contract tests:** Layer-1 (introspect Q1 `by_ast_name`: type coverage + slots + class_name
  match the schema) + Layer-2 (per-type golden render). This **is** the drift-flagging
  mechanism against the pinned `v1.11.3` tag above — no separate mechanism needed.

## Out of scope
- Numbering suppression / registry seed (P6 — this plan assumes numbers arrive on nodes).
- Per-format invocation (P7).
- `DecoratedCodeBlock` — not a wire-format node; whatever bridges the sideband map to
  `decoratedcodeblock.lua` is P7's concern, not this shim's.

## Consumes / Produces (seams)
- **Consumes:** P2 schema; P4 run machinery; P1's `Pandoc`-kind exclude-list (this shim's
  field-map audits assume `panel-tabset`'s sugar half survives to the cut, which depends on P1
  keeping it enabled — see P1 for the resolved list).
- **Produces for P7:** rendered custom nodes per format, **plus** the code-block-decorations
  sideband bridge P7 needs to design separately (this shim doesn't carry it).
  **Feeds back to P2:** `plain_data.type` for Proof (confirmed, not conditional). Theorem's
  `kind` needs no P2 change — resolved 2026-09-17 (see finding above): it's never passed through
  the constructor at all; Q2 owns it via P4's `crossref-<type>-title` param mechanism instead.
  **Coordinates with P3/P6:** Route-R correctness needs external-numbering mode. **Coordinates
  with P4:** the `crossref-<type>-title` params P4 builds from `RefTypeRegistry`.

## Coarse checklist
- [x] **Tabset routes R, not L** — decided with Gordon (frozen-decision reversal — see finding).
  Field-map audited clean. Landed in the design doc §3 table (2026-09-17, commit `8e9e546b4`,
  alongside Callout's reclassification from P6).
- [x] **Equation resolved to Route N, not R** — no Q1 `ast_name` exists at all (see finding);
  landed in the design doc §3 table (2026-09-17, commit `8e9e546b4`).
- [x] Add "Route N" to the design doc's vocabulary — now covers `CrossrefResolvedRef`
  (direct-to-inline) and `Equation` (direct-to-inline + numbering). `ExampleEmbed` is **not**
  a Route N case after all — resolved upstream in P1's Rust core (2026-09-17), out of this
  plan's scope entirely.
- [x] Resolve `CrossrefResolvedRef` directly to a Pandoc inline in the shim (mirror `refs.lua`);
  resolve `Equation` the same way (mirror `equations.lua`'s format-specific `RawInline`
  injection); do not attempt a Route-R constructor call for either — none exists. **Done:**
  implementation plan Tasks 4 (`3cc1870a3`) and 5 (`2328a638e`).
- [x] **`ExampleEmbed`'s non-HTML behavior — resolved 2026-09-17, decided with Gordon (see P1).**
  No longer this plan's open question at all: `example-embed-render` reclassified to
  format-parameterized B1, resolved entirely in Rust upstream of the cut.
- [x] Decide Theorem's `algorithm`/`alg` fix location; check the class-vs-identifier detection
  mismatch — **resolved 2026-09-17** (see finding above): fix in Q2's Rust core, not this shim;
  detection mismatch confirmed a non-issue for the other 8 classes. Actually adding the
  `THEOREM_CLASSES`/`RefTypeRegistry::BUILTINS` entries is a small separate `quarto-core` fix,
  outside this plan's implementation scope.
- [x] Audit Proof's, FloatRefTarget's, and Tabset's real Q1 constructors — done (see sections
  above). Proof needs a new `plain_data.type` field (P2 extension request); FloatRefTarget and
  Tabset confirmed clean.
- [x] Design the Q1-version pinning/drift-detection mechanism — **resolved 2026-09-17, decided
  with Gordon** (see finding above): pin to release tag `v1.11.3` (confirmed byte-identical to
  the currently vendored source), recorded in `resources/pandoc-filters/README.md` (P4); drift
  detection is the already-planned Layer-1/Layer-2 contract tests, no separate mechanism.
- [x] **No Route-L reconstruction needed for the current 8-type inventory** — Callout's
  reclassification to R (P6, 2026-09-17) means every wire type is now Route R or Route N;
  Route L is not vestigial in the design (the general rule still routes future presentation-only
  types there), just currently unused. Don't build dead Route-L machinery speculatively. **Done:**
  confirmed by construction — `quarto2-shim.lua`'s `routes` table carries exactly seven entries,
  each `"R"` or `"N"`, no `"L"` value, mechanically bound by `test_route_table_has_no_route_l`
  (Task 1) and `test_shim_routes_cover_the_schema` (Task 7).
- [x] Route-R reconstruction (wire → constructor, then post-construction `order` assignment per
  the Theorem finding) for R-types (Callout, Theorem, Proof, FloatRefTarget, Tabset); no
  re-defaulting of already-resolved fields. **Done:** implementation plan Tasks 2 (`79acc955e`)
  and 3 (`6b4e018ac`).
- [x] Route-N direct resolution for `CrossrefResolvedRef` and `Equation` **by calling Q1's own
  functions** — **corrected 2026-09-18, round 4 review: the full list is `refPrefix`,
  `refNumberOption`, `subrefNumber`, `refHyperlink`, `refDelim`, `crossrefOption`, `nbspString`
  for `CrossrefResolvedRef`** (the prior three-function list could produce a prefix but not a
  number), **plus `add_ref_prefix`'s nbsp logic reproduced inline (it's closure-local, not
  callable)**, **and `renderEquation(eq, label, alt, order)` for `Equation`** (confirm the
  fallback branch, `crossref/equations.lua:133-144`, not the LaTeX/Typst branches, is the one that
  reads `order`) — not reimplementing their logic. See the Route-N Finding above for the corrected
  worked examples and the Q1-is-normative decision. **File the confirmed `CrossrefResolvedRef`
  extension request with P2** (`cite_prefix`/`cite_mode`/`label_upper` — without them, the
  cite-mode-aware half of the Q1-is-normative decision cannot actually be implemented). **Done:**
  implementation plan Tasks 4 (`3cc1870a3`) and 5 (`2328a638e`); `cite_prefix`/`cite_mode`/
  `label_upper` all confirmed present in `plain_data` (P2 Task 3 landed ahead of this branch).
  `cite_prefix`-dependent cite-mode formatting remains a stated v1 scope-out (Task 4's own
  Deliberate v1 scope-outs), not a re-opened extension request — `cite_mode`/`label_upper`
  themselves are used.
- [x] **New (2026-09-18, round 4 review): state the shim group's traversal contract as
  bottom-up, not topdown** (see the traversal sub-finding above), and have P4's patch item show
  the actual filter-group literal, not just the `tappend` line — nested wire nodes (a
  `FloatRefTarget` inside a `Callout`'s `content` slot, exactly P6 Finding 5's fixture) depend on
  inner-before-outer conversion order. **Done:** Task 1 (`1b4596c72`) — `quarto2-shim.lua`'s
  filter-group entry documents the bottom-up contract inline; `test_nested_wire_nodes_convert_inner_first`
  binds it.
- [x] **New (2026-09-18, round 4 review): add a Layer-1 assertion that each Route-N function this
  shim depends on exists and is callable with the expected arity**, by name, from the probe
  script — cheap (runs in the same pandoc-Lua probe Layer-1 already needs) and converts a future
  Q1 signature change on one of these functions (the silent-wrong-output case; a rename is already
  loud via `attempt to call a nil value` → pandoc nonzero exit → P4's stderr diagnostic) from
  silently-wrong output into a contract-test failure. **Done:** Task 7 (`afb28cbf6`) — H7's
  arity census + `test_route_n_globals_have_expected_arity`.
- [x] Per-type field-map, corrected per the audits above; file the Proof `type` request with P2.
  **Done:** field maps implemented per-type in Tasks 2–3; `Proof.plain_data.type` confirmed
  present (P2 Task 3 landed ahead of this branch), consumed directly in Task 2's `route_proof`.
- [x] **New (2026-09-18): the shim's loading mechanism** — implemented in P4 (small marked patch
  to `main.lua`'s filter-list assembly, positioned before `quarto_normalize_filters` — see P4's
  Finding 2 and this plan's "shim's loading mechanism" Finding above), but this plan's shim code
  is what gets loaded, so confirm the shim's own file lives as a sibling to `customnodes/*.lua`
  under **our own** vendored-but-not-upstream tree (P4's README should list it as "ours," not part
  of the `v1.11.3` pin, so a re-vendor delete-and-recopy doesn't remove it). **Done:** confirmed —
  `resources/pandoc-filters/README.md`'s "Ours vs. pinned" list names `quarto2-shim.lua` and
  `quarto2-shim-probe.lua`, bound by `pandoc_filters::test_readme_records_the_pins`.
- [x] **The shim's error handling** per the Finding above — unrecognized `type_name` unwraps to
  slot content (not a hard fail); the Callout-`fail()` case falls back to raw-Div reconstruction
  when `ref_type` isn't in **`crossref.categories.by_ref_type`** (corrected 2026-09-18, round 4
  review — not `valid_ref_types()`, which is a strict superset and lets the theorem-family/`eq`/
  `sec` prefixes through to the same `fail()` unchanged) rather than aborting, **and emits a
  warning at the fallback** (see the compounding finding above — silent unnumbering plus a
  dangling `@ref` is worse than a missing prefix alone). **Done:** Task 6 (`9118e64d1`). One
  correction beyond the plan's own text, found via measurement: the guard needs
  `is_valid_ref_type(ref_type) AND by_ref_type[ref_type] == nil` (both gates, matching Q1's own
  two-layer check in `decorate_callout_title_with_crossref`), not `by_ref_type` alone — the
  literal single-condition guard as written here would misfire on every unlabeled or
  non-crossref-shaped callout. See the P5 implementation ledger for the full finding.
- [x] **New (2026-09-18, round 4 review): add a named Q2-only error-path test tier.** P7's
  Q1-parity golden harness structurally cannot cover any of this plan's three error-handling
  cases, by construction — each is precisely the case where Q1 has no counterpart output to
  capture (an unrecognized wire `type_name` cannot appear in a quarto-cli fixture; a Q2-only or
  Promised callout category is one Q1 itself rejects; a deliberate constructor crash has no
  golden). Name concrete fixtures now rather than leaving this to be discovered during
  implementation: `unknown-wire-type.qmd` (asserts slot content survives, wrapper is gone, one
  warning emitted), `callout-foreign-category.qmd` (`::: {#thm-x .callout-note}` — asserts the
  render *completes* with the fallback warning, not `FATAL QUARTO ERROR`), `proof-missing-type.qmd`
  (asserts the diagnostic carries pandoc's stderr verbatim and the temp JSON is retained). **Done:**
  Task 6 (`9118e64d1`) — all three fixtures created exactly as named; `unknown-wire-type.qmd`'s
  base type was corrected during implementation from the originally-envisioned shape to
  `.theorem .my-custom-highlight` after measurement showed a `.callout-note`-based fixture
  produced a vacuous discriminator (Q1's own class-keyed dispatcher independently recognized the
  residual class).
- [x] Layer-1 introspection test — **name the harness before writing it** (2026-09-18): it must
  run inside pandoc's own Lua (a probe script invoked via `pandoc --lua-filter`, since Q1's node
  registry — `by_ast_name` — is only populated by `main.lua`'s own imports; `mlua` inside `pampa`
  cannot load `main.lua`). Assert a **per-type name mapping**, not set-equality — Q1's slot names
  don't match Q2's for every type (`theorem.lua`/`proof.lua` declare `slots = {div, name}` against
  Q2's `content`/`title`; `panel-tabset.lua` declares no `slots` field at all, an explicit N/A for
  Tabset). Only Callout and FloatRefTarget match by name today. **Extended 2026-09-18, round 4
  review, Reviewer D: assert bidirectional totality, not just Q1-facing coverage** — the
  introspection above checks "does Q1 have a handler for every type the schema expects?" but not
  the inverse, "does this shim have a handler for every type the schema declares?", which is the
  only mechanical guard that would catch a 9th Q2 wire type shipping without shim support (the
  golden harness would not catch it either: an unhandled type's unwrap-and-drop path preserves
  slot content, so the extracted text is unchanged — a zero-byte snapshot diff for a fully
  semantics-stripped node). **Done:** Task 7 (`afb28cbf6`) — H7 (an env-gated census inside the
  shim's own filter group, decided with Gordon over a standalone sibling probe, which measurement
  showed sees an empty registry due to per-`-L`-file Lua state isolation); per-type mapping
  (`test_per_type_slot_mapping`) and bidirectional totality (`test_shim_routes_cover_the_schema`)
  both implemented.
- [x] Layer-2 goldens — **name one fixture + one expected-output shape before writing the
  harness** (2026-09-18): this plan is built before P7, so it cannot inherit P7's insta/semantic-
  extraction harness (`cargo xtask capture-pandoc-goldens`) — either build a narrower one-off
  harness for per-type Lua-level assertions (e.g. assert on the pandoc JSON the shim produces for
  one type, not a rendered docx), or explicitly defer Layer-2 to run after P7 lands and state that
  in this plan rather than leaving the ordering silently contradictory. **Done:** Task 8
  (`fe93e21bc`) — took the narrower-harness option; seven `insta` snapshots (one per wire type
  reaching a Pandoc target), `float-basic.qmd`/docx as the named worked example.

## Deferred in-plan questions

None remain open. Both of this plan's originally-deferred items are resolved:

- ~~Constructor default-passthrough policy per R-type (feed final values so no re-defaulting).~~
  **Resolved 2026-09-17, decided with Gordon** (see the Theorem field-map finding above and
  P4's fuller writeup): not a general policy to design, but one standing principle — **Q2 owns
  any presentation default it has an opinion on, now or in the future; Q1's own defaulting is
  only load-bearing for fields Q2 has no data for at all.** A full field-by-field census across
  all five Route-R types (Callout, Theorem, Proof, FloatRefTarget, Tabset) found exactly one
  field where this was a live, undecided question — Theorem's `kind` — everything else that
  looked similarly shaped (Tabset's `active`, Callout's `appearance`/`icon`) turned out to
  already be fully resolved by Q2 before the wire cut, making Q1's parallel defaulting dead
  code for Route R. `kind`'s resolution: Q2 supplies it via a `crossref-<type>-title` param
  (P4), not via the constructor — this plan's Route-R reconstruction checklist item above
  ("no re-defaulting of already-resolved fields") already states the resulting principle
  correctly and needs no further change.
- ~~`ExampleEmbed`'s Pandoc-tail behavior — genuinely unscoped, not just deferred.~~ **Resolved
  2026-09-17** (see finding above) — no longer deferred, no longer this plan's concern at all.

**This closes out all four of this plan's originally-flagged open design questions**
(`ExampleEmbed`'s non-HTML behavior, the `algorithm`/`alg` fix location, the Q1-version pinning
mechanism, and this constructor-passthrough policy) — plus the Callout routing question P6
found independently. What remains in the coarse checklist above is implementation work, not
open design.
