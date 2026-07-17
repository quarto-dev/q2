/*
 * template.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Template integration for HTML rendering.
 */

//! Template integration for document rendering.
//!
//! This module provides the integration layer between the quarto-doctemplate
//! engine and the Quarto render pipeline. It handles:
//!
//! - Default HTML template for standalone documents
//! - Conversion of Pandoc metadata to template values
//! - Rendering documents through the template engine
//!
//! ## Architecture
//!
//! The template system uses dependency injection: the rendered body content
//! is passed as a template variable, allowing the template to control the
//! overall document structure while the HTML writer controls content rendering.

use std::path::Path;

use quarto_doctemplate::{
    ChainedResolver, MemoryResolver, PartialResolver, Template, TemplateContext, TemplateValue,
};
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::format::{Format, is_minimal_html};

// =============================================================================
// Runtime Resolver
// =============================================================================

/// Resolver that loads partials via `SystemRuntime`, enabling WASM VFS access.
///
/// Unlike `FileSystemResolver` (which uses `std::fs`), this resolver goes
/// through the runtime abstraction layer, so it works in both native and
/// WASM contexts.
pub struct RuntimeResolver<'a> {
    runtime: &'a dyn SystemRuntime,
}

impl<'a> RuntimeResolver<'a> {
    /// Create a new resolver backed by the given runtime.
    pub fn new(runtime: &'a dyn SystemRuntime) -> Self {
        Self { runtime }
    }
}

impl PartialResolver for RuntimeResolver<'_> {
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String> {
        let partial_path = quarto_doctemplate::resolve_partial_path(name, base_path);
        self.runtime.file_read_string(&partial_path).ok()
    }
}

// =============================================================================
// Template Definitions
// =============================================================================

/// Minimal HTML5 template for `minimal: true` or `theme: none/pandoc` documents.
///
/// This template produces plain HTML without Bootstrap structure. It matches
/// TypeScript Quarto's output for `minimal: true`.
///
/// Template variables:
/// - `$pagetitle$` / `$title$` - document title
/// - `$body$` - rendered body content
/// - `$css$` - CSS stylesheets (external files)
/// - `$lang$` - document language
/// - `$header-includes$` - additional header content
/// - `$math$` - math-engine init markup (config block + loader script).
///   Populated by [`crate::stage::stages::MathJsStage`] when the
///   document contains math; rendered immediately before
///   `$for(scripts)$` so the inline config block lands BEFORE the
///   loader (what MathJax expects).
const MINIMAL_HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html$if(lang)$ lang="$lang$"$endif$>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
$if(pagetitle)$
<title>$pagetitle$</title>
$endif$
$for(css)$
<link rel="stylesheet" href="$css$">
$endfor$
$if(math)$
$math$
$endif$
$for(scripts)$
<script src="$scripts$"></script>
$endfor$
$for(header-includes)$
$header-includes$
$endfor$
</head>
<body>
$for(include-before)$
$include-before$
$endfor$
$body$
$for(include-after)$
$include-after$
$endfor$
</body>
</html>
"#;

/// Full HTML5 template with Bootstrap-compatible structure.
///
/// This template produces semantic HTML with:
/// - `<header id="title-block-header">` for document metadata
/// - `<main class="content">` wrapper for body content
/// - `<div id="quarto-content">` for layout structure
/// - Optional Table of Contents sidebar
///
/// The title block is emitted by the built-in `title-block` partial
/// (see [`TITLE_BLOCK_PARTIAL`] / [`TITLE_METADATA_PARTIAL`]), which a
/// document can override via `template-partials` with a file named
/// `title-block.html` (Q1 compatibility).
///
/// Template variables (in addition to minimal):
/// - `$title$` - document title (for title block)
/// - `$subtitle$` - document subtitle
/// - `$author$` - document author(s)
/// - `$by-author$` - normalized author list (written by
///   `AuthorsNormalizeTransform`; `$it.name.literal$` per entry)
/// - `$labels.*$` - title-block heading labels (same transform)
/// - `$rendered.has-title-block$` - whether any title-block content
///   exists (same transform); gates the `<header>` emission
/// - `$date$` - publication date
/// - `$abstract$` - document abstract (rendered as HTML blocks)
/// - `$body-classes$` - CSS classes for body element. When set, replaces
///   the `fullcontent` default entirely. Typically computed by
///   `SidebarRenderTransform` (which writes `rendered.navigation.body-classes`)
///   and copied into `body-classes` by `render_with_compiled_template`,
///   but a user filter or template variable can override it. When unset
///   and `rendered.navigation.toc` is present, `render_with_compiled_template`
///   sets it to the empty string so the body falls through to the default
///   (no-class) wide layout — needed because `fullcontent` allocates only
///   `0.14*margin-width` for the right margin and squashes a TOC to ~70px.
///   Mirrors TS Quarto's `format-html-bootstrap.ts` body-class logic.
/// - `$page-layout$` - page layout type (article, full, etc.)
/// - `$version$` - Quarto version for generator meta tag
/// - `$rendered.navigation.toc$` - Rendered TOC HTML (if toc: true)
/// - `$navigation.toc.title$` - TOC title (if set)
/// - `$rendered.navigation.navbar$` - Rendered navbar HTML (if navbar: set)
/// - `$rendered.navigation.sidebar$` - Rendered sidebar HTML (if website.sidebar: set)
/// - `$rendered.navigation.page_navigation$` - Rendered prev/next page-nav strip
/// - `$rendered.navigation.footer$` - Rendered page-footer HTML (if page-footer: set)
const FULL_HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html$if(lang)$ lang="$lang$"$endif$>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="quarto-rust-$version$">
$for(author-meta)$
<meta name="author" content="$author-meta$">
$endfor$
$if(date)$
<meta name="dcterms.date" content="$date$">
$endif$
$if(keywords)$
<meta name="keywords" content="$for(keywords)$$it$$sep$, $endfor$">
$endif$
$if(description-meta)$
<meta name="description" content="$description-meta$">
$endif$
$if(canonical-url)$
<link rel="canonical" href="$canonical-url$">
$endif$
$if(pagetitle)$
<title>$pagetitle$</title>
$endif$
$for(css)$
<link rel="stylesheet" href="$css$">
$endfor$
$if(math)$
$math$
$endif$
$for(scripts)$
<script src="$scripts$"></script>
$endfor$
$for(header-includes)$
$header-includes$
$endfor$
</head>
<body class="$body-classes$">
$if(rendered.navigation.navbar)$
$rendered.navigation.navbar$
$endif$
$for(include-before)$
$include-before$
$endfor$
$if(rendered.title-block-banner)$
$title-block()$
$endif$

<div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-$page-layout$">
$if(rendered.navigation.sidebar)$
$rendered.navigation.sidebar$
$endif$
$if(rendered.navigation.toc)$
<div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
<nav id="TOC" role="doc-toc" class="toc-active">
$if(navigation.toc.title)$
<h2 id="toc-title">$navigation.toc.title$</h2>
$endif$
$rendered.navigation.toc$
</nav>
$if(rendered.navigation.margin_categories)$
$rendered.navigation.margin_categories$
$endif$
</div>
$else$
$if(rendered.navigation.margin_categories)$
<div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
$rendered.navigation.margin_categories$
</div>
$endif$
$endif$

<main class="content$if(rendered.title-block-banner)$ quarto-banner-title-block$endif$" id="quarto-document-content">

$if(rendered.title-block-banner)$
$else$
$title-block()$
$endif$

$body$

$if(rendered.navigation.page_navigation)$
$rendered.navigation.page_navigation$
$endif$
</main>
</div>
$for(include-after)$
$include-after$
$endfor$
$if(rendered.navigation.footer)$
$rendered.navigation.footer$
$endif$
</body>
</html>
"#;

// =============================================================================
// Built-in template partials (title block)
// =============================================================================

/// Built-in `title-block` partial — the styled Quarto title block,
/// ported from Quarto 1's
/// `src/resources/formats/html/templates/title-block.html` (default
/// style; the banner variant lands with bd-364ol5lu).
///
/// Emitted only when `rendered.has-title-block` is set (written by
/// `AuthorsNormalizeTransform` when any title-block content exists),
/// so metadata-less documents produce no empty `<header>`.
///
/// P3 (bd-j6huijli) additions, both Q1-verbatim: the category chips
/// (gated on `quarto-template-params.title-block-categories`, written
/// by `AuthorsNormalizeTransform` unless the document sets
/// `title-block-categories: false`) and the `description` block. The
/// `hide-description` gate is ported with it; nothing in Q2 sets that
/// flag yet (Q1's book pipeline does, for chapter pages — design
/// decision Q11), so it is inert until a project pipeline needs it.
///
/// P5 (bd-364ol5lu): the partial branches internally on
/// `rendered.title-block-banner` (written by `TitleBannerTransform`)
/// instead of registering a separate `banner/title-block` partial —
/// in Q1 a user's `template-partials` file named `title-block.html`
/// shadows the built-in in *both* modes (Pandoc resolves partials by
/// basename; Q1's banner file is `banner/title-block.html`), and the
/// single Q2 name preserves exactly that override semantics. The
/// banner branch is Q1's `banner/title-block.html` verbatim:
/// title/subtitle/description/categories move *inside*
/// `div.quarto-title-banner > div.quarto-title.column-body`, the meta
/// grid stays below the banner, and there is no `hide-description`
/// gate (Q1 parity). The `page-columns page-full` classes on the
/// header and banner div are baked into the markup — Q1 gets them
/// from its generic bootstrap grid DOM postprocessor, which Q2
/// doesn't have. `quarto-template-params.banner-header-class` is
/// ported verbatim but currently has no producer (Q1 sets `toc-left`
/// from `toc-location`, which Q2 doesn't support yet).
///
/// P6 (bd-vkiwhcny): `title-block-style: none` renders Q1's fallback
/// — Pandoc's own plain title block
/// (`formats/html/pandoc/title-block.html`): a bare header with no
/// quarto classes, `h1.title`, `p.subtitle` without `lead`, one
/// `p.author` per author, `p.date`, and `div.abstract >
/// div.abstract-title`. Gated on `rendered.title-block-none` (written
/// by `AuthorsNormalizeTransform`); the fallback iterates the
/// normalized `by-author` names (Pandoc iterates raw `$author$`;
/// same output for every supported author shape) and uses
/// `$labels.abstract$` where Pandoc uses its own `$abstract-title$`
/// variable (same "Abstract" default, and the `abstract-title`
/// override keeps working — deviation documented here).
///
/// A document can replace this partial by listing a file named
/// `title-block.html` under `template-partials` (Q1 compatibility).
pub const TITLE_BLOCK_PARTIAL: &str = r#"$if(rendered.has-title-block)$
$if(rendered.title-block-none)$
<header id="title-block-header">
$if(title)$<h1 class="title">$title$</h1>
$endif$
$if(subtitle)$
<p class="subtitle">$subtitle$</p>
$endif$
$for(by-author)$
<p class="author">$it.name.literal$</p>
$endfor$
$if(date)$
<p class="date">$date$</p>
$endif$
$if(abstract)$
<div class="abstract">
<div class="abstract-title">$labels.abstract$</div>
$abstract$
</div>
$endif$
</header>
$elseif(rendered.title-block-banner)$
<header id="title-block-header" class="quarto-title-block default page-columns page-full$if(quarto-template-params.banner-header-class)$ $quarto-template-params.banner-header-class$$endif$">
<div class="quarto-title-banner page-columns page-full">
<div class="quarto-title column-body">
$if(title)$
<h1 class="title">$title$</h1>
$endif$
$if(subtitle)$
<p class="subtitle lead">$subtitle$</p>
$endif$
$if(description)$
<div>
<div class="description">
$description$
</div>
</div>
$endif$
$if(categories)$
$if(quarto-template-params.title-block-categories)$
<div class="quarto-categories">
$for(categories)$
<div class="quarto-category">$it$</div>
$endfor$
</div>
$endif$
$endif$
</div>
</div>
$title-metadata()$
</header>
$else$
<header id="title-block-header" class="quarto-title-block default">
<div class="quarto-title">
$if(title)$
<h1 class="title">$title$</h1>
$endif$
$if(subtitle)$
<p class="subtitle lead">$subtitle$</p>
$endif$
$if(categories)$
$if(quarto-template-params.title-block-categories)$
<div class="quarto-categories">
$for(categories)$
<div class="quarto-category">$it$</div>
$endfor$
</div>
$endif$
$endif$
</div>
$if(hide-description)$
$elseif(description)$
<div>
<div class="description">
$description$
</div>
</div>
$endif$
$title-metadata()$
</header>
$endif$
$endif$"#;

