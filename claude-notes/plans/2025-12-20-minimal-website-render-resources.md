# Minimal Website Render: Static Resources and Templates

## Overview

This document catalogs the static resources needed for a minimal Quarto website render, based on analysis of:
- The compiled output in `resources/basic-quarto-website-compiled/_site/`
- The quarto-cli source in `external-sources/quarto-cli/src/`

## Site Output Structure

A minimal Quarto website produces:

```
_site/
├── index.html           # Rendered page
├── about.html           # Additional page
├── styles.css           # User custom styles
├── search.json          # Search index (generated)
├── site_libs/
│   ├── bootstrap/       # Bootstrap CSS/JS
│   ├── clipboard/       # Clipboard.js
│   ├── quarto-html/     # Core Quarto HTML functionality
│   ├── quarto-nav/      # Navigation JS
│   └── quarto-search/   # Search JS (fuse, autocomplete)
```

## Static Library Files Required

### 1. Bootstrap (`site_libs/bootstrap/`)

**Source:** `quarto-cli/src/resources/formats/html/bootstrap/dist/`

| File | Purpose |
|------|---------|
| `bootstrap.min.js` | Bootstrap JavaScript |
| `bootstrap-icons.css` | Bootstrap icon font stylesheet |
| `bootstrap-icons.woff` | Bootstrap icon font file |
| `bootstrap-{hash}.min.css` | Compiled Bootstrap CSS (hash varies by theme/branding) |

**Note:** The CSS file is dynamically compiled from SCSS based on theme configuration. For our minimal render, we can use the pre-compiled CSS from the example output initially.

### 2. Quarto HTML (`site_libs/quarto-html/`)

**Source:** Various locations in quarto-cli

| File | Purpose | Source |
|------|---------|--------|
| `quarto.js` | Core Quarto functionality | `formats/html/quarto.js` |
| `anchor.min.js` | Heading anchor links | vendor |
| `popper.min.js` | Tooltip positioning | vendor |
| `tippy.umd.min.js` | Tooltip library | vendor |
| `tippy.css` | Tooltip styles | vendor |
| `tabsets/tabsets.js` | Tabset functionality | quarto source |
| `axe/axe-check.js` | Accessibility checking | quarto source |
| `quarto-syntax-highlighting-{hash}.css` | Code syntax highlighting | compiled from SCSS |

### 3. Quarto Navigation (`site_libs/quarto-nav/`)

**Source:** `quarto-cli/src/resources/projects/website/navigation/`

| File | Purpose |
|------|---------|
| `quarto-nav.js` | Website navigation functionality |
| `headroom.min.js` | Hide navbar on scroll |

### 4. Quarto Search (`site_libs/quarto-search/`)

**Source:** `quarto-cli/src/resources/projects/website/search/`

| File | Purpose |
|------|---------|
| `quarto-search.js` | Quarto search integration |
| `fuse.min.js` | Client-side fuzzy search |
| `autocomplete.umd.js` | Search autocomplete UI |

### 5. Clipboard (`site_libs/clipboard/`)

**Source:** vendor

| File | Purpose |
|------|---------|
| `clipboard.min.js` | Code copy button functionality |

## HTML Template Structure

### Main Pandoc Template

**Location:** `quarto-cli/src/resources/formats/html/pandoc/template.html`

The main template is quite minimal and uses Pandoc template variables:

```html
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" lang="$lang$" xml:lang="$lang$">
<head>
$metadata.html()$
<style>
$styles.html()$
</style>
<!-- htmldependencies:E3FAD763 -->
$for(header-includes)$
$header-includes$
$endfor$
$if(math)$
  <!-- math setup (MathJax/KaTeX) -->
$endif$
$for(css)$
<link rel="stylesheet" href="$css$" />
$endfor$
</head>
<body>
$for(include-before)$
$include-before$
$endfor$
$if(title)$
$title-block.html()$
$endif$
$if(toc)$
$toc.html()$
$endif$
$body$
$for(include-after)$
$include-after$
$endfor$
</body>
</html>
```

### Template Partials

**Location:** `quarto-cli/src/resources/formats/html/pandoc/`

| Partial | Purpose |
|---------|---------|
| `metadata.html` | `<meta>` tags for charset, viewport, generator |
| `styles.html` | Base inline CSS styles |
| `title-block.html` | Document title rendering |
| `toc.html` | Table of contents |

### Website Navigation Templates (EJS)

**Location:** `quarto-cli/src/resources/projects/website/templates/`

The website project type uses EJS templates to generate navigation HTML:

