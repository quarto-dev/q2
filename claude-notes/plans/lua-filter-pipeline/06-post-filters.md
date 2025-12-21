# Post Filters (quarto_post_filters)

**Source**: `main.lua` lines 383-536

**Purpose**: Format-specific rendering, output transformations, and post-processing for various output formats.

---

## Stages Overview

This is the largest filter group with ~29 stages. Many are format-conditional.

### Side-Effectful Stages

| Stage | Side Effects | Details |
|-------|--------------|---------|
| post-combined-cites-bibliography | `FW` | Writes cites JSON file |
| post-ipynb | `PA` | `pandoc.write()` for markdown/HTML |
| post-render-latex | `PA` | `pandoc.write()` for LaTeX |
| post-render-asciidoc | `PA` | `pandoc.write()` for AsciiDoc |
| post-pdf-images | `S` | `pandoc.pipe("rsvg-convert")` for SVG→PDF |
| post-render-email | `FR`, `FW`, `ENV` | Heavy I/O for email rendering |

### Format-Specific Stages (Pure or Pandoc API only)

| Stage | Format | Pandoc API |
|-------|--------|------------|
| post-render-jats | JATS | None |
| post-render-latex | LaTeX | `pandoc.write()` |
| post-render-typst | Typst | None |
| post-render-dashboard | Dashboard | None |
| post-ojs | OJS | None |
| post-render-html-fixups | HTML | None |
| post-render-gfm-fixups | GFM | None |
| post-render-hugo-fixups | Hugo | None |
| post-render-pptx-fixups | PPTX | None |
| post-render-revealjs-fixups | Reveal.js | None |

---

## Key Side Effects Details

### post-combined-cites-bibliography

**Source**: `quarto-post/cites.lua`

```lua
local file = io.open(citesFilePath, "w")
```

Writes citation data to JSON file for bibliography processing.

### post-pdf-images

**Source**: `quarto-post/pdf-images.lua`

```lua
local status, results = pcall(pandoc.pipe, "rsvg-convert", {"-f", "pdf", "-a", "-o", output, path}, "")
```

Calls external `rsvg-convert` to convert SVG images to PDF. **Blocks WASM**.

### post-render-email

**Source**: `quarto-post/email.lua`

Heavy I/O operations:
- `io.open()` for reading email templates and images
- `os.date()` for timestamps
- `os.getenv()` for Connect environment variables
- Multiple file writes for email metadata and attachments

**Completely blocks WASM** for email output format.

### post-ipynb

**Source**: `quarto-post/ipynb.lua`

Uses `pandoc.write()` for table rendering in notebooks.

---

## Summary

| Metric | Value |
|--------|-------|
| Total Stages | ~29 |
| Pure | ~22 |
| File Read | 2 (email templates, book markdown) |
| File Write | 3 (cites JSON, email files) |
| Subprocess | 1 (rsvg-convert) |
| Pandoc API | ~8 (`pandoc.read`, `pandoc.write`) |
| WASM Blocked | 2 (pdf-images, email) |

**WASM Notes**:
- `post-pdf-images`: SVG→PDF conversion requires external tool - skip or pre-convert
- `post-render-email`: Email format incompatible with WASM
- Many stages use `pandoc.write()` for format conversion - needs Pandoc or replacement

---

## Critical Observations

1. **Format dispatching**: Many stages check format before running. In Rust, this could be cleaner with trait-based dispatch.

2. **Pandoc API for format conversion**: Several stages use `pandoc.write()` to convert AST to specific formats (LaTeX, HTML, AsciiDoc). In Rust, we'd use quarto-doctemplate or format-specific writers.

3. **External tools**: `rsvg-convert` for SVG→PDF is an external dependency. Consider:
   - Bundling or providing alternative
   - Pre-converting images before filter run

4. **Email format**: The email filter is heavily I/O dependent and specific to Posit Connect. Consider this a specialized output type that may not be prioritized for WASM.
