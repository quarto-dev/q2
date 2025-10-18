## Stage 5: Engine Selection

**File:** `src/execute/engine.ts`
**Function:** `fileExecutionEngineAndTarget(file, flags, project)`

### What Happens

### 5.1 Registered Engines

Four engines are registered on module load:

```typescript
const kEngines: Map<string, ExecutionEngine> = new Map();

for (const engine of [knitrEngine, jupyterEngine, markdownEngine, juliaEngine]) {
  registerExecutionEngine(engine);
}
```

**Engine Implementations:**
- `knitrEngine` - `src/execute/rmd.ts` - R Markdown (Knitr)
- `jupyterEngine` - `src/execute/jupyter/jupyter.ts` - Jupyter notebooks + Python/Julia
- `markdownEngine` - `src/execute/markdown.ts` - Plain markdown (no computation)
- `juliaEngine` - `src/execute/julia.ts` - Julia native

### 5.2 Engine Selection Algorithm

```typescript
async function fileExecutionEngine(
  file: string,
  flags: RenderFlags | undefined,
  project: ProjectContext,
) {
  const ext = extname(file).toLowerCase();
  const reorderedEngines = reorderEngines(project);  // Allow project to specify order

  // 1. Try to claim by file extension
  for (const [_, engine] of reorderedEngines) {
    if (engine.claimsFile(file, ext)) {
      return engine;
    }
  }

  // 2. For .qmd or .md files, examine content
  if (kMdExtensions.includes(ext) || kQmdExtensions.includes(ext)) {
    const markdown = await project.resolveFullMarkdownForFile(undefined, file);
    return markdownExecutionEngine(markdown, reorderedEngines, flags);
  }

  return undefined;
}
```

#### Extension-Based Claims

| Extension | Claimed By | Why |
|-----------|------------|-----|
| `.ipynb` | jupyterEngine | Jupyter notebook format |
| `.Rmd` | knitrEngine | R Markdown file |
| `.jl` | juliaEngine | Julia script (rare) |
| `.qmd`, `.md` | *(content-based)* | Requires examining file |

#### Content-Based Selection (for .qmd/.md)

```typescript
function markdownExecutionEngine(markdown, reorderedEngines, flags) {
  const result = partitionYamlFrontMatter(markdown);
  if (result) {
    let yaml = readYamlFromMarkdown(result.yaml);
    yaml = mergeConfigs(yaml, flags?.metadata);

    // 1. Check if engine is declared in YAML
    for (const [_, engine] of reorderedEngines) {
      if (yaml[engine.name]) {  // e.g., yaml.jupyter
        return engine;
      }

      const format = metadataAsFormat(yaml);
      if (format.execute?.engine === engine.name) {
        return engine;
      }
    }
  }

  // 2. Check code block languages
  const languages = languagesInMarkdown(markdown);
  for (const language of languages) {
    for (const [_, engine] of reorderedEngines) {
      if (engine.claimsLanguage(language)) {
        return engine;
      }
    }
  }

  // 3. Check for non-handler languages → Jupyter
  const handlerLanguagesVal = handlerLanguages();
  for (const language of languages) {
    if (language !== "ojs" && !handlerLanguagesVal.includes(language)) {
      return jupyterEngine;
    }
  }

  // 4. Default to markdown engine (no computation)
  return markdownEngine;
}
```

**Language Claims:**

```typescript
// knitrEngine.claimsLanguage()
["r"]

// jupyterEngine.claimsLanguage()
["python", "julia", "bash", "sh", "perl", ...]

// markdownEngine.claimsLanguage()
[]  // Claims nothing by language

// juliaEngine.claimsLanguage()
["julia"]  // Higher priority if project specifies julia first
```

**Handler Languages** (not claimed by engines):
- `ojs` - Observable JavaScript (processed by OJS handler)
- `mermaid` - Diagrams (processed by diagram handler)
- `dot` - Graphviz (processed by diagram handler)

### 5.3 Target Creation

Once engine is selected:

```typescript
const target = await engine.target(file, quiet, markdown, project);
```

Each engine implements `target()` differently:

**markdown engine:**
```typescript
async target(file, quiet, markdown, project) {
  return {
    source: file,
    input: file,
    markdown: await project.resolveFullMarkdownForFile(this, file),
    metadata: readYamlFromMarkdown(markdown.yaml),
  };
}
```

**jupyter engine:**
```typescript
async target(file, quiet, markdown, project) {
  if (file.endsWith(".ipynb")) {
    // Convert .ipynb to .qmd
    const qmd = await jupyterToMarkdown(file);
    return {
      source: file,              // Original .ipynb
      input: qmd.outputFile,     // Converted .qmd
      markdown: qmd.markdown,
      metadata: qmd.metadata,
    };
  } else {
    // .qmd with jupyter engine
    return {
      source: file,
      input: file,
      markdown: await project.resolveFullMarkdownForFile(this, file),
      metadata: readYamlFromMarkdown(markdown.yaml),
    };
  }
}
```

**knitr engine:**
```typescript
async target(file, quiet, markdown, project) {
  // Similar to markdown engine
  return {
    source: file,
    input: file,
    markdown: await project.resolveFullMarkdownForFile(this, file),
    metadata: readYamlFromMarkdown(markdown.yaml),
  };
}
```

**Key Source Locations:**
- fileExecutionEngineAndTarget: `src/execute/engine.ts:230`
- markdownExecutionEngine: `src/execute/engine.ts:99`
- Engine registration: `src/execute/engine.ts:45`

### ExecutionEngine Interface

```typescript
interface ExecutionEngine {
  name: string;                                    // "jupyter", "knitr", "markdown"

  defaultExt: string;                              // ".qmd"
  defaultYaml: string;                             // YAML block to add

  validExtensions: () => string[];                 // [".qmd", ".md", ".ipynb"]
  claimsFile: (file: string, ext: string) => boolean;
  claimsLanguage: (language: string) => boolean;

  target: (file, quiet, markdown, project) => Promise<ExecutionTarget>;

  execute: (options: ExecuteOptions) => Promise<ExecuteResult>;

  dependencies?: (options) => Promise<DependenciesResult>;
  postprocess?: (options) => Promise<void>;
  postRender?: (file: RenderedFile, context) => Promise<void>;

  partitionedMarkdown?: (file: string, format?: Format) => Promise<PartitionedMarkdown>;

  canFreeze?: boolean;
  ignoreDirs?: () => string[];
  intermediateFiles?: (input: string) => string[];

  filterFormat?: (file, options, format) => Format;
  executeTargetSkipped?: (target, format, project) => void;
}
```

