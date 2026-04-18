# Plan 3: @quarto/jupyter

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Phases 3A-3D and 3F are independent. Phase 3E (wiring into engine-host) requires Plan 1a Phase 5 to have created the `@quarto/engine-host` package.
**Blocks:** Plan 4 (Julia Validation)
**Estimated sessions:** 2-3

## Overview

Create the `@quarto/jupyter` TypeScript package — a clean implementation of Jupyter notebook → markdown conversion and related utilities. This is the `quarto.jupyter` namespace of the QuartoAPI.

The Julia engine calls 7 methods from this namespace. The core function `toMarkdown()` is the single most complex piece of the entire engine extension project (~1300 lines of logic), but it's conceptually straightforward: walk notebook cells, format outputs as markdown, handle figures and HTML preservation.

**Reference:** The Quarto 1 implementations to study are in `~/src/quarto-cli/src/core/jupyter/` (a separate repository — quarto-dev/quarto-cli). Key files: `jupyter.ts` (main toMarkdown), `display-data.ts`, `tags.ts`, `labels.ts`, `preserve.ts`, `widgets.ts`, `types.ts`.

## What the Julia engine calls

| Method | What it does | Complexity |
|--------|-------------|------------|
| `toMarkdown(nb, opts)` | Convert Jupyter notebook → markdown string | **High** — the core function |
| `isPercentScript(file, exts)` | Check if file is a percent-format script | Low — check for `# %%` markers |
| `percentScriptToMarkdown(file)` | Convert percent script → markdown | Low — regex transformation |
| `assets(input, to)` | Compute asset directory paths for figures | Low — path computation |
| `resultIncludes(tempDir, deps)` | Extract pandoc includes from widget deps | Low — object transformation |
| `resultEngineDependencies(deps)` | Extract engine-specific deps | Low — pass-through |

## Work Items

### Phase 3A: Types and foundation

- [ ] Create `ts-packages/quarto-jupyter/package.json`:
  ```json
  {
    "name": "@quarto/jupyter",
    "version": "0.1.0",
    "type": "module",
    "main": "src/index.ts",
    "dependencies": { "yaml": "^2.0.0" }
  }
  ```

- [ ] Create `src/types.ts` — Jupyter notebook types:
  ```typescript
  interface JupyterNotebook {
      nbformat: number;
      nbformat_minor: number;
      metadata: NotebookMetadata;
      cells: JupyterCell[];
  }
  interface JupyterCell {
      cell_type: "code" | "markdown" | "raw";
      source: string | string[];
      metadata: CellMetadata;
      outputs?: CellOutput[];  // code cells only
      execution_count?: number | null;
  }
  interface CellOutput {
      output_type: "stream" | "display_data" | "execute_result" | "error";
      // Fields vary by output_type
      text?: string | string[];
      data?: MimeBundle;
      name?: string;  // "stdout" | "stderr" for stream
      ename?: string; // for error
      evalue?: string;
      traceback?: string[];
  }
  type MimeBundle = Record<string, string | string[] | object>;
  ```
  Reference: Quarto 1's `src/core/jupyter/types.ts`

- [ ] Create `src/constants.ts` — MIME type constants, cell option keys, etc.
  Reference: Quarto 1's `src/config/constants.ts` (just the subset we need)

### Phase 3B: Supporting modules

Small, focused modules that `toMarkdown` depends on. Each is self-contained.

- [ ] Create `src/display-data.ts` — MIME bundle dispatch:
  - `displayDataMimeType(output, options)` — select best MIME type from bundle
  - `displayDataIsImage(output)`, `displayDataIsTextPlain(output)`, etc.
  - MIME priority order: text/html > image/svg+xml > image/png > image/jpeg > text/markdown > text/latex > text/plain
  - Reference: Quarto 1's `src/core/jupyter/display-data.ts`
  - ~150 lines

- [ ] Create `src/tags.ts` — cell visibility logic:
  - `hideCell(options)`, `hideCode(options)`, `hideOutput(options)`, `hideWarnings(options)`
  - `includeCell(cell, options)`, `includeCode(cell, options)`, `includeOutput(cell, options)`
  - Based on cell-level `echo`, `include`, `output`, `warning` options
  - Reference: Quarto 1's `src/core/jupyter/tags.ts`
  - ~100 lines

- [ ] Create `src/labels.ts` — cell label and caption handling:
  - `cellLabel(cell)` — extract label from cell metadata or options
  - `cellLabelClass(label)` — generate CSS class from label
  - `resolveCaptions(cell)` — extract fig-cap, tbl-cap, etc.
  - Reference: Quarto 1's `src/core/jupyter/labels.ts`
  - ~100 lines

