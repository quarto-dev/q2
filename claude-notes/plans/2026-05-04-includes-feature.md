# `include-in-header` / `include-before-body` / `include-after-body` (HTML) — design draft

**Date:** 2026-05-04
**Status:** Draft for review. Beads: `bd-8kp3`.
**Scope:** HTML format only (matches current Q2 reach). Architecture chosen
to extend cleanly to PDF/DOCX/etc. when those land.

## Goal

Implement the user-facing `include-in-header`, `include-before-body`,
`include-after-body` document-metadata keys in Quarto 2 (HTML), plus the
Q1-compatible smart-include object form (`{file: …}` / `{text: …}`).
Re-use the navbar/footer "generate then render" pattern so that
built-in features (favicon, theme JS, eventual website-project
contributions) and user filters share one well-defined extension
surface.

## Background

### What exists today in Q2

Slot-wise, the template (`crates/quarto-core/src/template.rs:75-103`,
`132-235`) already has Pandoc-native slots in both the minimal and
full HTML templates:

```
$for(header-includes)$ … $endfor$    inside <head>
$for(include-before)$ … $endfor$     just after <body>
$for(include-after)$ … $endfor$      just before </body>
```

The helper `set_includes_list` in `template.rs:403` is what currently
fills those template variables. It does two things:

1. Reads the metadata keys `header-includes`, `include-before`,
   `include-after` (Pandoc-native; "inline content" form — single
   string or array of strings).
2. Appends programmatic content from `StageContext.includes:
   PandocIncludes` (`crates/quarto-core/src/stage/data.rs:247`).

`PandocIncludes` carries:
```rust
pub struct PandocIncludes {
    pub header_includes: Vec<String>,
    pub include_before: Vec<String>,
    pub include_after: Vec<String>,
}
```
and is populated by `EngineExecutionStage` from engine results
(Knitr / Jupyter; see `crates/quarto-core/src/stage/stages/engine_execution.rs:266-283`).

Two AST transforms already write into the metadata `header-includes`
list directly:

- `WebsiteFaviconTransform`
  (`crates/quarto-core/src/transforms/website_favicon.rs`)
- `WebsiteCanonicalUrlTransform` (similar pattern)

Block-level **content** include via `{{< include child.qmd >}}` is a
distinct feature handled by `IncludeExpansionStage`
(`crates/quarto-core/src/stage/stages/include_expansion.rs`). It runs
before the `DocumentProfile` checkpoint so spliced content is visible
to cross-doc tooling (`bd-xfwx`). It records each child file in
`DocumentAst.recorded_includes`, drained into `DocumentProfile.includes:
Vec<IncludeEntry>` for Phase-8 cache invalidation. **The new feature
described here is unrelated to that shortcode** but should reuse the
`IncludeEntry` infrastructure for cache-key invalidation of the
file-path-based includes.

### What's missing

1. The user-facing **file-path** keys `include-in-header`,
   `include-before-body`, `include-after-body` are not read anywhere
   in Q2. Documents that set them today have no effect.
2. The Q1 **smart-include object form** (`{file: <path>}` /
   `{text: "..."}`) is not understood.
3. There is no **canonical "rendered includes" location** the way
   `rendered.navigation.*` exists. Built-in features that need to
   inject HTML into `<head>` (favicon, canonical URL) write straight
   into the authored `header-includes` key, mixing user input with
   computed content. That works today but doesn't match the
   generate/render contract used elsewhere and gives user filters no
   stable "after Quarto resolved everything" hook.
4. `IncludeExpansionStage` produces `recorded_includes` for the
   shortcode form. The new file-path keys add a parallel set of
   external file dependencies that the cache-key story needs to
   include too.

### What Quarto 1 does (for reference)