/// Built-in `title-metadata` partial — the metadata grid below the
/// title, ported from Quarto 1's `title-metadata.html`.
///
/// When affiliations exist, authors render in the two-column
/// `.quarto-title-meta-author` grid (Authors/Affiliations headings)
/// and the plain `.quarto-title-meta` grid carries no authors cell
/// (Q1's `$if(by-affiliation)$` / `$elseif(by-author)$` split —
/// bd-ez0hiowa). P3 (bd-j6huijli) completes the grid with the
/// Modified and Doi cells (the doi linked to doi.org, Q1-verbatim)
/// and the trailing keywords block. (The description block lives in
/// [`TITLE_BLOCK_PARTIAL`], matching Q1's template split.)
///
/// Deviation from Q1's template text: Q1 gates the two-column grid on
/// `$if(by-affiliation/first)$`; doctemplate conditions don't take
/// pipes, and `AuthorsNormalizeTransform` only writes
/// `by-affiliation` when non-empty, so the plain variable test is
/// equivalent.
///
/// Like Q1, the `quarto-title-meta` grid div is emitted whenever the
/// title block renders, even if all its cells are empty.
pub const TITLE_METADATA_PARTIAL: &str = r#"$if(by-affiliation)$
<div class="quarto-title-meta-author">
<div class="quarto-title-meta-heading">$labels.authors$</div>
<div class="quarto-title-meta-heading">$labels.affiliations$</div>
$for(by-author)$
<div class="quarto-title-meta-contents">
<p class="author">$_title-meta-author()$</p>
</div>
<div class="quarto-title-meta-contents">
$for(by-author.affiliations)$
<p class="affiliation">$if(it.url)$<a href="$it.url$">$endif$$it.name$$if(it.url)$</a>$endif$</p>
$endfor$
</div>
$endfor$
</div>
$endif$
<div class="quarto-title-meta">
$if(by-affiliation)$
$elseif(by-author)$
<div>
<div class="quarto-title-meta-heading">$labels.authors$</div>
<div class="quarto-title-meta-contents">
$for(by-author)$
<p>$_title-meta-author()$</p>
$endfor$
</div>
</div>
$endif$
$if(date)$
<div>
<div class="quarto-title-meta-heading">$labels.published$</div>
<div class="quarto-title-meta-contents">
<p class="date">$date$</p>
</div>
</div>
$endif$
$if(date-modified)$
<div>
<div class="quarto-title-meta-heading">$labels.modified$</div>
<div class="quarto-title-meta-contents">
<p class="date-modified">$date-modified$</p>
</div>
</div>
$endif$
$if(doi)$
<div>
<div class="quarto-title-meta-heading">$labels.doi$</div>
<div class="quarto-title-meta-contents">
<p class="doi"><a href="https://doi.org/$doi$">$doi$</a></p>
</div>
</div>
$endif$
</div>
$if(abstract)$
<div>
<div class="abstract">
<div class="block-title">$labels.abstract$</div>
$abstract$
</div>
</div>
$endif$
$if(keywords)$
<div>
<div class="keywords">
<div class="block-title">$labels.keywords$</div>
<p>$for(keywords)$$it$$sep$, $endfor$</p>
</div>
</div>
$endif$"#;

/// Built-in `_title-meta-author` partial — one author's rendering
/// inside the title-block author lists, ported from Quarto 1's
/// `_title-meta-author.html`: the name (linked when the author has a
/// `url`), degrees after the name inside the link, an email icon
/// anchor (`quarto-title-author-email`), and an ORCID badge anchor
/// (`quarto-title-author-orcid`).
///
/// Deviations from Q1 (per design decision Q8): the ORCID badge is an
/// inline SVG (the ORCID glyph in brand green) instead of Q1's
/// base64 PNG `<img>`, and the email icon is an inline SVG of
/// Bootstrap Icons' `envelope` instead of the `bi bi-envelope` font
/// glyph (the icon font only ships with website projects). The
/// anchor class names — the extension/SCSS targets — are identical
/// to Q1's.
///
/// Evaluated inside `$for(by-author)$`, so `it` is one normalized
/// by-author entry.
pub const TITLE_META_AUTHOR_PARTIAL: &str = r##"$if(it.url)$<a href="$it.url$">$endif$$it.name.literal$$if(it.degrees)$, $for(it.degrees)$$it$$sep$, $endfor$$endif$$if(it.url)$</a>$endif$$if(it.email)$ <a href="mailto:$it.email$" class="quarto-title-author-email"><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" class="bi bi-envelope" viewBox="0 0 16 16" aria-hidden="true" focusable="false"><path d="M0 4a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H2a2 2 0 0 1-2-2zm2-1a1 1 0 0 0-1 1v.217l7 4.2 7-4.2V4a1 1 0 0 0-1-1zm13 2.383-4.708 2.825L15 11.105zm-.034 6.876-5.64-3.471L8 9.583l-1.326-.795-5.64 3.47A1 1 0 0 0 2 13h12a1 1 0 0 0 .966-.741M1 11.105l4.708-2.897L1 5.383z"/></svg></a>$endif$$if(it.orcid)$ <a href="https://orcid.org/$it.orcid$" class="quarto-title-author-orcid" aria-label="ORCID profile for $it.name.literal$"><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="#A6CE39" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M12 0C5.372 0 0 5.372 0 12s5.372 12 12 12 12-5.372 12-12S18.628 0 12 0zM7.369 4.378c.525 0 .947.431.947.947s-.422.947-.947.947a.95.95 0 0 1-.947-.947c0-.525.422-.947.947-.947zm-.722 3.038h1.444v10.041H6.647V7.416zm3.562 0h3.9c3.712 0 5.344 2.653 5.344 5.025 0 2.578-2.016 5.025-5.325 5.025h-3.919V7.416zm1.444 1.303v7.444h2.297c3.272 0 4.022-2.484 4.022-3.722 0-2.016-1.284-3.722-4.097-3.722h-2.222z"/></svg></a>$endif$"##;

/// Resolver holding the built-in HTML template partials.
///
/// Each partial is registered under both its bare name
/// (`title-block`, the form the built-in template uses) and its
/// `.html`-suffixed alias (`title-block.html`, the form Q1-ported
/// custom templates use), so either call syntax resolves.
pub fn builtin_html_partials() -> MemoryResolver {
    let mut resolver = MemoryResolver::new();
    for (name, content) in [
        ("title-block", TITLE_BLOCK_PARTIAL),
        ("title-metadata", TITLE_METADATA_PARTIAL),
        ("_title-meta-author", TITLE_META_AUTHOR_PARTIAL),
    ] {
        resolver.add(name, content);
        resolver.add(format!("{name}.html"), content);
    }
    resolver
}

// =============================================================================
// Template Compilation
// =============================================================================

/// Compile the minimal HTML template.
pub fn minimal_html_template() -> Result<Template> {
    Template::compile(MINIMAL_HTML_TEMPLATE)
        .map_err(|e| crate::error::QuartoError::other(e.to_string()))
}

/// Compile the full HTML template (resolving built-in partials).
pub fn full_html_template() -> Result<Template> {
    Template::compile_with_resolver(
        FULL_HTML_TEMPLATE,
        std::path::Path::new("<builtin>.html"),
        &builtin_html_partials(),
        0,
    )
    .map_err(|e| crate::error::QuartoError::other(e.to_string()))
}

/// Compile the default HTML template (minimal template for backwards compatibility).
pub fn default_html_template() -> Result<Template> {
    minimal_html_template()
}

/// Select and compile the appropriate template based on whether minimal mode is active.
///
/// Returns the minimal template when `minimal` is true, otherwise returns the
/// full template with Bootstrap-compatible structure.
pub fn select_template(minimal: bool) -> Result<Template> {
    if minimal {
        minimal_html_template()
    } else {
        full_html_template()
    }
}

/// Render a document to HTML using the template engine.
///
/// # Arguments
/// * `body` - The rendered body content (HTML)
/// * `meta` - Document metadata from the Pandoc AST (as ConfigValue)
///
/// # Returns
/// The complete HTML document as a string, plus any diagnostics
/// (e.g. `Q-10-2 Undefined variable`) emitted by the doctemplate
/// evaluator.
pub fn render_with_template(
    body: &str,
    meta: &ConfigValue,
) -> Result<(String, Vec<DiagnosticMessage>)> {
    let template = default_html_template()?;

    // Build template context from metadata
    let mut ctx = TemplateContext::new();

    // Add body content
    ctx.insert("body", TemplateValue::String(body.to_string()));

    // Convert and add metadata
    add_metadata_to_context(meta, &mut ctx);

    // Render the template, collecting diagnostics from the evaluator.
    let (html, diagnostics) = template.render_with_diagnostics(&ctx);
    let html = html.map_err(|()| {
        crate::error::QuartoError::other(
            "Template evaluation failed (see diagnostics for details)".to_string(),
        )
    })?;
    Ok((html, diagnostics))
}

