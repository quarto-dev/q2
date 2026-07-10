# Epic: All output formats via a Pandoc-writer hybrid

**Date:** 2026-08-20  **Updated:** 2026-09-18 (two passes) — see `git log --oneline -- claude-notes/plans/2026-08-20-pandoc-hybrid-epic.md`
for the full correction history. Latest (round 4 review, four dispatched Opus reviewers): added a
"What 'docx/pptx support' means in v1" paragraph to the Definition of done — the prior bullet
sampled four of design doc §12's seven limitations, which read materially rosier than the full
list to a non-participant (a PM writing release notes, a support engineer triaging a bug report).
Landed Gordon's decision on the project-mode containment gate (design doc §13). Verified and fixed
five Critical findings directly against real code across P1–P7 (`pptx` unresolvable; `order`
missing from P2's schema; the `crossref-<type>-prefix`/`-title` param split; the Callout
`fail()`-guard's wrong predicate; a pandoc-version mismatch between the vendored Lua and this
repo's CI). Still pre-implementation. (Prior pass, same day: three more Opus reviews found 13
Critical + ~29 Important findings — genuinely new failure modes, not drift — resolved per the
governing principle from Gordon: this epic wires up existing Pandoc + vendored-Q1-Lua behavior;
it does not implement new Q2 functionality to close gaps between Q2's own HTML renderer and what
Q1 already does. Corrected the Definition of done (crossref *numbers*, not full semantic parity);
added design doc §12/§13/§14; filed five strands for accepted-but-tracked gaps.)
**Status:** Shape drafts reviewed; ready for implementation ordering.
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)
**Research:** [`../research/2026-07-09-q1-filter-catalog.md`](../research/2026-07-09-q1-filter-catalog.md),
[`../research/2026-07-13-q1-format-typescript.md`](../research/2026-07-13-q1-format-typescript.md)
**Supersedes:** an earlier "Cut A / Cut B" framing (2026-07-10 draft), replaced by the
renderer-trinity / unified-cut design and removed from the tree on 2026-09-17 (see git history
if the old framing is ever needed).

## The problem

Q2 today produces only HTML and revealjs; every other `--to` target is rejected
(`crates/quarto/src/commands/render.rs:680-684`, corrected 2026-09-17 from `~628`;
`FormatIdentifier::is_native()` at `crates/quarto-core/src/format.rs:58-60`, corrected from
`61-63`). Q1 supports all formats by leaning on **Pandoc's
own writers** plus **Q1 Lua filters**. We reuse both rather than porting writers — for these
batch-mode formats there is no performance payoff, and reuse inherits Q1's ongoing maintenance.

## The shape (see the design note for the full argument)

Every output is a **format = shared format-neutral semantic core + per-format tail**. Q2's core
computes crossref numbers, callouts, theorems, floats **once** and emits them through the
**existing custom-node wire format** (built for q2-preview). This epic **adapts that wire format
from serving TS/React to serving Lua/Pandoc** — adding a *third renderer* to a boundary Q2
already has, rather than inventing a bridge. The Pandoc tail: skip Navigation, serialize the
wire format, hand it to `pandoc -f json -t <fmt>` + vendored Q1 Lua.

Key frozen decisions (design note §5–8): unified cut via `PipelineProfile`; four-bucket
transform classification; Route L (lower-to-raw, in Lua) / Route R (reconstruct + inject Q2
numbers) — **plus a third route, Route N ("no Q1 handler; resolve or render directly"), found
during the deep pass for types with no Q1 `ast_name` at all** (`Equation`, `CrossrefResolvedRef`
— `ExampleEmbed` was initially thought to be a third Route N case but is resolved entirely in
Rust upstream of the cut instead, see below); upstream Q1 `crossref-numbering: external`; no
Pandoc bundling + min version, native build only; versioned single-sourced wire schema.

## What the deep pass changed

