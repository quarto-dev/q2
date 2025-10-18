# Session Log: 2025-10-13 - Dependency Diagram Creation

## Summary

Created a comprehensive Graphviz dependency diagram visualizing all Kyoto/Quarto subsystems and their relationships. The diagram shows dependencies flowing from foundational components (MappedString, QuartoMD, ErrorReporting) through core infrastructure to rendering systems and user-facing tools.

## Tasks Completed

1. **Analyzed existing notes** to understand all subsystems and their dependencies
2. **Created Graphviz diagram** (`quarto-dependencies.dot`) with:
   - 7 layered clusters (External, Foundation, Core, Processing, Rendering, Postprocessing, Tools)
   - 19 nodes representing major subsystems
   - 34 dependency arrows showing relationships
   - Color-coded by layer for visual clarity
3. **Generated outputs**: PNG (128K) and SVG (28K) visualizations
4. **Reversed arrow direction** from "A depends on B" to "B is used by A" for clearer dependency flow
5. **Moved to notes**: Placed diagram in `claude-notes/quarto-dependencies.dot`
6. **Updated index**: Added diagram reference to `00-INDEX.md`

## Key Insights from Diagram

### Foundation Layer (Everything depends on these)
- **MappedString/SourceInfo**: Source location tracking through all transformations
- **quarto-markdown**: Rust parser producing typed Pandoc AST
- **Error Reporting**: ariadne + Markdown/Pandoc AST for beautiful error messages

### Dependency Flow
```
External Tools (Pandoc, Browser, Node.js)
    ↓
Foundation (MappedString, QuartoMD, ErrorReporting)
    ↓
Core Infrastructure (YAML System, Configuration System)
    ↓
Processing (Workflow, Engines, Formats)
    ↓
Rendering (SingleDoc, Website, Book)
    ↓
Postprocessing (HTML, Templates)
    ↓
Tools (LSP, CLI, MCP)
```

### Critical Dependencies

**Most depended-upon subsystems:**
1. **MappedString** → Used by: YAMLSystem, ConfigSystem, LSP (foundation for error locations)
2. **QuartoMD** → Used by: ErrorReporting, LSP, SingleDoc (AST source of truth)
3. **ErrorReporting** → Used by: YAMLSystem, Workflow, HTMLPost, Templates, LSP (error formatting)

**High-impact subsystems:**
- **Workflow** → Used by: Engines, Formats, SingleDoc, CLI (orchestration)
- **SingleDoc** → Used by: Website, CLI (rendering foundation)
- **Website** → Used by: Book, CLI (project rendering)

### External Dependencies
- **Pandoc**: Document conversion (SingleDoc shells out)
- **Browser (Chrome)**: Mermaid diagrams, screenshots (HTMLPost shells out)
- **Node.js**: OJS parser (Engines shell out)

## Technical Decisions

### Diagram Direction
**Initial**: Arrows pointed from dependent → dependency (A → B means "A depends on B")
**Final**: Arrows point from dependency → dependent (A → B means "B uses A")

**Rationale**: The final direction makes it easier to:
- See how foundational components flow upward
- Trace impact of changes (what would break if X changes?)
- Understand "used by" relationships intuitively

### Graphviz Over Mermaid
**User preference**: Always use Graphviz, never Mermaid

### Diagram Organization
- **Clusters**: Group related subsystems by architectural layer
- **Colors**: Distinct colors per layer for visual separation
- **Layout**: Top-to-bottom with orthogonal edges (splines=ortho)
- **Legend**: Explains arrow direction and dashed lines (external tools)

## File Locations

- **Source**: `/Users/cscheid/repos/github/cscheid/kyoto/claude-notes/quarto-dependencies.dot`
- **Outputs**:
  - `quarto-dependencies.png` (128K)
  - `quarto-dependencies.svg` (28K)
- **Reference**: Listed in `claude-notes/00-INDEX.md` under "Project Overview"

## Usage

To regenerate diagrams:
```bash
cd /Users/cscheid/repos/github/cscheid/kyoto
dot -Tpng -o quarto-dependencies.png claude-notes/quarto-dependencies.dot
dot -Tsvg -o quarto-dependencies.svg claude-notes/quarto-dependencies.dot
```

## Next Steps

This diagram serves as a visual reference for:
- Understanding subsystem relationships
- Planning implementation order (start with foundation layer)
- Identifying critical path dependencies
- Communicating architecture to stakeholders

**For future work**: Diagram can be updated as subsystems evolve or new components are added.

## Notes

- Diagram complements existing architectural documentation in notes
- Validates the "MappedString and YAML as critical infrastructure" finding
- Shows why LSP must be ported to Rust (depends on almost everything)
- Illustrates layered architecture approach for Rust port