/// Render a document with a pre-compiled template.
///
/// This is the shared rendering core used by all template rendering paths.
/// It builds the template context (body, metadata, CSS, JS, includes, version,
/// page-layout) and renders with the given template. Full-template extras
/// (`version`, `page-layout`) are always injected — unused variables are
/// harmlessly ignored.
///
/// # Arguments
/// * `template` - A compiled template
/// * `body` - The rendered body content (HTML)
/// * `meta` - Document metadata from the Pandoc AST (as ConfigValue).
///   Include slots are read from `meta.rendered.includes.{header,
///   before-body, after-body}` — populated upstream by
///   [`IncludeResolveStage`](crate::stage::IncludeResolveStage). Authored
///   `header-includes` / `include-before` / `include-after` literals at the
///   top level are NOT read here; they are folded into `rendered.includes.*`
///   by the resolve stage.
/// * `css_paths` - Paths to CSS files (relative to output HTML)
/// * `script_paths` - Paths to JS files (relative to output HTML)
///
/// # Returns
/// The complete HTML document as a string.
pub fn render_with_compiled_template(
    template: &Template,
    body: &str,
    meta: &ConfigValue,
    css_paths: &[String],
    script_paths: &[String],
) -> Result<(String, Vec<DiagnosticMessage>)> {
    let mut ctx = TemplateContext::new();
    ctx.insert("body", TemplateValue::String(body.to_string()));

    // Add metadata, excluding keys that are handled specially or shouldn't
    // leak into the template context as variables. The authored
    // `header-includes` / `include-before` / `include-after` keys (Pandoc
    // inline-content form) are also excluded here so they don't shadow the
    // resolved values. The resolve stage already folded them into
    // `rendered.includes.*`; reading them again here would double-count.
    add_metadata_to_context_except(
        meta,
        &mut ctx,
        &[
            "css",
            "template",
            "template-partials",
            "header-includes",
            "include-before",
            "include-after",
        ],
    );

    // Build combined CSS list: default resources first, then user-specified
    let mut css_list: Vec<TemplateValue> = css_paths
        .iter()
        .map(|p| TemplateValue::String(p.clone()))
        .collect();

    // Add any user-specified CSS from metadata
    if let Some(user_css) = extract_css_from_meta(meta) {
        css_list.extend(user_css);
    }

    ctx.insert("css", TemplateValue::List(css_list));

    // Build scripts list
    if !script_paths.is_empty() {
        let scripts_list: Vec<TemplateValue> = script_paths
            .iter()
            .map(|p| TemplateValue::String(p.clone()))
            .collect();
        ctx.insert("scripts", TemplateValue::List(scripts_list));
    }

    // Wire `rendered.includes.{header, before-body, after-body}` into the
    // Pandoc-native template variable names (kept stable per
    // claude-notes/plans/2026-05-04-includes-feature.md §Resolved questions
    // #2 — preserves portability of custom Pandoc templates).
    set_includes_list(&mut ctx, "header-includes", meta, "header");
    set_includes_list(&mut ctx, "include-before", meta, "before-body");
    set_includes_list(&mut ctx, "include-after", meta, "after-body");

    // Always inject full-template extras — custom templates may reference them,
    // and unused variables are harmlessly ignored.
    ctx.insert(
        "version",
        TemplateValue::String(env!("CARGO_PKG_VERSION").to_string()),
    );
    if ctx.get("page-layout").is_none() {
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
    }

    // Compute the body class. Order of precedence:
    //
    //   1. user-supplied `body-classes` (e.g. set in metadata) — kept as-is.
    //   2. `rendered.navigation.body-classes` (written by
    //      `SidebarRenderTransform`, e.g. `"nav-sidebar floating"`) —
    //      promoted to the top-level template variable. See bd-mgoh.
    //   3. TOC present but no sidebar → empty class. The body falls
    //      through to the default (no-class) wide grid, whose right
    //      margin column is `minmax(0.3*mw, 0.58*mw)` and has room for
    //      the TOC.
    //   4. Otherwise → `"fullcontent"`. That mixin's margin-seg{1,2}
    //      sum to only `0.28 * margin-width` (~70px at the default
    //      250px), which is intentional for content-heavy pages with
    //      no TOC — but would squash a TOC if one were present, which
    //      is why case (3) exists.
    //
    // Mirrors TS Quarto's body-class logic in
    // `src/format/html/format-html-bootstrap.ts`.
    if ctx.get("body-classes").is_none() {
        let from_meta = meta
            .get_path(&["rendered", "navigation", "body-classes"])
            .and_then(|v| v.as_plain_text());
        let has_toc = meta
            .get_path(&["rendered", "navigation", "toc"])
            .and_then(|v| v.as_plain_text())
            .is_some_and(|s| !s.is_empty());
        let structural = match (from_meta, has_toc) {
            (Some(s), _) => s,
            (None, true) => String::new(),
            (None, false) => "fullcontent".to_string(),
        };
        // bd-mtzry: append the color-mode class so theme-conditional CSS
        // can key off `body.quarto-light` (matches Q1 default). Dark-mode
        // theme support lands separately; for now we always emit `quarto-light`.
        let body_classes = append_color_mode_class(&structural);
        ctx.insert("body-classes", TemplateValue::String(body_classes));
    }

    // Render with diagnostics so undefined-variable warnings (etc.)
    // surface to callers instead of being silently dropped (bd-xdnk).
    let (html, diagnostics) = template.render_with_diagnostics(&ctx);
    let html = html.map_err(|()| {
        crate::error::QuartoError::other(
            "Template evaluation failed (see diagnostics for details)".to_string(),
        )
    })?;
    Ok((html, diagnostics))
}

/// Set a template `$for(...)$`-style includes variable from the canonical
/// `meta.rendered.includes.<slot>` location.
///
/// `template_var` is the Pandoc-native template variable name (e.g.
/// `header-includes`). `slot` is the corresponding key under
/// `rendered.includes.*` (one of `"header"`, `"before-body"`,
/// `"after-body"`).
///
/// `IncludeResolveStage` writes flat string arrays at this location,
/// folding authored YAML keys, smart-include `{file:..}` / `{text:..}`
/// objects, and engine-contributed `PandocIncludes`. If the array is empty
/// or absent (resolve stage didn't run), the template variable is not set
/// — `$for(template_var)$` then produces no output.
/// Append the active color-mode class (today always `quarto-light`)
/// to a structural body-class string. Empty input → `"quarto-light"`;
/// non-empty input → `"<structural> quarto-light"`. Idempotent: a
/// structural that already contains `quarto-light` is returned as-is.
///
/// bd-mtzry. Light/dark theme detection is not yet wired into the
/// pipeline (the `theme:` key today is a single Bootswatch name); when
/// it lands, this helper grows a `mode` argument and the call site
/// decides which class to emit. Until then `quarto-light` matches
/// Quarto 1's default body class for documents with no dark theme set.
fn append_color_mode_class(structural: &str) -> String {
    const LIGHT: &str = "quarto-light";
    let already = structural
        .split_whitespace()
        .any(|tok| tok == LIGHT || tok == "quarto-dark");
    if already {
        structural.to_string()
    } else if structural.is_empty() {
        LIGHT.to_string()
    } else {
        format!("{structural} {LIGHT}")
    }
}

fn set_includes_list(
    ctx: &mut TemplateContext,
    template_var: &str,
    meta: &ConfigValue,
    slot: &str,
) {
    let Some(arr) = meta
        .get_path(&["rendered", "includes", slot])
        .and_then(|v| v.as_array())
    else {
        return;
    };
    if arr.is_empty() {
        return;
    }
    let list: Vec<TemplateValue> = arr
        .iter()
        .filter_map(|v| v.as_plain_text().map(TemplateValue::String))
        .collect();
    if !list.is_empty() {
        ctx.insert(template_var, TemplateValue::List(list));
    }
}

/// Render a document with external resources.
///
/// Uses the default (minimal) HTML template with the given CSS paths.
pub fn render_with_resources(
    body: &str,
    meta: &ConfigValue,
    css_paths: &[String],
) -> Result<(String, Vec<DiagnosticMessage>)> {
    let template = default_html_template()?;
    render_with_compiled_template(&template, body, meta, css_paths, &[])
}

/// Render a document with format-based template selection.
///
/// Selects the appropriate template (minimal or full) based on
/// the format configuration, and adds CSS resource paths to the context.
pub fn render_with_format(
    body: &str,
    meta: &ConfigValue,
    _format: &Format,
    css_paths: &[String],
) -> Result<(String, Vec<DiagnosticMessage>)> {
    let minimal = is_minimal_html(meta);
    let template = select_template(minimal)?;
    // This convenience API takes raw (untransformed) metadata, so run
    // the author/label normalization the pipeline's
    // AuthorsNormalizeTransform would otherwise provide — the full
    // template's title block consumes its derived fields
    // (`by-author`, `labels`, `rendered.has-title-block`,
    // `author-meta`).
    let mut meta = meta.clone();
    crate::transforms::normalize_authors_meta(&mut meta);
    render_with_compiled_template(&template, body, &meta, css_paths, &[])
}

/// Compile the appropriate built-in template (minimal or full) with a custom
/// partial resolver. Used when extension metadata provides `template-partials`
/// without a custom `template`.
///
/// The `source_context` is the document's `SourceContext`; partial files
/// loaded by the resolver are registered here so their FileIds resolve
/// back to source slices when diagnostics reference them (bd-xdnk).
pub fn compile_builtin_template_with_partials(
    meta: &ConfigValue,
    resolver: &impl PartialResolver,
    source_context: &mut quarto_source_map::SourceContext,
) -> Result<Template> {
    let minimal = is_minimal_html(meta);
    let source = if minimal {
        MINIMAL_HTML_TEMPLATE
    } else {
        FULL_HTML_TEMPLATE
    };
    // User-supplied partials shadow the built-in ones (so a document
    // can override `title-block.html` Q1-style); built-ins resolve
    // anything the user didn't provide.
    let chained = ChainedResolver::new(resolver, builtin_html_partials());
    Template::compile_with_resolver_and_context(
        source,
        std::path::Path::new("<builtin>.html"),
        &chained,
        0,
        source_context,
    )
    .map_err(|e| crate::error::QuartoError::other(e.to_string()))
}

/// Add metadata from the Pandoc AST to the template context, excluding specific keys.
fn add_metadata_to_context_except(meta: &ConfigValue, ctx: &mut TemplateContext, exclude: &[&str]) {
    if let ConfigValueKind::Map(entries) = &meta.value {
        for entry in entries {
            if !exclude.contains(&entry.key.as_str()) {
                let value = metadata_entry_to_template_value(&entry.key, &entry.value);
                ctx.insert(&entry.key, value);
            }
        }
    }
}

/// Extract CSS paths from document metadata.
fn extract_css_from_meta(meta: &ConfigValue) -> Option<Vec<TemplateValue>> {
    if let ConfigValueKind::Map(entries) = &meta.value {
        for entry in entries {
            if entry.key == "css" {
                // Try string first
                if let Some(s) = entry.value.as_str() {
                    return Some(vec![TemplateValue::String(s.to_string())]);
                }
                // Try inlines (YAML values like `css: custom.css` are often parsed as inlines)
                if let ConfigValueKind::PandocInlines(content) = &entry.value.value {
                    let text = inlines_to_text(content);
                    return Some(vec![TemplateValue::String(text)]);
                }
                // Try array
                if let ConfigValueKind::Array(items) = &entry.value.value {
                    return Some(items.iter().map(config_value_to_template_value).collect());
                }
                return Some(Vec::new());
            }
        }
    }
    None
}