The shape drafts (2026-08-20) got every plan's *mechanism* right but several specifics wrong or
incomplete against the real code — the kind of thing only surfaces by reading the actual vendored
Lua/TS and the actual Rust, not by trusting the filter catalog or the design doc's prose. The
corrections that change how the epic reads as a whole (per-plan detail lives in each P-plan's
commit history):

- **Route-L/R table amended twice** (design doc §3, now landed): `Tabset` and `Callout` both
  move L→R — both carry real numbering that Route L's raw-Div reconstruction would silently
  discard. Tabset found/decided in P5; Callout found/decided in P6, by the same failure-mode
  reasoning (a shared indexer assigns `order` to more types than the original table accounted
  for). **The wire-format inventory now has no Route-L type at all** — every type P5's shim
  handles is Route R or Route N.
- **Category-registry "seed" (design §4 Problem C) was the wrong framing entirely.** There is no
  single Q1 mechanism to seed — built-in categories are split across four independent,
  non-metadata-seedable Lua mechanisms, and the one metadata-driven case (`crossref.custom`)
  already passes through Q2's metadata merge untouched and is already read unconditionally by
  Q1's own `custom.lua`. P6 narrowed this from "build an export" to "confirm the passthrough
  with a test" — a much smaller item than the epic originally scoped.
- **`enable-crossref`'s gate surface is two structurally different predicates, not one** (P3):
  an assign-numbers gate (`main.lua:718`, opposite polarity) and four present-numbers gates. The
  original single-swap framing would have been a real bug (re-enabling Q1's numbering under
  external mode), not just an audit gap.
- **Two wire types have no Q1 counterpart at all** (P5: `Equation`, `CrossrefResolvedRef` — Q1
  resolves both directly to inlines, no persisted node — this is Route N).
  `ExampleEmbed` was initially grouped with these but isn't a Route N case: P1 found its iframe
  is already emitted as a Pandoc `RawBlock("html", ...)` — the same primitive Q1's own docs use
  for the identical hand-authored pattern, with no supporting Q1 mechanism behind it either —
  so `example-embed-render` was reclassified B4→format-parameterized B1 and resolved entirely in
  Rust: it now runs for `Pandoc(fmt)` too, emitting everything but the iframe. `ExampleEmbed`
  never reaches P5's shim for Pandoc targets at all.
- **One confirmed P2 extension request**: `Proof` needs a new `plain_data.type` field (P5) — its
  Q1 constructor crashes without one; this is no longer a maybe.
- **`algorithm`/`alg` is a confirmed, still-open gap** in Q2's `theorem.rs` `THEOREM_CLASSES`
  (P5) — tracked there, cross-referenced (not duplicated) by P6's drift audit. Fix location
  resolved: Q2's Rust core (`theorem.rs` + `crossref/registry.rs`), not the shim.
- **The vendored Q1 source is pinned to release tag `v1.11.3`** (P4/P5, decided with Gordon) —
  confirmed byte-identical to the local checkout used throughout this review. Drift detection
  needs no new mechanism: P5's own Layer-1/Layer-2 contract tests are the tripwire, same
  philosophy P3 already established for its 3-file patch.
