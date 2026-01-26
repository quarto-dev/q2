# Phase 2: Enhanced Template System

**Parent Plan**: [`2026-01-24-html-rendering-parity.md`](./2026-01-24-html-rendering-parity.md)
**Beads Issue**: kyoto-nje
**Created**: 2026-01-26
**Updated**: 2026-01-26 (integrated title-block.scss into SASS compilation)
**Status**: In Progress (Phases 2.0-2.3 complete, 2.4 remaining)

---

## Overview

Phase 2 enhances the HTML template system to match TypeScript Quarto's behavior. A key insight from analyzing TS Quarto is that it supports **two distinct modes**:

1. **Minimal mode** (`minimal: true` or `theme: none`) - Plain HTML with no Bootstrap structure
2. **Full mode** (default) - Rich Bootstrap-based structure with `<main>`, sidebars, title blocks, etc.

Our current simple template is actually close to TS Quarto's minimal output. This phase adds:
- Infrastructure to detect and honor the `minimal` option
- A "full" template with Bootstrap-compatible structure
- Proper template selection based on format configuration

---

## TS Quarto Minimal Mode Analysis

### What `minimal: true` Does in TS Quarto

When `format: html: minimal: true` is set:

1. **Sets `theme: none`** (unless explicitly overridden)
2. **Disables all interactive features**:
   - `code-copy: false`
   - `anchor-sections: false`
   - `citations-hover: false`
   - `footnotes-hover: false`
   - `crossrefs-hover: false`
   - `fig-responsive: false`
   - `code-annotations: false`
3. **Skips Bootstrap formatting pipeline** - No bodyEnvelope (no `<main>`, no `<div id="quarto-content">`)
4. **Minimal CSS** - Only basic Quarto rules for figures/tables, no Bootstrap

### Theme Modes in TS Quarto

| Theme Setting | CSS | Structure | Interactive Features |
|---------------|-----|-----------|---------------------|
| `theme: none` | None | Plain `<body>` | None |
| `theme: pandoc` | Pandoc defaults | Plain `<body>` | None |
| `theme: <bootstrap>` (default) | Bootstrap + theme | Full structure with `<main>`, sidebars | All enabled |

### Code Path in TS Quarto

```typescript
// In format-html.ts themeFormatExtras()
if (theme === "none") {
  return { metadata: { documentCss: false } };  // No bodyEnvelope
} else if (theme === "pandoc") {
  return pandocExtras(format);                   // No bodyEnvelope
} else {
  return bootstrapExtras(...);                   // Includes bodyEnvelope
}
```

The `bodyEnvelope` (which wraps content in `<div id="quarto-content">`, `<main>`, sidebars) is **only added by `bootstrapExtras()`**.

---

## Architecture: Two Templates

### Minimal Template (Current)

Our current template already approximates TS Quarto's minimal output:

```html
<!DOCTYPE html>
<html$if(lang)$ lang="$lang$"$endif$>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  $if(pagetitle)$<title>$pagetitle$</title>$endif$
  $for(css)$<link rel="stylesheet" href="$css$">$endfor$
  $if(header-includes)$$header-includes$$endif$
</head>
<body>
$body$
</body>
</html>
```

This is appropriate for `minimal: true` documents.

### Full Template (New)

For non-minimal documents, we need:

```html
<!DOCTYPE html>
<html$if(lang)$ lang="$lang$"$endif$>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="generator" content="quarto-rust-$version$">
  $if(author)$<meta name="author" content="$author$">$endif$
  $if(date-iso)$<meta name="dcterms.date" content="$date-iso$">$endif$
  $if(keywords)$<meta name="keywords" content="$keywords$">$endif$
  $if(description)$<meta name="description" content="$description$">$endif$
  $if(canonical-url)$<link rel="canonical" href="$canonical-url$">$endif$
  $if(pagetitle)$<title>$pagetitle$</title>$endif$
  $for(css)$<link rel="stylesheet" href="$css$">$endfor$
  $if(header-includes)$$header-includes$$endif$
</head>
<body$if(body-classes)$ class="$body-classes$"$endif$>
$if(title)$
<header id="title-block-header" class="quarto-title-block">
  <div class="quarto-title">
    <h1 class="title">$title$</h1>
$if(subtitle)$
    <p class="subtitle">$subtitle$</p>
$endif$
  </div>
$if(has-title-meta)$
  <div class="quarto-title-meta">
$if(author)$
    <div class="quarto-title-meta-author">
      <div class="quarto-title-meta-heading">Author</div>
      <div class="quarto-title-meta-contents">$author$</div>
    </div>
$endif$
$if(date)$
    <div class="quarto-title-meta-date">
      <div class="quarto-title-meta-heading">Published</div>
      <div class="quarto-title-meta-contents">$date$</div>
    </div>
$endif$
  </div>
$endif$
$if(abstract)$
  <div class="abstract">
    <div class="abstract-title">Abstract</div>
    $abstract$
  </div>
$endif$
</header>
$endif$

<div id="quarto-content" class="page-columns page-rows-contents page-layout-$page-layout$">
  <main class="content" id="quarto-document-content">
$body$
  </main>
</div>
</body>
</html>
```