/// Add metadata from the Pandoc AST to the template context.
fn add_metadata_to_context(meta: &ConfigValue, ctx: &mut TemplateContext) {
    if let ConfigValueKind::Map(entries) = &meta.value {
        for entry in entries {
            let value = metadata_entry_to_template_value(&entry.key, &entry.value);
            ctx.insert(&entry.key, value);
        }
    }
}

/// Title-block metadata fields whose inline/block Markdown is rendered to
/// HTML (rather than flattened to plain text) so that markup like code
/// spans and emphasis survives into the body title block.
///
/// `pagetitle` is deliberately *not* listed: it feeds the head `<title>`
/// element, where HTML tags are invalid. It is derived as plain text by
/// `derive_pagetitle` (pampa's `template::config_merge`) and must stay so.
/// Likewise `description-meta` (not listed): the head's
/// `<meta name="description">` consumes it, derived as plain text from
/// `description` by `MetadataNormalizeTransform` — the same
/// `description`/`description-meta` split Pandoc's HTML writer and
/// Q1's `html.template` use, which is what lets `description` itself
/// render rich in the title block.
///
/// `author`/`date` are out of scope for now (an author can be an object or
/// list, and also feeds `<meta>` attribute contexts that require plain
/// text); they continue to flatten via the generic conversion. See
/// strand bd-5706gcrq. (The *display* author list is separately
/// normalized into `by-author` by `AuthorsNormalizeTransform`.)
///
/// `abstract` is not listed here: it renders in *block* context (its
/// paragraphs become `<p>` elements, Q1 parity) via
/// [`titleblock_field_to_block_html`].
const RICH_TITLE_BLOCK_FIELDS: &[&str] = &["title", "subtitle", "description"];

/// Convert a metadata entry to a template value, honoring the rich
/// title-block allowlist. Allowlisted fields whose value is Pandoc
/// inlines/blocks are rendered to HTML; everything else (and every
/// non-allowlisted field) uses the generic plain-text conversion.
fn metadata_entry_to_template_value(key: &str, value: &ConfigValue) -> TemplateValue {
    if key == "abstract"
        && let Some(html) = titleblock_field_to_block_html(value)
    {
        return TemplateValue::String(html);
    }
    if RICH_TITLE_BLOCK_FIELDS.contains(&key)
        && let Some(html) = titleblock_field_to_html(value)
    {
        return TemplateValue::String(html);
    }
    config_value_to_template_value(value)
}

/// Render a title-block field's Pandoc content to an HTML string.
///
/// Returns `None` for non-Pandoc values (scalars, arrays, maps, …) so the
/// caller falls back to the generic conversion. Uses the HTML writer's
/// default config, which emits no source-location annotations — the head
/// title block in the CLI render path is unannotated, and preview-path
/// annotations are tracked separately (bd-z37euevy).
fn titleblock_field_to_html(value: &ConfigValue) -> Option<String> {
    match &value.value {
        ConfigValueKind::PandocInlines(inlines) => {
            let mut out: Vec<u8> = Vec::new();
            pampa::writers::html::write_inlines_to(inlines, &mut out).ok()?;
            Some(String::from_utf8_lossy(&out).into_owned())
        }
        ConfigValueKind::PandocBlocks(blocks) => {
            let mut out: Vec<u8> = Vec::new();
            pampa::writers::html::write_blocks_to(blocks, &mut out).ok()?;
            Some(String::from_utf8_lossy(&out).into_owned())
        }
        _ => None,
    }
}

/// Render a title-block field's Pandoc content to HTML in **block**
/// context: paragraphs become `<p>` elements (Q1 title-block parity
/// for `abstract`). Inline content and scalar strings are wrapped in
/// a synthetic paragraph first.
fn titleblock_field_to_block_html(value: &ConfigValue) -> Option<String> {
    use quarto_pandoc_types::block::{Block, Paragraph};
    use quarto_pandoc_types::inline::{Inline, Str};

    let write = |blocks: &[Block]| -> Option<String> {
        let mut out: Vec<u8> = Vec::new();
        pampa::writers::html::write_blocks_to(blocks, &mut out).ok()?;
        Some(String::from_utf8_lossy(&out).into_owned())
    };

    match &value.value {
        ConfigValueKind::PandocBlocks(blocks) => write(blocks),
        ConfigValueKind::PandocInlines(inlines) => {
            let para = Block::Paragraph(Paragraph {
                content: inlines.clone(),
                source_info: value.source_info.clone(),
            });
            write(&[para])
        }
        _ => {
            let text = value.as_str()?;
            let para = Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: text.to_string(),
                    source_info: value.source_info.clone(),
                })],
                source_info: value.source_info.clone(),
            });
            write(&[para])
        }
    }
}

/// Convert a ConfigValue to a TemplateValue.
fn config_value_to_template_value(meta: &ConfigValue) -> TemplateValue {
    // Try string-like values first (handles Scalar(String), Path, Glob, Expr)
    if let Some(s) = meta.as_str() {
        return TemplateValue::String(s.to_string());
    }

    // Try boolean
    if let Some(b) = meta.as_bool() {
        return TemplateValue::Bool(b);
    }

    // Try integer
    if let Some(i) = meta.as_int() {
        return TemplateValue::String(i.to_string());
    }

    // Check for null
    if meta.is_null() {
        return TemplateValue::Null;
    }

    // Handle other variants
    match &meta.value {
        ConfigValueKind::PandocInlines(content) => {
            // Convert inlines to plain text for template use
            let text = inlines_to_text(content);
            TemplateValue::String(text)
        }
        ConfigValueKind::PandocBlocks(content) => {
            // Convert blocks to plain text for template use
            let text = blocks_to_text(content);
            TemplateValue::String(text)
        }
        ConfigValueKind::Array(items) => {
            let list_items: Vec<TemplateValue> =
                items.iter().map(config_value_to_template_value).collect();
            TemplateValue::List(list_items)
        }
        ConfigValueKind::Map(entries) => {
            let mut map = std::collections::HashMap::new();
            for entry in entries {
                let value = config_value_to_template_value(&entry.value);
                map.insert(entry.key.clone(), value);
            }
            TemplateValue::Map(map)
        }
        // Scalar variants already handled above (string, bool, int, null)
        // Path, Glob, Expr already handled by as_str()
        _ => TemplateValue::Null,
    }
}