- **"Who owns a presentation default when both Q1 and Q2 independently compute one" resolved as
  a standing principle, not a case-by-case call** (P4/P5, decided with Gordon): Q2 owns any
  presentation default it has an opinion on, now or in the future. A full field census across
  all five Route-R types found exactly one live instance — Theorem's `kind` — resolved via a
  `crossref-<type>-title` param sourced from Q2's `RefTypeRegistry`, not via the Lua constructor
  (which has no field for it and would render "Theorem 1 (Theorem)" if forced through the wrong
  one). This closed every open design question each P-plan had explicitly flagged for itself as
  of 2026-09-17. **Corrected the same day (epic-wide review, I11): that's narrower than "the last
  open design question anywhere in P1–P8"** — a subsequent pass surfaced three more, none
  previously flagged by any plan, **all three since resolved the same day** (design doc §11): the
  docx/pptx `filename`-header content loss is an accepted upstream Q1 gap, filed as
  quarto-dev/quarto-cli#14906; whether Q2 owns section numbering for the Pandoc leg turned out to
  be a real, pre-existing, general Q2 gap unrelated to this epic specifically, filed as braid
  strand `bd-5aklrxgi`; and the golden-parity artifact strategy now has a concrete proposal (see
  P7). One item remains genuinely open: P1's
  still-unverified premise that the Footnotes B1/B2-4 split is as internally decoupled as it
  looks (P1's own checklist still has this open). None of these change the frozen decisions
  above; they're gaps in what's been *verified*, not reversals.

## Prioritization

- **First formats: docx, pptx** — no post-processing, Pandoc built-in templates, no
  reference-doc resolution, simple Meta; yet they still exercise the hard shared path (callouts
  and figures are both Route R — **corrected 2026-09-17, epic-wide review I4: callouts were
  never Route L** — + the numbering-suppression crux, category registry no longer part of it,
  see below).
- **latex: stub only** — KOMA template context and the richest `formatExtras` still make it worth
  scoping in the design even as a stub. **Corrected 2026-09-17 (epic-wide review M5): "latexmk
  Tier-2 compile" is the wrong headline reason** — P7 and the research doc both confirm
  `--to latex` emits `.tex` directly (the file extension wins over the inner `pdf` recipe, so no
  latexmk/tectonic step applies to this epic's stub at all; that belongs to `--to pdf`, out of
  scope). The stub decision itself is unaffected.
- Tiering: see the research doc's revised table.

## Plans

All eight have had a full audit (2026-09-17) against real Rust + real vendored Q1 Lua/TS —
"Reviewed" below means the *shape*, not the implementation, which is still all unchecked.

| Plan | Scope (as corrected) | Reviewed against | Depends on |
|---|---|---|---|
| **P1** | Neutral-core + 5-variant `PipelineProfile`; split Footnotes; concrete Pandoc exclude-list; `CalloutResolve` reclassified B4 in design §6; `AppendixStructure` resolved to **B3** (last open design cell, decided with Gordon); `ExampleEmbedRender` reclassified B4→format-parameterized B1 (closes a P5 open question, decided with Gordon) | HTML/reveal/preview byte-identity | — |
| **P2** | Wire-format schema v1: real 8-type inventory; hand-mirror (not codegen, decided with Gordon) + cross-consumer test | round-trip + preview parity | — |
| **P3** | Upstream Q1 `crossref-numbering: external`: two predicates (`assignCrossrefNumbers` / `crossref_present()`), 4 real render-decoration gate sites (not 2); fallback = carry the 3-file/~6-line patch indefinitely | Q1 suite + Q2 golden | — |
| **P4** | Vendored-Q1 run machinery; resolved all 3 deferred `QUARTO_FILTER_PARAMS` open questions from the TS research; needs a synthetic single-file "project" value (Q1 TS never treats a render as project-less); vendor pinned to release tag `v1.11.3`; owns the `crossref-<type>-title` params that make Q2's `RefTypeRegistry` authoritative over Q1's own locale defaults | "run main.lua → bytes" smoke | — |
| **P5** | Lua shim: Route R/**N** (N is new — 2 types have no Q1 handler at all; no Route-L type remains in the wire-format inventory); Tabset **and** Callout reclassified R (Callout found independently by P6); Proof/Theorem/FloatRefTarget field-maps audited against real constructors, not assumed mechanical; `ExampleEmbed` moved entirely out of scope (resolved in P1). **All four of this plan's open design questions are now closed** — nothing left but implementation. | per-type golden | P2, P4 |
| **P6** | Category passthrough (retitled from "seed" 2026-09-17 — no export needed at all: four independent Q1 mechanisms, none metadata-seedable for built-ins; `crossref.custom` passthrough already works) — narrowed to a passthrough test + numbering-suppression wiring; Callout reclassified R | figure/theorem/callout-number parity | P3, P5 |
| **P7** | Per-format tail + invocation builder — docx, then pptx; latex stub; pulled in already-complete TS-orchestration research | Q1 golden per format | P1,P2,P4,P5 |
| **P8** | content-hidden / `when-format` gating (deferred); fixed a stale premise from the shape draft | — | — |

**Parallelism (corrected 2026-09-17, epic-wide review I1 — the previous "P1–P5 parallelizable"
line was self-contradicted by this very table, which lists P5 as depending on P2 and P4):**
P1, P2, P3 parallelizable immediately once the frozen decisions land; P4 after P2 (can start
against the frozen schema decision before P2's implementation fully lands); P5 after P1 (the
`Pandoc`-kind exclude-list decision — specifically keeping `panel-tabset`'s sugar half enabled),
P2, and P4; P6 after P3 **and** P5 (P6 now explicitly reuses P5's Route-R post-construction
order-assignment mechanism for Callout, and the Callout reclassification lands in P5's shim
file); P7 after P1/P2/P4/P5 (P6 too, for correct numbers in its golden). P8 is independent to
start; its docx/pptx smoke-fixture item is added to its own checklist once P7's tail exists (a
latent P7↔P8 cycle over this exact item, also found by this review, is now resolved that way).

### Implementation companions (2026-09-18)

Each plan above has a companion at
**`claude-notes/plans/2026-09-18-pandoc-hybrid-P<N>-implementation.md`** that converts its prose
Coarse checklist into `## Task N: <title>` units `superpowers:subagent-driven-development` can
dispatch one implementer per, and — per this repo's mandatory TDD rule — binds every test each
task needs to a named production seam and a named revert hunk *before* any code is written (the
`/prevalidating-test-seams` discipline). They add no scope: where a companion and its plan
disagree, the plan wins.

| Companion | Tasks | Notes |
|---|---|---|
| [`…-P1-implementation.md`](2026-09-18-pandoc-hybrid-P1-implementation.md) | 9 | no Pandoc tier, matching P1's scope |
| [`…-P2-implementation.md`](2026-09-18-pandoc-hybrid-P2-implementation.md) | 7 | adds a vitest tier for the cross-consumer test; Task 7 (`Meta` carriage) is reassigned from P4 and gates P4's transport smoke |
| [`…-P3-implementation.md`](2026-09-18-pandoc-hybrid-P3-implementation.md) | 8 | per-hunk "which tree" section (upstream / vendored / q2) |
| [`…-P4-implementation.md`](2026-09-18-pandoc-hybrid-P4-implementation.md) | 11 | names the task that unblocks the pandoc/Lua tier for P5–P7 |
| [`…-P5-implementation.md`](2026-09-18-pandoc-hybrid-P5-implementation.md) | 8 | discriminating-fixture section; the epic's densest vacuity risk |
| [`…-P6-implementation.md`](2026-09-18-pandoc-hybrid-P6-implementation.md) | 6 | what a revert hunk means for a "confirm the passthrough" test |
| [`…-P7-implementation.md`](2026-09-18-pandoc-hybrid-P7-implementation.md) | 12 | the only end-to-end (real-binary) tier; e2e coverage map |
| [`…-P8-implementation.md`](2026-09-18-pandoc-hybrid-P8-implementation.md) | 4 | revert hunks for a verification-only plan |

Each companion also carries a **Findings for Gordon** section — places where a test could not be
written the way its plan describes, or where a cited anchor had drifted. Three were decided on
2026-09-18 and are applied: P5's Layer-1 probe mechanism (mechanism (a), an env-gated dump inside
the shim), the missing callout `order == nil` guard (folded into P3's upstream patch as anchor
A7), and P8's format-variant matching gap (filed as `bd-n5hjadpr`, deliberately **not** fixed in
this epic — a pre-existing Q2 gap this epic only made visible). Two more were decided the same
day: P3's upstream PR now carries a **TypeScript plumb** for `crossref: numbering:` (its Task 8,
so the param is reachable by a real quarto-cli user and the external direction has upstream test
coverage), and P6's subfloat claim is re-scoped to **dormant**, with the underlying gap filed as
`bd-plcqhfcn` under the crossref epic `bd-jsbg`. **No findings remain open.**

## Definition of done (epic)

- docx + pptx render end-to-end through the hybrid with golden-parity to Q1; latex stubbed.
- Crossref/callout/theorem **numbers** computed once and identical across HTML and Pandoc
  formats. **Corrected 2026-09-18** (was "semantics... identical," found too strong by a
  user-facing/backward-compatibility review): *presentation* (crossref prefixes, title-delim,
  ref-hyperlink, labels) legitimately differs — the Pandoc leg gets Q1's fuller behavior for
  free, Q2's own HTML renderer doesn't (yet) match it. Tracked as `bd-wqdi1pd2`, not epic scope;
  see design doc §12.
- **Known, accepted v1 limitations — see design doc §12 for the full list and rationale**:
  mermaid diagrams have no rendering story for a Pandoc target (`bd-h1ub8f8z`); a `format:`
  block declaring more than one key renders only one, now with a warning (design doc §14);
  project-mode (website/book/manuscript) rendering to a Pandoc target is out of scope (design
  doc §13, now with a containment gate landed in P7, decided with Gordon); a `Post`-position user
  Lua filter can't see custom-node content (`bd-o90yz5mg`).

  **What "docx/pptx support" means in v1 (added 2026-09-18, round 4 review — a systemic-ripple
  review found this bullet's four-of-seven sample reads materially rosier than design doc §12 in
  full, which a PM writing release notes or a support engineer triaging "my docx looks wrong"
  would read from this section alone).** The full, honest envelope: single-document docx and
  pptx, with crossref/callout/theorem **numbers** correct and identical to HTML, and Q1's fuller
  crossref/callout **presentation** for free (prefixes, hyperlinks, delimiters) — minus: **no**
  mermaid rendering (diagram source text appears instead, captured as one labeled golden
  divergence, not a runtime warning); **no** section numbers (`number-sections`/`@sec-` refs,
  `bd-5aklrxgi`); **no** code-block `filename` headers (upstream Q1 gap,
  quarto-cli#14906); listing pages render prose-only; **no** project-mode rendering (websites,
  books, manuscripts); one format rendered per invocation, now with a warning if `format:`
  declares more; `Post`-position user Lua filters can't see inside custom-node content; an
  explicit `crossref:` presentation override (e.g. `fig-prefix`) is honored on the Pandoc leg and
  silently ignored on Q2's own HTML leg, with no diagnostic in either (`bd-wqdi1pd2`); and a
  callout whose crossref category Q2 knows and Q1 doesn't renders unnumbered, with a warning, and
  any reference to it cites a number that appears nowhere in the document. This is a real,
  useful, shippable v1 — it is not "docx/pptx support" in the sense a Quarto 1 user would hear
  the phrase, and release notes drawn from this bullet alone (rather than design doc §12 in full)
  would need correcting.
- The wire format is a versioned shared contract with an enforced neutral-core invariant,
  including P5's confirmed extension request (`Proof`'s `plain_data.type` field — without it
  Q1's constructor crashes on any Proof node).
- `theorem.rs`'s `THEOREM_CLASSES` gap (missing `algorithm`, found by P5) is closed before
  docx/pptx v1 ships if any in-scope test fixture uses `.algorithm` — otherwise track as an
  explicit follow-on. **Ownership fixed 2026-09-17 (epic-wide review, I10):** P5 disowns the
  implementation twice in its own text; P7 owns evaluating the condition (it owns the fixture
  set) and filing the follow-on if the condition doesn't fire.
- Vendored filters pass `cargo xtask lint`; the `crossref-numbering` change is upstreamed or
  carried as a marked patch; `cargo xtask verify` green.
- Tier-2 post-processing (pdf/typst/epub) and Tier-3 (`confluence-publish`) tracked as follow-ons.