- [ ] Create `src/preserve.ts` — HTML preservation:
  - `removeAndPreserveHtml(output)` — replace raw HTML with placeholder UUIDs
  - Returns `{ output: string, preserved: Record<string, string> }`
  - Used to protect HTML from Pandoc's markdown processing
  - Reference: Quarto 1's `src/core/jupyter/preserve.ts`
  - ~80 lines

- [ ] Create `src/widgets.ts` — Jupyter widget dependency extraction:
  - `widgetDependencies(outputs)` — find widget state in output MIME bundles
  - `widgetDependencyIncludes(deps, tempDir)` — generate script tags for widgets
  - Reference: Quarto 1's `src/core/jupyter/widgets.ts`
  - ~100 lines

- [ ] Create `src/pandoc-id.ts` — identifier generation:
  - `pandocAutoIdentifier(text)` — generate Pandoc-style IDs from heading text
  - Pure string manipulation, no dependencies
  - Reference: Quarto 1's `src/core/pandoc/pandoc-id.ts`
  - ~50 lines

- [ ] Create `src/cell-options.ts` — simplified cell options parsing:
  - Parse YAML from code cell comments (`#| key: value` lines)
  - Use `yaml` package directly (no schema validation)
  - Extract cell-level execution options
  - **Simplified from Quarto 1**: no schema validation, no tree-sitter
  - ~100 lines

### Phase 3C: Core toMarkdown function

The main conversion function. Takes a `JupyterNotebook` and options, returns markdown string.

- [ ] Create `src/to-markdown.ts`:
  ```typescript
  export interface JupyterToMarkdownOptions {
      language: string;           // e.g., "julia", "python"
      assets: JupyterAssets;      // figure output paths
      execute: CellExecuteOptions; // echo, include, output, warning defaults
      keepHidden: boolean;
      toHtml: boolean;
      toLatex: boolean;
      toMarkdown: boolean;
      toIpynb: boolean;
      toPresentation: boolean;
      figFormat: string;          // "png", "svg", "pdf", etc.
      figDpi: number;
      preserveCodeCellYaml?: boolean;
  }

  export interface JupyterToMarkdownResult {
      cellOutputs: string[];        // markdown for each cell
      pandoc: Record<string, unknown>;
      htmlPreserve: Record<string, string>;
      dependencies?: JupyterWidgetDependencies;
  }

  export function jupyterToMarkdown(
      nb: JupyterNotebook,
      options: JupyterToMarkdownOptions,
  ): JupyterToMarkdownResult;
  ```

- [ ] Implement cell walking logic:
  1. Iterate notebook cells
  2. For each markdown cell: emit source as-is
  3. For each code cell:
     a. Check visibility (echo, include, output options via tags.ts)
     b. Extract cell label and options
     c. Emit code fence with language and options
     d. Format each output (see below)
     e. Handle figure outputs (write to disk, emit `![]()` reference)
  4. For each raw cell: emit with format marker

- [ ] Implement output formatting:
  - **stream output** (stdout/stderr): emit as text, strip ANSI codes
  - **display_data / execute_result**: dispatch by MIME type (display-data.ts)
    - `text/html` → emit as raw HTML block (with preservation)
    - `image/png`, `image/jpeg` → decode base64, write to file, emit `![](path)`
    - `image/svg+xml` → write to file, emit `![](path)`
    - `text/plain` → emit as text output
    - `text/latex` → emit as math block
    - `text/markdown` → emit directly
    - `application/json` → emit as code block
  - **error output**: format traceback, strip ANSI codes

- [ ] Implement figure handling:
  - Write image data to `assets.figuresDir`
  - Generate filename from cell label or counter
  - Emit markdown image reference with optional caption, width/height
  - Handle `fig-format` option (request specific format from kernel)

- [ ] Implement HTML preservation:
  - Protect HTML outputs from Pandoc processing
  - Use UUID placeholders (preserve.ts)
  - Return preservation map for post-processing

