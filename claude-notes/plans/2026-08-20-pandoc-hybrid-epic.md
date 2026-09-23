# Epic: All output formats via a Pandoc-writer hybrid

**Date:** 2026-09-20
**Status:** Shape drafts reviewed; ready for implementation ordering.
**Design (authoritative):** [`../designs/pandoc-hybrid-architecture.md`](../designs/pandoc-hybrid-architecture.md)
**Research:** [`../research/2026-07-09-q1-filter-catalog.md`](../research/2026-07-09-q1-filter-catalog.md),
[`../research/2026-07-13-q1-format-typescript.md`](../research/2026-07-13-q1-format-typescript.md)

## The problem

Q2 today produces only HTML and revealjs; every other `--to` target is rejected
(`crates/quarto/src/commands/render.rs:680-684`; `FormatIdentifier::is_native()` at
`crates/quarto-core/src/format.rs:61-63`). Q1 supports all formats by leaning on **Pandoc's
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
numbers) / Route N ("no Q1 handler; resolve or render directly," for types with no Q1 `ast_name`
at all — `Equation`, `CrossrefResolvedRef`); upstream Q1 `crossref-numbering: external`; no
Pandoc bundling + min version, native build only; versioned single-sourced wire schema.

## Key architecture facts

- **Route-L/R table** (design doc §3): `Tabset` and `Callout` are both Route R — both carry real
  numbering that Route L's raw-Div reconstruction would silently discard. **The wire-format
  inventory has no Route-L type at all** — every type P5's shim handles is Route R or Route N.
- **Category registry is a passthrough, not a seed.** Built-in categories are split across four
  independent, non-metadata-seedable Lua mechanisms; the one metadata-driven case
  (`crossref.custom`) already passes through Q2's metadata merge untouched and is already read
  unconditionally by Q1's own `custom.lua`. P6 confirms the passthrough with a test.
- **`enable-crossref`'s gate surface is two structurally different predicates**: an
  assign-numbers gate (`if enableCrossRef then`, at `main.lua:718`; opposite polarity) and four
  present-numbers gates (P3).
- **Two wire types have no Q1 counterpart at all** (P5: `Equation`, `CrossrefResolvedRef` — Q1
  resolves both directly to inlines, no persisted node — this is Route N). `ExampleEmbed` is not
  a Route N case: its iframe is emitted as a Pandoc `RawBlock("html", ...)` — the same primitive
  Q1's own docs use for the identical hand-authored pattern — so `example-embed-render` is
  classified format-parameterized B1 and resolved entirely in Rust: it runs for `Pandoc(fmt)`
  too, emitting everything but the iframe. `ExampleEmbed` never reaches P5's shim for Pandoc
  targets at all.
- `Proof` needs a `plain_data.type` field (P5/P2) — its Q1 constructor crashes without one.
- `algorithm`/`alg` is a confirmed gap in Q2's `theorem.rs` `THEOREM_CLASSES` (P5), owned by
  Q2's Rust core (`theorem.rs` + `crossref/registry.rs`), not the shim. P7 owns evaluating
  whether any in-scope fixture uses it and filing the follow-on if not.
- **The vendored Q1 source is pinned to release tag `v1.11.3`** (P4/P5) — byte-identical to the
  local checkout. Drift detection needs no new mechanism: P5's own Layer-1/Layer-2 contract tests
  are the tripwire, same philosophy P3 already established for its 3-file patch.
- **Standing principle for presentation defaults:** Q2 owns any presentation default it has an
  opinion on, now or in the future. The one live instance across all Route-R types is Theorem's
  `kind`, resolved via a `crossref-<type>-title` param sourced from Q2's `RefTypeRegistry` (not
  via the Lua constructor, which has no field for it).
- The docx/pptx `filename`-header content loss is an accepted upstream Q1 gap, filed as
  quarto-dev/quarto-cli#14906. Q2 owning section numbering for the Pandoc leg is a real,
  pre-existing, general Q2 gap unrelated to this epic, filed as braid strand `bd-5aklrxgi`. The
  golden-parity artifact strategy has a concrete proposal (see P7).
