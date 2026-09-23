# P3 — Upstream Q1: crossref-numbering: external

**Date:** 2026-08-20  **Updated:** 2026-09-18 (round 4 review) — added a request to fold into this
plan's upstream PR: expose P5's Route-N functions on `quarto.doc.crossref`/`quarto.utils`, since
they're currently undefended bare globals and upstream's active `_quarto.modules` migration is
heading straight at them. Also noted that P4's own `main.lua` splice patch is a second,
independent edit to a file this plan also patches, and that it silently renumbers this plan's
`if enableCrossRef then` gate (`main.lua:718`) to line `719`; recommended anchor-text citation instead. (An unsolicited
external audit also claimed this plan's `enable-crossref` audit misses the
`layout/ipynb.lua:121,126` sites — checked directly and that claim is wrong: this plan's own audit
table already covers both, verified as a no-op; recorded so a future reader doesn't rediscover the
same false claim.)
**2026-09-17 (two passes)** — a pan-epic adversarial re-audit
corrected one small precision error in the ipynb renderer-pair evidence: the `981` renderer
doesn't uniformly "strip" caption/identifier, it transfers them onto the bare image except when
already inside a code-cell output (see the audit table below) — doesn't change the
`crossref_present()`/`not crossref_present()` recommendation for either renderer. (First pass,
same day: full audit of the `enable-crossref` gate surface completed — previous update was an
explicit first pass, not a full audit. The audit found a second, structurally distinct gate
class the prior draft's framing entirely missed, corrected the "known instances" line citations
(they pointed at the wrong lines), confirmed the filter catalog's floatreftarget.lua "~9/11"
claim with an exact per-branch breakdown, and resolved the fallback-policy aspiration with a
concrete recommendation.) **Also noted in round 4 review (2026-09-18): this plan's line-number
citations should anchor to `if enableCrossRef then` instead, since P4's splice renumbers the
site from `718` to `719` with no warning.**
**Status:** Shape draft
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)  |  Epic: `2026-08-20-pandoc-hybrid-epic.md`
**Implementation task breakdown + test-seam prevalidation:** [`2026-09-18-pandoc-hybrid-P3-implementation.md`](2026-09-18-pandoc-hybrid-P3-implementation.md) — this plan's Coarse checklist converted into dispatchable `## Task N` units, each test bound to a named production seam and revert hunk.

