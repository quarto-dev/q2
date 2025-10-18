## Stage 7: Engine Execution

**File:** `src/command/render/render-files.ts`
**Function:** `renderExecute(context, output, options)`

### What Happens

This is where **computational code is executed** (R, Python, Julia, etc.).

### 7.1 Freeze/Thaw Check

```typescript
const canFreeze = context.engine.canFreeze &&
  (context.format.execute[kExecuteEnabled] !== false);

if (context.project && !alwaysExecute && canFreeze) {
  const thaw = context.format.execute[kFreeze] ||
               (context.options.useFreezer ? "auto" : false);

  if (thaw) {
    // Try to use cached results
    const thawedResult = defrostExecuteResult(
      context.target.source,
      output,
      context.options.services.temp,
      thaw === true,
    );

    if (thawedResult) {
      return thawedResult;  // Skip execution!
    }
  }
}
```

**Freeze system:**
- Saves execution results to `_freeze/` directory
- Contains: markdown output, figures, engine dependencies
- Allows skipping expensive computations
- Controlled by `freeze: auto|true|false` in YAML

### 7.2 Execute Options

```typescript
const executeOptions: ExecuteOptions = {
  target: context.target,               // Input file
  resourceDir: resourcePath(),          // Quarto resources
  tempDir: context.options.services.temp.createDir(),
  dependencies: resolveDependencies,    // Whether to gather deps
  libDir: context.libDir,               // Output lib directory
  format: context.format,               // Resolved format
  projectDir: context.project?.dir,
  cwd: flags.executeDir || dirname(context.target.source),
  params: resolveParams(flags.params, flags.paramsFile),
  quiet: flags.quiet,
  previewServer: context.options.previewServer,
  handledLanguages: languages(),        // ojs, mermaid, etc.
  project: context.project,
};
```

### 7.3 Engine Execution

```typescript
const executeResult = await context.engine.execute(executeOptions);
```

Each engine implements `execute()` differently:

#### Markdown Engine

```typescript
// src/execute/markdown.ts
async execute(options: ExecuteOptions): Promise<ExecuteResult> {
  // No computation - just return markdown as-is
  return {
    engine: "markdown",
    markdown: options.target.markdown.value,
    supporting: [],
    filters: [],
  };
}
```

#### Jupyter Engine

```typescript
// src/execute/jupyter/jupyter.ts
async execute(options: ExecuteOptions): Promise<ExecuteResult> {
  // 1. Parse notebook/QMD into cells
  const nb = await jupyterFromFile(options.target.source);

  // 2. Start kernel
  const kernel = await jupyterKernel(
    languageForNotebook(nb),
    options.format.execute[kExecuteDaemon],
    options.tempDir,
  );

  // 3. Execute each code cell
  for (const cell of nb.cells) {
    if (cell.cell_type === "code") {
      const result = await kernel.execute(cell.source);
      cell.outputs = result.outputs;
      cell.execution_count = result.execution_count;
    }
  }

  // 4. Convert back to markdown
  const markdown = await notebookToMarkdown(nb);

  // 5. Gather dependencies
  const dependencies = extractJupyterDependencies(nb);

  return {
    engine: "jupyter",
    markdown: markdown,
    supporting: options.format.execute[kKeepIpynb]
      ? [originalIpynbPath]
      : [],
    filters: ["quarto"],  // Core quarto filter
    engineDependencies: dependencies,
  };
}
```

**Jupyter Details:**
- Uses `jupyter-kernel` library to communicate with kernels
- Supports kernel daemon mode (keeps kernel alive between renders)
- Captures: outputs, plots, widgets, error tracebacks
- Converts output to Pandoc-compatible markdown

#### Knitr Engine

```typescript
// src/execute/rmd.ts
async execute(options: ExecuteOptions): Promise<ExecuteResult> {
  // 1. Write R script to execute knitr
  const rScript = `
    knitr::knit(
      input = "${options.target.source}",
      output = "${outputMd}",
      quiet = ${options.quiet}
    )
  `;

  // 2. Execute R script
  await execProcess({
    cmd: "Rscript",
    args: ["-e", rScript],
    cwd: options.cwd,
  });

  // 3. Read output markdown
  const markdown = Deno.readTextFileSync(outputMd);

  // 4. Gather supporting files (plots, etc.)
  const figuresDir = join(options.target.source + "_files", "figure-html");
  const supporting = existsSync(figuresDir) ? [figuresDir] : [];

  return {
    engine: "knitr",
    markdown: markdown,
    supporting: supporting,
    filters: ["quarto"],
  };
}
```

**Knitr Details:**
- Shells out to R process
- Uses `knitr::knit()` to execute R code
- Figures saved to `{input}_files/figure-{format}/`
- Preserves R session state between chunks

### 7.4 Execute Result

```typescript
interface ExecuteResult {
  engine: string;                           // "jupyter", "knitr", "markdown"
  markdown: string;                         // Executed markdown
  supporting: string[];                     // Supporting files (figures, etc.)
  filters: string[];                        // Filters to apply

  includes?: PandocIncludes;                // HTML includes
  pandoc?: FormatPandoc;                    // Pandoc options to merge

  preserve?: Record<string, string>;        // Content to preserve
  postProcess?: boolean;                    // Needs post-processing?

  engineDependencies?: Record<string, EngineDependencies>;
  resourceFiles?: string[];                 // Additional resources

  metadata?: Metadata;                      // Additional metadata
}
```

### 7.5 Freeze Results

If freezing is enabled:

```typescript
if (context.project && !context.project.isSingleFile && canFreeze) {
  const freezeFile = freezeExecuteResult(
    context.target.source,
    output,
    executeResult,
  );

  // Copy to _freeze/
  copyToProjectFreezer(context.project, projRelativeFilesDir, false, true);
}
```

**Freeze structure:**
```
_freeze/
└── {relative-path-to-file}/
    ├── {format}/
    │   ├── execute-results.json    # Execution metadata
    │   └── figure-html/            # Figures
    │       ├── plot-1.png
    │       └── plot-2.png
    └── ...
```

**Key Source Locations:**
- renderExecute: `src/command/render/render-files.ts:120`
- Jupyter engine: `src/execute/jupyter/jupyter.ts`
- Knitr engine: `src/execute/rmd.ts`
- Markdown engine: `src/execute/markdown.ts`