Sources:
- Schema: `external-sources/quarto-cli/src/resources/schema/document-includes.yml`
- Constants: `external-sources/quarto-cli/src/config/constants.ts:533-535`
- Format extras: `external-sources/quarto-cli/src/format/html/format-html.ts:653-665`
- Pandoc plumbing:
  `external-sources/quarto-cli/src/command/render/pandoc.ts:874-929`
  (merges extras-contributed includes into the Pandoc defaults file)
- Body envelope:
  `external-sources/quarto-cli/src/command/render/pandoc.ts:1702-1731`
  (writes website-navbar HTML to a temp file, prepends/appends to
  the include-\* lists)
- User docs for the **shortcode** include:
  `external-sources/quarto-web/docs/authoring/includes.qmd` (note: Q1
  does not have a dedicated docs page for the slot includes —
  schema-generated reference only).

Q1's flow for `include-in-header: foo.html`:

1. User sets the key in YAML (or extension/format extras contribute
   `[kIncludeInHeader]: [tempfile]`).
2. `pandoc.ts` merges extras + defaults into a `--defaults` file
   passed to Pandoc.
3. Pandoc reads the file from disk and substitutes its contents
   verbatim into the `$header-includes$` template slot.

So in Q1 the slot includes flow through Pandoc itself, not through
Quarto's own template engine. The "smart-include" type and the body
envelope are Quarto's only contributions.

In Q2, **Pandoc is not in the loop** — `quarto_doctemplate` renders
the template directly. The work done by Pandoc + its `--include-*`
flags has to be done in Q2's pipeline.

## Design

### Two well-known metadata locations

Mirror the navbar / footer pattern: an authored location read by a
generate stage, and a rendered location read by the template (and
by user filters that want the resolved view).

**Authored (input):**
- `include-in-header`
- `include-before-body`
- `include-after-body`

Each accepts the same shape Q1 accepts (the Q1 `smart-include`
schema, see `document-includes.yml`):

```yaml
include-in-header: foo.html               # path
include-before-body: { file: bar.html }   # path (object form)
include-after-body: { text: "<div/>" }    # literal text
include-in-header:                         # array
  - foo.html
  - { text: "<meta name=… >" }
```

The legacy inline-content keys `header-includes`, `include-before`,
`include-after` continue to work and are folded into the rendered
location by the same generate stage.

**Rendered (output of include resolution):**

Proposal: `rendered.includes.{header, before-body, after-body}` —
each holds a flat list of literal strings, ready to drop into the
template via `$for(...)$` loops.

```yaml
rendered:
  includes:
    header: ["<style>…</style>", "<meta …>"]
    before-body: ["<header>…</header>"]
    after-body: ["<script>…</script>"]
```

This parallels `rendered.navigation.{navbar, sidebar, toc, footer,
page_navigation}` and gives user filters one stable place to inspect
or extend the final include set.

### Pipeline placement

```
Parse → Merge → IncludeExpansion → DocumentProfile checkpoint →
LinkResolution → UnwrapProfile → PreEngineSugaring → Engine →
ThemeCSS → UserFilters(pre) → AstTransforms → UserFilters(post) →
ResourceReport → CodeHighlight → RenderHtmlBody → ApplyTemplate
```

The new work happens inside `AstTransforms` (the JIT pipeline built
by `build_transform_pipeline` in `crates/quarto-core/src/pipeline.rs:677`).

**Two new transforms**, in the Normalization phase, near the top:

