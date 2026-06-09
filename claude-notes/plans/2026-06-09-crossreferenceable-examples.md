# Cross-referenceable Example blocks

**Strand:** bd-t3cert81 (discovered-from bd-z1smhvuo, the embed feature)
**Date:** 2026-06-09
**Status:** DESIGN — iterate before execution. No code yet.
**Builds on:** `claude-notes/plans/2026-06-09-website-example-iframe-embed.md`
(Phases 1–2: the `.embed-example-iframe` transform, staging, page-relative src).
**Crossref background:** `claude-notes/plans/2026-04-15-crossref-design.md`.

## Goal

Turn the `.embed-example-iframe` block into a **first-class, cross-referenceable
Quarto 2 type**, on the same footing as figures, tables, and theorems. When an
author gives an embed a crossref id, the example is **auto-numbered** and prose
can reference it:

```markdown
::: {.embed-example-iframe #PREFIX-03-fragments file="/examples/presentations/03-fragments/slides.html"}
[View source](https://github.com/quarto-dev/q2/tree/main/examples/presentations/03-fragments)
:::

As @PREFIX-03-fragments shows, fragments reveal content step by step.
```

`@PREFIX-03-fragments` resolves to an "Example 1" link, numbered and rendered
through the **same crossref index/resolve/render machinery** as `@fig-…` /
`@thm-…`. Without an id, the block stays a plain embed (today's Phase 1–2
behavior) — numbering is **opt-in via the id**, exactly like figures.

## How Quarto 2 crossrefs work (the integration contract)

Crossref-capable nodes all flow through one uniform contract: a `CustomNode`
whose `plain_data` carries the triple `{ref_type, kind, identifier}`. The
indexer/resolver/renderer have **zero type-specific code** — they key off that
triple. (`crates/quarto-core/src/crossref/target.rs:29`.)

Pipeline (main render path, `crates/quarto-core/src/pipeline.rs`):

1. **Sugar transforms** turn author syntax into canonical `CustomNode`s, each
   populating the triple:
   - `CalloutTransform`, `TheoremSugarTransform` (`transforms/theorem.rs`),
     `ProofSugarTransform`, `FloatRefTargetSugarTransform`
     (`transforms/float_ref_target.rs`), `EquationLabelTransform` —
     pipeline.rs:1056–1059.
2. **`CrossrefIndexTransform`** (`transforms/crossref_index.rs`, pipeline.rs:1062)
   walks in document order, assigns a per-`ref_type` counter + section number,
   writes `plain_data.order = {section, order}`, and records each `identifier`
   in a `CrossrefIndex`.
3. **`CrossrefResolveTransform`** (pipeline.rs:1063) finds `Cite` nodes whose id
   prefix matches a registered ref-type (`RefTypeRegistry::classify_cite_id`),
   looks them up in the index, and rewrites them to a
   `CustomNode("CrossrefResolvedRef")`.
4. **`CrossrefRenderTransform`** (`transforms/crossref_render.rs`, pipeline.rs:1144,
   finalization) turns crossref `CustomNode`s back into writer-visible shapes:
   floats → `Figure`/`Div` with a numbered caption ("Figure 1: …"); resolved
   refs → `Link(#id, "Figure\u{a0}1")`.

The **ref-type registry** (`crossref/registry.rs`) is the authoritative list of
prefixes. Built-ins are a hardcoded table (`registry.rs:78`); users can add
types via `crossref.custom` YAML or promise ids via `crossref.ids`
(`stage/stages/pre_engine_sugaring.rs:84`). A `RefTypeDef` is
`{ref_type, kind, source, source_info}`.

## ⚠️ The central conflict: `exm` is already taken

`exm` is **already a built-in ref-type** mapping to the theorem-like "Example"
environment (the `.example` class → kind "Example"):

- `crossref/registry.rs:90` → `("exm", "Example")`
- `transforms/theorem.rs:67` → `("example", "exm", "Example")`

So the user-proposed `@exm-03-fragments` would collide with theorem-examples,
and `TheoremSugarTransform` already claims `#exm-…` ids (its `match_theorem_id`)
— a `.embed-example-iframe #exm-foo` div would be grabbed as a *Theorem* before
our transform could see it. **The runnable-demo Example needs its own prefix.**

### Decision 1 — RESOLVED (user, 2026-06-09): prefix `demo`, kind `Demo`

