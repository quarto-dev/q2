## Stage 10: Postprocessing & Finalization

**File:** `src/command/render/render.ts`
**Function:** `renderPandoc()` → `complete()`

### What Happens

The `renderPandoc()` function returns a `PandocRenderCompletion` object with a `complete()` method that is called after all formats are rendered.

### 10.1 Engine Postprocessing

```typescript
if (executeResult.postProcess) {
  await context.engine.postprocess({
    engine: context.engine,
    target: context.target,
    format,
    output: recipe.output,
    tempDir: context.options.services.temp.createDir(),
    projectDir: context.project?.dir,
    preserve: executeResult.preserve,
    quiet: context.options.flags?.quiet,
  });
}
```

**Example (Jupyter engine):**
- Restores preserved regions (e.g., raw HTML blocks)

### 10.2 HTML Postprocessors

```typescript
const htmlPostProcessResult = await runHtmlPostprocessors(
  pandocResult.inputMetadata,
  pandocResult.inputTraits,
  pandocOptions,
  htmlPostprocessors,
  htmlFinalizers,
  renderedFormats,
  quiet,
);
```

**What runHtmlPostprocessors does:**

```typescript
async function runHtmlPostprocessors(
  inputMetadata,
  inputTraits,
  options,
  htmlPostprocessors,
  htmlFinalizers,
  renderedFormats,
  quiet,
) {
  const outputFile = isAbsolute(options.output)
    ? options.output
    : join(dirname(options.source), options.output);

  // 1. Read HTML file
  const htmlInput = Deno.readTextFileSync(outputFile);
  const doc = await parseHtml(htmlInput);

  // 2. Run each postprocessor
  const postProcessResult: HtmlPostProcessResult = {
    resources: [],
    supporting: [],
  };

  for (const postprocessor of htmlPostprocessors) {
    const result = await postprocessor(doc, {
      inputMetadata,
      inputTraits,
      renderedFormats,
      quiet,
    });

    postProcessResult.resources.push(...result.resources);
    postProcessResult.supporting.push(...result.supporting);
  }

  // 3. Run finalizers
  for (const finalizer of htmlFinalizers) {
    await finalizer(doc);
  }

  // 4. Write modified HTML
  const htmlOutput = doc.documentElement?.outerHTML;
  Deno.writeTextFileSync(outputFile, htmlOutput);

  return postProcessResult;
}
```

**Common HTML Postprocessors:**

1. **Bootstrap Postprocessor**
   - Injects Bootstrap CSS/JS
   - Processes Bootstrap components

2. **Quarto HTML Postprocessor**
   - Injects Quarto CSS/JS
   - Processes code blocks
   - Adds line numbers
   - Handles code folding

3. **Overflow-X Postprocessor**
   - Fixes horizontal overflow in cells

4. **KaTeX Postprocessor**
   - Renders math with KaTeX
   - Replaces Pandoc math placeholders

5. **Resource Discovery Postprocessor**
   - Finds referenced resources (images, etc.)
   - Adds to resource list

6. **Fix Empty HREFs**
   - Adds empty `href=""` to anchor elements
   - Required for CSS pseudo-selectors

### 10.3 Generic Postprocessors

```typescript
const postProcessSupporting: string[] = [];
const postProcessResources: string[] = [];

if (pandocResult.postprocessors) {
  for (const postprocessor of pandocResult.postprocessors) {
    const result = await postprocessor(outputFile);
    if (result && result.supporting) {
      postProcessSupporting.push(...result.supporting);
    }
    if (result && result.resources) {
      postProcessResources.push(...result.resources);
    }
  }
}
```

**Example (LaTeX):**
- Runs latexmk to generate PDF
- Collects auxiliary files

### 10.4 Self-Contained Output

```typescript
selfContained = isSelfContainedOutput(flags, format, finalOutput);

if (selfContained && isHtmlFileOutput(format.pandoc)) {
  await pandocIngestSelfContainedContent(
    outputFile,
    format.pandoc[kResourcePath],
  );
}
```

**Self-contained processing:**
- Inlines CSS files
- Inlines JavaScript files
- Embeds images as data URIs
- Creates single-file output

### 10.5 Recipe Completion

```typescript
finalOutput = (await recipe.complete(pandocOptions)) || recipe.output;
```

**Recipe `complete()` hook:**
- For PDF: runs latexmk
- For DOCX: no additional processing
- For HTML: no additional processing (already done)

**Example (PDF recipe):**

```typescript
// src/command/render/output-tex.ts
complete: async (pandocOptions: PandocOptions) => {
  if (pandocOptions.format.render[kLatexAutoMk] !== false) {
    // Run latexmk to generate PDF
    await latexmk({
      input: recipe.output,
      format: pandocOptions.format,
      tempDir: pandocOptions.services.temp,
    });

    // Change output from .tex to .pdf
    const [dir, stem] = dirAndStem(recipe.output);
    return join(dir, stem + ".pdf");
  }

  return recipe.output;
},
```

### 10.6 Supporting Files

```typescript
let supporting = filesDir ? executeResult.supporting : undefined;

// Add injected libs
if (filesDir && isHtmlFileOutput(format.pandoc)) {
  const filesLibs = join(dirname(context.target.source), context.libDir);
  if (existsSync(filesLibs) && (!supporting || !supporting.includes(filesLibs))) {
    supporting = supporting || [];
    supporting.push(filesLibs);
  }
}

// Add HTML postprocessor supporting files
if (htmlPostProcessResult.supporting && htmlPostProcessResult.supporting.length > 0) {
  supporting = supporting || [];
  supporting.push(...htmlPostProcessResult.supporting);
}

// Add generic postprocessor supporting files
if (postProcessSupporting && postProcessSupporting.length > 0) {
  supporting = supporting || [];
  supporting.push(...postProcessSupporting);
}
```

**Supporting files** include:
- `{input}_files/` directory (figures, etc.)
- `{libDir}/` directory (CSS/JS libraries)
- Additional files from postprocessors

### 10.7 Cleanup

```typescript
if (cleanup !== false) {
  renderCleanup(
    context.target.input,
    finalOutput,
    format,
    context.project,
    cleanupSelfContained,      // Remove supporting files for self-contained
    executionEngineKeepMd(context),
  );
}
```

**Cleanup removes:**
- Intermediate markdown files (unless `keep-md: true`)
- Self-contained supporting files
- Empty directories

### 10.8 Return Rendered File

```typescript
const result: RenderedFile = {
  isTransient: recipe.isOutputTransient,
  input: projectPath(context.target.source),
  markdown: executeResult.markdown,
  format,
  supporting: supporting
    ? supporting.filter(existsSync).map(file =>
        context.project ? relative(context.project.dir, file) : file
      )
    : undefined,
  file: recipe.isOutputTransient
    ? finalOutput
    : projectPath(finalOutput),
  resourceFiles: {
    globs: pandocResult.resources,
    files: resourceFiles.concat(htmlPostProcessResult.resources).concat(postProcessResources),
  },
  selfContained: selfContained,
};

return result;
```

**RenderedFile** contains:
- Input file path
- Output file path
- Supporting files
- Resource files
- Metadata about the render

**Key Source Locations:**
- renderPandoc complete: `src/command/render/render.ts:208`
- runHtmlPostprocessors: `src/command/render/render.ts:530`
- renderCleanup: `src/command/render/cleanup.ts`

