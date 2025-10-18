## Stage 4: Render Context Creation

**File:** `src/command/render/render-contexts.ts`
**Function:** `renderContexts(file, options, forExecute, ...)`

### What Happens

This is **the most complex stage** where all configuration is resolved and merged.

### 4.1 Engine and Target Resolution

```typescript
const { engine, target } = await fileExecutionEngineAndTarget(
  file.path,
  options.flags,
  project,
);
```

**What `target` contains:**
- `input`: Absolute path to input file
- `source`: Absolute path to source file (may differ for notebooks)
- `markdown`: Full markdown content (as MappedString with source tracking)
- `metadata`: Parsed YAML front matter

**Engine selection logic** (see Stage 5 for details)

### 4.2 Format Resolution

```typescript
const formats = await resolveFormats(
  file,
  target,
  engine,
  options,
  notebookContext,
  project,
  enforceProjectFormats,
);
```

This creates a format specification for each output format (html, pdf, etc.).

#### Metadata Hierarchy

Metadata is merged from **4 levels** (in order of precedence):

```
1. Default format metadata (from format definitions)
   ↓
2. Project metadata (for single file: empty)
   ↓
3. Directory metadata (_metadata.yml in same directory)
   ↓
4. Document metadata (YAML front matter)
   ↓
5. CLI flags (--metadata, -M)
```

The merge uses `mergeConfigs()` which:
- Deep merges objects
- **Concatenates arrays** (doesn't replace)
- Handles special cases (bibliography, css, includes)

#### Format Resolution Details

**For each format** (e.g., "html", "pdf"):

1. **Parse Format String**
   ```typescript
   const formatDesc = parseFormatString(format);
   // Examples:
   // "html" → { baseFormat: "html", variants: [] }
   // "html+smart" → { baseFormat: "html", variants: ["+smart"] }
   // "dashboard-html" → { extension: "dashboard", baseFormat: "html" }
   ```

2. **Load Default Writer Format**
   ```typescript
   const writerFormat = defaultWriterFormat(formatDesc.formatWithVariants);
   ```

   Loads built-in format definition from `src/format/`:
   - `html/format-html.ts`
   - `pdf/format-pdf.ts`
   - `docx/format-docx.ts`
   - etc.

3. **Load Extension Format** (if applicable)
   ```typescript
   const extensionMetadata = await readExtensionFormat(
     target.source,
     formatDesc,
     options.services.extension,
     project,
   );
   ```

   For custom formats like `dashboard-html`:
   - Finds extension in `_extensions/`
   - Reads format contribution from extension's `_extension.yml`

4. **Merge Format Metadata**
   ```typescript
   mergedFormats[format] = mergeFormatMetadata(
     defaultWriterFormat,     // Built-in format
     extensionMetadata,       // Extension's contribution
     userFormat,              // User's configuration
   );
   ```

5. **Apply Format Filters**
   ```typescript
   // Engine-specific filtering
   if (engine.filterFormat) {
     format = engine.filterFormat(target.source, options, format);
   }

   // Project-type-specific filtering
   if (projType.filterFormat) {
     format = projType.filterFormat(target.source, format, project);
   }
   ```

#### Format Data Structure

```typescript
interface Format {
  identifier: {
     [kTargetFormat]: string;        // "html"
     [kBaseFormat]: string;          // "html" (without variants)
  };

  metadata: Metadata;                // User-visible metadata

  pandoc: FormatPandoc;              // Pandoc options
    // to, from, writer, template, filters, etc.

  render: FormatRender;              // Render options
    // output-ext, keep-md, keep-source, etc.

  execute: FormatExecute;            // Execution options
    // enabled, cache, freeze, daemon, etc.

  language: LanguageTranslations;    // Localized strings

  formatExtras?: FormatExtrasProvider; // Dynamic extras
}
```

### 4.3 Pre-Engine Language Cell Handling

For `.qmd` files (not Jupyter notebooks):

```typescript
const { markdown, results } = await handleLanguageCells({
  name: "",
  temp: options.services.temp,
  format: context.format,
  markdown: context.target.markdown,
  context,
  flags: options.flags,
  stage: "pre-engine",
});

context.target.markdown = markdown;
context.target.preEngineExecuteResults = results;
```

**What this does:**
- Processes special cell types (diagram cells, etc.)
- Executes mermaid diagrams, graphviz, etc.
- Injects dependencies (CSS, JS)
- Returns modified markdown + results

**Important:** This happens BEFORE engine execution, so engines see processed markdown.

### 4.4 Context Creation

For each format, creates a `RenderContext`:

```typescript
interface RenderContext {
  target: ExecutionTarget;      // Input file + metadata
  options: RenderOptions;       // Flags, services, args
  engine: ExecutionEngine;      // Selected engine
  format: Format;               // Resolved format
  active: boolean;              // Is this format being rendered?
  project: ProjectContext;      // Project (minimal for single file)
  libDir: string;              // Library directory path
}
```

**Key Source Locations:**
- renderContexts: `src/command/render/render-contexts.ts:203`
- resolveFormats: `src/command/render/render-contexts.ts:394`
- resolveFormatsFromMetadata: `src/command/render/render-contexts.ts:91`

