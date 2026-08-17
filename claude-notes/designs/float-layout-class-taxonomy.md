# Float / layout DOM class taxonomy for HTML output (bd-hcp8m3ve)

**Date:** 2026-07-21
**Strand:** bd-hcp8m3ve (feature; `related` to epic bd-4doe9lvt)
**Unblocks:** bd-9fz5fweg (figures/floats/layout CSS port)
**Decision (Carlos, 2026-07-21):** match Q1's class names **verbatim** — this
minimizes CSS *and* DOM churn for projects migrating from Q1 (user CSS,
filters, and themes that select on `.quarto-figure`/`.quarto-layout-*` keep
working).

## Why this exists

Q2's crossref renderer deliberately emits native `<figure>` with no classes
("avoids needing a CSS class taxonomy right away",
`crates/quarto-core/src/transforms/crossref_render.rs:232`), and
`layout-ncol` divs pass through as `<div data-layout-ncol="2">`. The
bd-eias3e39 audit showed the whole figures/floats/layout family of
`_quarto-rules.scss` (and swaths of `_quarto.scss` already sitting dead in
`_bootstrap-rules.scss`) is blocked on this one decision, not on individual
rules.

## The Q1 DOM contract (measured from quarto-cli source)

Sources: `src/resources/filters/customnodes/floatreftarget.lua`
(`float_reftarget_render_html_figure`, `create_figcaption`),
`src/resources/filters/layout/html.lua` (`PanelLayout` renderer,
`renderHtmlFigure`).

### 1. Crossref float (figure kind)

```html
<div id="fig-x" class="quarto-float quarto-figure quarto-figure-{left|center|right}
                       [listing] [forwarded caption/column classes]"
     [style from image]>
  <figure class="quarto-float quarto-float-{fig|tbl|lst|<custom>}">
    <div aria-describedby="fig-x-caption-0ceaefa1-69ba-4598-a22c-09a6ac19f8ca">
      <!-- float content; inner image captions stripped -->
    </div>
    <figcaption id="fig-x-caption-0ceaefa1-69ba-4598-a22c-09a6ac19f8ca"
                class="quarto-float-caption-{top|bottom|margin}
                       quarto-float-caption quarto-float-{fig|…}
                       [quarto-uncaptioned]">
      Figure 1: …
    </figcaption>
  </figure>
</div>
```

Notes:
- The outer div carries the crossref id and **all three** of `quarto-float`,
  `quarto-figure`, `quarto-figure-<align>`; align comes from `fig-align`
  (default `center`), read off the contained Image.
- The `<figure>` carries `quarto-float` + `quarto-float-<ref_type>` — this is
  the `figure.quarto-float-tbl` that `_quarto-rules.scss:243–253` selects on.
- Subfloats swap `quarto-float-<ref>` → `quarto-subfloat-<ref>` and the
  caption gains `quarto-subfloat-caption`.
- The figcaption id suffix is a **fixed uuid constant**
  (`0ceaefa1-69ba-4598-a22c-09a6ac19f8ca`, `floatreftarget.lua:703`) to avoid
  colliding with user ids; `aria-describedby` on the content wrapper points at
  it. **Q2 drops the uuid** — see "The figcaption-uuid finding" below.
- Listings force `align=left` and add class `listing`.

### 2. Standalone (non-crossref) captioned figure

`renderHtmlFigure` produces the same outer shape without the float classes:
`<div class="quarto-figure quarto-figure-<align>"><figure>…<figcaption>…` —
the wrapper is added only if no `quarto-figure-*` class is already present.

### 3. Layout panel

```html
<div id="…" class="quarto-layout-panel [user classes]">
  <div class="quarto-layout-row [quarto-layout-valign-{top|bottom|center}]">
    <div class="quarto-layout-cell [quarto-layout-cell-subref]"
         style="flex-basis: 50.0%;justify-content: flex-start;">…</div>
    …
  </div>…
</div>
```

- Cell width → `flex-basis` (computed from `layout`/`layout-ncol`);
  `layout-align` → `justify-content`; `layout-valign` → the row class.
- A *captioned/id'd* panel is first wrapped as a FloatRefTarget, rendered as
  shape 1, then its outer attr is **replaced** by
  `(id, {quarto-layout-panel[, margin-caption]})`.
- Table-containing cells with a float parent get `data-ref-parent` (the
  `quarto-layout-cell[data-ref-parent]` rules already ported into
  `_bootstrap-rules.scss` select on this).

## Q2 target architecture

### Where the classes get made

Follow the transform-pipeline-phases contract: this is **format-specific
presentation** consuming resolved crossref structure, so it happens in
**`Finalization`, at/after `crossref-render`** — concretely, in
`render_float_ref_target` (crossref_render.rs), which already runs per-render
and is the single place floats become writer-visible.

### Representation: attributed native nodes, no raw HTML

Mechanism options considered:

- **(A) Transform-side construction with native nodes** *(recommended)*:
  `render_float_ref_target` emits the Q1 shape as `Div(outer) >
  Figure(attrred) > [Div(content), caption]`, using `Block::Figure`'s existing
  `attr` for the `<figure>` classes. The one gap: Pandoc's `Caption` carries
  no attr, so the **writers** synthesize the figcaption id/classes from
  metadata the transform leaves on the Figure attr (see below).
- (B) RawBlock `<figure>`/`<figcaption>` construction: no writer changes, but
  opaque to the preview React renderer (kills component rendering, block
  editing, and richtext for figures). Rejected.
- (C) Extend `quarto-pandoc-types` `Caption` with an `Attr`: cleanest model
  but diverges from Pandoc JSON — a wire-format break for filters. Rejected
  for now; can be revisited if figcaption metadata outgrows the kv scheme.