## Goal
Contribute a backward-compatible change to quarto-cli that **decouples "assign crossref numbers"
from "present crossref numbers"**, so Q2 can supply numbers while Q1 still renders captions/refs.
This lets us keep the vendored Q1 sources **verbatim** (set a param, don't fork) — see
"Fallback policy" below for what "verbatim" means once the patch is carried locally.

## The change — two predicates, not one

The prior draft implied a single swap everywhere: `if not param("enable-crossref")` becomes
`if not crossref_present()`. **The full audit below shows the vendored Lua has two structurally
different gates that need opposite-polarity predicates — conflating them would be a real bug,
not just an audit-completeness gap.**

### 1. The assignment-group gate — `if enableCrossRef then` (`main.lua:718`), not `main.lua:226` as previously cited

`main.lua:226-227` is only the `param()` *read*:
```lua
-- see whether the cross ref filter is enabled
local enableCrossRef = param("enable-crossref", true)
```
That line is not itself a gate. The actual behavioral gate is **491 lines later** — the site with
`if enableCrossRef then` (at `main.lua:718`):
```lua
if enableCrossRef then
  tappend(quarto_filter_list, quarto_crossref_filters)
end
```
`quarto_crossref_filters` (`main.lua:671-707`) is **the entire numbering/index/@ref-resolve
group** the design doc's Goal names, as one list: `crossref_mark_subfloats`,
`crossref_preprocess_theorems`, and a combined filter running `file_metadata`, `qmd`,
`sections`, `crossref_figures` (assigns `float.order`, calls `indexAddEntry` — confirmed at
`crossref/figures.lua:27-37`), `equations`, `crossref_theorems` (assigns `thm.order` **and**
`proof.order` — confirmed at `crossref/theorems.lua:27,37`), `crossref_callouts` (assigns
`callout.order` — confirmed at `customnodes/callout.lua:467-480`), plus `resolveRefs`,
`crossrefMetaInject`, `writeIndex`. **This single `if` is the entire "assign numbers" surface —
one gate, not a family of per-type gates**, because `crossref_figures`/`crossref_theorems`/
`crossref_callouts` are merged into one `combineFilters{}` call and run as a unit.

For `crossref-numbering: "external"` this group must still be **skipped** (Q2 already assigned
order/index/refs), so this site needs a **new** predicate — `crossref_present()` would be
*wrong* here, since under external mode `crossref_present()` is `true`, which would re-enable
numbering assignment (exactly backwards):
```lua
local assignCrossrefNumbers = enableCrossRef and param("crossref-numbering", "quarto") ~= "external"
...
if assignCrossrefNumbers then
  tappend(quarto_filter_list, quarto_crossref_filters)
end
```
Everything upstream of this line already runs unconditionally regardless of `enableCrossRef` —
confirmed by tracing `initCrossrefIndex()` (`main.lua:222`, before the `enableCrossRef` read)
and `initialize_custom_crossref_categories()` (called from `quarto-init/metainit.lua:10`, part
of `quarto_init_filters`, also unconditional). This is the "keep category registration" half of
the design doc's sentence — it needs **no code change**, because it's already unconditional
today.

### 2. The render-decoration gates — 4 sites, not the "2 known instances" previously cited

(`floatreftarget.lua:197` in the prior draft was itself imprecise — the real lines are 196 and
242, two different functions, plus a third site at 964/981 not mentioned at all.) These read
`order`/`parent_id` already assigned on the node and print "Figure N:" / "Table N:" etc.:

- `floatreftarget.lua:196` — `decorate_caption_with_crossref` early-return
- `floatreftarget.lua:242` — `full_caption_prefix` early-return
- `floatreftarget.lua:964`/`981` — a `FloatRefTarget`-for-ipynb renderer-selection **pair**
  with genuinely different render bodies (964 decorates + wraps in `pandoc.Figure`; 981 collapses
  to a bare `Image`, **transferring** `float.identifier`/`float.caption_long` onto it unless the
  float is already inside a code-cell output — corrected 2026-09-17, a prior pass said "strips,"
  which only holds for the code-cell-output branch) — a real fork, not cosmetic
- `modules/callouts.lua:22` — `decorate_callout_title_with_crossref` early-return

These become `crossref_present()` (964/981 becomes a `crossref_present()` /
`not crossref_present()` pair), where, unchanged from the prior draft:
```
crossref_present() = enableCrossRef or param("crossref-numbering", "quarto") == "external"
```

## Full audit (checklist item 1 — done)

A full-tree grep for the literal string `enable-crossref` under
`/Users/gordon/src/quarto-cli/src/resources/filters/` returns exactly **8 lines across 4
files**: `main.lua:227`, `layout/ipynb.lua:121,126`, `customnodes/floatreftarget.lua:196,242,965,982`,
`modules/callouts.lua:22`. **Note for future audits:** a literal-string grep alone would have
missed the assignment-group gate at `if enableCrossRef then` (`main.lua:718`) — the master gate reads the *local variable* `enableCrossRef`, not the
string `"enable-crossref"`, so it only surfaces by also grepping the bare variable name and
reading `main.lua` end-to-end. That's exactly what the prior draft's "first pass" missed.

| Site | What it gates | Classification | Action |
|---|---|---|---|
| `main.lua:227` | (param read only) | not a gate | extend to also read `crossref-numbering` |
| `if enableCrossRef then` (`main.lua:718`) | entire numbering/index/@ref-resolve filter group | **assign-numbers** | new `assignCrossrefNumbers` predicate (opposite polarity from `crossref_present()`) |
| `floatreftarget.lua:196` | `decorate_caption_with_crossref` | present-numbers | → `crossref_present()` |
| `floatreftarget.lua:242` | `full_caption_prefix` | present-numbers | → `crossref_present()` |
| `floatreftarget.lua:964`/`981` | FloatRefTarget-ipynb renderer pair (real fork — different bodies) | present-numbers | → `crossref_present()` / `not crossref_present()` |
| `layout/ipynb.lua:121`/`126` | PanelLayout renderer-selection pair | **verified no-op** — both predicates dispatch to the *same* function, `render_ipynb_layout` (confirmed by reading both registrations, lines 120-127: identical callback in both `add_renderer` calls) | **no change** — the crossref flag has zero effect on this renderer's output either way |
| `modules/callouts.lua:22` | `decorate_callout_title_with_crossref` | present-numbers | → `crossref_present()` |
| `customnodes/theorem.lua:278` | `if order == nil then return el end` — no `enable-crossref` string at all, implicit gate | present-numbers, **already correct as written** | **no patch needed** — `thm.order` is non-nil exactly when the assign-group ran (today) or when Q2 populates it via the wire (external mode), so this site auto-adapts once P6 wires `order` onto the node |
| `crossref/tables.lua:229` (`float_title_prefix`), `floatreftarget.lua:218`/`272` (subfloat `order == nil`) | downstream nil-guards *inside* the already-gated decoration functions | not a gate — defensive `warn()`-and-skip | **no patch, but this is the regression-check's failure mode**: if P6's wiring misses populating `order` on a labeled node, this silently drops the caption prefix with a warning rather than erroring — the Q2 golden (checklist item 3) must positively assert the prefix text is present, not just that render doesn't crash |
| `customnodes/proof.lua` | — | **out of scope for this plan** — confirmed by reading its renderer (`add_renderer("Proof", ...)`, lines 76-120+): it never reads `.order` and prints no number, only a name/type label. Proof's crossref-adjacent gap (a missing `plain_data.type` field) is P5's finding, not a numbering-gate issue. | none |

### Cross-check against the filter catalog's "~9/11 format branches are format-not-in-q2" claim

Confirmed accurate, with an exact breakdown the catalog didn't give. `floatreftarget.lua` has
**11** `add_renderer("FloatRefTarget", …)` registrations total (lines 183, 308, 659, 666, 878,
949, 964, 981, 1002, 1183, 1222):

- **2 relevant to this epic's v1 targets**, and **both already call
  `decorate_caption_with_crossref`** (confirmed by reading each body): `docx`/`odt`
  (line 666-670) and `pptx` (line 1222-1237). These are the concrete, load-bearing reason
  `floatreftarget.lua:196` matters — it's not an abstract audit item, it's the exact line that
  currently suppresses "Figure N:" in Q2's docx/pptx output whenever `enable-crossref` reads
  false, and needs to instead read `crossref_present()`.
- **1 not relevant despite technically being a format Q2 emits** — `html` (line 659-664): Q2
  has its own native HTML writer and never routes html through this Lua path at all, so this
  branch is dead code from Q2's perspective regardless of the crossref gate.
- **1 generic fallback** (line 183, predicate `true`) — catches any format with no specific
  branch; not format-specific.
- **7 are genuinely `format-not-in-q2` today**: `latex` (308, stubbed only per the epic),
  `asciidoc` (878), `jats` (949), the `ipynb` pair (964/981 — the two are already-audited
  crossref-gate sites, no format-not-in-q2 label needed for them separately), `typst` (1002),
  `gfm` (1183).

11 total − 2 relevant = 9, which matches the catalog's "~9/11" — but the catalog's single label
`format-not-in-q2` blurs together two different reasons (html is native-writer-covered, not
format-not-in-q2; the fallback isn't format-specific at all). Worth noting so a future reader
doesn't over-trust the catalog's one-line summary for this file specifically.

## In scope
- ~~Full audit of every `enable-crossref` gate site~~ **Done** (above).
- Implement `assignCrossrefNumbers` (new predicate, at `if enableCrossRef then`, `main.lua:718`) and `crossref_present()`
  (new predicate, the 4 render-decoration sites above); Q1 tests bit-for-bit on default **and**
  on `enable-crossref: false`.
- Q2 golden showing external-mode keeps "Figure N:" from an injected order — must positively
  assert the prefix text renders (see the regression-check note in the audit table above; a
  missing-`order` bug degrades to a silent `warn()`, not a failure, so the golden has to check
  the string, not just "render didn't crash").
- **Fallback policy** (resolved — see below).
- **Regression-check today's `enable-crossref: false` behavior** against the new
  `crossref_present()`/`assignCrossrefNumbers` gates — bit-for-bit on both the unset-default
  case and the explicit-`false` case.

## Fallback policy (resolved)

**Recommendation: carry the patch indefinitely; no timebox.** The audit above shows the total
patch surface is small and stable: **3 files, ~6 edited lines**
(the `if enableCrossRef then` gate at `main.lua:718`; `floatreftarget.lua:196,242,965,982`; `modules/callouts.lua:22`), none of which
touch logic likely to move under normal Q1 development (they're all top-of-function early-return
guards or a single `if` around a filter-list append). Opening the upstream PR is still worth
doing — merging removes the 3-file diff entirely and other quarto-cli consumers may want
`crossref-numbering: external` too — but nothing about the plan of record should depend on it
merging on any particular schedule. This differs from a permanent-fork situation (e.g. this
repo's `wasm-bindgen-futures-patch`, which forks an entire crate because upstream cannot accept
the change) — this is a few local edits to files Q2 already vendors verbatim, low-risk to keep
indefinitely.

**Correction (2026-09-18, round 4 review) — the "3 files, ~6 lines" accounting above is now
stale, for one confirmed reason. (An unsolicited external audit of the real quarto-cli tree also
claimed this plan "misses" the `layout/ipynb.lua:121,126` sites — checked directly against this
plan's own audit table above and that claim is wrong: this plan already covers both lines,
verifies both predicates dispatch to the identical function, and correctly concludes no patch is
needed. Recorded here only so a future reader doesn't rediscover the same false claim.)**

**P4's `main.lua` splice (Finding 2, its own plan) is a second, independent edit to one of
these same three files**, needed to load P5's shim (see P4 for the mechanism). This plan's
fallback policy and the vendoring README P4 plans to create both need to account for **two
plans, two edits, in two marker categories**:
- **`QUARTO-PATCH(upstream PR …)`** — P3's edits (at `main.lua` with anchor text `if enableCrossRef then`, plus the 4 render-decoration sites), each marked with a `QUARTO-PATCH(upstream PR #<N>)` comment indicating they're candidates for merging upstream.
- **`Q2-local, permanent`** — P4's shim-loading splice, references a Q2-only file (`quarto_pandoc_shim_filters`) that can never be upstreamed.

**Cite this plan's own patch sites by anchor text, not line number**, in this plan and
wherever else they're referenced (the epic doc cites `if enableCrossRef then` at `main.lua:718`) — P4's splice, once
implemented, inserts a line between `main.lua:712` and `:713`, which silently renumbers this
plan's `718` site to `719` with nothing to flag it. Anchor text for the assignment-group gate:
`if enableCrossRef then`. **P4's vendoring README must carry the same inventory** of the two
marker categories so a future re-vendor doesn't mistake P4's permanent splice for a
carried-pending-upstream edit that can be dropped once a PR merges.

**Mechanism** (this also resolves what the prior draft's "marked vendored patch" aspiration was
waiting on — it does not need anything from P4 beyond what P4 already plans): P4 vendors Q1's
filters as a plain in-repo directory copy (`resources/pandoc-filters/` + `include_dir!`), not a
git submodule or applied patch file — so "carrying a patch" is simply: edit the 3 vendored files
directly, with a `-- QUARTO-PATCH(upstream PR quarto-dev/quarto-cli#<N>): ...`
comment at each edited site. No patch-apply tooling is needed. **Drift detection is already
assigned to P5** per the design doc ("the P5 contract test flags if a Q1 bump moves the gate") —
a behavioral test (render with `crossref-numbering: external` and assert the caption prefix
appears) fails immediately if a future re-vendor silently overwrites the edited files, without
needing bespoke diff tracking. Nothing here is blocked on P4 beyond the marker-category
coordination noted above.

**Finding, added 2026-09-18 (round 4 review, Reviewer D, corroborated independently by a
dedicated vendoring-fragility audit): bundle a second ask into this same upstream PR — expose
P5's Route-N functions on `quarto.doc.crossref`/`quarto.utils`.** P5's Route-N shim calls four
Q1 Lua functions directly by name (`refPrefix`, `crossrefOption`, `refHyperlink`,
`renderEquation`, plus `refNumberOption`/`subrefNumber`/`refDelim`/`nbspString` per P5's corrected
function list) — all currently undefended bare file-scope globals with **zero test coverage** in
quarto-cli (a `git log -S` on each shows no test references any of them), so a future rename would
break the shim silently while every upstream test stays green. This risk is concretely live, not
theoretical: quarto-cli's own `#14702` (2026-07-17, "Use `_quarto.modules` registry instead of
redundant module requires in filters") is an **active migration of exactly this class of global**
into a module registry, touching 24 filter files so far; `crossref/` hasn't been reached yet, but
the direction of travel points straight at these functions. There is already a **sanctioned
precedent for exposing internals for exactly this use case** —
`customnodes/floatreftarget.lua:239` does `quarto.doc.crossref.decorate_caption_with_crossref =
decorate_caption_with_crossref`, with a comment explaining it's for the docusaurus renderer, "an
extension that doesn't have access to the internal filters namespace" (Q2's shim is structurally
the same situation). **Recommendation: fold a request to expose the Route-N function set on
`quarto.doc.crossref`/`quarto.utils` into this same PR** — cheap to ask for now, alongside the
`crossref-numbering: external` request, and it converts these functions from "undefended global"
to "documented contract" before the `_quarto.modules` migration reaches `crossref/`. Not a
blocker — the shim can call the bare globals today regardless of whether this lands — but worth
asking for in the same PR rather than as an afterthought once a rename actually breaks something.

This is a judgment call on process, not a technical fork — flagged here for visibility rather
than as a blocking question, since either answer (indefinite-carry vs. a hard timebox) leaves
the actual engineering checklist unchanged. Revisit if the patch surface grows materially beyond
the ~6 lines identified here.

## Out of scope
- Q2-side wiring that *sets* the param and seeds the registry (P6). The shim (P5).

## Consumes / Produces (seams)
- **Produces for P6:** the `external` mode P6 activates and the guarantee that registration +
  decoration survive it.

## Coarse checklist
- [x] Full audit of `enable-crossref` gate sites (all 8 literal-string hits plus the
      variable-only `if enableCrossRef then` gate site (`main.lua:718`) the literal grep misses); cross-checked against
      `floatreftarget.lua`'s 11 format branches (exact 2-relevant / 1-native-writer /
      1-fallback / 7-format-not-in-q2 breakdown).
- [x] Implement `assignCrossrefNumbers` (at `if enableCrossRef then`, `main.lua:718`) and `crossref_present()` at the 4
      render-decoration sites; Q1 tests bit-for-bit on default **and** on
      `enable-crossref: false`. Done upstream (quarto-cli `quarto2-crossref-numbering`, Tasks 2-3,
      commits `83d48d8e8..8061f246a`) and carried into q2's vendored tree (Task 6, commit
      `9dae23206`) — the predicates ended up in a new `modules/crossref_numbering.lua`, not inline
      in `main.lua` as this item assumed, since they need to be `require`-able. q2-side behavioral
      coverage: `crates/quarto-core/tests/integration/crossref_numbering_matrix.rs` (Task 7).
