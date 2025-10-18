## Key Design Patterns

### 1. Metadata Merging

**Pattern:** Progressive refinement through merging

```
Default Format Metadata
  ↓ (merge)
Project Metadata
  ↓ (merge)
Directory Metadata
  ↓ (merge)
Document Metadata
  ↓ (merge)
CLI Flags
  ↓
Final Metadata
```

**Implementation:**
```typescript
function mergeConfigs(base: Metadata, ...overrides: Metadata[]): Metadata {
  let result = cloneDeep(base);

  for (const override of overrides) {
    for (const key of Object.keys(override)) {
      if (Array.isArray(result[key]) && Array.isArray(override[key])) {
        // Concatenate arrays (don't replace)
        result[key] = [...result[key], ...override[key]];
      } else if (isObject(result[key]) && isObject(override[key])) {
        // Recursively merge objects
        result[key] = mergeConfigs(result[key], override[key]);
      } else {
        // Replace scalars
        result[key] = override[key];
      }
    }
  }

  return result;
}
```

### 2. MappedString (Source Tracking)

**Pattern:** Preserve source locations through transformations

```typescript
type MappedString = {
  value: string;              // Current content
  map?: SourceMap;            // How to map back to original
};

// Example transformation:
const original: MappedString = {
  value: "Hello World",
  map: { file: "doc.qmd", offset: 10 }
};

const transformed = mappedDiff(original, "HELLO WORLD");
// transformed.map still points to doc.qmd:10
```

**Why this matters:**
- Error messages can point to original source lines
- Validation errors reference actual YAML locations
- Debugging is possible even after many transformations

### 3. FormatExtras (Plugin Architecture)

**Pattern:** Formats can inject behavior at multiple stages

```typescript
interface FormatExtras {
  filters?: {
    pre?: string[];            // Filters before main processing
    post?: string[];           // Filters after main processing
  };

  metadata?: Metadata;         // Additional metadata
  metadataOverride?: Metadata; // Metadata that wins over everything

  pandoc?: FormatPandoc;       // Pandoc options
  args?: string[];             // Pandoc command-line arguments

  postprocessors?: Array<     // Generic postprocessors
    (output: string) => Promise<PostprocessResult>
  >;

  html?: {
    [kHtmlPostprocessors]?: HtmlPostProcessor[];
    [kHtmlFinalizers]?: Array<(doc: Document) => Promise<void>>;
    [kDependencies]?: Dependency[];
    [kBodyEnvelope]?: BodyEnvelope;
  };

  templateContext?: TemplateContext;
}
```

**Usage:**
```typescript
// Format definition
export function htmlFormat(): Format {
  return {
    formatExtras: async (...args) => {
      return {
        filters: {
          post: ["quarto-html"],
        },
        html: {
          [kHtmlPostprocessors]: [bootstrapPostprocessor],
          [kDependencies]: [bootstrapDependency()],
        },
      };
    },
  };
}
```

### 4. Recipe Pattern (Output-Specific Behavior)

**Pattern:** Encapsulate format-specific output logic

```typescript
interface OutputRecipe {
  output: string;                       // Output file path
  format: Format;                       // Format configuration
  keepYaml: boolean;                    // Keep YAML in markdown output?
  args: string[];                       // Pandoc arguments

  complete: (options: PandocOptions) => Promise<string | void>;
  // Called after pandoc completes, can modify output

  finalOutput?: string;                 // Final output path (after complete)
  isOutputTransient?: boolean;          // Is output temporary?
}
```

**Example (PDF):**
```typescript
const pdfRecipe: OutputRecipe = {
  output: "doc.tex",
  format: pdfFormat,
  keepYaml: false,
  args: [],

  complete: async (options) => {
    // Run latexmk
    await latexmk({ input: "doc.tex", format: options.format });
    return "doc.pdf";  // Changed from .tex to .pdf
  },

  finalOutput: "doc.pdf",
};
```

### 5. Lifetime Pattern (Resource Management)

**Pattern:** Automatic cleanup of resources

```typescript
interface Lifetime {
  cleanup(): void;
  attach(resource: { cleanup: () => void }): void;
}

// Usage:
const fileLifetime = createNamedLifetime("render-file");
try {
  // Attach resources
  fileLifetime.attach({
    cleanup() {
      resetFigureCounter();
    },
  });

  // Do work...
  await renderFileInternal(...);
} finally {
  fileLifetime.cleanup();  // Automatically cleans up all attached resources
}
```