### Template Selection Logic

```rust
// In template.rs or pipeline code
pub fn select_template(format: &Format) -> Template {
    // Check format.html.minimal (default: false)
    let minimal = format
        .get_html_config()
        .and_then(|html| html.get_bool("minimal"))
        .unwrap_or(false);

    // Also check theme - "none" or "pandoc" implies minimal structure
    let theme = format
        .get_html_config()
        .and_then(|html| html.get_string("theme"));

    let use_minimal = minimal || matches!(theme.as_deref(), Some("none") | Some("pandoc"));

    if use_minimal {
        minimal_html_template()
    } else {
        full_html_template()
    }
}
```

---

## Design Decisions

### 1. Two Separate Templates (Not Conditionals)

**Decision**: Maintain two distinct template strings rather than one template with many conditionals.

**Rationale**:
- Minimal template stays simple and fast
- Full template is readable without deeply nested conditionals
- Matches TS Quarto's architecture (different code paths for theme modes)
- Easier to test each template independently

### 2. TitleBlockTransform Behavior by Mode

**Decision**: TitleBlockTransform behavior depends on template mode.

| Mode | TitleBlockTransform Behavior |
|------|------------------------------|
| Minimal | Add h1 to AST body (current behavior) |
| Full | Skip h1 - template handles title block |

**Implementation**: Pass a flag or check format config in the transform.

### 3. CSS Strategy by Mode

**Decision**: Different CSS strategies for each mode.

| Mode | CSS |
|------|-----|
| Minimal | Basic Quarto rules only (figures, tables, code) - current styles.css |
| Full | Bootstrap-compatible styles + title block + layout CSS |

For Phase 2, we'll add title block styles to styles.css. Bootstrap integration is future work.

### 4. Feature Flags from Format Config

**Decision**: Read feature flags from `format.html.*` to control behavior.

Key flags to support (matching TS Quarto):
- `minimal` - Use minimal template
- `theme` - "none", "pandoc", or theme name
- `fig-responsive` - Add responsive image classes
- `anchor-sections` - Add anchored heading classes (future)
- `code-copy` - Add copy button scaffolding (future)

---

## Phased Implementation

### Phase 2.0: Minimal Mode Infrastructure (P0) ✅

**Goal**: Add infrastructure to detect minimal mode and select appropriate template.

**Status**: COMPLETE (2026-01-26)

#### Work Items

- [x] **Add format config helpers**
  - Created `get_metadata()`, `get_metadata_string()`, `get_metadata_bool()` in Format
  - Added `use_minimal_html()` method that checks `minimal`, `theme: none`, `theme: pandoc`
  - Location: `crates/quarto-core/src/format.rs`

- [x] **Extract current template as MINIMAL_HTML_TEMPLATE**
  - Renamed `DEFAULT_HTML_TEMPLATE` to `MINIMAL_HTML_TEMPLATE`
  - `default_html_template()` now returns minimal template for backwards compatibility

- [x] **Add FULL_HTML_TEMPLATE constant**
  - Full template with `<main>`, `<header id="title-block-header">`, metadata tags
  - Includes title block, author/date metadata, body classes, page-layout

- [x] **Add template selection function**
  - `select_template(format: &Format) -> Result<Template>`
  - `render_with_format(body, meta, format, css_paths)` for format-aware rendering

- [x] **Wire template selection into ApplyTemplateStage**
  - `apply_template.rs` now uses `render_with_format()` for automatic template selection
  - Custom templates still work via explicit template parameter

- [x] **Update TitleBlockTransform**
  - Added `should_add_h1()` method that checks format mode
  - Full mode (default): skips h1 - template handles title block
  - Minimal mode: adds h1 to AST body

#### Success Criteria

- [x] `minimal: true` uses minimal template (current behavior)
- [x] `minimal: false` (default) uses full template
- [x] `theme: none` uses minimal template
- [x] `theme: pandoc` uses minimal template
- [x] TitleBlockTransform respects template mode

