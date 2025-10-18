## Stage 8: Language Cell Handlers

**File:** `src/command/render/render-files.ts`
**Location:** After engine execution, before Pandoc

### What Happens

### 8.1 Mapped Diff

```typescript
let mappedMarkdown: MappedString;

if (!isJupyterNotebook(context.target.source)) {
  mappedMarkdown = mappedDiff(
    context.target.markdown,      // Original
    baseExecuteResult.markdown,   // After execution
  );
} else {
  mappedMarkdown = asMappedString(baseExecuteResult.markdown);
}
```

**Purpose of `mappedDiff`:**
- Tracks which parts of markdown changed during execution
- Preserves source location information through transformations
- Enables accurate error reporting

### 8.2 Handle Language Cells

```typescript
const languageCellHandlerOptions: LanguageCellHandlerOptions = {
  name: "",
  temp: tempContext,
  format: recipe.format,
  markdown: mappedMarkdown,
  context,
  flags: options.flags || {},
  stage: "post-engine",
};

const { markdown, results } = await handleLanguageCells(
  languageCellHandlerOptions,
);
```

**Language Handlers:**

Each handler processes specific code block types:

```typescript
// src/core/handlers/base.ts
const handlers: Record<string, LanguageCellHandler> = {
  "ojs": ojsHandler,           // Observable JavaScript
  "mermaid": diagramHandler,   // Mermaid diagrams
  "dot": diagramHandler,       // Graphviz
  // ... etc
};
```

#### OJS Handler

```typescript
// src/core/handlers/ojs.ts
async function ojsHandler(
  context: LanguageCellHandlerContext,
): Promise<LanguageCellHandlerResult> {
  // 1. Extract OJS cells
  const cells = extractOjsCells(context.markdown);

  // 2. For each cell, generate HTML
  const htmlCells = [];
  for (const cell of cells) {
    const html = await ojsCompile(cell.source);
    htmlCells.push(html);
  }

  // 3. Replace cells in markdown
  let markdown = context.markdown;
  for (let i = 0; i < cells.length; i++) {
    markdown = markdown.replace(
      cells[i].text,
      `\n${htmlCells[i]}\n`
    );
  }

  // 4. Return dependencies
  return {
    markdown,
    includes: {
      [kIncludeInHeader]: [ojsRuntime],
      [kIncludeAfterBody]: [ojsInitScript],
    },
    supporting: [ojsRuntimeFiles],
  };
}
```

#### Diagram Handler

```typescript
// src/core/handlers/diagram.ts
async function diagramHandler(
  context: LanguageCellHandlerContext,
): Promise<LanguageCellHandlerResult> {
  const cells = extractDiagramCells(context.markdown);

  for (const cell of cells) {
    // 1. Generate diagram
    let result;
    if (cell.language === "mermaid") {
      result = await renderMermaid(cell.source);
    } else if (cell.language === "dot") {
      result = await renderGraphviz(cell.source);
    }

    // 2. Save to file
    const outputPath = join(filesDir, `diagram-${cell.id}.${result.ext}`);
    Deno.writeFileSync(outputPath, result.data);

    // 3. Replace with image reference
    context.markdown = context.markdown.replace(
      cell.text,
      `![${cell.caption}](${outputPath})`
    );
  }

  return { markdown: context.markdown, supporting: [filesDir] };
}
```

### 8.3 OJS Execute Result

After handlers, special OJS processing:

```typescript
const { executeResult, resourceFiles: ojsResourceFiles } =
  await ojsExecuteResult(
    context,
    mappedExecuteResult,
    ojsBlockLineNumbers,
  );
```

**OJS Compilation:**
- Parses OJS code
- Generates JavaScript modules
- Injects Observable runtime
- Handles OJS imports and reactivity

### 8.4 Merge Handler Results

```typescript
const mergeHandlerResults = (
  results: HandlerContextResults | undefined,
  executeResult: MappedExecuteResult,
  context: RenderContext,
) => {
  if (results === undefined) return;

  // Merge includes
  if (executeResult.includes) {
    executeResult.includes = mergeConfigs(
      executeResult.includes,
      results.includes,
    );
  } else {
    executeResult.includes = results.includes;
  }

  // Add supporting files
  executeResult.supporting.push(...results.supporting);
};

mergeHandlerResults(
  context.target.preEngineExecuteResults,  // Pre-engine handlers
  mappedExecuteResult,
  context,
);
mergeHandlerResults(results, mappedExecuteResult, context);
```

### 8.5 Keep Markdown

```typescript
const keepMd = executionEngineKeepMd(context);
if (keepMd && context.format.execute[kKeepMd]) {
  Deno.writeTextFileSync(keepMd, executeResult.markdown.value);
}
```

Saves intermediate markdown (e.g., `doc.html.md`) if requested.

**Key Source Locations:**
- handleLanguageCells: `src/core/handlers/base.ts:handleLanguageCells`
- OJS handler: `src/core/handlers/ojs.ts`
- Diagram handler: `src/core/handlers/diagram.ts`
- mappedDiff: `src/core/mapped-text.ts`

