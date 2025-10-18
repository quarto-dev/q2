## Complete Data Flow

```
User runs: quarto render doc.qmd

1. CLI (cmd.ts)
   ├─ Parse arguments
   ├─ Create services
   └─ Call render()
       ↓
2. Render Coordinator (render-shared.ts)
   ├─ Initialize YAML validation
   ├─ Create singleFileProjectContext
   └─ Call renderFiles()
       ↓
3. File Rendering (render-files.ts)
   ├─ Create temp context
   └─ Call renderFileInternal()
       ↓
4. Context Creation (render-contexts.ts)
   ├─ Select engine (fileExecutionEngineAndTarget)
   │  └─ Returns: { engine, target }
   │      - target.markdown (MappedString)
   │      - target.metadata (YAML)
   ├─ Resolve formats (resolveFormats)
   │  ├─ Merge metadata (project → directory → document → CLI)
   │  ├─ Load format definitions (html, pdf, etc.)
   │  └─ Returns: Format objects
   └─ Pre-engine handlers (handleLanguageCells)
       ↓
5. YAML Validation (validate-document.ts)
   ├─ Load schemas
   └─ Validate metadata
       ↓
6. Engine Execution (renderExecute)
   ├─ Check freeze/thaw
   ├─ Execute engine
   │  └─ Engine-specific execution:
   │      - Jupyter: Start kernel, execute cells
   │      - Knitr: Shell to R, run knitr::knit()
   │      - Markdown: Pass through
   ├─ Freeze results (if enabled)
   └─ Returns: ExecuteResult
       - markdown (executed)
       - supporting (figures, etc.)
       - filters
       - includes
       ↓
7. Post-Engine Handlers (handleLanguageCells)
   ├─ Mapped diff (track changes)
   ├─ Process language cells (OJS, diagrams)
   └─ Merge handler results
       ↓
8. Notebook Rendering (ensureNotebookContext)
   └─ Render any embedded notebooks
       ↓
9. Pandoc (renderPandoc → runPandoc)
   ├─ Merge engine results
   ├─ Generate defaults
   ├─ Resolve format extras
   │  ├─ Filters
   │  ├─ Postprocessors
   │  ├─ Dependencies
   │  └─ Template
   ├─ Write temp files
   │  ├─ Input markdown
   │  ├─ Metadata YAML
   │  └─ Defaults YAML
   ├─ Build command
   └─ Execute pandoc
       - Runs filters (pre → user → crossref → quarto → post → format-specific)
       - Generates output (HTML, PDF, etc.)
       ↓
10. Postprocessing (complete)
    ├─ Engine postprocess
    ├─ HTML postprocessors
    │  ├─ Parse HTML
    │  ├─ Run postprocessors (Bootstrap, Quarto, etc.)
    │  ├─ Run finalizers
    │  └─ Write modified HTML
    ├─ Generic postprocessors
    ├─ Self-contained (if requested)
    ├─ Recipe completion (e.g., latexmk for PDF)
    ├─ Cleanup
    └─ Return RenderedFile

Final output: doc.html (and supporting files)
```