1. **`IncludeResolveTransform`** ("generate" half).
   - Reads `include-in-header` / `include-before-body` /
     `include-after-body` from `ast.meta`.
   - For each entry:
     - String → treat as file path (Q1 parity). Read via
       `ctx.runtime.file_read(path_relative_to_doc)` so this works
       under WASM/VFS without branching.
     - `{file: path}` → same as bare string.
     - `{text: literal}` → use the literal verbatim.
   - Resolves the legacy inline keys (`header-includes`,
     `include-before`, `include-after`) too — strings are already
     literal text.
   - Folds engine-contributed `PandocIncludes` (from
     `StageContext.includes`) into the same rendered lists.
   - Writes the resulting flat lists to `rendered.includes.header`,
     `rendered.includes.before-body`, `rendered.includes.after-body`.
   - Records each resolved file in a side-channel (extend
     `DocumentProfile.includes` with a kind tag, **or** add a new
     `DocumentProfile.file_includes: Vec<IncludeEntry>` field —
     decision below) so Phase-8 cache invalidation picks up changes
     to the included files.
   - Diagnostics for missing files / unreadable paths follow the
     `IncludeExpansionStage` pattern (warnings, not hard errors;
     see `Q-5-2` etc.).

2. **`IncludeRenderTransform`** ("render" half).
   - For HTML: this is essentially a no-op today since the literal
     strings are already HTML-ready. The transform exists as a
     stable hook so non-HTML formats (when added) can convert
     literal-text smart-includes through their writer (e.g. wrap
     in `\input{}` for LaTeX) without changing
     `IncludeResolveTransform`.
   - Phase 1 ships an `IncludeRenderTransform` that does nothing
     for HTML and lives behind a feature dispatch in case we need
     it. Or, simpler: defer adding it until a non-HTML format needs
     it. **Open question 1.**

**Migrate the existing direct writers** to the new contract:

- `WebsiteFaviconTransform` (and `WebsiteCanonicalUrlTransform` if
  it does the same) currently appends to authored `header-includes`.
  Refactor to push into `rendered.includes.header` after
  `IncludeResolveTransform` has run. Order in the JIT pipeline:
  `IncludeResolveTransform` runs first; favicon / canonical / future
  contributors run after. (Concretely: `IncludeResolveTransform`
  goes early in Normalization, the website transforms move to a
  later "navigation/header contribution" point — no behavior
  change for the user, but the contribution surface becomes
  uniform.)

**Template wiring** (`crates/quarto-core/src/template.rs`):

- Replace the body of `set_includes_list` so it reads from
  `meta.rendered.includes.{header, before-body, after-body}`
  instead of from authored `header-includes` / `include-before` /
  `include-after` plus `PandocIncludes`. After this, `PandocIncludes`
  becomes purely an internal pipeline-data structure routed through
  `IncludeResolveTransform`; the template no longer reaches into
  `StageContext.includes` directly. (`render_with_compiled_template`
  signature loses its `includes: &PandocIncludes` parameter — or
  keeps it as `&[]` for back-compat during migration.)
- Template `$for(header-includes)$` etc. → `$for(rendered.includes.header)$` etc.
  Or: leave the template variable names alone and just route content
  into the existing `header-includes` / `include-before` / `include-after`
  template variables from `rendered.includes.*`. The latter avoids
  touching templates and the public extension contract (custom
  templates that use `$header-includes$`); it's also what the existing
  helper does. **Open question 2.**

### Where the file-include set lives in `DocumentProfile`

Two options:

**(a)** Reuse `DocumentProfile.includes: Vec<IncludeEntry>` and add a
  `kind: IncludeKind { Shortcode, FileSlot }` discriminator.

**(b)** Add a sibling field
  `DocumentProfile.file_includes: Vec<IncludeEntry>`.

Option (a) is denser and treats "this document depends on these
files" uniformly for cache invalidation. Option (b) keeps the
shortcode set untouched and is a smaller diff. **Lean: (a)**, paired
with a `DOCUMENT_PROFILE_VERSION` bump (Phase 8 already has the
machinery for this — see `bd-r82e` / `bd-fegm`). **Open question 3.**

### `PandocIncludes` lifetime

`StageContext.includes: PandocIncludes` exists today because engines
produce content before `AstTransforms` runs and the data has nowhere
better to live. Once `IncludeResolveTransform` exists, it can drain
`StageContext.includes` into `rendered.includes.*` and clear it.
That keeps `StageContext.includes` as the engine→pipeline conduit
and `rendered.includes.*` as the canonical post-resolution location.
Engines don't change. **Confirm: this is the cleanest split.**

