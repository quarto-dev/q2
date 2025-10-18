# Session Notes: Error Reporting & Console Print Subsystem Design

**Date:** 2025-10-12
**Topic:** Design research and planning for new error-reporting subsystem

## Summary

Completed comprehensive design research for a new error-reporting and console print subsystem for Quarto (Rust). The system will combine ariadne's visual error reporting with Markdown-based message construction, following tidyverse error message guidelines.

## Key Decisions Made

### 1. Message Content Format
- **API Surface:** Accept Markdown strings
- **Internal Representation:** Pandoc AST
- **Optimization:** Defer compile-time `md!()` macro - start with runtime parsing
  - Discussed procedural macro approach (compile-time Markdown → AST)
  - Decision: Keep it simple initially, add macro later if profiling shows need

### 2. Scope
- **Rust-only design** - Use Rust idioms fully
- No cross-language compatibility requirements
- If needed: Use WASM bindings (same approach as quarto-markdown)

### 3. Semantic Markup
- **Use Pandoc span syntax with classes:** `` `text`{.class} ``
- Examples: `.file`, `.engine`, `.format`, `.option`, `.code`
- Already supported by quarto-markdown parser
- Same syntax Quarto users already know
- Writers (ANSI, HTML) can style based on semantic classes

### 4. Theming
- **Defer theming decision**
- Build multiple output writers from Pandoc AST:
  - **ANSI writer** for terminal (colored, formatted)
  - **HTML writer** for themable contexts (CSS styling)
  - **JSON** for machine-readable (via ariadne)
  - **Plain text** for non-TTY contexts

### 5. Tidyverse Guidelines
- **Encode in API design** using builder pattern
- Methods like `.problem()`, `.add_detail()`, `.add_hint()` naturally encourage tidyverse structure
- Everything becomes Pandoc AST (can be processed programmatically before rendering)
- Optional validation in `.build()` method

## Architecture Overview

```
┌─────────────────┐
│  Markdown API   │  "The `jupyter`{.engine} kernel failed"
└────────┬────────┘
         │ parse (runtime)
         ▼
┌─────────────────┐
│   Pandoc AST    │  Inline::Code { text: "jupyter",
└────────┬────────┘                attr: { classes: ["engine"] } }
         │
         ├─► ANSI Writer  → Terminal output (colored)
         ├─► HTML Writer  → Themable web output
         ├─► JSON Writer  → Machine-readable
         └─► Plain Writer → Non-TTY contexts
```

## Core API Design

```rust
DiagnosticMessage::builder()
    .error("Incompatible types")
    .problem("Cannot combine date and datetime types")
    .add_detail("`x`{.arg} has type `date`{.type}")
    .add_detail("`y`{.arg} has type `datetime`{.type}")
    .add_hint("Convert both to the same type?")
    .at_location(span)
    .build()
```

Renders via ariadne to:

```
Error: Incompatible types
  ┌─ script.qmd:42:5
  │
42│ combine(x, y)
  │         ^^^^^ Cannot combine date and datetime types
  │
  = `x` has type `date`
  = `y` has type `datetime`
  = Convert both to the same type?
```

## Implementation Phases

### Phase 1: Core Error Types
- `DiagnosticMessage` struct
- `MessageContent` enum (Plain/Markdown/PandocAst)
- `DetailItem` for structured details
- `SourceSpan` for locations

### Phase 2: ariadne Integration
- `.to_ariadne_report()` for visual output
- `.to_json()` for structured output
- Multiple source spans support

### Phase 3: Console Output Helpers
- `Console` struct with color support
- ANSI writer for Pandoc AST
- Simple helpers: `.success()`, `.warning()`, `.info()`
- Markdown rendering: `.print_markdown()`

### Phase 4: Message Builder
- Builder pattern with tidyverse structure
- Type-safe, self-documenting API
- Optional validation
- Ergonomic string → MessageContent conversion

## Sources of Inspiration

### 1. Ariadne (Rust crate)
- Visual error reporting with labeled spans
- Multi-line, multi-file support
- 8-bit and 24-bit color
- **Usage:** Rendering layer for our errors
- **Note:** quarto-markdown's parser error system is separate (LALR-specific)

### 2. R 'cli' Package
- Semantic elements over plain strings
- Markup dialect (`.pkg`, `.file`, `.code`)
- CSS-like theming
- Glue-style interpolation
- **Inspiration:** Structured message construction

### 3. Tidyverse Style Guide
- Four-part error structure: Problem → Location → Details → Hints
- Use "must"/"can't" in problem statements
- Bullet lists with ✖ (error) and ℹ (info)
- Max 5 problems, under 80 chars
- **Implementation:** Encode in API design

## Key Design Insights

1. **Pandoc AST as universal format** - Parse once, render many ways
2. **Progressive enhancement** - Plain backticks work, classes optional
3. **Dogfooding** - Use Quarto's own markdown parser for messages
4. **Type-safe guidance** - API structure teaches good practices
5. **Separation of concerns** - Construction → AST → Rendering

## Open Questions (Lower Priority)

- Specific semantic class vocabulary (evolve during implementation)
- Integration strategy with existing error systems
- Additional output formats beyond ANSI/HTML/JSON
- Internationalization approach
- Domain-specific error builders (YAMLError, EngineError, etc.)

## Documentation Created

**Primary Document:** `claude-notes/error-reporting-design-research.md`

Contains:
- Full analysis of all three inspiration sources
- Detailed design decisions with rationale
- Complete API examples and usage patterns
- Implementation phases with code
- Compile-time macro technical analysis
- Open questions and recommendations

## Next Steps (Future Sessions)

1. Prototype Phase 1 (Core Error Types) in Rust
2. Test with existing error use cases
3. Implement ANSI writer for Pandoc AST
4. Evaluate ergonomics and iterate
5. Document error message style guide
6. Expand to Phases 2-4 based on learnings

## References

- Ariadne docs: https://docs.rs/ariadne/latest/ariadne/
- R cli package: https://cli.r-lib.org/
- Tidyverse style guide: https://style.tidyverse.org/errors.html
- quarto-markdown parser: `external-sources/quarto-markdown/`

---

**Session completed successfully. Design is ready for implementation.**