#### Tests

- [x] Document with `minimal: true` → minimal HTML structure
- [x] Document with `theme: none` → minimal HTML structure
- [x] Document with default settings → full HTML structure with `<main>` wrapper
- [x] TitleBlockTransform adds h1 for minimal, skips for full
- [x] 57 new tests added (format helpers, template selection, title block modes)

---

### Phase 2.1: Full Template Structure (P0) - Mostly Complete

**Goal**: Implement the full template with semantic HTML structure.

**Status**: MOSTLY COMPLETE (2026-01-26) - Only CSS update remaining

#### Work Items

- [x] **Add `<main>` wrapper to full template**
  - `<main class="content" id="quarto-document-content">`
  - `<div id="quarto-content" class="page-columns page-rows-contents page-layout-$page-layout$">`

- [x] **Add title block header**
  - `<header id="title-block-header" class="quarto-title-block">`
  - Title with `<h1 class="title">`
  - Subtitle with `<p class="subtitle">`
  - Author metadata section with `<div class="quarto-title-meta">`
  - Abstract section with `<div class="abstract">`

- [x] **Add body classes support**
  - `<body$if(body-classes)$ class="$body-classes$"$endif$>`

- [x] **Add page-layout class**
  - Default to `page-layout-article` (set automatically by `render_with_format`)
  - Support from format config via template variable

- [x] **Integrate title-block.scss into SASS compilation**
  - Added `TEMPLATES_RESOURCES` embedded resource for templates directory
  - Added `load_title_block_layer()` function to load and parse title-block.scss
  - Updated `compile_theme_css()` and `compile_default_css()` (both native and WASM)
  - Title block styles now included in compiled Bootstrap CSS
  - Location: `crates/quarto-sass/src/bundle.rs`, `crates/quarto-sass/src/compile.rs`

- [ ] **Update styles.css for minimal mode** (DEFERRED)
  - Basic rules for figures, tables, code
  - Not needed for full mode since Bootstrap CSS handles it

#### Success Criteria

- [x] Full template outputs `<main>` wrapper
- [x] Full template outputs `<div id="quarto-content">`
- [x] Title renders in `<header id="title-block-header">`
- [x] Subtitle renders when present
- [x] Body classes applied when specified

#### Tests

- Full mode document → has `<main class="content">` wrapper
- Full mode document → has `<div id="quarto-content">` wrapper
- Document with title → `<header id="title-block-header">` present
- Document with subtitle → subtitle in title block
- Document without title → no title-block-header

---

### Phase 2.2: Head Metadata (P1) ✅

**Goal**: Add semantic metadata to HTML `<head>` in full template.

**Status**: COMPLETE (2026-01-26) - All metadata tags implemented in FULL_HTML_TEMPLATE

#### Work Items

- [x] **Add generator meta tag**
  - `<meta name="generator" content="quarto-rust-$version$">`
  - Version set automatically via `env!("CARGO_PKG_VERSION")` in `render_with_format()`

- [x] **Add author meta tag**
  - `<meta name="author" content="$author$">` when author present
  - Note: Complex author flattening deferred to future work

- [x] **Add date meta tag**
  - `<meta name="dcterms.date" content="$date$">` when date present
  - Note: ISO formatting deferred - uses date as provided

- [x] **Add canonical URL, keywords, description**
  - `<link rel="canonical" href="$canonical-url$">` when present
  - `<meta name="keywords" content="$keywords$">` when present
  - `<meta name="description" content="$description$">` when present

#### Success Criteria

- [x] Generator meta tag present in full mode
- [x] Author/date/keywords/description when specified

---

### Phase 2.3: Title Metadata Section (P1) ✅

**Goal**: Add the metadata section within title block.

**Status**: COMPLETE (2026-01-26) - Implemented in FULL_HTML_TEMPLATE

#### Work Items

- [x] **Add `<div class="quarto-title-meta">`**
  - Author section: `<div class="quarto-title-meta-author">` with heading/contents
  - Date section: `<div class="quarto-title-meta-date">` with heading/contents

- [x] **Conditional rendering**
  - Title-meta section only rendered when `$author$` is present
  - Date section only rendered when both author and date present
  - Note: `has-title-meta` flag not needed - template conditionals handle this

- [x] **Add abstract support**
  - `<div class="abstract">` with `<div class="abstract-title">Abstract</div>`

- [ ] **Add CSS for title-meta** (DEFERRED)
  - Can be added when visual styling is needed

#### Success Criteria