For (A), the transform sets on the Figure's attr kvs (consumed and stripped by
both writers, never emitted as HTML attributes). Everything expressible as a
native attr — the outer div's classes, the `<figure>` classes, the content
wrapper's `aria-describedby` — is set directly by the transform; the kvs exist
only for figcaption synthesis and placement, which Pandoc's attr-less
`Caption` cannot carry. The complete list (Q1 FloatRefTarget counterpart in
parens):

| kv | value | Q1 source | drives |
| --- | --- | --- | --- |
| `data-qf-ref-type` | `fig`/`tbl`/`lst`/custom kind | `ref_type_from_float(float)` (crossref category) | `quarto-float-<ref>` on the figcaption (the `<figure>` copy is a plain class set by the transform) |
| `data-qf-caption-location` | `top`/`bottom`/`margin` | `cap_location(float)` (`fig-cap-location` etc.) | `quarto-float-caption-<loc>` class + whether `<figcaption>` is written before or after content |
| `data-qf-caption-id` | the collision-checked id | `float.identifier .. "-caption-" .. uuid` | `id=` on the figcaption; must equal the `aria-describedby` the transform already set |
| `data-qf-uncaptioned` | `1`/absent | `float.is_uncaptioned` | `quarto-uncaptioned` class |
| `data-qf-subfloat` | `1`/absent | `float.parent_id ~= nil` | `quarto-subfloat-caption` + `quarto-subfloat-<ref>` in place of the `quarto-float-*` pair |

Five kvs total; `data-qf-caption-id` is the one added by the uuid decision
below (Q1 derived the id inline; Q2 computes it transform-side where the
document-wide id set is available, so the writer must receive the chosen
value rather than re-derive it).

Both consumers implement the same small synthesis: the pampa HTML writer
(`writers/html.rs` Figure arm) and the preview React renderer
(`preview-renderer/src/q2-preview/blocks/Figure.tsx`). The synthesis is ~30
lines each and locked by parity tests (writer snapshot ↔ React render on the
same AST).

### What stays out of scope here

- The **layout engine** (parsing `layout`/`layout-ncol`/`layout-valign`,
  width math, row splitting) is its own feature — filed as a sub-strand;
  shapes 3's classes are its acceptance contract.
- The CSS itself: bd-9fz5fweg ports the rules *after* this strand's DOM
  lands (its `blocks` dep already points here).
- anchorjs (`.quarto-figure > .anchorjs-link`) stays in the backlog strand;
  the `div[id^="tbl-"] { position: relative }` positioning context becomes
  live automatically once table floats are figure-wrapped.

## Proposed phases (each TDD; test-first)

1. **Figure floats** — outer div (`quarto-float quarto-figure
   quarto-figure-<align>`), attrred `<figure>`, figcaption synthesis (id +
   classes + `aria-describedby`), `fig-align` handling. Snapshot +
   `test_build_transform_pipeline_phase_ordering` stays green.
2. **Table/listing floats** — replace the current Div+caption-paragraph shape
   with shape 1 (`figure.quarto-float-tbl` + `<figcaption>`); listings get
   `listing` class + left align.
3. **Standalone captioned figures** — wrap in `quarto-figure
   quarto-figure-<align>` (shape 2).
4. **React renderer parity** — Figure.tsx (+ Div rendering already generic);
   parity test between writer output and React DOM.
5. **Layout panels** — separate sub-strand (layout mini-engine), acceptance =
   shape 3.

Expected churn: pampa/quarto-core snapshots containing crossref figures will
change (figure DOM gains wrapper + classes); phase5 baseline `doc.html` is
title-only and should NOT shift. Every changed snapshot must be itemized in
the commit per the snapshot policy.

## The figcaption-uuid finding (investigated 2026-07-21)

Traced `0ceaefa1-69ba-4598-a22c-09a6ac19f8ca` through all of quarto-cli:

- It appears **exactly once** — its definition in `floatreftarget.lua:703` —
  and the id it builds has **exactly one consumer**: the `aria-describedby`
  attribute on the float's content wrapper three lines later.
- No regex, JS, CSS, or other filter matches the id pattern anywhere
  (`quarto.js` only matches `margin-caption` *classes*; lightbox doesn't read
  caption ids). Introduced in the FloatRefTarget PR itself (`8ba05ff2a`,
  #6620, 2023-09-15).
- Classification: **namespace-collision guard**, not a regex-architecture
  workaround. It prevents a user-authored `fig-x-caption` id from colliding
  with the generated one (duplicate ids = invalid HTML + broken aria/anchor
  resolution). Q1's filter architecture couldn't cheaply see the document's
  full id set, so an unguessable suffix was the correct cheap move there.

**Decision (Carlos, 2026-07-21): drop the uuid.** Q2 keeps the *purpose*
(collision-free figcaption id for `aria-describedby`) with a better
mechanism: emit the human-readable `<float-id>-caption`, checked against a
document-wide id set collected in one pass over the AST, disambiguating
(`<float-id>-caption-1`, …) only on actual collision. The chosen id travels
to the writers via `data-qf-caption-id`. The `aria-describedby` wiring stays
verbatim.

## Resolved questions

1. **Figcaption uuid**: dropped — semantic id + collision check (above).
2. **Bare `quarto-float` on the outer div**: **yes**, emit it (Q1-verbatim;
   themes may select on it even though `_quarto-rules.scss` doesn't).
3. **kv naming**: `data-qf-*` scheme as tabled above (five kvs). Q1 has no
   counterpart naming to preserve — these are Q2-internal, stripped at write
   time; deviation from Q1 is expected since the pipeline differs.

## Open questions

4. **Phase 2 timing**: replacing the table-float Div shape changes rendered
   DOM for existing documents (captions become `<figcaption>`); fine to do in
   the same PR as Phase 1, or staged separately?
