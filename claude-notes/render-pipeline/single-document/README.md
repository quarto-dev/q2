# Single Document Render Pipeline in Quarto-CLI

**Date:** 2025-10-11
**Purpose:** Comprehensive analysis of what happens when rendering a single document (`quarto render doc.qmd`) outside of a project
**Status:** Complete

## Executive Summary

This document traces the complete execution path when rendering a single Quarto document that is NOT part of a `_quarto.yml` project. The pipeline consists of 10 major stages, from CLI argument parsing through final output cleanup, involving ~50+ TypeScript modules and orchestrating multiple external tools (Pandoc, computation engines, filters).

**Key Insight:** The rendering pipeline is fundamentally a **transformation chain** with **metadata merging** at each stage:
```
User Document → QMD Parser → Engine Execution → Markdown → Pandoc Filters → HTML/PDF → Postprocessing → Final Output
```

Each stage preserves and transforms metadata, building up a complete configuration that represents the merger of defaults, user intent, engine requirements, and format-specific needs.

## Pipeline Overview

```
1. CLI Entry (cmd.ts)
   ↓
2. Main Render Coordinator (render-shared.ts)
   ↓
3. File Rendering (render-files.ts)
   ├─→ 4. Context Creation (render-contexts.ts)
   ├─→ 5. Engine Selection (engine.ts)
   ├─→ 6. YAML Validation (validate-document.ts)
   ├─→ 7. Engine Execution (render-execute)
   ├─→ 8. Language Cell Handlers (handlers/)
   ├─→ 9. Pandoc Conversion (pandoc.ts)
   └─→ 10. Postprocessing & Finalization (render.ts)
```

## Documentation Structure

This analysis has been split into the following documents for easier navigation:

### Pipeline Stages

1. **[CLI Entry Point](stages/01-cli-entry-point.md)** - Argument parsing, flag normalization, service creation
2. **[Main Render Coordinator](stages/02-main-coordinator.md)** - YAML validation init, project context detection
3. **[File Rendering Setup](stages/03-file-rendering-setup.md)** - Progress setup, temp context, lifetime management
4. **[Render Context Creation](stages/04-context-creation.md)** - Engine resolution, format resolution, metadata hierarchy
5. **[Engine Selection](stages/05-engine-selection.md)** - Registered engines, selection algorithm, target creation
6. **[YAML Validation](stages/06-yaml-validation.md)** - Schema loading, validation process, special cases
7. **[Engine Execution](stages/07-engine-execution.md)** - Freeze/thaw, execute options, engine-specific execution
8. **[Language Cell Handlers](stages/08-language-cell-handlers.md)** - OJS handler, diagram handler, mapped diff
9. **[Pandoc Conversion](stages/09-pandoc-conversion.md)** - Markdown processing, filter execution, Pandoc invocation
10. **[Postprocessing & Finalization](stages/10-postprocessing.md)** - Engine postprocessing, HTML/generic postprocessors, cleanup

### Specialized Topics

- **[PDF Rendering](pdf-rendering.md)** - Complete analysis of PDF-specific pipeline differences (recipe selection, LaTeX processing, multi-stage compilation)
- **[Complete Data Flow](data-flow.md)** - End-to-end data transformations and metadata flow
- **[Key Design Patterns](design-patterns.md)** - Metadata merging, MappedString, FormatExtras, Recipe pattern, Lifetime pattern
- **[File Organization Summary](file-organization.md)** - TypeScript source code organization and module responsibilities
- **[Key Data Structures](data-structures.md)** - ExecutionTarget, Format, RenderContext, ExecuteResult, RenderedFile
- **[Timing Estimates](timing-estimates.md)** - Complexity estimates for Rust port implementation
- **[Rust Port Implications](rust-port-implications.md)** - Recommendations for trait-based architecture, critical components
- **[Conclusion](conclusion.md)** - Summary and strategic recommendations

