## Stage 9: Pandoc Conversion

**File:** `src/command/render/pandoc.ts`
**Function:** `runPandoc(options, sysFilters)`

This is the **largest and most complex stage**, converting markdown to final output format.

### What Happens

### 9.1 Setup

```typescript
const pandocEnv: { [key: string]: string } = {};
const beforePandocHooks: (() => unknown)[] = [];
const afterPandocHooks: (() => unknown)[] = [];
```

### 9.2 Merge Engine Results

```typescript
// Merge includes from engine
if (executeResult.includes) {
  format.pandoc = mergePandocIncludes(
    format.pandoc || {},
    executeResult.includes,
  );
}

// Merge pandoc options from engine
if (executeResult.pandoc) {
  format.pandoc = mergeConfigs(
    format.pandoc || {},
    executeResult.pandoc,
  );
}
```

### 9.3 Generate Defaults

```typescript
let allDefaults = (await generateDefaults(options)) || {};
```

**Pandoc defaults** are generated from `Format`:

```typescript
// src/command/render/defaults.ts
async function generateDefaults(options: PandocOptions) {
  const defaults: Record<string, unknown> = {};

  // Output format
  defaults.to = options.format.pandoc.to || "html";
  defaults.from = options.format.pandoc.from || "markdown";

  // Template
  if (options.format.pandoc.template) {
    defaults.template = options.format.pandoc.template;
  }

  // TOC
  if (options.format.pandoc.toc) {
    defaults["table-of-contents"] = true;
    defaults["toc-depth"] = options.format.pandoc["toc-depth"] || 3;
  }

  // Bibliography
  if (options.format.metadata.bibliography) {
    defaults.bibliography = options.format.metadata.bibliography;
    defaults["cite-method"] = options.format.pandoc["cite-method"] || "citeproc";
  }

  // Filters (initially empty, filled later)
  defaults.filters = [];

  // Variables
  defaults.variables = options.format.pandoc.variables || {};

  // ... many more options

  return defaults;
}
```

### 9.4 Format Extras

```typescript
const projectExtras = options.project?.formatExtras
  ? await options.project.formatExtras(
      options.source,
      options.flags,
      options.format,
      options.services,
    )
  : {};

const formatExtras = options.format.formatExtras
  ? await options.format.formatExtras(
      options.source,
      options.markdown,
      options.flags,
      options.format,
      options.libDir,
      options.services,
      options.offset,
      options.project,
      options.quiet,
    )
  : {};

const inputExtras = mergeConfigs(projectExtras, formatExtras);
const extras = await resolveExtras(
  options.source,
  inputExtras,
  options.format,
  cwd,
  options.libDir,
  dependenciesFile,
  options.project,
);
```

**Format Extras** provide:
- Additional filters
- HTML postprocessors
- Dependencies (CSS, JS libraries)
- Metadata overrides
- Pandoc arguments
- Template context

**Example (HTML format):**

```typescript
// src/format/html/format-html.ts
export function htmlFormat(): Format {
  return {
    formatExtras: async (
      source,
      markdown,
      flags,
      format,
      libDir,
      services,
      offset,
      project,
      quiet,
    ): Promise<FormatExtras> => {
      const extras: FormatExtras = {
        filters: {
          post: ["quarto-html"],  // HTML filter
        },
        html: {
          [kHtmlPostprocessors]: [
            bootstrapPostprocessor,
            quartoHtmlPostprocessor,
          ],
          [kHtmlFinalizers]: [
            quartoHtmlFinalizer,
          ],
          [kDependencies]: [
            // Bootstrap CSS/JS
            bootstrapDependency(),
            // Quarto CSS/JS
            quartoDependency(),
          ],
        },
        metadata: {
          // Document class for LaTeX
        },
      };

      return extras;
    },
  };
}
```

### 9.5 Postprocessors and Filters

```typescript
const postprocessors: Array<
  (output: string) => Promise<{ supporting?: string[]; resources?: string[] } | void>
> = [];

const htmlPostprocessors: Array<HtmlPostProcessor> = [];
const htmlFinalizers: Array<(doc: Document) => Promise<void>> = [];

// Add postprocessors from extras
postprocessors.push(...(extras.postprocessors || []));
htmlPostprocessors.push(...(extras.html?.[kHtmlPostprocessors] || []));
htmlFinalizers.push(...(extras.html?.[kHtmlFinalizers] || []));

// Add built-in HTML postprocessors
if (isHtmlFileOutput(options.format.pandoc)) {
  htmlPostprocessors.push(overflowXPostprocessor);

  if (options.flags?.katex || options.format.pandoc[kHtmlMathMethod] === "katex") {
    htmlPostprocessors.push(katexPostProcessor());
  }

  if (!projectIsWebsite(options.project)) {
    htmlPostprocessors.push(discoverResourceRefs);
    htmlPostprocessors.push(fixEmptyHrefs);
  }
}
```

