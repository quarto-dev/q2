# HTML image dimension lifting (`width=60%` → `style`, `750px` → `750`)

**Strand:** bd-html-image-dims-t5ogxjxd
**Status:** implemented on `braid/bd-html-image-dims-t5ogxjxd-image-dim-lifting` (go-ahead 2026-11-17).

## Problem

```markdown
![](a.png){width=60%}
![](b.png){width=750px}
```

|          | `a.png`               | `b.png`           |
|----------|-----------------------|-------------------|
| Quarto 1 | `style="width:60.0%"` | `width="750"`     |
| q2 0.43  | `width="60%"`         | `width="750px"`   |

The HTML `width`/`height` attributes take a bare non-negative integer, so
both q2 forms are invalid HTML. Browsers tolerate them, so the pages look
right. The field report counts 39 such images on 18 Connect pages.

q2's HTML writer (`crates/pampa/src/writers/html.rs:981`, `Inline::Image`)
passes the attr through `write_attr` verbatim. Nothing upstream normalizes
the values.

## What Pandoc does (the Quarto 1 behavior)

The lifting happens in Pandoc's HTML writer, not in Quarto 1's Lua.
`external-sources/pandoc/src/Text/Pandoc/Writers/HTML.hs:729-753`:

- `imgAttrsToHtml` removes `width` and `height` from the key-values. It
  then appends `dimensionsToAttrList attr` and merges every `style` entry
  into one `style`, joined with `;`. The author's own `style` comes first.
- `dimensionsToAttrList` parses each dimension with `ImageSize.lengthToDim`
  (`ImageSize.hs:268`):
  - `px` or no unit → `Pixel (floor n)` → stays an attribute: `width="750"`.
  - `%`, `cm`, `mm`, `in`/`inch`, `pt`, `pc`, `em` → `style="width:<n><unit>"`.
    Percent uses Haskell `show` on a Double, which is where `60.0%` comes
    from. The other units use `showFl`, which trims trailing zeros.
  - **Anything else (`auto`, `50vw`, `calc(…)`) is dropped silently**,
    because the `isNotDim` filter removes the key whether or not it parses.
- Only images go through this path. It is the sole caller, at
  `HTML.hs:1607`.

## Does q2 already have format-specific stages after "generate"?

Yes, but not as a generate/render boundary. Format specificity happens at
two levels.

1. **Per-family stage lists.** HTML, revealjs and dashboard (render and
   preview) all run `build_html_pipeline_stages_with_options`
   (`pipeline.rs:395`). Pandoc-writer formats run
   `build_pandoc_pipeline_stages`. Each list therefore contains
   HTML-family-only stages *after* `UserFiltersStage::post`:
   `EquationNumberStage`, `MathMlStage`, `CodeHighlightStage`,
   `MathJsStage` and `RenderHtmlBodyStage`.
2. **Inside `AstTransformsStage`** (`build_transform_pipeline`,
   `pipeline.rs:2342`), transforms are format-specific in two ways. Some
   are pushed conditionally (`if is_dashboard`, `footer_render_stage(is_revealjs)`,
   `reveal_finalization_transforms`). Others gate themselves on
   `ctx.format.identifier.is_html_based()`, which is exactly
   Html | Revealjs | Dashboard. Examples are `mermaid`, `graphviz`,
   `hephaestus`, `panel_tabset` and `responsive_image`.

The "generate" / "render" split is only a naming convention for transforms
inside the single `AstTransformsStage`, which is ordered by
`TransformPhase` (Normalization → Crossref → Navigation → Finalization).
**No Lua filter slot exists between them.** `UserFiltersStage::pre` and
`::post` bracket the whole stage. bd-0fd0 tracks adding that slot. This
work does not need it.

For this fix: if the lifting runs as a stage *after*
`UserFiltersStage::post`, post filters never see it. That defeats the goal.
If it runs as a **Finalization-phase `AstTransform` gated on
`is_html_based()`**, it lands inside `AstTransformsStage`, so it runs
before `UserFiltersStage::post`. Post filters then see `style="width:60%"`
and can rewrite it. This matches the existing `responsive_image` pattern.

## Proposed design

### New transform: `ImageDimensionsTransform` (`image-dimensions`)

`crates/quarto-core/src/transforms/image_dimensions.rs`

- `phase()` → `TransformPhase::Finalization`.
- Return immediately unless `ctx.format.identifier.is_html_based()`.
- Walk every image with `image_walk::for_each_image_mut`, the same walker
  `responsive_image` uses. This includes crossref-rendered figures.
- For each of `width` and `height`, in that order:
  - parse the value as `<number><unit>` (Pandoc's `numUnit`: digits and `.`,
    then the rest is the unit);
  - `px` / empty unit → replace with `floor(n)` as an integer string, and
    keep it as an attribute;
  - any other value → remove the key and append `<dim>:<value>` verbatim
    (D2).
