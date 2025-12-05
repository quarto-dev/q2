# Template Variables Reference

This document describes the template variable design for quarto-markdown-pandoc's doctemplate integration.

## Design Principle

**Templates are pure functions of metadata plus body.**

If a template uses a variable, the document author is responsible for ensuring that value is available in the document metadata. This can be done via:
- YAML frontmatter in the source document
- JSON or Lua filters that inject values
- External configuration/tooling

This design maximizes composability and avoids environmental dependencies that would be problematic in contexts like WASM runtimes.

## Supported Variables

### `$body$`

The only implicitly-provided variable. Contains the rendered document content.

### All Other Variables

All other variables come from document metadata. There are no "magic" variables injected by the runtime.

## Pandoc's Automatic Variables (Not Supported)

For reference, Pandoc automatically injects the following variables. We intentionally do not support these to maintain composability and WASM compatibility.

### Core Environment Variables (Pandoc)

| Variable | Type | Pandoc Description | Why Not Supported |
|----------|------|--------------------|--------------------|
| `sourcefile` | List/empty | Input filename(s) from command line | Environment-dependent; meaningless in WASM |
| `outputfile` | String | Output filename (`-` if stdout) | Environment-dependent; meaningless in WASM |
| `curdir` | String | Working directory | Environment-dependent; meaningless in WASM |
| `pandoc-version` | String | Pandoc version | Couples templates to runtime |
| `pdf-engine` | String? | PDF engine name | Workflow-specific |

### CLI Option Variables (Pandoc)

| Variable | Type | Pandoc Description | Why Not Supported |
|----------|------|--------------------|--------------------|
| `toc` | Boolean | Non-null if `--toc` specified | CLI-dependent; use metadata instead |
| `toc-title` | String? | Table of contents title | Use metadata |
| `numbersections` | Boolean | Non-null if `--number-sections` specified | CLI-dependent; use metadata instead |
| `header-includes` | List | Contents from `-H` flag | Use metadata or filters |
| `include-before` | List | Contents from `-B` flag | Use metadata or filters |
| `include-after` | List | Contents from `-A` flag | Use metadata or filters |
| `css` | List | Stylesheets from `--css` | Use metadata |
| `title-prefix` | String? | From `--title-prefix` | Use metadata |
| `epub-cover-image` | String? | From `--epub-cover-image` | Use metadata |

### Format-Specific Variables (Pandoc)

| Variable | Type | Pandoc Description | Why Not Supported |
|----------|------|--------------------|--------------------|
| `date-meta` | String | `date` in ISO 8601 format | Use a filter to normalize dates |
| `meta-json` | JSON | All metadata as JSON | Use metadata directly |
| `dzslides-core` | String | DZSlides JavaScript | Format-specific runtime injection |
| `emphasis-commands` | String | ConTeXt emphasis commands | Format-specific runtime injection |

## Migration from Pandoc Templates

If you have Pandoc templates that use automatic variables, you have two options:

1. **Add to document metadata**: Put the values in your YAML frontmatter
   ```yaml
   ---
   title: My Document
   toc: true
   numbersections: true
   ---
   ```

2. **Use a filter**: Write a JSON or Lua filter that injects the values
   ```lua
   -- inject-variables.lua
   function Meta(meta)
     meta.sourcefile = PANDOC_STATE.input_files
     return meta
   end
   ```

## Rationale

This design choice prioritizes:

1. **Composability**: Templates work identically regardless of how/where they're invoked
2. **WASM compatibility**: No filesystem or environment dependencies
3. **Predictability**: Template output depends only on explicit inputs
4. **Testability**: Templates can be tested with just metadata + body, no mocking needed

The tradeoff is that some Pandoc templates may need modification to work with quarto-markdown-pandoc. We consider this acceptable because:
- Most templates primarily use document metadata, not magic variables
- Filters provide a clean migration path for templates that need environment info
- The benefits of composability outweigh the migration cost