### 9.6 Template Processing

```typescript
const templateContext = extras.templateContext;
if (templateContext) {
  const template = userTemplate
    ? resolvePath(userTemplate)
    : templateContext.template;

  const partials: string[] = templateContext.partials || [];
  partials.push(...userPartials);

  const stagedTemplate = await stageTemplate(
    options,
    extras,
    { template, partials },
  );

  allDefaults[kTemplate] = stagedTemplate;
}
```

**Template Staging:**
- Copies template to temp directory
- Processes template partials
- Injects format-specific content

### 9.7 Filter Assembly

```typescript
allDefaults.filters = [
  ...extras.filters?.pre || [],        // Pre-filters
  ...allDefaults.filters || [],        // User filters
  ...extras.filters?.post || [],       // Post-filters (quarto-html, etc.)
];
```

**Quarto Filter Chain** (typical for HTML):

```
1. Pre-filters (format-specific)
   ↓
2. User filters (from metadata.filters)
   ↓
3. quarto-pre          # Preprocessing
   ↓
4. crossref            # Cross-references
   ↓
5. quarto              # Main quarto filter
   ↓
6. quarto-post         # Post-processing
   ↓
7. quarto-html         # HTML-specific
```

### 9.8 Filter Parameters

```typescript
const paramsJson = await filterParamsJson(
  pandocArgs,
  options,
  allDefaults,
  formatFilterParams,
  filterResultsFile,
  dependenciesFile,
);

pandocEnv["QUARTO_FILTER_PARAMS"] = encodeBase64(
  JSON.stringify(paramsJson),
);
```

**Filter Parameters** include:
- Format information
- Project information
- Resource paths
- Dependencies file
- Crossref settings
- Much more...

Filters access these via `QUARTO_FILTER_PARAMS` environment variable.

### 9.9 Metadata Processing

```typescript
// Remove front matter from markdown
const partitioned = partitionYamlFrontMatter(options.markdown);
const engineMetadata = partitioned?.yaml
  ? readYamlFromMarkdown(partitioned.yaml)
  : {};
const markdown = partitioned?.markdown || options.markdown;

// Merge metadata
const pandocMetadata = safeCloneDeep(options.format.metadata || {});
for (const key of Object.keys(engineMetadata)) {
  if (!isQuartoMetadata(key) && !isIncludeMetadata(key)) {
    pandocMetadata[key] = engineMetadata[key];
  }
}

// Resolve dates
pandocMetadata[kDate] = resolveAndFormatDate(
  options.source,
  pandocMetadata[kDate],
  pandocMetadata[kDateFormat],
);

// Resolve authors
const authors = parseAuthor(pandocMetadata[kAuthor], true);
if (authors) {
  pandocMetadata[kAuthor] = authors.map(author => cslNameToString(author.name));
}
```

### 9.10 Write Input Files

```typescript
// Write markdown to temp file
const inputTemp = options.services.temp.createFile({
  prefix: "quarto-input",
  suffix: ".md",
});
Deno.writeTextFileSync(inputTemp, markdown);

// Write metadata to temp file
const metadataTemp = options.services.temp.createFile({
  prefix: "quarto-metadata",
  suffix: ".yml",
});
Deno.writeTextFileSync(metadataTemp, stringify(pandocMetadata));

// Write defaults to temp file
const defaultsFile = await writeDefaultsFile(
  allDefaults,
  options.services.temp,
);
```

### 9.11 Build Pandoc Command

```typescript
const cmd = [
  pandocBinaryPath(),
  "+RTS", "-K512m", "-RTS",     // Increase stack size
  "--defaults", defaultsFile,
  inputTemp,
  "--metadata-file", metadataTemp,
  ...pandocArgs,
];
```

### 9.12 Execute Pandoc

```typescript
const result = await execProcess({
  cmd: cmd[0],
  args: cmd.slice(1),
  cwd,
  env: pandocEnv,
});
```

### 9.13 Return Results

```typescript
if (result.success) {
  return {
    inputMetadata: pandocMetadata,
    inputTraits: {},
    resources: [],
    postprocessors,
    htmlPostprocessors: isHtmlOutput(options.format.pandoc)
      ? htmlPostprocessors
      : [],
    htmlFinalizers: isHtmlDocOutput(options.format.pandoc)
      ? htmlFinalizers
      : [],
  };
} else {
  return null;
}
```

**Key Source Locations:**
- runPandoc: `src/command/render/pandoc.ts:308`
- generateDefaults: `src/command/render/defaults.ts`
- resolveExtras: `src/command/render/pandoc.ts:1412`