- Merge into `style`: start with the existing `style` value with any trailing
  `;` trimmed, then add the lifted declarations, joined with `;`. Pandoc
  orders it the same way.
- Idempotent. A second run finds integer attributes and no non-pixel
  values, so it is a no-op.

### Placement

Push it **last** in the Finalization list of `build_transform_pipeline`,
after `ResponsiveImageTransform`, revealjs auto-stretch, `LlmsCaptureTransform`
and the dashboard transforms. The reasons:

- `responsive_image` and `auto_stretch` decide based on whether `height`
  and `width` attributes are *present* (`auto_stretch.rs:492`). Lifting
  first would change what they decide. Quarto 1 runs both before the
  writer, so running after them is the parity order.
- `llms` capture should keep the author's `{width=60%}` in its markdown.
- No Finalization transform should need to understand the lifted form.

The transform still runs before `UserFiltersStage::post`, which is the goal.

### Preview (`q2-preview`, hub-client)

`ts-packages/preview-renderer/src/q2-preview/inlines/Image.tsx` already
does the equivalent lifting client-side (lines 20-22 and 46-54). It
**ignores the `style` key-value** ("v1 passes width/height/id/classes/title
only"). If the transform ran in preview, lifted images would lose their
size. Per D3, `Image.tsx` learns to read `style`.

### pampa writer

Leave it unchanged. It stays a faithful attr printer, and the transform
owns the policy. A post filter that writes back `width="60%"` gets what it
wrote (D4).

## Decisions (cscheid, 2026-11-17)

- **D1. Percent formatting:** `width:60%`, not Pandoc's `60.0%`. Emit the
  shortest form and trim trailing zeros for every unit. Before changing
  expectations, check whether any Q1-parity snapshot compares this byte for
  byte.
- **D2. Unparseable values are kept, not dropped.** The rule is: only `N` or
  `Npx` (a non-negative number, optionally followed by `px`) stays an
  attribute, floored to an integer. Every other value goes into `style`
  verbatim as `<dim>:<value>`. This also covers CSS units added in future,
  with no code change.
- **D3. Preview runs the transform as well.** `Image.tsx` learns to parse the
  `style` key-value into `CSSProperties`. *As implemented:* preview-renderer
  already had `cssStringToObject` (`q2-preview/utils.tsx`, used by Span and
  Div), so `Image.tsx` uses that. Nothing moved out of
  `ReactAstSlideRenderer.tsx` in hub-client. Preview and render give filters
  the same AST.
- **D4. The pampa writer is unchanged.** The transform owns the policy. A
  post filter that writes `width="60%"` back gets what it wrote.
- **D5. Quarto 1 filter divergence is accepted and documented.** It goes in
  a new section of `docs/guides/authoring/lua-filters.qmd`, which already
  holds the Quarto 1 differences (see "Equation numbers: the
  `quarto-eq-number` attribute"). That section explains that pre filters
  see the author's `width`/`height`, while post filters (HTML family only)
  see `N`/`Npx` as an integer attribute and every other value moved into
  `style`.
- **D6. Scope is `Image` only**, which matches Pandoc.

## Work items (all done)

1. `image_dimensions.rs`: transform plus pure helper
   `lift_dimensions(&mut Attr)` with unit tests. Cover px, bare, float
   px (`750.7px` → `750`), %, in/cm/mm/pt/pc/em, merging with an existing
   `style` (with and without a trailing `;`), both dimensions,
   idempotency, `0`, whitespace around the value, and D2 pass-through
   (`auto`, `50vw`, `calc(100% - 2em)`).
2. Register it in `transforms/mod.rs` and push it last in Finalization in
   `build_transform_pipeline`. Update the pipeline doc comment and any
   transform-name lists or snapshots (`test_build_transform_pipeline_phase_ordering`
   and the preview deny-list drift tests near `pipeline.rs:4741`).
3. Gate tests: html / revealjs / dashboard lift; docx / typst / gfm do
   not.
4. Integration test (`crates/quarto-core/tests/integration/…`, per
   `.claude/rules/integration-tests.md`): render the two-image doc to
   HTML, assert `style="width:60%"` and `width="750"`. Add a Lua post
   filter test that sees `attributes.style` and can rewrite it.
5. Preview (D3): `Image.tsx` merges `cssStringToObject(style)` with its
   existing width/height handling, with new cases in `Image.test.tsx`.
6. Docs (D5): add the new section to `docs/guides/authoring/lua-filters.qmd`.
   Update the units paragraph in `docs/guides/authoring/figures.qmd` only if
   it needs to change.
7. `cargo xtask verify`.