- [x] Author/date in title-meta section
- [x] Abstract renders correctly
- [x] Section only present when metadata exists

---

### Phase 2.4: Layout and Future Hooks (P2)

**Goal**: Add structure for future TOC/sidebar features.

#### Work Items

- [ ] **Add sidebar placeholders (commented)**
  - Ready for Phase 6 (TOC) integration

- [ ] **Add page-layout variations**
  - `page-layout-article`, `page-layout-full`

- [ ] **Document extension points**

---

## Implementation Strategy

### Order of Work

1. **Phase 2.0** - Infrastructure first (enables everything else)
2. **Phase 2.1** - Basic full template structure
3. **Phase 2.2** - Head metadata (can be parallel with 2.1)
4. **Phase 2.3** - Title metadata section
5. **Phase 2.4** - Layout hooks (can be deferred)

### Testing Strategy

1. **Unit tests for template selection**
   - Various format config combinations

2. **Unit tests for each template**
   - Minimal template renders correctly
   - Full template renders correctly

3. **Integration tests**
   - Full pipeline with `minimal: true`
   - Full pipeline with default settings

4. **Comparison tests**
   - Compare output structure with TS Quarto for same inputs

### Files to Modify

| File | Changes |
|------|---------|
| `crates/quarto-core/src/template.rs` | Add FULL_HTML_TEMPLATE, template selection |
| `crates/quarto-core/src/format.rs` | Add format config helpers |
| `crates/quarto-core/src/transforms/title_block.rs` | Check template mode |
| `crates/quarto-core/src/stage/stages/apply_template.rs` | Use template selection |
| `crates/quarto-core/resources/styles.css` | Add full mode styles |

### Files to Create

| File | Purpose |
|------|---------|
| `crates/quarto-core/src/format_config.rs` | Format configuration utilities (optional, may go in format.rs) |

---

## Hub-Client Considerations

Both templates work in WASM/hub-client:
- Template selection uses the same format config
- No platform-specific code needed
- CSS works in both contexts

The `minimal: true` option is particularly useful for hub-client when embedding rendered content.

---

## Discovered Issues

### TS Quarto Layer Parsing Bug in title-block.scss

**Issue**: <https://github.com/quarto-dev/quarto-cli/issues/13960>

While integrating `title-block.scss`, we discovered that the file uses layer boundary markers that don't match TS Quarto's own regex:

1. `/*-- scss: functions --*/` has a space after the colon (not recognized by regex)
2. `/*-- scss:variables --*/` uses "variables" which isn't a valid layer name (regex only allows: uses, functions, rules, defaults, mixins)
3. Only `/*-- scss:rules --*/` is recognized

**Impact**: The functions and variables in title-block.scss end up in the `defaults` section rather than their intended sections. The SCSS still compiles because:
- Functions in defaults work (SCSS is permissive about ordering in the assembled string)
- Variables in defaults is actually where they should be for `!default` variables
- Rules are correctly separated

**Our Approach**: We matched TS Quarto's behavior exactly to ensure test parity. Code references to the issue are in:
- `crates/quarto-sass/src/bundle.rs` - `load_title_block_layer()` docs
- `crates/quarto-sass/src/bundle.rs` - `test_load_title_block_layer()` comments

---

## Open Questions (Updated)

1. **Version string source**: Resolved - use `env!("CARGO_PKG_VERSION")`

2. **Where to store format config helpers?**
   - Option A: Add to existing `format.rs`
   - Option B: Create new `format_config.rs`
   - Recommend: Add to `format.rs` initially, refactor if it grows

3. **Should minimal mode disable SectionizeTransform?**
   - TS Quarto still produces sections in minimal mode (Pandoc does this)
   - Recommend: Keep SectionizeTransform active in both modes

---

## References

### Internal
- Parent plan: `claude-notes/plans/2026-01-24-html-rendering-parity.md`
- Current template: `crates/quarto-core/src/template.rs`
- TitleBlockTransform: `crates/quarto-core/src/transforms/title_block.rs`
- Format definition: `crates/quarto-core/src/format.rs`

### External (TS Quarto)
- Minimal mode: `external-sources/quarto-cli/src/format/html/format-html.ts` (lines 180-187, 680-695)
- Theme handling: `external-sources/quarto-cli/src/format/html/format-html.ts` (themeFormatExtras function)
- Bootstrap extras: `external-sources/quarto-cli/src/format/html/format-html-bootstrap.ts` (bodyEnvelope)
- Body envelope template: `external-sources/quarto-cli/src/resources/formats/html/templates/before-body-article.ejs`