- [ ] ANSI code handling:
  - Strip ANSI escape codes from text outputs
  - Simple regex replacement (not full ANSI→HTML conversion like Quarto 1's deno-dom approach)
  - Can add HTML conversion later if needed

- [ ] Reference: Quarto 1's `src/core/jupyter/jupyter.ts` function `jupyterToMarkdown` (~lines 380-700)

### Phase 3D: Utility functions

The simpler methods that the Julia engine also calls.

- [ ] Create `src/percent-script.ts`:
  - `isPercentScript(file, extensions?)` — check if file has `# %%` markers and matching extension
  - `percentScriptToMarkdown(file)` — convert percent-format script to markdown:
    - `# %%` → code cell boundaries
    - `# %% [markdown]` → markdown cells
    - Other content → code cells
  - Reference: Quarto 1's `src/core/jupyter/percent.ts`
  - ~80 lines

- [ ] Create `src/assets.ts`:
  - `assets(input, to?)` — compute figure directory paths:
    ```typescript
    function assets(input: string, to?: string): JupyterAssets {
        const stem = basename(input, extname(input));
        const baseDir = join(dirname(input), stem + "_files");
        const figDir = join(baseDir, figureDirForFormat(to));
        return { baseDir, figDir, supportingDir: baseDir };
    }
    ```
  - ~30 lines

- [ ] Create `src/result-helpers.ts`:
  - `resultIncludes(tempDir, deps?)` — extract pandoc includes from widget deps
  - `resultEngineDependencies(deps?)` — pass-through or wrap engine deps
  - ~40 lines

- [ ] Create `src/index.ts` — re-export everything

### Phase 3E: Integration with engine-host

Wire `@quarto/jupyter` into the `quarto.jupyter` namespace in `@quarto/engine-host`.

- [ ] Update `@quarto/engine-host/src/quarto-api.ts`:
  ```typescript
  import { jupyterToMarkdown, isPercentScript, ... } from "@quarto/jupyter";

  function buildJupyterNamespace(context: EngineHostContext) {
      return {
          toMarkdown: (nb, opts) => jupyterToMarkdown(nb, opts),
          isPercentScript: (file, exts) => isPercentScript(file, exts),
          percentScriptToMarkdown: (file) => percentScriptToMarkdown(file),
          assets: (input, to) => assets(input, to),
          resultIncludes: (dir, deps) => resultIncludes(dir, deps),
          resultEngineDependencies: (deps) => resultEngineDependencies(deps),
      };
  }
  ```
- [ ] Add `@quarto/jupyter` as dependency of `@quarto/engine-host`

### Phase 3F: Testing

Check existing ts-packages for the test runner convention (likely Vitest or Deno test). Run `npm install` from the repo root after creating the package.

- [ ] Unit tests for each supporting module (display-data, tags, labels, preserve, widgets)
- [ ] Unit tests for cell options parsing
- [ ] Integration test: convert a simple notebook JSON (2 code cells, 1 markdown cell) → markdown
- [ ] Integration test: notebook with image output → figure written to disk + markdown reference
- [ ] Integration test: notebook with HTML output → preservation markers
- [ ] Integration test: notebook with error output → formatted traceback
- [ ] Test with a real `.ipynb` file (e.g., from Jupyter's test fixtures)

## Design Notes

### Simplified vs. Quarto 1

Key simplifications in our rewrite:

1. **No YAML schema validation** for cell options — just parse with js-yaml
2. **No deno-dom** for ANSI→HTML — just strip ANSI codes (can add conversion later)
3. **No tree-sitter** — cell options parsing uses regex/yaml
4. **No MappedString provenance** — just plain strings with filenames
5. **Flattened options types** — `JupyterToMarkdownOptions` instead of pulling in `ExecuteOptions` → `Format` → `ProjectContext` → ...

These simplifications mean ~1300 lines of clean code vs. ~5000+ lines of tangled Quarto 1 code with 30+ transitive dependencies.

### Accuracy target

The output should match Quarto 1's for the common cases:
- Code cells with text, image, and HTML outputs
- Cell visibility options (echo, include, output)
- Figure file generation and referencing
- HTML preservation

Edge cases where we may differ:
- Rare MIME types (vdom, plotly — add support as needed)
- Complex widget dependency chains
- ANSI color preservation in output (we strip, Quarto 1 converts to HTML spans)

### Future: Quarto 1 adoption

This package is designed to be importable by Quarto 1, replacing:
- `src/core/jupyter/jupyter.ts` (the `jupyterToMarkdown` function)
- `src/core/jupyter/display-data.ts`, `tags.ts`, `labels.ts`, `preserve.ts`, `widgets.ts`
- Parts of `src/core/jupyter/jupyter-shared.ts`

The API signatures are compatible. Quarto 1 would need to adapt its options types to match our flattened `JupyterToMarkdownOptions`.

## Success Criteria

- [ ] `@quarto/jupyter` package with all 7 methods the Julia engine uses
- [ ] `toMarkdown` correctly converts notebooks with code, markdown, and raw cells
- [ ] Image outputs write files to disk and emit correct markdown references
- [ ] HTML outputs use preservation markers
- [ ] Error outputs format tracebacks readably
- [ ] All tests pass
- [ ] Integrated into `@quarto/engine-host`'s QuartoAPI