Locked: **prefix `demo`, kind `Demo`**, caption template **"Demo N: …"**. The
user explicitly rejected having both `xmp` and `exm` as built-ins ("not good
enough"), and confirmed the 3-letter-prefix convention is an artificial Q1 limit
we need not keep. "Demo 1: …" reads slightly peculiarly but conveys the right
message and emphasizes the full runnable demo project. Constants:
`EXAMPLE_REF_TYPE = "demo"`, `EXAMPLE_KIND = "Demo"`.

**Integration confirmed (no crossref-core changes needed):**
- `CrossrefIndexTransform` numbers any CustomNode whose `plain_data` carries the
  triple — it keys off `has_crossref_plain_data` (crossref_index.rs:241), not a
  type-name list. A new `CustomNode("ExampleEmbed")` is numbered automatically.
- `CrossrefRenderTransform` dispatches on specific type-names
  (crossref_render.rs:143–149); `"ExampleEmbed"` falls through untouched, so our
  own render step owns it. `@demo-…` refs render via the generic
  `render_resolved_ref` once `demo` is in the registry.
- Ordering: the sugar must run **before** `FloatRefTargetSugarTransform`
  (pipeline.rs:1058) — once `demo` is registered, that transform would otherwise
  claim a `#demo-…` Div as a generic float. We move the embed sugar to just
  before `TheoremSugarTransform` (1056) and consume the Div there.

### (Historical) prefix options considered

Two sub-questions:

**(a) Prefix.** Needs to be free (not in `registry.rs:78`'s table) and read well
in prose. Candidates:

| Prefix | `@…` reads as | Notes |
| --- | --- | --- |
| **`xmp`** (recommended) | `@xmp-03-fragments` | Terse "example"; clearly prefix-shaped; free. |
| `demo` | `@demo-03-fragments` | Clearest about "runnable demo"; pairs with kind "Demo". |
| `eg` | `@eg-03-fragments` | "e.g." = for example; short, intuitive; unusual as a crossref prefix. |

**(b) Kind label** (the rendered word — "Example 1" vs "Demo 1"):

- **Reuse kind "Example"** → renders "Example 1". Matches the user's wording, but
  a single document that *also* used theorem-`exm` examples would then show two
  independent "Example N" sequences (confusing). In the docs website this is
  only theoretical — those pages don't use math-style theorem-examples.
- **New kind "Demo"** → renders "Demo 1". Zero ambiguity with theorem-examples,
  but changes the noun from the user's "Example".

**Recommendation:** prefix `xmp`, kind **"Example"** (honor the user's wording;
the double-"Example" risk is theoretical for the docs use case). Fall back to
prefix `demo` + kind "Demo" if the user wants guaranteed zero ambiguity. This is
the one decision to lock before coding.

## Architecture: make the embed a crossref float

An embed-example is structurally a **float** — a container (the iframe) plus a
numbered caption — exactly like a figure. So it should follow the
`FloatRefTarget` route, *not* the theorem route (no title/body theorem shape)
and *not* `crossref.custom` YAML (this is a built-in type, always available).

### Restructure the transform (sugar → CustomNode → render)

Phase 1 built `ExampleEmbedTransform` as an **immediate** `Div → iframe RawBlock`
rewrite running early (pipeline.rs:1011, before the crossref phase). For
crossref participation the example must be visible to `CrossrefIndexTransform`
as a `CustomNode` carrying the triple. So split it (mirroring
Callout → CalloutResolve, and the float sugar → render split):

1. **Sugar step — `ExampleEmbedSugarTransform`** (runs in the sugar phase,
   alongside / before the float sugar at pipeline.rs:1058, and crucially
   **before** `TheoremSugarTransform` so a stray theorem match can't claim it):
   - Match `Div.embed-example-iframe`.
   - Validate `file=` (the Phase-1 static-only contract: reject `.qmd` → Q-5-5;
     missing → Q-5-4). Carry `file`, sizing, `title` in `plain_data`/slots.
   - **If the div has an id with the new prefix** (`#xmp-…`): produce a
     `CustomNode("ExampleEmbed")` with `plain_data = {ref_type: "xmp", kind:
     "Example", identifier: "xmp-…"}` + slots `{caption?, source_link}`. This is
     what the crossref index numbers.
   - **If no crossref id**: produce the same `CustomNode` but with an **empty
     triple** (like `ProofSugarTransform` leaves `ref_type` unset, `proof.rs`),
     so the indexer skips it — an unnumbered plain embed.
2. **Index/resolve (unchanged):** `CrossrefIndexTransform` numbers any
   `ExampleEmbed` whose `ref_type` is set and writes `plain_data.order`;
   `CrossrefResolveTransform` resolves `@xmp-…` cites → `CrossrefResolvedRef`
   (this already works for any registered prefix — only the registry entry is
   needed).
3. **Render step — `ExampleEmbedRenderTransform`** (finalization, near
   `CrossrefRenderTransform`, pipeline.rs:1144): turn `CustomNode("ExampleEmbed")`
   into the final markup — the `<iframe>` (page-relative `src` via the Phase-2
   `resolve_static_resource_href`), a numbered **"Example N: caption"** caption
   when numbered (read from `plain_data.order`), and the source link. Without a
   number, emit today's plain container.

> Why a dedicated render step rather than teaching `CrossrefRenderTransform`'s
> float renderer to emit an iframe: keeps crossref-render generic (it knows
> figures/divs, not iframes), and keeps all embed-specific HTML in one module.
> The shared machinery only does numbering + ref-link text.

### Registry registration

Add the chosen prefix to the built-in table (`crossref/registry.rs:78`), e.g.
`("xmp", "Example")`. That single entry makes `@xmp-…` classify as a crossref
(not a citation) everywhere — index, resolve, and the `RefTypeRegistry` seeded
in `pre_engine_sugaring.rs`.

### Caption source (Decision 2 — minor)

A numbered float needs caption text ("Example 1: <caption>"). Where from?
Options: (a) a `title=`/caption attribute on the div; (b) the fallback link's
text; (c) a dedicated caption paragraph in the div body (like figure captions).
**Lean:** support an optional caption (attribute or a caption para); when absent,
render a bare "Example 1" with no trailing text (figures allow caption-less
numbering too). Decide during execution.

## Interaction with Phases 1–2 (backward compatible)

- The static-only contract, `file=`, staging (`cargo xtask stage-doc-examples`),
  and page-relative `src` (`resolve_static_resource_href`) all carry over
  unchanged — they move into the new render step.
- **No-id embeds render exactly as today** (plain container). Crossref is purely
  additive.
- The 8 migrated docs placeholders keep working; we can opt a few into ids to
  demonstrate `@xmp-…` once the type lands.

## Open questions for the user

1. **Decision 1 (blocking): prefix + kind.** `xmp` + "Example" (recommended) vs
   `demo` + "Demo" (zero ambiguity) vs `eg` + "Example". `exm` is unavailable.
2. **Caption source / shape** (Decision 2): attribute vs caption-paragraph vs
   reuse the source-link text; and whether caption-less numbering is allowed.
3. **Scope of "Example" as a type:** built-in (always available, recommended) vs
   docs-only via `crossref.custom`. Built-in is the natural home for a
   Quarto-wide feature.
4. Should the rendered caption hyperlink the example (anchor target for the
   `@ref` jump), and where does the `#xmp-…` anchor live in the DOM?

## Phasing (TDD-first) — Decision 1 = `demo`/"Demo"

### Phase A — registry + ref-type plumbing ✅
- [x] Added `("demo", "Demo")` to the built-in ref-type table
  (`crossref/registry.rs`); unit test `builtin_has_demo_distinct_from_exm`
  asserts `classify_cite_id("demo-…")` resolves and stays distinct from `exm`.

### Phase B — sugar transform (crossref-aware CustomNode) ✅
- [x] `ExampleEmbedTransform` rewritten as a **sugar** step:
  `Div.embed-example-iframe` → `CustomNode("ExampleEmbed")`; triple populated
  only when `file=` validates **and** the id is `demo-…`; no/foreign id → no
  triple (indexer skips). Registered **before** the theorem/float sugar so a
  `#demo-…` div is consumed here (never claimed as a generic float).
- [x] Moved the `file=` validation (Q-5-4/Q-5-5) into the sugar step.
- [x] Unit tests: sugar produces the node; demo-id → triple; non-demo id → no
  triple; missing/`.qmd` file → diagnostic + unnumbered.

### Phase C — numbering + reference resolution ✅
- [x] **No crossref-core changes** — `CrossrefIndex`/`Resolve`/`Render` handle
  `ExampleEmbed` via the generic triple path. Integration fixtures
  (`crossref_fixtures.rs`): two demos → order 1 & 2; `@demo-frag` resolves (no
  miss diagnostic); `demo` counter independent of theorem `exm`; an embed with
  no `demo-` id is not indexed.

### Phase D — render step ✅
- [x] `ExampleEmbedRenderTransform` (finalization, after `CrossrefRender`):
  numbered "Demo N: caption" + iframe (page-relative src) + source link; no
  order → plain container (Phase-1 parity); container carries the `#demo-…`
  anchor. Unit tests cover numbered/unnumbered/bad-file/nested.
- [x] End-to-end through `q2 render` (HTML **and** `--to revealjs`): `#demo-…`
  examples render `Demo N:` captions and `@demo-…` → `quarto-xref` "Demo N"
  links to the anchor. Full `quarto-core` suite green (2252) + 36 crossref
  fixtures.

### Phase E — docs demonstration ✅
- [x] Opted the fragments example in `docs/presentations/revealjs/index.qmd`
  into `#demo-fragments` with a caption, and referenced it in prose
  (`@demo-fragments`). `q2 render docs/` verified: `@demo-fragments` →
  "Demo 1" xref link, caption "Demo 1: …", `id="demo-fragments"` anchor; the
  other 7 examples stay unnumbered. (Numbering all examples / a usage section on
  the feature page is a docs-content follow-up.)

## Out of scope

- The preview-mode (`q2 preview`) verification of Phases 1–2 (tracked there).
- The Lua API entry point for `resolve_static_resource_href` (bd-cic0dfdp).
- Changing the existing theorem-`exm` "Example" environment.
