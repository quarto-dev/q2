# Session Log: Surface Syntax Converter Design

**Date**: 2025-10-13
**Duration**: ~2 hours
**Focus**: Analyzing and designing separation of surface syntax conversion from execution engines

## Session Overview

User requested analysis of separating surface syntax conversion (.ipynb, percent scripts, R spin scripts → qmd) from execution engines in the new Quarto architecture.

## Context

User references:
- Documentation: https://quarto.org/docs/computations/render-scripts.html (percent/spin scripts)
- Documentation: https://quarto.org/docs/computations/python.html (ipynb rendering)
- Current implementation: Engines declare which files they "claim" (src/execute/types.ts:33-34)

Current design has engines handle both:
1. Surface syntax conversion (different file formats → qmd)
2. Code execution (running code cells)

User's question: Could engines consult a centralized converter registry instead of implementing conversion themselves?

## User Requirements (from clarifying questions)

1. **R spin scripts**: Eventually pure Rust (fast), but acceptable to call R temporarily
2. **Scope**: Must support future syntax extensions (not just fixed set)
3. **Extensibility**: Independent from engines - new syntaxes don't require engine changes
4. **Integration**: Fits into extensible pipeline design vision
5. **Goal**: Better factoring, reduced engine responsibilities

## Research Process

### 1. Reviewed Existing Notes
- Read 00-INDEX.md for architecture overview
- Examined single-document-render-pipeline.md for engine behavior
- Found engine selection algorithm (Stage 5) and execution flow

### 2. Examined quarto-cli Source Code

**Engine interface** (src/execute/types.ts:33-34):
```typescript
claimsFile: (file: string, ext: string) => boolean;
claimsLanguage: (language: string) => boolean;
markdownForFile(file: string): Promise<MappedString>;
```

**Jupyter engine** (src/execute/jupyter/jupyter.ts):
- Lines 151-154: `claimsFile()` checks .ipynb OR isJupyterPercentScript
- Lines 162-174: `markdownForFile()` converts based on file type
  - .ipynb → markdownFromNotebookJSON()
  - percent script → markdownFromJupyterPercentScript()
  - else → mappedStringFromFile()

**Knitr engine** (src/execute/rmd.ts):
- Lines 68-71: `claimsFile()` checks .rmd/.rmarkdown OR isKnitrSpinScript
- Lines 77-83: `markdownForFile()` converts R spin scripts