## User-facing API surface

Document-level YAML, behaving exactly like Q1:

```yaml
---
title: My Doc
include-in-header:
  - file: extra-meta.html
  - text: |
      <meta name="theme-color" content="#ff0">
include-before-body: header-banner.html
include-after-body:
  - { text: "<script>console.log('hi')</script>" }
---
```

Existing inline-content keys (Pandoc-native) keep working unchanged:

```yaml
header-includes: |
  <style>h1 { color: red; }</style>
include-before: |
  <div class="banner">…</div>
```

For an HTML-only first cut, the file-relative resolution for paths
follows the document's directory (`document_dir.join(path)`). Once
website projects are in scope, project-root-anchored absolute paths
(`/foo.html` resolved against the project root) become valid too —
the same convention `IncludeExpansionStage` uses. **Open question 4.**

## Test strategy

Per CLAUDE.md TDD policy, tests come before implementation. Three
layers:

1. **Unit tests on `IncludeResolveTransform`** —
   - bare string path → file read, content appears in
     `rendered.includes.header`.
   - `{file: path}` object form → identical to bare string.
   - `{text: literal}` → no file read, content appears verbatim.
   - Array of mixed forms.
   - Missing file → warning diagnostic, no panic, other includes
     still resolve.
   - Legacy `header-includes` / `include-before` / `include-after`
     keys folded in the right slots.
   - Engine `PandocIncludes` folded in.
   - Order preservation: authored entries appear before
     contributed entries (matches Q1 ordering decisions in
     `pandoc.ts:874-929`).

2. **Pipeline-level integration tests** in
   `crates/quarto-core/tests/`:
   - End-to-end render via `render_qmd_to_html` (or
     `render_document_to_file` for CLI parity, per CLAUDE.md
     §End-to-end verification) showing that `include-in-header:
     foo.html` results in `foo.html`'s content appearing inside
     `<head>` of the produced HTML.
   - Same for `include-before-body`, `include-after-body`.
   - Smart-include object forms.
   - Migrated `WebsiteFaviconTransform` still emits the
     `<link rel="icon">` (regression check after the contributor
     migration).

3. **Fixture-based CLI smoke** under `crates/quarto/tests/smoke-all/`:
   - A fixture document with all three include-* keys set, plus
     legacy `header-includes`, plus a custom favicon. Snapshot the
     rendered HTML. Phase 7's `/tmp/q2-phase7-smoke/` style
     end-to-end inspection per CLAUDE.md.

## Phasing

This is a small enough feature to ship as one PR, but the work
splits naturally into checkpoints. Starting from existing
`build_transform_pipeline` and `set_includes_list`:

- [x] **0. Tests first (TDD).** New-feature license per CLAUDE.md
      §TDD lets us co-develop tests + implementation; 8 unit tests
      in `crates/quarto-core/src/stage/stages/include_resolve.rs`
      cover bare-string / `{file:..}` / `{text:..}` / array of
      mixed forms / missing-file warning / legacy-key fold /
      engine-`PandocIncludes` fold / Q1-parity ordering.
- [x] **1. `IncludeResolveStage`** — new pipeline stage (not
      transform; it needs `StageContext.runtime` for file reads).
      Reads `include-in-header` / `include-before-body` /
      `include-after-body` plus the legacy inline keys, resolves
      smart-includes, writes flat string arrays to
      `meta.rendered.includes.{header, before-body, after-body}`.
      Records file-slot dependencies on `DocumentAst.recorded_includes`
      so the next stage drains them into the profile. Diagnostics
      `Q-5-4` (missing file) and `Q-5-5` (invalid form). Pipeline
      placement: between `IncludeExpansionStage` and
      `DocumentProfileStage` so file deps reach `profile.includes`
      for cache invalidation.
