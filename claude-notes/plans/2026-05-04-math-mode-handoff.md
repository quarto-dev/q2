# Math-mode (MathJax / KaTeX / …) implementation — handoff from bd-4eyf

**Status:** Not started. Notes for the next session.
**Predecessor work:** bd-4eyf (Bootstrap JS injection) — see
`claude-notes/plans/2026-05-04-bootstrap-js-injection.md`.

This document captures findings from the bd-4eyf session that are
directly load-bearing for math-mode work. Read this first; it will
let you skip the learning loop bd-4eyf went through.

## TL;DR — what changes vs. the Bootstrap JS approach

Quarto 1 delegates math-mode injection entirely to Pandoc by setting
`html-math-method: mathjax` (or `katex`, `webtex`, …) in pandoc
options. q2 cannot reuse that mechanism because **q2's HTML pipeline
does not invoke Pandoc** — we render HTML ourselves. So q2 needs its
own injection path.

The bd-4eyf "predicate → register `js:*` artifact" pattern *almost*
fits, but math has two complications Bootstrap didn't:

1. **An inline configuration block** is required (e.g. `<script>window.MathJax = { ... }</script>`) before the loader script. The artifact pipeline only emits external `<script src="…">` tags — it has no path for inline content.
2. **A trigger that depends on document content**, not just metadata. "Does this document contain math?" requires an AST walk; bd-4eyf's predicate (`is_minimal_html` + `theme_config.suppress_bootstrap`) is metadata-only and runs in microseconds.

These two together are why bd-4eyf deferred the generic `JsFeature`
abstraction — math is the case where it might actually pay for itself.

## Architectural pieces you'll touch

Read these in roughly this order before designing:

- **`crates/quarto-core/src/stage/stages/bootstrap_js.rs`** — the prototype to copy. The "predicate → `Project`-scoped `js:` artifact" pattern is documented in its module-level doc comment.
- **`crates/quarto-core/src/stage/stages/apply_template.rs:166-167, 313`** — `collect_artifact_urls` is what turns `js:*` artifacts into `<script src="…">` tags. **It only handles external scripts** (artifacts with a `path`); inline content has no slot here. This is the design pinch-point for math.
- **`crates/quarto-core/src/stage/stages/include_resolve.rs`** + the `rendered.includes.{header, before-body, after-body}` contract — *this* is how raw HTML (including inline `<script>` blocks) currently reaches the rendered template. The MathJax config script may need to ride this rail rather than the artifact rail.
- **`crates/quarto-core/src/pipeline.rs`** — `build_html_pipeline_stages_with_options()` (native) and `build_wasm_html_pipeline()` (hub-client). The `#[cfg(not(target_arch = "wasm32"))]` gate pattern bd-4eyf established for omitting native-only stages from WASM is the cleanest precedent. Decide upfront whether math should ship to hub-client (probably yes — math display does not have the iframe-reinit-stateful-component problem Bootstrap does).
- **`crates/quarto-core/src/format.rs:278`** — `is_minimal_html` predicate. **Important gotcha** documented in `bootstrap_js.rs`: this reads root-level `theme:` only; format-nested `format.html.theme: none` is *not* flattened to root by `MetadataMergeStage`. Use `quarto_sass::ThemeConfig::from_config_value(&doc.ast.meta).suppress_bootstrap` for the canonical "Bootstrap is in use" check, or *combine* both predicates as bd-4eyf did. Math probably wants its own predicate, but if it ever depends on the theme decision, use the same combined approach.
- **`resources/scss/README.md`** + `resources/js/README.md` — the vendoring conventions. If you vendor MathJax, mirror the layout under `resources/js/mathjax/` and document the source URL + version + bump policy.

## The trigger question (this is the hard one)

Bootstrap JS triggers on metadata: "is a Bootstrap-backed theme
active?" — checkable in O(1) on the format/document meta.

Math triggers on **content**: "does this document contain at least one
`Math` element in the AST?" — requires a walk. Options:

1. **Walk in a dedicated stage** that runs late enough to see the final AST (after engines / sugaring / transforms — those can introduce math via crossref equations). This is the pure approach but adds an O(N) AST walk to every render.
2. **Piggyback on an existing walker.** `RenderHtmlBodyStage` already traverses every node to emit HTML. Set a flag on `StageContext` when it sees a `Math` element. Then a tiny stage between `RenderHtmlBodyStage` and `ApplyTemplateStage` reads the flag and registers artifacts. Cheaper. Tighter coupling — the renderer becomes responsible for a side-channel signal.
3. **Use the document profile.** `DocumentProfileStage` already snapshots the AST at the checkpoint; `EquationLabelTransform` already counts equation labels (`crates/quarto-core/src/transforms/`). If `profile.has_math` (new field) gets set during profiling, math injection becomes a metadata-style predicate again. Cleanest if the profile contract can be extended cheaply.

Recommendation: option 3 if `DocumentProfile` already sees post-sugar
crossref equations; otherwise option 2. Option 1 is the most
expensive and probably unnecessary.

**Don't forget:** `EquationLabelTransform` introduces `Math` blocks
*from* `$$ … $$ {#eq-…}` syntax during sugaring. The trigger walk must
run *after* this transform, or it will miss labelled equations.

## The inline config-script question

MathJax wants something like:

```html
<script>
window.MathJax = {
  tex: { inlineMath: [['$', '$'], ['\\(', '\\)']] },
  // …
};
</script>
<script src="…/mathjax.js" defer></script>
```

The artifact pipeline can emit the external `<script>` (`js:mathjax`
artifact, same shape as `js:bootstrap`) but **not the inline config
block**. Three plausible paths:

1. **Use `rendered.includes.header`** for the inline block — same rail `IncludeResolveStage` uses for raw HTML. The math stage would push a string into `meta.rendered.includes.header` and the artifact handles only the external script. Two halves, same destination (`<head>`), but they're decoupled in the code.
2. **Extend the artifact API** with an "inline" variant — `Artifact::inline_script(content)` that emits a `<script>…</script>` directly rather than `<script src="…">`. Touches the `collect_artifact_urls` contract. Probably the most invasive option.
3. **Bake the config into the loader.** Vendor a small wrapper script (`mathjax-init.js`) that does `window.MathJax = {...}; document.write('<script src="mathjax.js">…')` or similar. Single artifact, no template-side change, but harder to make the config user-controllable.

Recommendation: option 1 (`rendered.includes.header` for inline,
`js:mathjax` artifact for external). It reuses two existing rails
without inventing a third. The "two halves" criticism is worth
~5 lines of doc, not a refactor.

## Vendor vs CDN vs both?

Bootstrap was tiny (80 KB), so vendoring was an easy call. MathJax
is *much* bigger:

- Full MathJax 3 distribution: ~70 MB unpacked (includes every font / output mode / extension).
- Common components-only loader: ~1 MB.
- Smallest bootstrap-loader: ~150 KB.

Quarto 1 vendors the components-only build. The size delta vs Bootstrap
is real — vendoring 1 MB into the CLI binary affects download size and
fresh-clone build time. Decision worth recording up front; both
extremes have precedent.

If you go CDN-default with vendor-fallback, that's a third design
question you didn't have to answer for Bootstrap. The
`is_minimal_html`-style metadata knob (`mathjax.source: cdn|local`)
is the user-facing surface.

## Hub-client / WASM

Unlike Bootstrap (deliberately omitted from WASM because the
iframe-per-render preview blows away stateful Bootstrap components),
math display is **stateless** — typeset on load, done. Hub-client
*should* render math. Don't blindly copy bd-4eyf's
`#[cfg(not(target_arch = "wasm32"))]` gate; for math, both pipelines
get the stage. (Confirm by trying a dollar-sign-equation in
hub-client preview before committing.)

If the size of the vendored MathJax bundle is what's blocking the
WASM bundle, the CDN-default path solves that for free.

## Configuration knobs (scope decision)

Quarto 1 supports `html-math-method: mathjax | katex | webtex | gladtex | mathml | plain`.
Each has different semantics:

- `mathjax`, `katex` — client-side typesetting, ship a JS runtime.
- `webtex` — server-side image rendering, no JS.
- `gladtex` — alt-text only, no JS.
- `mathml` — pass-through, no JS.

q2 does not need to ship all of these on day one. Pick a default
(probably `mathjax`) and an explicit user-controllable knob; defer the
others. **Decide before you start writing code** — the predicate matrix
balloons fast if you support all five.

## Test strategy

The bd-4eyf test pattern to copy:

- **Unit tests** for the trigger predicate (math present / absent / nested in callout / inside code block / etc.) in the new stage's `#[cfg(test)] mod tests`.
- **Integration tests** in a new `crates/quarto-core/tests/math_mode_pipeline.rs` driving `render_to_file` + `ProjectPipeline`, asserting:
  - Math-bearing render emits the script tag(s) and the on-disk file(s).
  - Math-free render emits neither.
  - Multi-page website ships one shared copy.
  - Nested-page relative URL.
- **Live browser smoke** via chrome-devtools-mcp — actually load a rendered page and assert MathJax typeset something visible (e.g. that a `$x^2$` body produced a `<mjx-container>` in the DOM). The bd-4eyf session proved this works and is the most decisive evidence the runtime is wired up. Don't skip it.

## Snapshot baseline

`crates/quarto-core/tests/fixtures/phase5-single-doc-baseline/expected_hashes.txt` was re-captured for bd-4eyf because the new
`<script>` tag changed `doc.html`'s SHA. The baseline `doc.qmd` is
math-free, so math-mode work should *not* change `doc.html` again —
the math stage must skip on math-free input. If the hash shifts, the
predicate is over-triggering. (Useful canary.)

## bd-telo dependency

`bd-telo` (filed during bd-4eyf): q2 today only reads `navbar:` from
the top level of `_quarto.yml`, not from nested `website.navbar:`. If
math-mode is documented for users in user-facing docs that recommend
the natural `website.navbar:` shape, the docs will not match what
works. Either land bd-telo first, or be careful about the YAML shape
in user-facing docs/examples for math.

## Things that *don't* need re-deriving

bd-4eyf already settled these — don't re-design them:

- **Artifact-based external scripts** are the right shape for the JS payload (uses `ApplyTemplateStage`'s existing `js:` collector, gets the right URL via `ResourceResolverContext`, lands in the project lib dir for websites with the `quarto/` namespace, lands per-page for single-doc).
- **Project scope** is the right scope for math assets (shared across pages in a website, mirrors the theme CSS layout).
- **Vendoring layout under `resources/js/<feature>/`** with an `include_bytes!` is the established pattern; just add a section to `resources/js/README.md`.
- **TDD with a noop stub for red phase** is the local norm; CLAUDE.md mandates it. The failure messages bd-4eyf used (positive cases fail, skip cases pass-via-false-positive) are documented in the bd-4eyf plan.
- **Script ordering** is alphabetic-by-key and there's no SAT solver coming. If math needs to load *after* Bootstrap (it doesn't), pick a key that sorts after `bootstrap`.

## Open question for the next session's first message

Before designing, get an answer on:

> Do we want math-mode to support multiple engines (mathjax + katex
> at minimum), or is mathjax-only an acceptable v1?

This single decision shapes the whole stage — single-feature stage vs.
parameterized-by-engine stage vs. one-stage-per-engine. Don't start
without it.