**Conversion implementations**:
- `src/core/jupyter/jupyter-filters.ts:33` - markdownFromNotebookJSON (~10 lines, pure JS)
- `src/execute/jupyter/percent.ts:34` - markdownFromJupyterPercentScript (~60 lines, pure JS)
- `src/execute/rmd.ts:428` - markdownFromKnitrSpinScript (calls R's knitr::spin())

### 3. Key Observations

**Conversion characteristics**:
| Format | Lines | Complexity | Dependencies |
|--------|-------|------------|--------------|
| .ipynb | ~10 | Low | None |
| Percent | ~60 | Medium | None |
| R spin | ~20 | Medium | **R runtime** |

**Current coupling problems**:
1. Engines must know about ALL surface syntaxes they support
2. Conversion logic scattered (core/, execute/jupyter/, execute/rmd.ts)
3. Two-stage conversion for jupyter (.ipynb → qmd → transient notebook)
4. Testing requires mocking entire engine infrastructure

## Design Proposal

### Core Architecture

Created `SourceConverter` trait that handles pure syntax transformation:

```rust
pub trait SourceConverter {
    fn name(&self) -> &str;
    fn claims_file(&self, path: &Path, content_hint: Option<&str>) -> bool;
    fn convert(&self, input: &ConverterInput) -> Result<ConvertedSource>;
}

pub struct ConvertedSource {
    pub qmd: String,
    pub source_map: SourceMap,
    pub suggested_engine: Option<String>,
    pub metadata: Metadata,
    pub original_format: String,
}
```

### Key Design Decisions

1. **Two-phase selection**: Find converter (file inspection) → select engine (qmd content)
2. **Source mapping**: SourceMap tracks qmd positions → original file for error reporting
3. **Metadata preservation**: original_format field enables format-specific defaults
4. **Engine suggestion**: Converters can suggest engines, but qmd metadata wins
5. **Phased R spin**: Start with subprocess call, migrate to pure Rust later

### Benefits Identified

✅ **Separation of concerns**: Converters = syntax, engines = execution
✅ **Independent extension**: Add converters OR engines independently
✅ **Better testing**: Pure function tests for converters
✅ **Performance**: Caching and parallelization opportunities
✅ **Third-party friendly**: Simple trait implementation
✅ **LSP support**: Source maps enable multi-view editing
✅ **Simpler engines**: No more claimsFile/markdownForFile methods

### Challenges Addressed

1. **File claiming coordination**: Two-phase lookup (converter → engine)
2. **Metadata preservation**: ConvertedSource.original_format + defaults
3. **Source mapping**: SourceMap through entire pipeline
4. **R spin performance**: Phased approach (subprocess → pure Rust)
5. **Converter options**: ConvertOptions struct from format metadata
6. **Migration path**: Adapter pattern for incremental refactoring

## Deliverables

### Main Document
Created **surface-syntax-converter-design.md** (~600 lines) with:
- Current architecture analysis (file claiming, conversion locations, coupling points)
- Proposed Rust API design (SourceConverter trait, ConvertedSource, registry)
- Complete file processing flow with code examples
- Example converter implementation (PercentScriptConverter)
- Benefits analysis (7 major benefits)
- Challenges and solutions (6 challenges addressed)
- Implementation roadmap (8-13 weeks, 5 phases)
- Comparison table (current vs. proposed)
- Open questions for discussion
- Strong recommendation with rationale

### Index Updates
- Added to "Rendering Pipeline Architecture" section
- Added session log entry

## Key Technical Insights

1. **Converters are pure**: 2 of 3 current converters are pure text transformation with zero runtime dependencies

2. **Engine simplification**: Removing claimsFile/markdownForFile saves ~200-300 LOC from engines

3. **Performance wins**: Conversion caching (hash-based) + parallel conversion in projects

4. **LSP benefits**: Source maps enable "show original" vs "show converted qmd" views

5. **Two-stage jupyter conversion**: Only the first stage (ipynb → qmd) should be in converter. Second stage (qmd → transient notebook) remains engine-internal.

6. **File claiming coordination**: The key design challenge is coordinating converter+engine selection, solved by two-phase lookup

## Recommendations Summary

**Verdict**: ✅ **Strongly Recommended**

**DO**:
- Implement this design
- Start with 3 core converters (ipynb, percent, r-spin)
- Use adapter pattern during migration
- Begin with simple source maps (line numbers only)

**DON'T**:
- Block on pure Rust R spin converter (use subprocess initially)
- Over-engineer source mapping (YAGNI)

**Timeline**: 8-13 weeks across 5 phases (parallelizable)

## Open Questions Documented

1. Converter naming convention (extension vs. descriptive)?
2. Engine suggestion strength (hint vs. preference)?
3. Metadata merging strategy?
4. Error handling (fatal vs. fallback)?
5. Caching strategy (disk vs. memory)?

## User Feedback During Session

User provided excellent clarifications:
- R spin: "probably call into R for a while, but ideally we wouldn't"
- Scope: "I want to consider potential future syntaxes"
- Extensibility: "engines would need to register new surface syntaxes or provide their own implementations" → No, independent!
- Design goal: "reduce the set of responsibilities of the Quarto engines and arrive at a better factored solution"

User's framing helped narrow the design significantly.

## Next Steps

For next session:
1. Review design document for any concerns/questions
2. Refine API based on discussion
3. Consider starting implementation (Phase 1: Foundation)
4. Discuss open questions (naming, suggestions, caching)
5. Potentially prototype IpynbConverter to validate design

## Files Modified

- Created: `claude-notes/surface-syntax-converter-design.md`
- Updated: `claude-notes/00-INDEX.md` (added to pipeline architecture section + session log)
- Created: `claude-notes/session-logs/2025-10-13-surface-syntax-converter-design.md` (this file)
- **Updated** (after user feedback): Added "Future Converter Example: Rustdoc" section showing how .rs files with doc comments could convert to qmd for multi-format output (revealjs, PDF, typst). Demonstrates extensibility value proposition with concrete example (~150 lines of implementation sketch, before/after examples, design implications)

## Follow-up: Rustdoc Converter Example

**User request**: Add rustdoc as a future converter example to demonstrate extensibility.

**Rationale**: Rust source files with rustdoc comments (`///`, `//!`) could serve as Quarto source, enabling multi-format output (revealjs presentations, PDF, typst) beyond rustdoc's HTML-only output.

**Added to design document**:
- Complete converter implementation sketch (~100 lines)
- Before/after example (lib.rs → qmd → multiple formats)
- Value proposition: Source code as single source of truth for presentations/docs
- Design implications: Shows converter independence, source mapping, format flexibility
- Similar future converters: JSDoc, Python docstrings, Go godoc, Julia Pluto, Observable

**Key insight**: This pattern enables a whole category of "literate programming from source documentation" workflows. The converter architecture makes these trivial to add without touching core Quarto.

**Impact**: Strengthens the extensibility argument. Third parties could implement language-specific documentation converters independently.

## Time Breakdown

- Research & code reading: ~45 min
- Design & API iteration: ~45 min
- Documentation writing: ~30 min
- Follow-up (rustdoc example): ~15 min
- **Total**: ~2 hours 15 min

## Notes for Future Sessions

- This design complements explicit-workflow-design.md (extensible pipelines)
- Consider how converters fit into DAG workflow representation
- Source mapping is critical for both CLI error reporting AND LSP features
- The adapter pattern will be key to incremental migration
- Pure Rust R spin converter is good optimization target after MVP