- [x] **2. Template wiring** — `set_includes_list` reads
      from `meta.rendered.includes.<slot>` and feeds the existing
      `$header-includes$` / `$include-before$` / `$include-after$`
      template variables (names kept stable per Resolved-question
      #2). Dropped the `&PandocIncludes` parameter from
      `render_with_compiled_template`. `ApplyTemplateStage::run`
      drains any post-resolve `ctx.includes` (engine output that
      lands during the engine stage, plus shortcode/Lua
      contributions) into `rendered.includes.*` via a new
      `append_pandoc_includes` helper before rendering. Updated
      callers in `apply_template.rs` and `tests/navigation_e2e.rs`.
- [x] **3. Migrate contributors** — `WebsiteFaviconTransform`
      now appends to `rendered.includes.header` instead of the
      authored top-level `header-includes` key. Inline tests
      updated to read from the new location. Pipeline order
      naturally guarantees `IncludeResolveStage` runs first
      (pre-checkpoint) and the favicon/canonical-url transforms
      run later in `AstTransformsStage`. `WebsiteCanonicalUrlTransform`
      did not need migration (it writes the top-level
      `canonical-url` key, separate from include slots).
- [x] **4. Cache-key invalidation for file-slot includes** —
      `IncludeResolveStage` runs before `DocumentProfileStage`,
      so `recorded_includes` (now containing both shortcode and
      file-slot entries) is drained into `profile.includes` by
      the existing path. `bd-r82e` cache hashing already includes
      every entry. Decision: deferred the optional
      `IncludeKind { Shortcode, FileSlot }` discriminator; the
      cache-key contract works without a tag and bumping
      `DOCUMENT_PROFILE_VERSION` for diagnostic-only metadata
      would invalidate every existing on-disk cache for marginal
      benefit. Filed as a follow-up note (no separate `bd` issue
      until a consumer needs it). Integration test
      `crates/quarto-core/tests/include_resolve_pipeline.rs::file_slot_include_lands_in_profile_includes`
      pins the contract.
- [x] **5. End-to-end verification** — CLI render of fixture, eyeball
      generated HTML, record observed snippet in this plan
      (per CLAUDE.md §End-to-end verification).

      **Fixture:** `target/q2-includes-smoke/` (gitignored, under
      `target/`). Files:
      - `head.html` — `<meta name="q2-smoke" content="in-header-from-file">` plus a small `<style>`.
      - `banner.html` — `<aside class="q2-banner">BEFORE-BODY-FROM-FILE</aside>`.
      - `test.qmd` exercising **all four user-facing forms** plus a legacy inline key:
        ```yaml
        include-in-header: head.html               # bare-string path
        include-before-body:
          file: banner.html                          # smart {file: ...}
        include-after-body:
          text: "<script>console.log('AFTER-BODY-FROM-TEXT');</script>"  # smart {text: ...}
        header-includes: '<meta name="legacy-inline" content="from-header-includes">'
        ```

      **Invocation:** `cargo run --bin q2 -- render test.qmd`
      (from `target/q2-includes-smoke/`).

      **Observed output** (`test.html`, abbreviated to the
      include-relevant portions):

      ```html
      <head>
        ...
        <meta name="legacy-inline" content="from-header-includes">      <!-- legacy header-includes -->
        <meta name="q2-smoke" content="in-header-from-file">             <!-- include-in-header file content -->
        <style>p.q2-mark { color: rebeccapurple; }</style>               <!-- second line of head.html -->
      </head>
      <body class="fullcontent">
      <aside class="q2-banner">BEFORE-BODY-FROM-FILE</aside>             <!-- include-before-body file content -->
      ...
      <main>...<p>This is the rendered document body.</p>...</main>
      ...
      <script>console.log('AFTER-BODY-FROM-TEXT');</script>              <!-- include-after-body text content -->
      </body>
      ```

      Each include lands in the correct slot. Inspection performed
      manually; the HTML was regenerated after fixing two issues
      uncovered by this run:
      - A `RawInline`-vs-`Str` round-trip in the YAML reader meant
        that user-authored HTML in inline-style keys (`header-includes`
        with an embedded `<meta>`) was being dropped because
        `as_plain_text()` skips `RawInline`. Fixed by introducing a
        `literal_html_text` helper that walks `PandocInlines`
        preserving raw markup and original quote characters.
      - The same issue affected `{text: "<script>…'…'…</script>"}` —
        the YAML reader parsed it as markdown, and the smart-quote
        conversion was producing unicode quotes. Fixed in the same
        helper by emitting the original `'` / `"` characters from
        `Inline::Quoted` nodes.
- [ ] **6. Docs note** — file an entry under `bd-tr81` (the docs
      epic) or write a Q2-side `docs/authoring/includes.qmd`-style
      page covering both the shortcode form (already documented in
      Q1) and the slot form (not documented in Q1). Deferred to a
      separate session — the docs epic owns user-facing docs work.

## Resolved questions

1. **Render half**: defer `IncludeRenderTransform` until a non-HTML
   format needs it.
2. **Template variable names**: keep `$header-includes$` /
   `$include-before$` / `$include-after$`. Feed them from
   `rendered.includes.*`. Keeping the Pandoc-native names makes
   custom Pandoc templates portable into Q2 even as we reduce our
   reliance on them.
3. **Profile field shape**: extend `IncludeEntry` with a `kind`
   tag (shortcode vs file-slot). Bump `DOCUMENT_PROFILE_VERSION`
   per the Phase-8 versioning machinery.
4. **Path resolution**: document-relative for the first cut.
   **Forward note for the implementer (aspirational — does not
   exist yet, expected soon):** Q2 plans `!path`-tagged YAML
   scalars that carry the source location of the YAML they came
   from (project `_quarto.yml`, directory `_metadata.yml`, document
   front matter). The right long-term resolution is "anchor the
   path at the directory of the YAML that produced it," not "anchor
   at the document being rendered." A future refactor (out of
   scope here) will introduce an intermediate path representation
   that distinguishes fully-resolved / document-relative /
   project-root-relative. Design choices in the include resolver
   should not preclude that — but until `!path` lands, this feature
   ships with plain document-relative resolution.