- **Open:** P1's premise that the Footnotes B1/B2-4 split is as internally decoupled as it looks
  is still unverified (see P1's own checklist).
- Two gaps surfaced during implementation-task review are tracked outside this epic: P8's
  format-variant matching gap (pre-existing Q2 gap, filed as `bd-n5hjadpr`, not fixed here); P6's
  subfloat claim is dormant, with the underlying gap filed as `bd-plcqhfcn` under crossref epic
  `bd-jsbg`.

## Prioritization

- **First formats: docx, pptx** — no post-processing, Pandoc built-in templates, no
  reference-doc resolution, simple Meta; yet they still exercise the hard shared path (callouts
  and figures are both Route R + the numbering-suppression crux; category registry is a
  passthrough, not part of it).
- **latex: stub only** — KOMA template context and the richest `formatExtras` still make it worth
  scoping in the design even as a stub. `--to latex` emits `.tex` directly (the file extension
  wins over the inner `pdf` recipe, so no latexmk/tectonic step applies to this epic's stub; that
  belongs to `--to pdf`, out of scope).
- Tiering: see the research doc's revised table.

## Plans

"Reviewed against" below means the *shape*, not the implementation, which is still all unchecked.

| Plan | Scope | Reviewed against | Depends on |
|---|---|---|---|
| **P1** | Neutral-core + 5-variant `PipelineProfile`; split Footnotes; concrete Pandoc exclude-list; `CalloutResolve` classified B4 in design §6; `AppendixStructure` is **B3**; `ExampleEmbedRender` classified B4→format-parameterized B1 | HTML/reveal/preview byte-identity | — |
| **P2** | Wire-format schema v1: real 8-type inventory; hand-mirror (not codegen) + cross-consumer test | round-trip + preview parity | — |
| **P3** | Upstream Q1 `crossref-numbering: external`: two predicates (`assignCrossrefNumbers` / `crossref_present()`), 4 real render-decoration gate sites; fallback = carry the 3-file/~6-line patch indefinitely | Q1 suite + Q2 golden | — |
| **P4** | Vendored-Q1 run machinery; resolves the `QUARTO_FILTER_PARAMS` open questions from the TS research; needs a synthetic single-file "project" value (Q1 TS never treats a render as project-less); vendor pinned to release tag `v1.11.3`; owns the `crossref-<type>-title` params that make Q2's `RefTypeRegistry` authoritative over Q1's own locale defaults | "run main.lua → bytes" smoke | — |
| **P5** | Lua shim: Route R/N (2 types have no Q1 handler at all; no Route-L type remains in the wire-format inventory); Tabset **and** Callout classified R; Proof/Theorem/FloatRefTarget field-maps audited against real constructors; `ExampleEmbed` out of scope (resolved in P1) | per-type golden | P2, P4 |
| **P6** | Category passthrough (no export needed — four independent Q1 mechanisms, none metadata-seedable for built-ins; `crossref.custom` passthrough already works) — a passthrough test + numbering-suppression wiring; Callout classified R | figure/theorem/callout-number parity | P3, P5 |
| **P7-foundation** | Format-agnostic CLI plumbing: the `render.rs` native-format gate relaxation + routing through `render_qmd_to_pandoc`, the multi-format-render warning (§14), the project-mode containment gate (§13), and B3 shared-services wiring (resource staging, link rewriting) — none of it docx/pptx-specific. Needed by P7 and by any later format follow-on (typst, epub). | structural (CLI reachability), no golden | P1, P2, P4 |
| **P7** | Per-format tail + invocation builder — docx, then pptx; latex stub; pulled in already-complete TS-orchestration research. The docx/pptx-specific facts (defaults, forwarding allow-list, callout icons, `Meta` mapping) plus the golden-parity harness. | Q1 golden per format | P7-foundation, P1, P2, P4, P5 |
| **P8** | content-hidden / `when-format` gating — **complete 2026-09-20**, all 4 tasks done (Tasks 1-3 verification + Task 4's docx/pptx smoke fixture, once unblocked by P7-foundation's Task 3 and P7's Task 4) | per-target Pandoc render + sentinel inspection | P7-foundation, P7 (Task 4 only) |

**Parallelism.** P1, P2, P3 parallelizable immediately once the frozen decisions land; P4 after
P2 (can start against the frozen schema decision before P2's implementation fully lands); P5
after P1 (the `Pandoc`-kind exclude-list decision — specifically keeping `panel-tabset`'s sugar
half enabled), P2, and P4; P6 after P3 **and** P5 (P6 reuses P5's Route-R post-construction
order-assignment mechanism for Callout, and the Callout reclassification lands in P5's shim
file); **P7-foundation is startable immediately, after P1/P2/P4 only** — in parallel with P5
(and with P8's Tasks 1-3, and with anything else already unblocked); P7 after P7-foundation,
P1/P2/P4/P5 (P6 too, for correct numbers in its golden). P8's Tasks 1-3 are independent to start;
its docx/pptx smoke-fixture item (Task 4) is added once P7-foundation's Task 3 and P7's Task 4
both exist.

**Typst and epub** (see their own follow-on plans) depend on P7-foundation directly for the
CLI-gate/invocation-builder pattern. Their implementation may target
`feature/pandoc-writer-hybrid` (this epic's integration branch) directly, in parallel with
P5/P6/P7 — it does not need to wait for a `main` merge. This does not change either plan's
technical dependency graph (still P7-foundation + P1/P2/P4 for core wiring, P3/P6 added for the
crossref/numbering verification bullet) — only where the resulting commits land.

### Implementation companions

Each plan above has a companion at
**`claude-notes/plans/2026-09-18-pandoc-hybrid-P<N>-implementation.md`** (P7-foundation's
companion is dated 2026-09-20) that converts its prose Coarse checklist into `## Task N: <title>`
units `superpowers:subagent-driven-development` can dispatch one implementer per, and — per this
repo's mandatory TDD rule — binds every test each task needs to a named production seam and a
named revert hunk *before* any code is written (the `/prevalidating-test-seams` discipline). They
add no scope: where a companion and its plan disagree, the plan wins.

| Companion | Tasks | Notes |
|---|---|---|
| [`…-P1-implementation.md`](2026-09-18-pandoc-hybrid-P1-implementation.md) | 9 | no Pandoc tier, matching P1's scope |
| [`…-P2-implementation.md`](2026-09-18-pandoc-hybrid-P2-implementation.md) | 7 | adds a vitest tier for the cross-consumer test; Task 7 (`Meta` carriage) is reassigned from P4 and gates P4's transport smoke |
| [`…-P3-implementation.md`](2026-09-18-pandoc-hybrid-P3-implementation.md) | 8 | per-hunk "which tree" section (upstream / vendored / q2) |
| [`…-P4-implementation.md`](2026-09-18-pandoc-hybrid-P4-implementation.md) | 11 | names the task that unblocks the pandoc/Lua tier for P5–P7 |
| [`…-P5-implementation.md`](2026-09-18-pandoc-hybrid-P5-implementation.md) | 8 | discriminating-fixture section |
| [`…-P6-implementation.md`](2026-09-18-pandoc-hybrid-P6-implementation.md) | 6 | what a revert hunk means for a "confirm the passthrough" test |
| [`…-P7-foundation-implementation.md`](2026-09-20-pandoc-hybrid-P7-foundation-implementation.md) | 4 | format-agnostic CLI plumbing (gate relaxation, multi-format warning, project-mode containment, B3 services) that P7, typst, and epub all build on; depends only on P1/P2/P4 |
| [`…-P7-implementation.md`](2026-09-18-pandoc-hybrid-P7-implementation.md) | 8 | the only end-to-end (real-binary) tier; e2e coverage map |
| [`…-P8-implementation.md`](2026-09-18-pandoc-hybrid-P8-implementation.md) | 4 | revert hunks for a verification-only plan |

## Definition of done (epic)

- docx + pptx render end-to-end through the hybrid with golden-parity to Q1; latex stubbed.
- Crossref/callout/theorem **numbers** computed once and identical across HTML and Pandoc
  formats. *Presentation* (crossref prefixes, title-delim, ref-hyperlink, labels) legitimately
  differs — the Pandoc leg gets Q1's fuller behavior for free, Q2's own HTML renderer doesn't
  (yet) match it. Tracked as `bd-wqdi1pd2`, not epic scope; see design doc §12.
- **Known, accepted v1 limitations — see design doc §12 for the full list and rationale**:
  mermaid diagrams have no rendering story for a Pandoc target (`bd-h1ub8f8z`); a `format:` block
  declaring more than one key renders only one, with a warning (design doc §14); project-mode
  (website/book/manuscript) rendering to a Pandoc target is out of scope, gated in P7-foundation
  (design doc §13); a `Post`-position user Lua filter can't see custom-node content
  (`bd-o90yz5mg`).

  **What "docx/pptx support" means in v1.** The full, honest envelope: single-document docx and
  pptx, with crossref/callout/theorem **numbers** correct and identical to HTML, and Q1's fuller
  crossref/callout **presentation** for free (prefixes, hyperlinks, delimiters) — minus: **no**
  mermaid rendering (diagram source text appears instead, captured as one labeled golden
  divergence, not a runtime warning); **no** section numbers (`number-sections`/`@sec-` refs,
  `bd-5aklrxgi`); **no** code-block `filename` headers (upstream Q1 gap, quarto-cli#14906);
  listing pages render prose-only; **no** project-mode rendering (websites, books, manuscripts);
  one format rendered per invocation, with a warning if `format:` declares more; `Post`-position
  user Lua filters can't see inside custom-node content; an explicit `crossref:` presentation
  override (e.g. `fig-prefix`) is honored on the Pandoc leg and silently ignored on Q2's own HTML
  leg, with no diagnostic in either (`bd-wqdi1pd2`); and a callout whose crossref category Q2
  knows and Q1 doesn't renders unnumbered, with a warning, and any reference to it cites a number
  that appears nowhere in the document.
- The wire format is a versioned shared contract with an enforced neutral-core invariant,
  including `Proof`'s `plain_data.type` field (without it Q1's constructor crashes on any Proof
  node).
- `theorem.rs`'s `THEOREM_CLASSES` gap (missing `algorithm`) is closed before docx/pptx v1 ships
  if any in-scope test fixture uses `.algorithm` — otherwise track as an explicit follow-on. P7
  owns evaluating the condition (it owns the fixture set) and filing the follow-on if the
  condition doesn't fire.
- Vendored filters pass `cargo xtask lint`; the `crossref-numbering` change is upstreamed or
  carried as a marked patch; `cargo xtask verify` green.
- Tier-2 post-processing (pdf/typst/epub) and Tier-3 (`confluence-publish`) tracked as follow-ons.
