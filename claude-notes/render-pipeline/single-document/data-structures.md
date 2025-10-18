## Key Data Structures

### ExecutionTarget

```typescript
interface ExecutionTarget {
  source: string;              // Original input file
  input: string;               // Actual input (may be converted .ipynb → .qmd)
  markdown: MappedString;      // Full markdown content with source tracking
  metadata: Metadata;          // Parsed YAML front matter

  preEngineExecuteResults?: HandlerContextResults;  // Pre-engine handler results
}
```

### Format

```typescript
interface Format {
  identifier: {
    [kTargetFormat]: string;   // "html", "pdf", etc.
    [kBaseFormat]: string;     // "html" (without variants)
  };

  metadata: Metadata;           // User-visible metadata (title, author, etc.)

  pandoc: FormatPandoc;         // Pandoc options (to, from, template, toc, etc.)

  render: FormatRender;         // Render options (output-ext, keep-md, etc.)

  execute: FormatExecute;       // Execution options (enabled, cache, freeze, etc.)

  language: LanguageTranslations;  // Localized strings

  formatExtras?: FormatExtrasProvider;  // Dynamic extras function

  mergeAdditionalFormats?: (...configs: Format[]) => Format;  // Merge helper
}
```

### RenderContext

```typescript
interface RenderContext {
  target: ExecutionTarget;      // Input file + metadata
  options: RenderOptions;       // Flags, services, args
  engine: ExecutionEngine;      // Selected engine
  format: Format;               // Resolved format
  active: boolean;              // Is this format being rendered?
  project: ProjectContext;      // Project context
  libDir: string;              // Library directory path
}
```

### ExecuteResult

```typescript
interface ExecuteResult {
  engine: string;               // "jupyter", "knitr", "markdown"
  markdown: string;             // Executed markdown
  supporting: string[];         // Supporting files (figures, etc.)
  filters: string[];            // Filters to apply

  includes?: PandocIncludes;    // HTML includes
  pandoc?: FormatPandoc;        // Pandoc options to merge
  preserve?: Record<string, string>;  // Content to preserve
  postProcess?: boolean;        // Needs post-processing?

  engineDependencies?: Record<string, EngineDependencies>;
  resourceFiles?: string[];     // Additional resources
  metadata?: Metadata;          // Additional metadata
}
```

### RenderedFile

```typescript
interface RenderedFile {
  input: string;                // Input file path
  markdown: string;             // Final executed markdown
  format: Format;               // Format used
  file: string;                 // Output file path
  supporting?: string[];        // Supporting files
  resourceFiles: {
    globs: string[];            # Resource globs
    files: string[];            # Specific resource files
  };
  selfContained: boolean;       // Is output self-contained?
  isTransient?: boolean;        # Is output temporary?
}
```

