## File Organization Summary

**Key Directories:**

```
src/
├── command/render/          # Render command implementation
│   ├── cmd.ts              # CLI entry point
│   ├── render-shared.ts    # Main render coordinator
│   ├── render-files.ts     # File rendering logic
│   ├── render-contexts.ts  # Context creation & format resolution
│   ├── render.ts           # Pandoc rendering & postprocessing
│   ├── pandoc.ts           # Pandoc execution
│   ├── output.ts           # Output recipes
│   ├── filters.ts          # Filter management
│   ├── cleanup.ts          # Cleanup logic
│   └── ...
│
├── execute/                 # Execution engines
│   ├── engine.ts           # Engine selection
│   ├── jupyter/            # Jupyter engine
│   ├── rmd.ts              # Knitr engine
│   ├── markdown.ts         # Markdown engine
│   └── julia.ts            # Julia engine
│
├── core/
│   ├── handlers/           # Language cell handlers
│   │   ├── base.ts        # Handler infrastructure
│   │   ├── ojs.ts         # OJS handler
│   │   └── diagram.ts     # Diagram handler
│   ├── schema/            # YAML validation
│   ├── mapped-text.ts     # Source tracking
│   └── ...
│
├── format/                 # Format definitions
│   ├── html/              # HTML format
│   ├── pdf/               # PDF format
│   ├── docx/              # DOCX format
│   └── ...
│
└── project/               # Project infrastructure
    └── types/
        └── single-file/   # Single file "project"
```