/// Convert inlines to plain text.
fn inlines_to_text(inlines: &[quarto_pandoc_types::inline::Inline]) -> String {
    use quarto_pandoc_types::inline::Inline;

    let mut result = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(s) => result.push_str(&s.text),
            Inline::Space(_) => result.push(' '),
            Inline::SoftBreak(_) => result.push(' '),
            Inline::LineBreak(_) => result.push('\n'),
            Inline::Emph(e) => result.push_str(&inlines_to_text(&e.content)),
            Inline::Strong(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Underline(u) => result.push_str(&inlines_to_text(&u.content)),
            Inline::Strikeout(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Superscript(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Subscript(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::SmallCaps(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Quoted(q) => {
                result.push('"');
                result.push_str(&inlines_to_text(&q.content));
                result.push('"');
            }
            Inline::Code(c) => result.push_str(&c.text),
            Inline::Math(m) => result.push_str(&m.text),
            Inline::Link(l) => result.push_str(&inlines_to_text(&l.content)),
            Inline::Image(i) => result.push_str(&inlines_to_text(&i.content)),
            Inline::Span(s) => result.push_str(&inlines_to_text(&s.content)),
            Inline::Cite(c) => result.push_str(&inlines_to_text(&c.content)),
            Inline::Note(n) => result.push_str(&blocks_to_text(&n.content)),
            _ => {}
        }
    }
    result
}

/// Convert blocks to plain text.
fn blocks_to_text(blocks: &[quarto_pandoc_types::block::Block]) -> String {
    use quarto_pandoc_types::block::Block;

    let mut result = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        match block {
            Block::Plain(p) => result.push_str(&inlines_to_text(&p.content)),
            Block::Paragraph(p) => result.push_str(&inlines_to_text(&p.content)),
            Block::Header(h) => result.push_str(&inlines_to_text(&h.content)),
            Block::CodeBlock(c) => result.push_str(&c.text),
            Block::BlockQuote(b) => result.push_str(&blocks_to_text(&b.content)),
            Block::Div(d) => result.push_str(&blocks_to_text(&d.content)),
            Block::LineBlock(l) => {
                for line in &l.content {
                    result.push_str(&inlines_to_text(line));
                    result.push('\n');
                }
            }
            Block::OrderedList(o) => {
                for item in &o.content {
                    result.push_str(&blocks_to_text(item));
                    result.push('\n');
                }
            }
            Block::BulletList(b) => {
                for item in &b.content {
                    result.push_str(&blocks_to_text(item));
                    result.push('\n');
                }
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::attr::{AttrSourceInfo, empty_attr};
    use quarto_pandoc_types::block::*;
    use quarto_pandoc_types::inline::*;
    use quarto_pandoc_types::{ListNumberDelim, ListNumberStyle};
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    fn dummy_source_info() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    #[test]
    fn test_default_template_compiles() {
        let result = default_html_template();
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_simple_document() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "pagetitle".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("Test Document", dummy_source_info()),
            }],
            dummy_source_info(),
        );

        let body = "<p>Hello, World!</p>";
        let result = render_with_template(body, &meta);

        assert!(result.is_ok());
        let (html, _diags) = result.unwrap();
        assert!(html.contains("<title>Test Document</title>"));
        assert!(html.contains("<p>Hello, World!</p>"));
        assert!(html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn test_render_with_css() {
        let meta = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "pagetitle".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("Test", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "css".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_array(
                        vec![
                            ConfigValue::new_string("style1.css", dummy_source_info()),
                            ConfigValue::new_string("style2.css", dummy_source_info()),
                        ],
                        dummy_source_info(),
                    ),
                },
            ],
            dummy_source_info(),
        );

        let body = "<p>Content</p>";
        let result = render_with_template(body, &meta);

        assert!(result.is_ok());
        let (html, _diags) = result.unwrap();
        assert!(html.contains(r#"<link rel="stylesheet" href="style1.css">"#));
        assert!(html.contains(r#"<link rel="stylesheet" href="style2.css">"#));
    }

    #[test]
    fn test_render_with_resources() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "pagetitle".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("Test", dummy_source_info()),
            }],
            dummy_source_info(),
        );

        let css_paths = vec!["lib/styles.css".to_string(), "lib/theme.css".to_string()];
        let result = render_with_resources("<p>Body</p>", &meta, &css_paths);

        assert!(result.is_ok());
        let (html, _diags) = result.unwrap();
        assert!(html.contains(r#"href="lib/styles.css"#));
        assert!(html.contains(r#"href="lib/theme.css"#));
    }

    #[test]
    fn test_render_with_resources_combines_css() {
        let meta = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "pagetitle".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("Test", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "css".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("user.css", dummy_source_info()),
                },
            ],
            dummy_source_info(),
        );

        let css_paths = vec!["default.css".to_string()];
        let result = render_with_resources("<p>Body</p>", &meta, &css_paths);

        assert!(result.is_ok());
        let (html, _diags) = result.unwrap();
        // Both default and user CSS should be present
        assert!(html.contains("default.css"));
        assert!(html.contains("user.css"));
    }

    // === ConfigValue conversion tests ===

    #[test]
    fn test_config_value_conversion_string() {
        let meta = ConfigValue::new_string("test", dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("test".to_string()));
    }

    #[test]
    fn test_config_value_conversion_bool() {
        let meta = ConfigValue::new_bool(true, dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::Bool(true));
    }

    #[test]
    fn test_config_value_conversion_bool_false() {
        let meta = ConfigValue::new_bool(false, dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::Bool(false));
    }

    #[test]
    fn test_config_value_conversion_int() {
        // Test integer conversion via a map since ConfigValue doesn't expose direct int construction
        // The actual int handling is tested via the config_value_to_template_value function
        // when it encounters Scalar(Integer) in the AST
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "num".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("42", dummy_source_info()), // String representation
            }],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        // Map conversion works
        match value {
            TemplateValue::Map(map) => {
                assert!(map.contains_key("num"));
            }
            _ => panic!("Expected Map"),
        }
    }

    #[test]
    fn test_config_value_conversion_null() {
        let meta = ConfigValue::null(dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::Null);
    }

    #[test]
    fn test_config_value_conversion_path() {
        let meta = ConfigValue::new_path("./data.csv".to_string(), dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("./data.csv".to_string()));
    }

    #[test]
    fn test_config_value_conversion_glob() {
        let meta = ConfigValue::new_glob("*.qmd".to_string(), dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("*.qmd".to_string()));
    }

    #[test]
    fn test_config_value_conversion_expr() {
        let meta = ConfigValue::new_expr("params$x".to_string(), dummy_source_info());
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("params$x".to_string()));
    }

    #[test]
    fn test_config_value_conversion_inlines() {
        let meta = ConfigValue::new_inlines(
            vec![
                Inline::Str(Str {
                    text: "Hello".to_string(),
                    source_info: dummy_source_info(),
                }),
                Inline::Space(Space {
                    source_info: dummy_source_info(),
                }),
                Inline::Str(Str {
                    text: "World".to_string(),
                    source_info: dummy_source_info(),
                }),
            ],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("Hello World".to_string()));
    }

    #[test]
    fn test_config_value_conversion_blocks() {
        let meta = ConfigValue::new_blocks(
            vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Test paragraph".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        assert_eq!(value, TemplateValue::String("Test paragraph".to_string()));
    }

    #[test]
    fn test_config_value_conversion_list() {
        let meta = ConfigValue::new_array(
            vec![
                ConfigValue::new_string("a", dummy_source_info()),
                ConfigValue::new_string("b", dummy_source_info()),
            ],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        match value {
            TemplateValue::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], TemplateValue::String("a".to_string()));
                assert_eq!(items[1], TemplateValue::String("b".to_string()));
            }
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn test_config_value_conversion_map() {
        let meta = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "key1".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("value1", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "key2".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_bool(true, dummy_source_info()),
                },
            ],
            dummy_source_info(),
        );
        let value = config_value_to_template_value(&meta);
        match value {
            TemplateValue::Map(map) => {
                assert_eq!(map.len(), 2);
                assert_eq!(
                    map.get("key1"),
                    Some(&TemplateValue::String("value1".to_string()))
                );
                assert_eq!(map.get("key2"), Some(&TemplateValue::Bool(true)));
            }
            _ => panic!("Expected Map"),
        }
    }

    // === extract_css_from_meta tests ===

    #[test]
    fn test_extract_css_string() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("style.css", dummy_source_info()),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        assert!(css.is_some());
        let css_list = css.unwrap();
        assert_eq!(css_list.len(), 1);
        assert_eq!(css_list[0], TemplateValue::String("style.css".to_string()));
    }

    #[test]
    fn test_extract_css_inlines() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_inlines(
                    vec![Inline::Str(Str {
                        text: "inline.css".to_string(),
                        source_info: dummy_source_info(),
                    })],
                    dummy_source_info(),
                ),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        assert!(css.is_some());
        let css_list = css.unwrap();
        assert_eq!(css_list.len(), 1);
        assert_eq!(css_list[0], TemplateValue::String("inline.css".to_string()));
    }

    #[test]
    fn test_extract_css_array() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_array(
                    vec![
                        ConfigValue::new_string("a.css", dummy_source_info()),
                        ConfigValue::new_string("b.css", dummy_source_info()),
                    ],
                    dummy_source_info(),
                ),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        assert!(css.is_some());
        let css_list = css.unwrap();
        assert_eq!(css_list.len(), 2);
    }

    #[test]
    fn test_extract_css_not_present() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "title".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string("Test", dummy_source_info()),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        assert!(css.is_none());
    }

    #[test]
    fn test_extract_css_null_value() {
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "css".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::null(dummy_source_info()),
            }],
            dummy_source_info(),
        );
        let css = extract_css_from_meta(&meta);
        // Returns empty vec for non-recognized css value
        assert!(css.is_some());
        assert!(css.unwrap().is_empty());
    }

    // === inlines_to_text tests ===

    #[test]
    fn test_inlines_to_text_soft_break() {
        let inlines = vec![
            Inline::Str(Str {
                text: "Line1".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::SoftBreak(SoftBreak {
                source_info: dummy_source_info(),
            }),
            Inline::Str(Str {
                text: "Line2".to_string(),
                source_info: dummy_source_info(),
            }),
        ];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "Line1 Line2");
    }

    #[test]
    fn test_inlines_to_text_line_break() {
        let inlines = vec![
            Inline::Str(Str {
                text: "Line1".to_string(),
                source_info: dummy_source_info(),
            }),
            Inline::LineBreak(LineBreak {
                source_info: dummy_source_info(),
            }),
            Inline::Str(Str {
                text: "Line2".to_string(),
                source_info: dummy_source_info(),
            }),
        ];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "Line1\nLine2");
    }

    #[test]
    fn test_inlines_to_text_emph() {
        let inlines = vec![Inline::Emph(Emph {
            content: vec![Inline::Str(Str {
                text: "emphasized".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "emphasized");
    }

    #[test]
    fn test_inlines_to_text_strong() {
        let inlines = vec![Inline::Strong(Strong {
            content: vec![Inline::Str(Str {
                text: "bold".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "bold");
    }

    #[test]
    fn test_inlines_to_text_underline() {
        let inlines = vec![Inline::Underline(Underline {
            content: vec![Inline::Str(Str {
                text: "underlined".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "underlined");
    }

    #[test]
    fn test_inlines_to_text_strikeout() {
        let inlines = vec![Inline::Strikeout(Strikeout {
            content: vec![Inline::Str(Str {
                text: "struck".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "struck");
    }

    #[test]
    fn test_inlines_to_text_superscript() {
        let inlines = vec![Inline::Superscript(Superscript {
            content: vec![Inline::Str(Str {
                text: "2".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "2");
    }

    #[test]
    fn test_inlines_to_text_subscript() {
        let inlines = vec![Inline::Subscript(Subscript {
            content: vec![Inline::Str(Str {
                text: "i".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "i");
    }

    #[test]
    fn test_inlines_to_text_smallcaps() {
        let inlines = vec![Inline::SmallCaps(SmallCaps {
            content: vec![Inline::Str(Str {
                text: "smallcaps".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "smallcaps");
    }

    #[test]
    fn test_inlines_to_text_quoted() {
        let inlines = vec![Inline::Quoted(Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![Inline::Str(Str {
                text: "quoted".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "\"quoted\"");
    }

    #[test]
    fn test_inlines_to_text_code() {
        let inlines = vec![Inline::Code(Code {
            attr: quarto_pandoc_types::attr::Attr::default(),
            text: "code()".to_string(),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "code()");
    }

    #[test]
    fn test_inlines_to_text_math() {
        let inlines = vec![Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x^2".to_string(),
            source_info: dummy_source_info(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "x^2");
    }

    #[test]
    fn test_inlines_to_text_link() {
        let inlines = vec![Inline::Link(Link {
            attr: quarto_pandoc_types::attr::Attr::default(),
            content: vec![Inline::Str(Str {
                text: "link text".to_string(),
                source_info: dummy_source_info(),
            })],
            target: ("https://example.com".to_string(), String::new()),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
            target_source: quarto_pandoc_types::attr::TargetSourceInfo::empty(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "link text");
    }

    #[test]
    fn test_inlines_to_text_span() {
        let inlines = vec![Inline::Span(Span {
            attr: quarto_pandoc_types::attr::Attr::default(),
            content: vec![Inline::Str(Str {
                text: "span content".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = inlines_to_text(&inlines);
        assert_eq!(text, "span content");
    }

    // === blocks_to_text tests ===

    #[test]
    fn test_blocks_to_text_plain() {
        let blocks = vec![Block::Plain(Plain {
            content: vec![Inline::Str(Str {
                text: "plain text".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "plain text");
    }

    #[test]
    fn test_blocks_to_text_paragraph() {
        let blocks = vec![Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: "paragraph".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "paragraph");
    }

    #[test]
    fn test_blocks_to_text_header() {
        let blocks = vec![Block::Header(Header {
            level: 1,
            attr: quarto_pandoc_types::attr::Attr::default(),
            content: vec![Inline::Str(Str {
                text: "Heading".to_string(),
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "Heading");
    }

    #[test]
    fn test_blocks_to_text_code_block() {
        let blocks = vec![Block::CodeBlock(CodeBlock {
            attr: quarto_pandoc_types::attr::Attr::default(),
            text: "fn main() {}".to_string(),
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "fn main() {}");
    }

    #[test]
    fn test_blocks_to_text_blockquote() {
        let blocks = vec![Block::BlockQuote(BlockQuote {
            content: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "quoted".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "quoted");
    }

    #[test]
    fn test_blocks_to_text_div() {
        let blocks = vec![Block::Div(Div {
            attr: quarto_pandoc_types::attr::Attr::default(),
            content: vec![Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "div content".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })],
            source_info: dummy_source_info(),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        })];
        let text = blocks_to_text(&blocks);
        assert_eq!(text, "div content");
    }

    #[test]
    fn test_blocks_to_text_lineblock() {
        let blocks = vec![Block::LineBlock(LineBlock {
            content: vec![
                vec![Inline::Str(Str {
                    text: "Line 1".to_string(),
                    source_info: dummy_source_info(),
                })],
                vec![Inline::Str(Str {
                    text: "Line 2".to_string(),
                    source_info: dummy_source_info(),
                })],
            ],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert!(text.contains("Line 1"));
        assert!(text.contains("Line 2"));
    }

    #[test]
    fn test_blocks_to_text_ordered_list() {
        let blocks = vec![Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Default, ListNumberDelim::Default),
            content: vec![vec![Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "Item 1".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })]],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert!(text.contains("Item 1"));
    }

    #[test]
    fn test_blocks_to_text_bullet_list() {
        let blocks = vec![Block::BulletList(BulletList {
            content: vec![vec![Block::Plain(Plain {
                content: vec![Inline::Str(Str {
                    text: "Bullet".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            })]],
            source_info: dummy_source_info(),
        })];
        let text = blocks_to_text(&blocks);
        assert!(text.contains("Bullet"));
    }

    #[test]
    fn test_blocks_to_text_multiple() {
        let blocks = vec![
            Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Para 1".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
            Block::Paragraph(Paragraph {
                content: vec![Inline::Str(Str {
                    text: "Para 2".to_string(),
                    source_info: dummy_source_info(),
                })],
                source_info: dummy_source_info(),
            }),
        ];
        let text = blocks_to_text(&blocks);
        assert!(text.contains("Para 1"));
        assert!(text.contains("Para 2"));
        // Should have newline between blocks
        assert!(text.contains('\n'));
    }

    // === Template compilation tests ===

    #[test]
    fn test_minimal_template_compiles() {
        let template = minimal_html_template();
        assert!(template.is_ok());
    }

    #[test]
    fn test_full_template_compiles() {
        let template = full_html_template();
        assert!(template.is_ok());
    }

    // === Template selection tests ===

    #[test]
    fn test_select_template_default_is_full() {
        let template = select_template(false).unwrap();

        // Render with minimal context to verify it's the full template
        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Hello</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));
        let html = template.render(&ctx).unwrap();

        // Full template has quarto-content and main wrapper
        assert!(html.contains("quarto-content"));
        assert!(html.contains("<main class=\"content\""));
    }

    #[test]
    fn test_select_template_minimal_true() {
        let template = select_template(true).unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Hello</p>".to_string()));
        let html = template.render(&ctx).unwrap();

        // Minimal template does NOT have quarto-content
        assert!(!html.contains("quarto-content"));
        assert!(!html.contains("<main class=\"content\""));
        // But does have body
        assert!(html.contains("<body>"));
        assert!(html.contains("<p>Hello</p>"));
    }

    #[test]
    fn test_select_template_not_minimal_uses_full() {
        let template = select_template(false).unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Hello</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));
        let html = template.render(&ctx).unwrap();

        // Full template
        assert!(html.contains("quarto-content"));
    }

    // === Full template structure tests ===

    #[test]
    fn test_full_template_title_block() {
        let template = full_html_template().unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Content</p>".to_string()));
        ctx.insert("title", TemplateValue::String("My Document".to_string()));
        ctx.insert("subtitle", TemplateValue::String("A Subtitle".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));
        // The title-block partial gates on the flag the pipeline's
        // AuthorsNormalizeTransform derives.
        let mut rendered = std::collections::HashMap::new();
        rendered.insert("has-title-block".to_string(), TemplateValue::Bool(true));
        ctx.insert("rendered", TemplateValue::Map(rendered));

        let html = template.render(&ctx).unwrap();

        assert!(html.contains("<header id=\"title-block-header\""));
        assert!(html.contains("<h1 class=\"title\">My Document</h1>"));
        assert!(html.contains("<p class=\"subtitle lead\">A Subtitle</p>"));
    }

    #[test]
    fn test_full_template_no_title_block_without_title() {
        let template = full_html_template().unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Content</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));
        // No title set

        let html = template.render(&ctx).unwrap();

        // No title block header without title
        assert!(!html.contains("<header id=\"title-block-header\""));
        // But still has quarto-content wrapper
        assert!(html.contains("quarto-content"));
    }

    #[test]
    fn test_full_template_metadata() {
        let template = full_html_template().unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Content</p>".to_string()));
        // The head's author tag iterates the normalized `author-meta`
        // list (one <meta> per author), not the raw `author` value.
        ctx.insert(
            "author-meta",
            TemplateValue::List(vec![TemplateValue::String("Jane Doe".to_string())]),
        );
        ctx.insert("date", TemplateValue::String("2024-01-15".to_string()));
        // Keywords arrive as a list (YAML `keywords: [rust, quarto]`);
        // the head meta joins them with ", ". The description meta
        // consumes the plain-text `description-meta` derived by
        // MetadataNormalizeTransform, never the (possibly rich)
        // `description` itself.
        ctx.insert(
            "keywords",
            TemplateValue::List(vec![
                TemplateValue::String("rust".to_string()),
                TemplateValue::String("quarto".to_string()),
            ]),
        );
        ctx.insert(
            "description-meta",
            TemplateValue::String("A sample document".to_string()),
        );
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));

        let html = template.render(&ctx).unwrap();

        assert!(html.contains("<meta name=\"author\" content=\"Jane Doe\">"));
        assert!(html.contains("<meta name=\"dcterms.date\" content=\"2024-01-15\">"));
        assert!(html.contains("<meta name=\"keywords\" content=\"rust, quarto\">"));
        assert!(html.contains("<meta name=\"description\" content=\"A sample document\">"));
    }

    #[test]
    fn test_full_template_generator_meta() {
        let template = full_html_template().unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Content</p>".to_string()));
        ctx.insert("version", TemplateValue::String("1.2.3".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));

        let html = template.render(&ctx).unwrap();

        assert!(html.contains("<meta name=\"generator\" content=\"quarto-rust-1.2.3\">"));
    }

    #[test]
    fn test_full_template_page_layout_class() {
        let template = full_html_template().unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Content</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("full".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));

        let html = template.render(&ctx).unwrap();

        assert!(html.contains("page-layout-full"));
    }

    #[test]
    fn test_full_template_body_classes() {
        let template = full_html_template().unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Content</p>".to_string()));
        ctx.insert(
            "body-classes",
            TemplateValue::String("my-class another-class".to_string()),
        );
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));

        let html = template.render(&ctx).unwrap();

        // body-classes is the full class list — no hardcoded `fullcontent` prefix.
        assert!(html.contains("<body class=\"my-class another-class\">"));
    }

    #[test]
    fn test_full_template_default_body_class_is_fullcontent() {
        // No sidebar, no TOC → `render_with_compiled_template` falls back to
        // `fullcontent`. That mixin lets the body content span more of the
        // page since there's nothing in the right margin to make room for.
        //
        // bd-mtzry: the color-mode class `quarto-light` is appended so
        // theme-conditional CSS can key off `body.quarto-light` (matches
        // Quarto 1's default body class).
        let template = full_html_template().unwrap();
        let meta = ConfigValue::null(dummy_source_info());

        let (html, _diags) =
            render_with_compiled_template(&template, "<p>Content</p>", &meta, &[], &[]).unwrap();

        assert!(
            html.contains("<body class=\"fullcontent quarto-light\">"),
            "expected default body class `fullcontent quarto-light`; got: {}",
            &html[..html.len().min(800)]
        );
    }

    /// When a page has a TOC but no sidebar, the body must NOT get the
    /// `fullcontent` class — that mixin's margin segments are sized at
    /// `0.14 * margin-width` each, giving a ~70px TOC column that
    /// overflows horizontally. The default (no-class) layout
    /// (`page-columns-default-wide`) has a `minmax(0.3*mw, 0.58*mw)`
    /// margin-seg2 that leaves room for the TOC. Mirrors TS Quarto's
    /// `format-html-bootstrap.ts` body-class logic.
    #[test]
    fn test_full_template_toc_present_yields_empty_body_class() {
        let template = full_html_template().unwrap();

        let mut meta = ConfigValue::null(dummy_source_info());
        meta.insert_path(
            &["rendered", "navigation", "toc"],
            ConfigValue::new_string("<nav id=\"TOC\"></nav>", dummy_source_info()),
        );

        let (html, _diags) =
            render_with_compiled_template(&template, "<p>Content</p>", &meta, &[], &[]).unwrap();

        // bd-mtzry: even when the structural classes drop to empty,
        // `quarto-light` still appears so theme-conditional CSS keys
        // off `body.quarto-light` (matches Q1's default body class).
        assert!(
            html.contains("<body class=\"quarto-light\">"),
            "expected body class `quarto-light` when TOC present and no body-classes; got: {}",
            &html[..html.len().min(800)]
        );
        assert!(
            !html.contains("<body class=\"fullcontent"),
            "must NOT use `fullcontent` class when a TOC is rendered (margins too narrow); got: {}",
            &html[..html.len().min(800)]
        );
    }

    /// bd-mtzry: even when the caller supplies an explicit `body-classes`
    /// (e.g. via metadata or `SidebarRenderTransform`), the color-mode
    /// class is still appended. Quarto 1 always emits a `quarto-light` /
    /// `quarto-dark` class so theme-conditional CSS works regardless of
    /// the structural class. Today only `quarto-light` is emitted
    /// (light/dark theme support is tracked elsewhere).
    #[test]
    fn test_full_template_color_mode_class_appended_to_user_body_classes() {
        let template = full_html_template().unwrap();
        let mut meta = ConfigValue::null(dummy_source_info());
        meta.insert_path(
            &["rendered", "navigation", "body-classes"],
            ConfigValue::new_string("docked", dummy_source_info()),
        );

        let (html, _diags) =
            render_with_compiled_template(&template, "<p>Content</p>", &meta, &[], &[]).unwrap();

        assert!(
            html.contains("<body class=\"docked quarto-light\">"),
            "expected user body-classes + quarto-light; got: {}",
            &html[..html.len().min(800)]
        );
    }

    #[test]
    fn test_full_template_no_sidebar_wrapper() {
        // Sidebar HTML must appear as a direct child of #quarto-content,
        // not inside a `<div id="quarto-sidebar-container">` wrapper.
        // The SCSS rules in resources/scss target `#quarto-sidebar`
        // directly as a grid child; a wrapper would intercept the
        // grid placement.
        use std::collections::HashMap;

        let template = full_html_template().unwrap();

        let mut nav_map = HashMap::new();
        nav_map.insert(
            "sidebar".to_string(),
            TemplateValue::String(
                "<nav id=\"quarto-sidebar\" class=\"sidebar sidebar-navigation sidebar-floating\"><span>SIDEBAR_BODY</span></nav>"
                    .to_string(),
            ),
        );
        let mut rendered_map = HashMap::new();
        rendered_map.insert("navigation".to_string(), TemplateValue::Map(nav_map));

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Content</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));
        ctx.insert("rendered", TemplateValue::Map(rendered_map));

        let html = template.render(&ctx).unwrap();

        // Sidebar HTML rendered.
        assert!(
            html.contains("<nav id=\"quarto-sidebar\""),
            "expected sidebar HTML in output; got: {}",
            html
        );
        // No wrapper div.
        assert!(
            !html.contains("quarto-sidebar-container"),
            "wrapper div should be gone; got: {}",
            html
        );
        assert!(
            !html.contains("class=\"sidebar-column\""),
            "sidebar-column class should be gone; got: {}",
            html
        );

        // The sidebar must appear *between* `<div id="quarto-content"` and
        // `<main`. That confirms it's a direct grid child of #quarto-content,
        // not nested inside <main> or anywhere else.
        let content_idx = html
            .find("id=\"quarto-content\"")
            .expect("quarto-content div present");
        let sidebar_idx = html
            .find("<nav id=\"quarto-sidebar\"")
            .expect("sidebar nav present");
        let main_idx = html.find("<main").expect("main element present");
        assert!(
            content_idx < sidebar_idx && sidebar_idx < main_idx,
            "sidebar must sit between #quarto-content opening and <main>; \
             content_idx={}, sidebar_idx={}, main_idx={}",
            content_idx,
            sidebar_idx,
            main_idx
        );
    }

    #[test]
    fn test_full_template_quarto_container_class() {
        // For parity with Q1, #quarto-content carries `quarto-container`
        // alongside the page-columns / page-layout classes.
        let template = full_html_template().unwrap();

        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Content</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));

        let html = template.render(&ctx).unwrap();

        // Match either order; grep for the substring as a class token.
        assert!(
            html.contains("id=\"quarto-content\" class=\"quarto-container ")
                || html.contains("class=\"quarto-container ") && html.contains("quarto-content"),
            "expected quarto-container class on #quarto-content; got: {}",
            &html[..html.len().min(800)]
        );
    }

    // === render_with_format tests ===

    #[test]
    fn test_render_with_format_minimal() {
        use crate::format::Format;
        use quarto_pandoc_types::ConfigMapEntry;

        let format = Format::html();
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "minimal".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_bool(true, dummy_source_info()),
            }],
            dummy_source_info(),
        );
        let css_paths = vec!["styles.css".to_string()];

        let (html, _diags) =
            render_with_format("<p>Hello</p>", &meta, &format, &css_paths).unwrap();

        // Should be minimal template
        assert!(!html.contains("quarto-content"));
        assert!(html.contains("<p>Hello</p>"));
        assert!(html.contains("styles.css"));
    }

    #[test]
    fn test_render_with_format_full() {
        use crate::format::Format;

        let format = Format::html(); // Default is full
        let meta = ConfigValue::null(dummy_source_info());
        let css_paths = vec!["styles.css".to_string()];

        let (html, _diags) =
            render_with_format("<p>Hello</p>", &meta, &format, &css_paths).unwrap();

        // Should be full template
        assert!(html.contains("quarto-content"));
        assert!(html.contains("<main class=\"content\""));
        assert!(html.contains("<p>Hello</p>"));
        // Should have version from env
        assert!(html.contains("quarto-rust-"));
        // Should have default page-layout
        assert!(html.contains("page-layout-article"));
    }

    #[test]
    fn test_render_with_format_full_with_metadata() {
        use crate::format::Format;

        let format = Format::html();
        let meta = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "title".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("My Title", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "author".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("Jane Doe", dummy_source_info()),
                },
            ],
            dummy_source_info(),
        );
        let css_paths = vec![];

        let (html, _diags) =
            render_with_format("<p>Content</p>", &meta, &format, &css_paths).unwrap();

        // Should have title block
        assert!(html.contains("<header id=\"title-block-header\""));
        assert!(html.contains("My Title"));
        // Should have author meta
        assert!(html.contains("<meta name=\"author\" content=\"Jane Doe\">"));
    }

    // === RuntimeResolver tests ===

    #[test]
    fn test_runtime_resolver_loads_partial() {
        use quarto_doctemplate::PartialResolver;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let template_path = tmp.path().join("template.html");
        let partial_path = tmp.path().join("header.html");

        std::fs::write(&template_path, "$header()$").unwrap();
        std::fs::write(&partial_path, "<h1>Header Content</h1>").unwrap();

        let runtime = quarto_system_runtime::NativeRuntime::new();
        let resolver = RuntimeResolver::new(&runtime);

        let result = resolver.get_partial("header", &template_path);
        assert_eq!(result, Some("<h1>Header Content</h1>".to_string()));
    }

    #[test]
    fn test_runtime_resolver_returns_none_for_missing() {
        use quarto_doctemplate::PartialResolver;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let template_path = tmp.path().join("template.html");
        std::fs::write(&template_path, "").unwrap();

        let runtime = quarto_system_runtime::NativeRuntime::new();
        let resolver = RuntimeResolver::new(&runtime);

        let result = resolver.get_partial("nonexistent", &template_path);
        assert!(result.is_none());
    }

    #[test]
    fn test_runtime_resolver_resolves_extension_from_base() {
        use quarto_doctemplate::PartialResolver;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let template_path = tmp.path().join("template.html");
        // Partial without extension — should pick up .html from template
        let partial_path = tmp.path().join("footer.html");

        std::fs::write(&template_path, "").unwrap();
        std::fs::write(&partial_path, "<footer>The End</footer>").unwrap();

        let runtime = quarto_system_runtime::NativeRuntime::new();
        let resolver = RuntimeResolver::new(&runtime);

        // Request "footer" (no extension) — should resolve to footer.html
        let result = resolver.get_partial("footer", &template_path);
        assert_eq!(result, Some("<footer>The End</footer>".to_string()));
    }

    // =================================================================
    // Tests for scripts, header-includes, include-before, include-after
    // =================================================================

    /// Build a `meta` value with `rendered.includes.{header, before-body,
    /// after-body}` populated to the given lists. Used by the include-slot
    /// tests below to exercise the post-resolve template wiring.
    fn meta_with_rendered_includes(
        header: &[&str],
        before_body: &[&str],
        after_body: &[&str],
    ) -> ConfigValue {
        use quarto_pandoc_types::config_value::ConfigMapEntry;
        use quarto_source_map::SourceInfo;
        let si = SourceInfo::for_test;
        let to_array = |items: &[&str]| {
            ConfigValue::new_array(
                items
                    .iter()
                    .map(|s| ConfigValue::new_string(s.to_string(), si()))
                    .collect(),
                si(),
            )
        };
        let includes = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "header".to_string(),
                    key_source: si(),
                    value: to_array(header),
                },
                ConfigMapEntry {
                    key: "before-body".to_string(),
                    key_source: si(),
                    value: to_array(before_body),
                },
                ConfigMapEntry {
                    key: "after-body".to_string(),
                    key_source: si(),
                    value: to_array(after_body),
                },
            ],
            si(),
        );
        let mut meta = ConfigValue::null(si());
        meta.insert_path(&["rendered", "includes"], includes);
        meta
    }

    #[test]
    fn test_template_renders_scripts() {
        let template = minimal_html_template().unwrap();
        let meta = ConfigValue::null(quarto_source_map::SourceInfo::for_test());

        let (html, _diags) = render_with_compiled_template(
            &template,
            "<p>body</p>",
            &meta,
            &[],
            &["libs/kbd/kbd.js".to_string()],
        )
        .unwrap();

        assert!(
            html.contains(r#"<script src="libs/kbd/kbd.js"></script>"#),
            "expected script tag, got: {}",
            html
        );
    }

    #[test]
    fn test_template_renders_header_includes() {
        let template = minimal_html_template().unwrap();
        let meta =
            meta_with_rendered_includes(&["<meta name=\"test\" content=\"value\">"], &[], &[]);

        let (html, _diags) =
            render_with_compiled_template(&template, "<p>body</p>", &meta, &[], &[]).unwrap();

        assert!(
            html.contains("<meta name=\"test\" content=\"value\">"),
            "expected header include, got: {}",
            html
        );
        // Should be in <head>
        let head_end = html.find("</head>").unwrap();
        let include_pos = html.find("<meta name=\"test\"").unwrap();
        assert!(include_pos < head_end, "header include should be in <head>");
    }

    #[test]
    fn test_template_renders_include_before_and_after() {
        let template = minimal_html_template().unwrap();
        let meta = meta_with_rendered_includes(
            &[],
            &["<div class=\"before\">BEFORE</div>"],
            &["<div class=\"after\">AFTER</div>"],
        );

        let (html, _diags) =
            render_with_compiled_template(&template, "<p>body</p>", &meta, &[], &[]).unwrap();

        let before_pos = html.find("BEFORE").unwrap();
        let body_pos = html.find("<p>body</p>").unwrap();
        let after_pos = html.find("AFTER").unwrap();

        assert!(
            before_pos < body_pos,
            "include-before should appear before body"
        );
        assert!(
            after_pos > body_pos,
            "include-after should appear after body"
        );
    }

    fn nav_map(key: &str, html: &str) -> TemplateValue {
        use std::collections::HashMap;
        let mut navigation = HashMap::new();
        navigation.insert(key.to_string(), TemplateValue::String(html.to_string()));
        let mut rendered = HashMap::new();
        rendered.insert("navigation".to_string(), TemplateValue::Map(navigation));
        TemplateValue::Map(rendered)
    }

    #[test]
    fn test_full_template_renders_navbar_slot() {
        let template = full_html_template().unwrap();
        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Body</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));
        ctx.insert(
            "rendered",
            nav_map("navbar", "<nav class=\"navbar\">NAV</nav>"),
        );

        let html = template.render(&ctx).unwrap();

        let nav_pos = html.find("<nav class=\"navbar\">NAV</nav>").unwrap();
        let body_pos = html.find("<p>Body</p>").unwrap();
        assert!(
            nav_pos < body_pos,
            "navbar should appear before body: {}",
            html
        );
    }

    #[test]
    fn test_full_template_renders_footer_slot() {
        let template = full_html_template().unwrap();
        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Body</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));
        ctx.insert(
            "rendered",
            nav_map("footer", "<footer class=\"footer\">FOOT</footer>"),
        );

        let html = template.render(&ctx).unwrap();

        let body_pos = html.find("<p>Body</p>").unwrap();
        let footer_pos = html.find("<footer class=\"footer\">FOOT</footer>").unwrap();
        assert!(
            footer_pos > body_pos,
            "footer should appear after body: {}",
            html
        );
    }

    #[test]
    fn test_full_template_omits_navbar_and_footer_when_absent() {
        let template = full_html_template().unwrap();
        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String("<p>Body</p>".to_string()));
        ctx.insert("page-layout", TemplateValue::String("article".to_string()));
        ctx.insert("version", TemplateValue::String("0.1.0".to_string()));

        let html = template.render(&ctx).unwrap();

        assert!(!html.contains("<nav class=\"navbar\""));
        assert!(!html.contains("<footer class=\"footer\""));
    }

    #[test]
    fn test_template_empty_includes_produce_no_tags() {
        let template = minimal_html_template().unwrap();
        let meta = ConfigValue::null(quarto_source_map::SourceInfo::for_test());

        let (html, _diags) =
            render_with_compiled_template(&template, "<p>body</p>", &meta, &[], &[]).unwrap();

        assert!(
            !html.contains("<script"),
            "no script tags expected, got: {}",
            html
        );
        assert!(
            !html.contains("BEFORE"),
            "no include-before expected, got: {}",
            html
        );
        assert!(
            !html.contains("AFTER"),
            "no include-after expected, got: {}",
            html
        );
    }

    /// bd-xdnk: undefined-variable warnings from the doctemplate evaluator
    /// must surface in the returned diagnostics vec, not be silently dropped.
    #[test]
    fn test_undefined_variable_emits_diagnostic() {
        // Custom template with a reference to a variable the document
        // does not provide.
        let template_src = "<header>by $author-greeting$</header>$body$";
        let template =
            quarto_doctemplate::Template::compile(template_src).expect("template should compile");
        let meta = ConfigValue::null(quarto_source_map::SourceInfo::for_test());

        let (html, diagnostics) =
            render_with_compiled_template(&template, "<p>body</p>", &meta, &[], &[]).unwrap();

        assert!(
            html.contains("<p>body</p>"),
            "body should still render, got: {}",
            html
        );

        let undef = diagnostics.iter().find(|d| {
            d.code.as_deref() == Some("Q-10-2")
                && d.kind == quarto_error_reporting::DiagnosticKind::Warning
        });
        assert!(
            undef.is_some(),
            "expected Q-10-2 warning for undefined variable, got: {:?}",
            diagnostics
        );
    }

    // ─────────── L5 phase 7: margin sidebar with categories ───────────

    /// Render through the full HTML template (not the minimal one
    /// that `render_with_template` uses by default). The L5 phase 7
    /// tests need to exercise the `#quarto-margin-sidebar` region,
    /// which only lives in `FULL_HTML_TEMPLATE`.
    fn render_full(body: &str, meta: &ConfigValue) -> String {
        let template = full_html_template().expect("full template compiles");
        let mut ctx = TemplateContext::new();
        ctx.insert("body", TemplateValue::String(body.to_string()));
        // The full template's title block consumes derived metadata
        // (`by-author`, `labels`, `rendered.has-title-block`) that the
        // pipeline's AuthorsNormalizeTransform writes before the
        // template stage; apply the same normalization here.
        let mut meta = meta.clone();
        crate::transforms::normalize_authors_meta(&mut meta);
        add_metadata_to_context(&meta, &mut ctx);
        let (html, _diags) = template.render_with_diagnostics(&ctx);
        html.expect("template renders")
    }

    /// Build a meta with the requested combination of nested
    /// `rendered.navigation.toc` and `rendered.navigation.margin_categories`
    /// strings.
    fn meta_with_navigation(toc: Option<&str>, margin_categories: Option<&str>) -> ConfigValue {
        let mut nav_entries: Vec<ConfigMapEntry> = Vec::new();
        if let Some(toc_html) = toc {
            nav_entries.push(ConfigMapEntry {
                key: "toc".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string(toc_html, dummy_source_info()),
            });
        }
        if let Some(cats_html) = margin_categories {
            nav_entries.push(ConfigMapEntry {
                key: "margin_categories".to_string(),
                key_source: dummy_source_info(),
                value: ConfigValue::new_string(cats_html, dummy_source_info()),
            });
        }
        let nav = ConfigValue::new_map(nav_entries, dummy_source_info());
        let rendered = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "navigation".to_string(),
                key_source: dummy_source_info(),
                value: nav,
            }],
            dummy_source_info(),
        );
        ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "pagetitle".to_string(),
                    key_source: dummy_source_info(),
                    value: ConfigValue::new_string("Test", dummy_source_info()),
                },
                ConfigMapEntry {
                    key: "rendered".to_string(),
                    key_source: dummy_source_info(),
                    value: rendered,
                },
            ],
            dummy_source_info(),
        )
    }

    // L5 plan §"Tests" #34
    #[test]
    fn full_template_emits_margin_sidebar_with_only_toc() {
        let meta = meta_with_navigation(Some("<ul><li>toc-entry</li></ul>"), None);
        let html = render_full("<p>body</p>", &meta);
        assert!(
            html.contains(r#"<div id="quarto-margin-sidebar""#),
            "expected sidebar wrapper; got: {html}"
        );
        assert!(html.contains("<nav id=\"TOC\""));
        assert!(html.contains("toc-entry"));
        // No category container.
        assert!(!html.contains("quarto-listing-category"));
    }

    // L5 plan §"Tests" #35
    #[test]
    fn full_template_emits_margin_sidebar_with_only_categories() {
        let cat_html = r#"<h5 class="quarto-listing-category-title">Categories</h5>
<div class="quarto-listing-category category-default">
<div class="category" data-category="">All</div>
</div>"#;
        let meta = meta_with_navigation(None, Some(cat_html));
        let html = render_full("<p>body</p>", &meta);
        assert!(
            html.contains(r#"<div id="quarto-margin-sidebar""#),
            "sidebar wrapper expected; got: {html}"
        );
        // No TOC nav.
        assert!(!html.contains("<nav id=\"TOC\""));
        assert!(html.contains(r#"class="quarto-listing-category-title""#));
    }

    // L5 plan §"Tests" #36
    #[test]
    fn full_template_emits_margin_sidebar_with_both() {
        let cat_html = r#"<h5 class="quarto-listing-category-title">Categories</h5>"#;
        let meta = meta_with_navigation(Some("<ul><li>toc-entry</li></ul>"), Some(cat_html));
        let html = render_full("<p>body</p>", &meta);
        assert!(html.contains(r#"<div id="quarto-margin-sidebar""#));
        // Both inside the same sidebar container.
        let sidebar_open = html.find(r#"<div id="quarto-margin-sidebar""#).unwrap();
        let sidebar_close = html[sidebar_open..]
            .find("</div>\n\n<main")
            .map(|i| sidebar_open + i)
            .or_else(|| {
                html[sidebar_open..]
                    .find("</main>")
                    .map(|i| sidebar_open + i)
            })
            .unwrap_or(html.len());
        let sidebar_html = &html[sidebar_open..sidebar_close];
        assert!(
            sidebar_html.contains("<nav id=\"TOC\"") && sidebar_html.contains("Categories"),
            "expected both TOC and categories inside the same sidebar; got: {sidebar_html}"
        );
        // TOC must come before categories (TOC first per the
        // L5 sub-plan's template change).
        let toc_pos = sidebar_html.find("<nav id=\"TOC\"").unwrap();
        let cats_pos = sidebar_html.find("Categories").unwrap();
        assert!(
            toc_pos < cats_pos,
            "TOC must precede categories; got order: {sidebar_html}"
        );
    }

    // L5 plan §"Tests" #37
    #[test]
    fn full_template_omits_margin_sidebar_when_neither_set() {
        let meta = meta_with_navigation(None, None);
        let html = render_full("<p>body</p>", &meta);
        assert!(
            !html.contains(r#"<div id="quarto-margin-sidebar""#),
            "sidebar must be absent when neither toc nor categories are set; got: {html}"
        );
    }

    // === Rich-Markdown title-block fields (bd-5706gcrq) ===
    //
    // Inline-valued title-block metadata (title, subtitle, abstract) must
    // be rendered to HTML so code spans, emphasis, etc. survive into the
    // `<h1 class="title">` / `<p class="subtitle">` / abstract block.
    // Previously these were flattened to plain text by
    // `config_value_to_template_value`.

    /// A `Code` inline `\`text\``.
    fn code_inline(text: &str) -> Inline {
        Inline::Code(Code {
            attr: empty_attr(),
            text: text.to_string(),
            source_info: dummy_source_info(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn str_inline(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: dummy_source_info(),
        })
    }

    fn entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: dummy_source_info(),
            value,
        }
    }

    /// Extract the inner text of `<h1 class="title">…</h1>` for assertions.
    fn h1_title(html: &str) -> String {
        let open = r#"<h1 class="title">"#;
        let start = html.find(open).expect("h1 title present") + open.len();
        let end = html[start..].find("</h1>").expect("h1 closes") + start;
        html[start..end].to_string()
    }

    #[test]
    fn title_code_span_renders_as_html_code_element() {
        // title: Multiformat branding with `_brand.yml`
        let title = ConfigValue::new_inlines(
            vec![
                str_inline("Multiformat branding with "),
                code_inline("_brand.yml"),
            ],
            dummy_source_info(),
        );
        let meta = ConfigValue::new_map(vec![entry("title", title)], dummy_source_info());
        let html = render_full("<p>body</p>", &meta);
        assert_eq!(
            h1_title(&html),
            "Multiformat branding with <code>_brand.yml</code>",
            "code span must render as a <code> element in the title h1"
        );
    }

    #[test]
    fn title_emphasis_renders_as_html_em_element() {
        // title: An *emphatic* title
        let title = ConfigValue::new_inlines(
            vec![
                str_inline("An "),
                Inline::Emph(Emph {
                    content: vec![str_inline("emphatic")],
                    source_info: dummy_source_info(),
                }),
                str_inline(" title"),
            ],
            dummy_source_info(),
        );
        let meta = ConfigValue::new_map(vec![entry("title", title)], dummy_source_info());
        let html = render_full("<p>body</p>", &meta);
        assert_eq!(h1_title(&html), "An <em>emphatic</em> title");
    }

    #[test]
    fn subtitle_inline_markup_renders_as_html() {
        let title = ConfigValue::new_inlines(vec![str_inline("Doc")], dummy_source_info());
        let subtitle = ConfigValue::new_inlines(
            vec![str_inline("about "), code_inline("things")],
            dummy_source_info(),
        );
        let meta = ConfigValue::new_map(
            vec![entry("title", title), entry("subtitle", subtitle)],
            dummy_source_info(),
        );
        let html = render_full("<p>body</p>", &meta);
        assert!(
            html.contains(r#"<p class="subtitle lead">about <code>things</code></p>"#),
            "subtitle code span must render as <code>; got: {html}"
        );
    }

    #[test]
    fn pagetitle_stays_plain_text_when_title_is_rich() {
        // The head <title> uses `pagetitle`, which must remain plain text
        // (HTML tags are invalid inside <title>). A rich `title` must not
        // bleed markup into the head element.
        let title = ConfigValue::new_inlines(
            vec![str_inline("Branding with "), code_inline("_brand.yml")],
            dummy_source_info(),
        );
        let meta = ConfigValue::new_map(
            vec![
                entry("title", title),
                entry(
                    "pagetitle",
                    ConfigValue::new_string("Branding with _brand.yml", dummy_source_info()),
                ),
            ],
            dummy_source_info(),
        );
        let html = render_full("<p>body</p>", &meta);
        assert!(
            html.contains("<title>Branding with _brand.yml</title>"),
            "head <title> must stay plain text; got: {html}"
        );
        assert!(
            !html.contains("<title>Branding with <code>"),
            "head <title> must not contain markup; got: {html}"
        );
    }

    #[test]
    fn non_titleblock_inline_field_is_not_htmlized() {
        // Only the allowlisted title-block fields are rendered to HTML.
        // Arbitrary inline-valued metadata (which may land in attribute
        // contexts) must keep flattening to plain text via the generic
        // conversion, so the generic converter is unchanged.
        let value = ConfigValue::new_inlines(
            vec![str_inline("plain "), code_inline("code")],
            dummy_source_info(),
        );
        assert_eq!(
            config_value_to_template_value(&value),
            TemplateValue::String("plain code".to_string()),
            "generic conversion must remain plain-text flattening"
        );
    }
}
