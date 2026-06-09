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

### Decision 1 (OPEN — user's call): prefix + kind

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

## Phasing (TDD-first, once Decision 1 is locked)

### Phase A — registry + ref-type plumbing
- [ ] Add the chosen prefix to the built-in ref-type table; unit test that
  `classify_cite_id("xmp-foo")` resolves and `@xmp-foo` is treated as a crossref.

### Phase B — sugar transform (crossref-aware CustomNode)
- [ ] TDD: `Div.embed-example-iframe #xmp-foo` → `CustomNode("ExampleEmbed")`
  with the triple populated; no-id div → empty triple (skipped by indexer);
  ensure `TheoremSugarTransform` does **not** claim it (ordering / class guard).
- [ ] Move the Phase-1 `file=` validation (Q-5-4/Q-5-5) into the sugar step.

### Phase C — numbering + reference resolution
- [ ] Integration test: two numbered examples → "Example 1", "Example 2";
  `@xmp-foo` in prose → `Link(#xmp-foo, "Example\u{a0}1")`; a dangling
  `@xmp-missing` emits the standard crossref miss diagnostic.

### Phase D — render step
- [ ] `ExampleEmbedRenderTransform`: numbered "Example N: caption" + iframe
  (page-relative src) + source link; no-id → plain container (Phase-1 parity).
- [ ] End-to-end through `q2 render docs/` (HTML **and** revealjs): a doc with
  `#xmp-…` ids + `@xmp-…` refs renders numbered examples and working ref links;
  inspect the HTML / browser.

### Phase E — docs + wrap
- [ ] Opt one or two revealjs-doc examples into ids and reference them in prose
  as a live demonstration; document the `.embed-example-iframe` + `#xmp-` +
  `@xmp-` usage on the feature page.

## Out of scope

- The preview-mode (`q2 preview`) verification of Phases 1–2 (tracked there).
- The Lua API entry point for `resolve_static_resource_href` (bd-cic0dfdp).
- Changing the existing theorem-`exm` "Example" environment.