5. **Project-level inheritance**: yes — `include-in-header` set in
   `_quarto.yml` applies to every page. The metadata merge stage
   already does this for free; users get `!prefer` / `!concat` for
   replace-vs-append control. Path resolution still anchors at the
   YAML source per #4.
6. **Format-conditional excludes**: skip the disable list — only
   HTML renders exist today.

## Non-goals

- Non-HTML output formats (PDF, DOCX, …). Deferred until those
  formats land; the `IncludeRenderTransform` hook keeps a slot for
  them.
- A `bodyEnvelope`-style aggregate API for project-wide
  contributions (Q1's mechanism for website navbar). The
  `rendered.includes.*` location plus per-feature transforms
  already gives us the same expressive power without needing a new
  named struct.
- Pandoc-style `--metadata-file` / `--metadata-files` (those are a
  separate feature in `document-includes.yml` that do not flow
  through the include slots).

## Related work

- `IncludeExpansionStage` (`bd-xfwx`, plan
  `2026-04-24-include-expansion-merge.md`) — sibling feature
  (block-level shortcode include).
- `bd-r82e` — `DocumentProfile.includes` invalidation; the plan
  here adds the file-slot side of that work.
- Phase 7 `WebsiteFaviconTransform` / `WebsiteCanonicalUrlTransform`
  (`bd-b9mz`, plan `2026-04-27-websites-phase-7.md`) — the
  in-place writers that this plan migrates to the new contract.
- Website-epic Phase 6 `LinkRewriteTransform` (`bd-v30t`) — same
  generate/render-pair shape as proposed here.