| Template | Purpose |
|----------|---------|
| `nav-before-body.ejs` | Main navigation HTML (navbar, sidebar, content wrapper) |
| `nav-after-body-preamble.ejs` | Footer and page navigation |
| `nav-after-body-postamble.ejs` | Closing content wrapper |
| `navbrand.ejs` | Navbar brand/logo |
| `navitem.ejs` | Individual nav items |
| `navitem-dropdown.ejs` | Dropdown nav items |
| `navsearch.ejs` | Search button |
| `sidebar.ejs` | Sidebar navigation |
| `sidebaritem.ejs` | Sidebar items |

### Key HTML Structure (Website Output)

```html
<body class="nav-fixed quarto-light">
  <div id="quarto-search-results"></div>

  <header id="quarto-header" class="headroom fixed-top">
    <nav class="navbar navbar-expand-lg" data-bs-theme="dark">
      <!-- navbar content -->
    </nav>
  </header>

  <div id="quarto-content" class="quarto-container page-columns page-rows-contents page-layout-article page-navbar">
    <!-- sidebar (optional) -->
    <div id="quarto-margin-sidebar" class="sidebar margin-sidebar">
      <!-- margin content / TOC -->
    </div>

    <main class="content" id="quarto-document-content">
      <header id="title-block-header" class="quarto-title-block default">
        <!-- title block -->
      </header>

      <!-- document body -->
    </main>
  </div>

  <script id="quarto-html-after-body">
    <!-- initialization script -->
  </script>
</body>
```

## Dependency Resolution Flow

In quarto-cli, dependencies are resolved through:

1. **Format Dependencies** (`format-html.ts`):
   - Adds `quarto-html` dependency with core scripts/styles

2. **Bootstrap Extras** (`format-html-bootstrap.ts`):
   - Adds Bootstrap CSS, JS, icons via `bootstrapFormatDependency()`

3. **Website Navigation Extras** (`website-navigation.ts`):
   - Adds `quarto-nav` dependency
   - Adds `quarto-search` dependency (if search enabled)

4. **Dependency Writer** (`pandoc-html.ts`):
   - Collects all dependencies
   - Copies files to `site_libs/{dependency-name}/`
   - Injects `<script>` and `<link>` tags into HTML

## Implementation Strategy for Minimal Rust Port

### Phase 1: Static Resource Copying

1. Create a `resources/site_libs/` directory with pre-compiled versions of all libraries
2. Copy these to output `_site/site_libs/` during render
3. Use fixed hash versions for CSS files initially

### Phase 2: Template Rendering

1. Implement Pandoc template engine (our `quarto-doctemplate` crate)
2. Port the main HTML template with partials
3. Use pre-generated navigation HTML initially (skip EJS templating)

### Phase 3: Dynamic Compilation

1. Integrate SCSS compilation for themes (later)
2. Implement EJS-equivalent templating for navigation (later)
3. Add search index generation (later)

## Files to Copy for Minimal Render

For an absolute minimum website render, copy these from the example output:

```
resources/basic-quarto-website-compiled/_site/site_libs/
├── bootstrap/
│   ├── bootstrap-2d6b043f54d60c02f5aeed4beddc8498.min.css
│   ├── bootstrap-icons.css
│   ├── bootstrap-icons.woff
│   └── bootstrap.min.js
├── clipboard/
│   └── clipboard.min.js
├── quarto-html/
│   ├── anchor.min.js
│   ├── popper.min.js
│   ├── quarto.js
│   ├── quarto-syntax-highlighting-3aa970819e70fbc78806154e5a1fcd28.css
│   ├── tabsets/tabsets.js
│   ├── tippy.css
│   └── tippy.umd.min.js
├── quarto-nav/
│   ├── headroom.min.js
│   └── quarto-nav.js
└── quarto-search/
    ├── autocomplete.umd.js
    ├── fuse.min.js
    └── quarto-search.js
```

## Key Differences: Website vs Default HTML

| Aspect | Default HTML | Website |
|--------|--------------|---------|
| Template | `template.html` only | Same, plus navigation EJS templates |
| Dependencies | `quarto-html`, `bootstrap` | + `quarto-nav`, `quarto-search` |
| libDir | `{stem}_files` | `site_libs` (shared) |
| outputDir | same as input | `_site` |
| Post-render | None | Sitemap, search index, aliases |
| Page structure | Simple body | Navbar + content wrapper + sidebar |

## Next Steps

1. [ ] Copy static `site_libs` from example to project resources
2. [ ] Implement basic HTML template rendering in `pico-quarto-render`
3. [ ] Generate proper navigation HTML (initially hardcoded)
4. [ ] Add `site_libs` copying to output
5. [ ] Implement search index generation (later)