- [x] Q2 golden showing external-mode keeps "Figure N:" from an injected order (positive string
      assertion, not just no-crash). **Moved to P6 Task 4** (decided 2026-09-18, see this plan's
      implementation companion's "Moved out" section) — its revert hunks bind to P5's `order`
      passthrough and P6's param wiring, not to this plan's own hunks. Not re-added here.
- [x] Decide and document the upstream-stall fallback policy: carry indefinitely, no timebox
      (see "Fallback policy" above).
- [x] **New (2026-09-18, round 4 review): cite this plan's patch sites by anchor text, not line
      number**, and note in the vendoring README (P4) that the tree carries two plans' edits in
      two marker categories (upstreamable-PR-linked for this plan's edits; Q2-local-permanent for
      P4's shim-loading splice) — P4's splice renumbers this plan's `if enableCrossRef then` gate site from `main.lua:718` to `719`
      with nothing to flag it otherwise. Done (Task 1, q2 worktree `pandoc-hybrid-p3` commit
      `09ed32a53`, cherry-picked into `feature/pandoc-writer-hybrid` as `24d648fa4`).
- [x] Open the upstream PR; mark the vendored files with a
      `QUARTO-PATCH(upstream PR quarto-dev/quarto-cli#14913)` comment at each site; confirm P5's
      contract test covers drift detection (no new mechanism needed beyond P5's existing plan).
      PR open: https://github.com/quarto-dev/quarto-cli/pull/14913. Vendored-tree markers landed
      on **6 files**, not the 3 this item originally named (Task 6, commit `9dae23206` —
      `modules/crossref_numbering.lua`'s introduction added touch points this plan didn't
      anticipate). P5 drift-detection confirmation: recorded in Task 6's own ledger entry.
- [x] **New (2026-09-18, round 4 review): fold a request into the same PR to expose P5's Route-N
      functions** (`refPrefix`, `crossrefOption`, `refHyperlink`, `renderEquation`,
      `refNumberOption`, `subrefNumber`, `refDelim`, `nbspString`) on `quarto.doc.crossref`/
      `quarto.utils`, following the existing `decorate_caption_with_crossref` precedent — see the
      Finding above. Cheap to ask for now, before upstream's active `_quarto.modules` migration
      (`#14702`) reaches `crossref/` and turns these into module-scoped fields with no warning to
      q2. Not a blocker for P5's implementation either way. Done — upstream Task 5, commit
      `0c497cd6d`, included in PR #14913 (6 commits, 33 files per
      `gh pr view 14913 --json files,commits`).
