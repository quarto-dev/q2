# Session Log: October 11, 2025

## Session Focus

Analysis of five critical features for the Quarto Rust port:
1. Configuration merging with source location tracking
2. YAML tags support (particularly `!expr`)
3. Rust CLI organization patterns and architecture recommendations
4. Single document render pipeline (complete execution trace)
5. PDF rendering pipeline (LaTeX compilation and postprocessing)

## Work Completed

### 1. Configuration Merging Analysis

**Document**: `config-merging-analysis.md`

**Investigation**:
- Analyzed `mergeConfigs` function in quarto-cli (core/config.ts and config/metadata.ts)
- Identified 33 files using mergeConfigs across the codebase
- Studied hierarchical merging: default → extension → project → directory → document → CLI flags
- Examined special merge behaviors (array concatenation, Pandoc variants, unmergeable keys)

**Key findings**:
- Current TypeScript uses lodash's mergeWith (eager, deep merging)
- Arrays are concatenated and deduplicated (not replaced)
- Custom merge behaviors via mergeConfigsCustomized
- **Critical limitation**: Source location information is lost after merge

**Design recommendation**: ⭐ **AnnotatedParse Merge** (Strategy 4 of 4 evaluated)
- Eagerly merge AnnotatedParse trees while preserving source_info for each node
- Each merged value maintains origin via SourceInfo transformation chains
- Supports all merge semantics via customizer trait (MergeCustomizer)
- Naturally integrates with existing validation infrastructure
- Fully serializable for LSP caching

**Why this approach**:
- Leverages existing AnnotatedParse structure (already used for YAML parsing)
- Perfect source tracking (validation errors point to correct file:line:column)
- No new data structures needed
- Integrates with unified SourceInfo design from previous analysis

**Implementation timeline**: 6-8 weeks
- Week 1: Core merge implementation
- Week 2: Customizer system (special cases like Pandoc variants)
- Week 2-3: Integration with config loading and validation
- Week 3: Performance optimization
- Week 4: LSP integration and caching

### 2. YAML Tags Analysis

**Document**: `yaml-tags-analysis.md`

**Investigation**:
- Searched for `!expr` tag usage across quarto-cli codebase (20 files)
- Analyzed TypeScript YAML schema definitions (js-yaml-schema.ts, yaml.ts)
- Studied tree-sitter YAML parser tag handling
- Examined validation integration (ignoreExprViolations error handler)
- Researched yaml-rust2 tag support capabilities

**Key findings**:
- `!expr` tag marks R/Python/Julia expressions for runtime evaluation
- Used in cell options: `fig-cap: !expr paste("Air", "Quality")`
- Used for conditionals: `eval: !expr knitr::is_html_output()`
- TypeScript wraps tagged values as objects: `{ value: "...", tag: "!expr" }`
- Validation specifically ignores type errors for `!expr` values

**yaml-rust2 capabilities**:
```rust
pub enum Event {
    Scalar(String, TScalarStyle, usize, Option<Tag>),  // ✅ Tag support
    SequenceStart(usize, Option<Tag>),                 // ✅ Tag support
    MappingStart(usize, Option<Tag>),                  // ✅ Tag support
    // ...
}

pub struct Tag {
    pub handle: String,  // e.g., "!"
    pub suffix: String,  // e.g., "expr"
}
```

**Design recommendation**: Extend AnnotatedParse with tag field
```rust
pub struct AnnotatedParse {
    pub start: usize,
    pub end: usize,
    pub result: YamlValue,
    pub kind: YamlKind,
    pub source_info: SourceInfo,
    pub components: Vec<AnnotatedParse>,
    pub tag: Option<YamlTag>,  // ← NEW
    pub errors: Option<Vec<YamlError>>,
}
```

**Tagged value representation**: Match TypeScript for compatibility
- Input: `fig-cap: !expr paste("A", "B")`
- Output: `{ "value": "paste(\"A\", \"B\")", "tag": "!expr" }`

**Validation integration**:
- Error handler checks for `{ tag: "!expr", value: ... }` structure
- Skips type validation for tagged expressions
- Preserves tags through configuration merging

**Implementation timeline**: 3-4 weeks
- Week 1: AnnotatedParse extension and parser integration
- Week 2: Validation and merging updates
- Week 3: Integration testing with real Quarto docs
- Week 4: Polish and edge cases

### 3. Rust CLI Organization Patterns

**Document**: `rust-cli-organization-patterns.md`

**Investigation**:
- Surveyed 6 major Rust CLI tools with multiple subcommands
- Analyzed source code organization of: cargo, rustup, just, turborepo, git-cliff, ripgrep
- Identified 4 distinct organizational patterns
- Researched clap usage patterns and configuration management strategies
- Studied Casey Rodarmor's blog on "just" architecture
- Examined cargo contributor guide on subcommand organization
- Reviewed turborepo's Go → Rust migration strategy

**Programs analyzed**:

1. **cargo** - Rust package manager
   - Commands: build, test, run, check, add, update, publish, etc.
   - Pattern: Commands directory + Ops separation
   - Structure: `src/bin/cargo/commands/` for CLI, `src/cargo/ops/` for logic

2. **rustup** - Rust toolchain installer
   - Commands: update, install, default, toolchain, target, component
   - Pattern: Standard flat Rust structure
   - Structure: `src/cli/` with command modules

3. **just** - Command runner by Casey Rodarmor
   - ~70 modules, 11,000 LOC
   - Pattern: Flat module structure with centralized imports
   - Structure: All modules in `src/`, clear compilation pipeline
   - Philosophy: Flat is easier to navigate than deep hierarchies

4. **turborepo** - Monorepo build system
   - Commands: run, prune, generate, login, link, init, daemon
   - Pattern: Workspace with thin CLI wrapper
   - Structure: 20+ crates under `crates/`, main CLI delegates to `turborepo-lib`
   - Migration: Go → Rust using FFI during transition

5. **git-cliff** - Changelog generator
   - Pattern: Simple workspace (CLI + core)
   - Structure: `git-cliff/` (CLI) + `git-cliff-core/` (library)

6. **ripgrep** - Fast search tool
   - Pattern: Highly modular workspace
   - Structure: Separate crates for grep, ignore, globset, termcolor
   - Evolution: Started monolithic, evolved to reusable components

**Four organizational patterns identified**:

**Pattern 1: Commands Directory (Cargo-Style)**
```
src/bin/[tool]/
├── main.rs
└── commands/
    ├── command1.rs
    └── command2.rs
[tool]/ops/
```
- Best for: 10+ subcommands, clear separation of concerns
- Example: cargo

**Pattern 2: Flat Module Structure (Just-Style)**
```
src/
├── main.rs
├── config.rs
├── subcommand.rs
└── [feature modules]
```
- Best for: Medium complexity, single developer/small team
- Example: just (scales to ~70 modules)

**Pattern 3: Workspace with Thin CLI Wrapper (Turborepo-Style)**
```
crates/
├── [tool]/          # CLI binary (thin)
├── [tool]-lib/      # Core logic
├── [tool]-feature1/ # Modular features
└── [tool]-feature2/
```
- Best for: Large complex tools, reusable components
- Example: turborepo, ripgrep

**Pattern 4: Commands + Ops Separation (Cargo Hybrid)**
```
src/bin/[tool]/commands/  # CLI parsing
[tool]/ops/               # Implementation
```
- Best for: Tools also used as libraries
- Example: cargo

**Common technical choices**:
- **CLI parsing**: clap v4.x with derive macros (universal)
- **Config management**: Context struct pattern (Kevin K's blog)
- **Error handling**: anyhow (apps), thiserror (custom errors)
- **Logging**: tracing (modern), env_logger (simple)
- **Config files**: TOML (most common in Rust)

**Design recommendation**: ⭐ **Pattern 3 + Pattern 1** (Workspace + Commands Directory)

```
crates/
├── kyoto/                    # Main CLI (commands directory inside)
│   └── src/
│       ├── main.rs
│       └── commands/         # 17 command files
├── kyoto-core/               # Core rendering
├── kyoto-engines/            # Engine system
├── kyoto-filters/            # Filter system
├── kyoto-handlers/           # Diagram handlers
├── kyoto-formats/            # Output formats
├── kyoto-config/             # Configuration
├── kyoto-yaml/               # YAML infrastructure
├── kyoto-lsp/                # LSP server
├── kyoto-extensions/         # Extension system
└── kyoto-util/               # Shared utilities
```

**Rationale for this structure**:
1. **17+ subcommands** need clear organization → Commands directory
2. **Parallel compilation** → Workspace (10+ crates compile in parallel)
3. **Reusable components** → Separate crates (kyoto-yaml, kyoto-engines)
4. **Third-party engines** → Trait-based extensibility in separate crate
5. **Incremental migration** → Port one crate at a time
6. **Testing isolation** → Test engines without CLI

**Why not other patterns**:
- Pattern 2 (Flat): Too many commands (17+), no ownership boundaries
- Single crate: Slower compilation, harder to reuse, can't parallelize
- Pattern 4 only: Single crate gets huge (100k+ LOC), no parallel builds

**Implementation phases**:
1. **Phase 1**: kyoto/ + kyoto-core/ + kyoto-util/ (MVP)
2. **Phase 2**: + kyoto-engines/ + kyoto-config/ + kyoto-yaml/
3. **Phase 3**: + kyoto-filters/ + kyoto-handlers/ + kyoto-formats/
4. **Phase 4**: + kyoto-lsp/ + kyoto-extensions/

**Alignment with user requirements**:
- ✅ Engine extensibility: Trait in kyoto-engines crate
- ✅ Parallelization: rayon for multiple files, async for execution
- ✅ Pipeline flexibility: Builder pattern with pluggable stages

### 4. Single Document Render Pipeline Analysis

**Document**: `single-document-render-pipeline.md`

**Investigation**:
- Traced complete execution path for `quarto render doc.qmd`
- Analyzed ~50+ TypeScript modules involved in rendering
- Documented 10 major pipeline stages
- Studied engine selection algorithm (jupyter, knitr, markdown, julia)
- Examined metadata merging across 5 sources
- Analyzed Pandoc execution and filter chain
- Documented HTML postprocessing system
- Studied freeze/thaw caching system

**10 Pipeline Stages Identified:**

1. **CLI Entry** (`cmd.ts`)
   - Argument parsing with cliffy
   - Flag extraction (execute, cache, freeze, etc.)
   - Service creation (temp, notebook, extension)

2. **Render Coordinator** (`render-shared.ts`)
   - YAML validation initialization
   - Project context detection
   - Single file project context creation

3. **File Rendering Setup** (`render-files.ts`)
   - Temp context management
   - Lifetime-based resource management
   - Default pandoc renderer setup

4. **Context Creation** (`render-contexts.ts`) **[Most Complex]**
   - Engine and target resolution
   - Format resolution (html, pdf, etc.)
   - Metadata merging from 5 sources
   - Pre-engine language cell handlers

5. **Engine Selection** (`engine.ts`)
   - 4 registered engines: knitr, jupyter, markdown, julia
   - Extension-based claims (.ipynb → jupyter, .Rmd → knitr)
   - Content-based selection (YAML inspection, code block languages)

6. **YAML Validation** (`validate-document.ts`)
   - Schema loading from `resources/schema/`
   - Document and format-specific validation
   - Expression tag handling (`!expr`)

7. **Engine Execution** (`renderExecute`)
   - Freeze/thaw check for caching
   - Engine-specific execution:
     * Jupyter: Start kernel, execute cells, convert outputs
     * Knitr: Shell to R, run knitr::knit()
     * Markdown: Pass through
   - Freeze results to `_freeze/` directory

8. **Language Cell Handlers** (`handleLanguageCells`)
   - Mapped diff for source tracking
   - OJS compilation (Observable JavaScript)
   - Diagram rendering (mermaid, graphviz)
   - Dependency injection

9. **Pandoc Conversion** (`pandoc.ts`) **[Largest Stage]**
   - Merge engine results
   - Generate defaults file
   - Resolve format extras (filters, postprocessors, dependencies)
   - Template processing
   - Filter assembly (pre → user → crossref → quarto → post → format-specific)
   - Execute pandoc with filters

10. **Postprocessing & Finalization** (`render.ts`)
    - Engine postprocessing
    - HTML postprocessors (Bootstrap, Quarto, KaTeX, etc.)
    - Generic postprocessors
    - Self-contained output processing
    - Recipe completion (latexmk for PDF)
    - Cleanup

**Key Findings:**

**Data Flow:**
```
User Document
  ↓
QMD Parser (target creation)
  ↓
Engine Execution (code → outputs)
  ↓
Language Handlers (OJS, diagrams)
  ↓
Pandoc + Filters (markdown → HTML/PDF)
  ↓
Postprocessors (HTML manipulation)
  ↓
Final Output
```

**Metadata Flow:**
```
Default Format Metadata
  ↓ merge
Project Metadata
  ↓ merge
Directory Metadata (_metadata.yml)
  ↓ merge
Document Metadata (YAML front matter)
  ↓ merge
CLI Flags (--metadata, -M)
  ↓
Final Metadata
```

**Engine Selection Algorithm:**
1. Try file extension claims (.ipynb, .Rmd)
2. For .qmd/.md: check YAML for engine declaration
3. Check code block languages (r → knitr, python → jupyter)
4. Check for non-handler languages → jupyter
5. Default to markdown engine

**Format Extras (Plugin Architecture):**
- Formats inject behavior via `formatExtras()` function
- Returns: filters, postprocessors, dependencies, metadata
- Example: HTML format adds Bootstrap, Quarto CSS/JS, HTML postprocessors

**HTML Postprocessing Chain:**
1. Parse HTML with deno-dom
2. Run postprocessors (Bootstrap, Quarto, KaTeX, resource discovery)
3. Run finalizers
4. Write modified HTML

**Timing Estimates:**
- Context creation: 50-500ms
- Engine execution: 100ms-60s+ (dominates for computational docs)
- Pandoc: 500ms-60s (significant for large docs)
- Postprocessing: 100ms-2s
- Total: 1-120s (highly variable)

**Critical for Rust Port:**

1. **Metadata Merging**: Use AnnotatedParse merge strategy (preserves source info)
2. **Engine Trait**: Extensible engine system with registration
3. **MappedString**: Source tracking through transformations
4. **HTML Manipulation**: Use html5ever + scraper
5. **Workspace Structure**: Modular crates (kyoto-engines, kyoto-filters, etc.)

**Design Patterns Identified:**

1. **Progressive Refinement**: Metadata merged through pipeline
2. **Source Tracking**: MappedString preserves locations
3. **Plugin Architecture**: FormatExtras for extensibility
4. **Recipe Pattern**: Output-specific behavior encapsulation
5. **Lifetime Pattern**: Automatic resource cleanup

## Technical Decisions Made

1. **Configuration Merging Strategy**: AnnotatedParse merge (eager with source tracking)
   - Rationale: Leverages existing infrastructure, perfect source tracking, serializable

2. **YAML Tags Support**: Full tag support via yaml-rust2 Event API
   - Rationale: Library provides complete primitives, compatible with TypeScript behavior

3. **CLI Architecture**: Workspace architecture (turborepo-style) + Commands directory (cargo-style)
   - Rationale: 17+ subcommands need clear organization; workspace enables parallel compilation, reusable crates, and third-party engine extensibility; proven at scale by cargo and turborepo

## Documentation Updates

- Created `config-merging-analysis.md` (comprehensive analysis with 4 strategies evaluated)
- Created `yaml-tags-analysis.md` (complete tag support design)
- Created `rust-cli-organization-patterns.md` (survey of 6 major Rust CLIs with architecture recommendations)
- Created `single-document-render-pipeline.md` (complete trace of `quarto render doc.qmd` through 10 stages, 50+ modules, with Rust port implications)
- Updated `00-INDEX.md` with new documents and technical decisions

## Key Insights

### Configuration Merging
- The problem is not just merging values, but preserving their provenance
- AnnotatedParse is the natural vehicle for this (already has source_info)
- Validation can report errors at correct source locations after merge
- Special merge behaviors (variants, disableable arrays) handled via trait

### YAML Tags
- yaml-rust2 provides everything needed (no limitations)
- Tags preserved at two levels: AnnotatedParse.tag field + wrapped value structure
- Minimal overhead: < 1 KB per document, negligible performance impact
- Extensible design allows future tags beyond !expr

### CLI Organization
- Workspace architecture is optimal for complex tools like Quarto
- Commands directory pattern (cargo-style) scales to 17+ subcommands
- Separate crates enable parallel compilation (10+ crates → faster builds)
- Modular design (kyoto-engines, kyoto-filters) enables third-party contributions
- Thin CLI wrapper + feature crates proven by turborepo and ripgrep
- clap v4 with derive macros is de facto standard for Rust CLIs
- Context struct pattern provides single source of truth for configuration

### Single Document Rendering
- Pipeline is fundamentally a transformation chain with metadata merging
- 10 stages from CLI to final output, involving 50+ modules
- Engine execution and Pandoc conversion are the bottlenecks (seconds to minutes)
- Everything else is sub-second (context creation, validation, postprocessing)
- Metadata flows through entire pipeline, refined at each stage
- Source tracking (MappedString) is critical for error reporting
- Format extras provide plugin architecture for extensibility
- HTML postprocessing uses DOM manipulation (deno-dom in TypeScript)
- Freeze/thaw system caches expensive computations
- Engine trait pattern enables extensibility (knitr, jupyter, markdown, julia)

## Integration with Previous Work

All four analyses build on previous architectural decisions:

**1. Config merging and YAML tags** build on **unified SourceInfo design** (unified-source-location-design.md):
- SourceInfo tracks positions through transformations (substring, concat, etc.)
- AnnotatedParse uses SourceInfo for YAML node positions
- Merge creates new AnnotatedParse with SourceInfo chains preserved
- Tags stored in AnnotatedParse alongside SourceInfo

**2. CLI architecture** provides the structure to organize all previous work:
- `kyoto-yaml/` crate contains AnnotatedParse + MappedString + validation
- `kyoto-config/` crate contains config merging with SourceInfo preservation
- `kyoto-lsp/` crate builds on serializable SourceInfo for caching
- `kyoto-core/` orchestrates pipeline using all infrastructure crates
- `kyoto/` thin CLI wrapper delegates to core implementation

**3. Render pipeline** unifies all previous designs:
- Uses AnnotatedParse merge for configuration (Stage 4)
- Uses YAML tags for expression handling (Stage 6, 7)
- Organized in workspace structure with modular crates (all stages)
- MappedString tracks sources through transformations (Stage 8)
- Engine trait provides extensibility (Stage 5, 7)
- Format extras enable plugin architecture (Stage 9)

**Complete dependency chain**:
```
quarto-markdown (AST with SourceInfo)
    ↓
Unified SourceInfo (in kyoto-util/)
    ↓
AnnotatedParse + Tags (in kyoto-yaml/)
    ↓
Config Merging (in kyoto-config/)
    ↓
Validation + Schemas (in kyoto-yaml/)
    ↓
Pipeline (in kyoto-core/)
    ↓
Commands (in kyoto/src/commands/)
    ↓
CLI Entry Point (in kyoto/src/main.rs)
```

Parallel to this:
```
Engines (kyoto-engines/) ─┐
Filters (kyoto-filters/) ─┼→ Pipeline (kyoto-core/)
Handlers (kyoto-handlers/) ─┘
```

### 5. PDF Rendering Analysis

**Document**: `single-document-render-pipeline.md` (PDF section added)

**Investigation**:
- Analyzed PDF format definition (`format-pdf.ts`)
- Studied LaTeX output recipe system (`output-tex.ts`, `latexmk.ts`)
- Traced PDF compilation process (`pdf.ts`)
- Examined LaTeX postprocessor (line-by-line text manipulation)
- Documented multi-run compilation logic
- Analyzed auto-installation of missing LaTeX packages

**Key findings**:

**Pipeline Divergence**: Stages 1-8 are identical to HTML, but stages 9-10 differ significantly:
- **Stage 9**: Pandoc outputs `.tex` file (not `.html`)
- **Stage 9.5**: LaTeX postprocessor modifies `.tex` line-by-line
- **Stage 10**: Recipe's `complete()` method runs latexmk to compile PDF

**PDF Format Defaults** (vs HTML):
```typescript
{
  execute: {
    [kFigWidth]: 5.5,      // Narrower than HTML (7)
    [kFigHeight]: 3.5,     // Smaller than HTML (5)
    [kFigFormat]: "pdf",   // Vector graphics
    [kFigDpi]: 300,        // High DPI for print
  },
  render: {
    [kLatexAutoMk]: true,      // Use quarto's latexmk
    [kLatexAutoInstall]: true, // Auto-install missing packages
    [kLatexClean]: true,       // Remove auxiliary files
    [kLatexMinRuns]: 1,
    [kLatexMaxRuns]: 10,
  }
}
```

**LaTeX Postprocessor** (9 passes):
1. Sidecaption processing
2. Callout float handling
3. Table column margins
4. GUID replacement (cross-references)
5. Bibliography processing (biblatex, natbib, citeproc)
6. Margin citations
7. Footnote → sidenote conversion
8. Code annotation processing
9. Caption footnote extraction

Unlike HTML (DOM manipulation), PDF uses **line-by-line text manipulation** of the `.tex` file.

**Multi-Stage Compilation**:
```
Markdown
  → Pandoc → .tex file
  → LaTeX postprocessor → modified .tex file
  → lualatex run 1 → .aux, .log, .pdf (draft)
  → bibtex/biber → .bbl (bibliography)
  → makeindex → .ind (index)
  → lualatex run 2 → .pdf (with refs)
  → lualatex run 3 → .pdf (stable cross-refs)
  → ...
  → lualatex run N → .pdf (final)
```

**Recompilation Detection**: Parse `.log` file for indicators:
- "Rerun to get cross-references right"
- "Rerun to get citations correct"
- "There were undefined references"
- "Label(s) may have changed"

**Auto-Installation**: If compilation fails due to missing packages:
1. Update tlmgr (TeX package manager)
2. Update existing packages
3. Parse log file to find missing packages
4. Install packages via TinyTeX
5. Retry compilation

**Auxiliary File Cleanup**: Remove 18+ auxiliary files:
- `.aux`, `.log`, `.toc`, `.lof`, `.lot` (navigation)
- `.bcf`, `.blg`, `.bbl` (bibliography)
- `.idx`, `.ind`, `.ilg` (index)
- `.fls`, `.out`, `.nav`, `.snm`, `.vrb`, etc.

**Implications for Rust Port**:

1. **LaTeX Postprocessing** needs string processing (not DOM):
   ```rust
   pub fn pdf_latex_postprocessor(tex_path: &Path, format: &Format) -> Result<()> {
     let mut lines: Vec<String> = read_lines(tex_path)?;
     lines = process_sidecaptions(lines);
     lines = process_bibliography(lines, format);
     lines = convert_footnotes_to_sidenotes(lines);
     write_lines(tex_path, lines)?;
     Ok(())
   }
   ```

2. **PDF Recipe** must handle multi-run compilation:
   ```rust
   impl OutputRecipe for PdfRecipe {
     async fn complete(&self, options: PandocOptions) -> Result<PathBuf> {
       let pdf_path = generate_pdf(LatexmkOptions {
         input: &self.tex_file,
         engine: &self.engine,
         min_runs: self.min_runs,
         max_runs: self.max_runs,
       }).await?;
       Ok(pdf_path)
     }
   }
   ```

3. **Log File Parsing** for recompilation detection:
   ```rust
   pub fn needs_recompilation(log_path: &Path) -> Result<bool> {
     let log_text = std::fs::read_to_string(log_path)?;
     const INDICATORS: &[&str] = &[
       "Rerun to get cross-references right",
       "There were undefined references",
     ];
     Ok(INDICATORS.iter().any(|ind| log_text.contains(ind)))
   }
   ```

4. **Package Manager Integration** with TinyTeX:
   ```rust
   pub struct TinyTexPackageManager {
     tinytex_bin: PathBuf,
   }

   impl PackageManager for TinyTexPackageManager {
     async fn install_packages(&self, packages: &[String]) -> Result<bool> {
       // Call tlmgr to install missing packages
     }
   }
   ```

5. **Auxiliary File Cleanup**:
   ```rust
   pub fn cleanup_latex_artifacts(working_dir: &Path, stem: &str) -> Result<()> {
     const AUX_EXTENSIONS: &[&str] = &[
       "log", "aux", "idx", "ind", "ilg", "toc", "lof", "lot",
       "bcf", "blg", "bbl", "fls", "out", "nav", "snm", "vrb"
     ];
     // Remove all matching files
   }
   ```

**Key Difference Summary**:
| Aspect | HTML | PDF |
|--------|------|-----|
| Pandoc Output | `.html` | `.tex` |
| Postprocessing | DOM manipulation (parse HTML tree) | Line-by-line text processing |
| Compilation | Single pass (Pandoc only) | Multi-pass (latexmk with 1-10 runs) |
| Dependencies | CSS/JS libraries | LaTeX packages (auto-installed) |
| Bibliography | Pandoc citeproc | bibtex/biber (separate tool) |
| Auxiliary Files | Minimal | 18+ files (.aux, .log, .toc, .bbl, etc.) |

## Estimates Updated

- Configuration merging: 6-8 weeks
- YAML tags: 3-4 weeks
- Combined: ~10-12 weeks (some parallel work possible)

These integrate with the previously estimated YAML work:
- MappedString + YAML: 6-8 weeks (from earlier analysis)
- Total YAML system: ~16-20 weeks for complete implementation

## Next Steps (When Work Resumes)

With architecture now defined, implementation can begin:

1. **Create workspace structure** (1-2 days)
   - Set up Cargo workspace with initial crates (kyoto/, kyoto-core/, kyoto-util/)
   - Configure workspace dependencies and build settings
   - Create basic CLI with clap derive and commands directory
   - Validate workspace compiles and can run basic commands

2. **Foundation implementation** (choose one to start):
   - **Option A**: Begin MappedString/SourceInfo implementation (foundation for everything)
   - **Option B**: Prototype AnnotatedParse merge (validate design assumptions)
   - **Option C**: Prototype tag support in parser (prove yaml-rust2 integration)
   - **Option D**: Implement 2-3 simple commands to validate CLI pattern

3. **Parallel work** (once foundation is laid):
   - Core infrastructure (SourceInfo, MappedString, AnnotatedParse)
   - CLI commands (can port simple ones while infrastructure develops)
   - Engine system (design trait, implement markdown engine)
   - Config system (loading, merging with source tracking)

### 6. Website Project Rendering Analysis

**Document**: `website-project-rendering.md`

**Investigation**:
- Analyzed project detection and type system (`project-context.ts`, `register.ts`)
- Studied website project type definition (`website.ts`)
- Traced navigation system implementation (`website-navigation.ts` - 1628 lines)
- Examined search indexing system (`website-search.ts` - 724 lines)
- Documented sitemap generation (`website-sitemap.ts` - 190 lines)
- Analyzed listing system for blog/gallery pages
- Traced multi-file rendering coordination

**Key findings**:

**Website projects add 2 new stages to the single-document pipeline**:
- **Stage 0**: Project Detection (searches up directory tree for `_quarto.yml`)
- **Stage 1**: Pre-Render Hook (builds global navigation state before any files render)
- **Stages 2-10**: Same as single-document rendering (references `single-document-render-pipeline.md`)
- **Stage 11**: Post-Render Hooks (generate sitemap.xml, search.json, RSS feeds, redirect aliases)

**ProjectType System**:
```typescript
export const websiteProjectType: ProjectType = {
  type: "website",
  libDir: "site_libs",
  outputDir: "_site",

  // Run once before any files render
  preRender: async (context) => {
    await initWebsiteNavigation(context);
  },

  // Run for each file during rendering
  formatExtras: async (project, source, flags, format, services) => {
    // Inject navigation, search, breadcrumbs
    return { html: { bodyEnvelope, dependencies, postprocessors } };
  },

  // Run once after all files render
  postRender: async (context, incremental, outputFiles) => {
    await updateSitemap(context, outputFiles, incremental);
    await updateSearchIndex(context, outputFiles, incremental);
    completeListingGeneration(context, outputFiles);
    await updateAliases(context, outputFiles);
  }
};
```

**Navigation State (Global Singleton)**:
```typescript
{
  navbar: NavigationItem[],     // Top navigation bar
  sidebars: Map<string, Sidebar>, // Sidebars by id
  footer: FooterConfig,          // Footer content
  pageNavigation: {              // Previous/next page links
    prevPage?: NavigationItem,
    nextPage?: NavigationItem
  }
}
```

**Format Extras Injection** (per-file during rendering):
- Navbar HTML (rendered from EJS template)
- Sidebar HTML (expanded to show current page)
- Breadcrumbs (derived from sidebar hierarchy)
- Search dependencies (Fuse.js, search UI)
- Body envelope (HTML wrapper around page content)
- Navigation styles and scripts

**Search Indexing** (section-level granularity):
```typescript
{
  objectID: "page.html#section-id",
  href: "page.html#section-id",
  title: "Page Title",
  section: "Section Title",
  text: "Section content text...",
  crumbs: ["Home", "Category", "Page Title"]
}
```

**Sitemap Generation** (XML with lastmod):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/page.html</loc>
    <lastmod>2025-10-11</lastmod>
  </url>
</urlset>
```

**Multi-File Rendering Flow**:
```
1. Project Detection → Find _quarto.yml
2. Pre-Render Hook → Build global navigation state
3. For each input file:
   a. Resolve format
   b. Inject format extras (navigation, search)
   c. Execute stages 2-10 (same as single document)
4. Post-Render Hooks → Generate sitemap, search, listings
```

**Incremental Rendering**:
- Sitemap: Diff existing entries, update only changed files
- Search: Diff existing entries, update only changed sections
- Listings: Regenerate RSS feeds for changed categories
- Aliases: Update redirect pages for changed files

**Implications for Rust Port**:

1. **ProjectType Trait**:
```rust
#[async_trait]
pub trait ProjectType: Send + Sync {
    fn type_name(&self) -> &str;
    fn lib_dir(&self) -> &str;
    fn output_dir(&self) -> &str;

    async fn pre_render(&self, context: &ProjectContext) -> Result<()> {
        Ok(()) // Default: no-op
    }

    async fn format_extras(
        &self,
        project: &ProjectContext,
        source: &Path,
        format: &Format,
    ) -> Result<FormatExtras> {
        Ok(FormatExtras::default())
    }

    async fn post_render(
        &self,
        context: &ProjectContext,
        incremental: bool,
        output_files: &[ProjectOutputFile],
    ) -> Result<()> {
        Ok(())
    }
}
```

2. **Website ProjectType Implementation**:
```rust
pub struct WebsiteProjectType {
    navigation_state: Arc<RwLock<NavigationState>>,
}

#[async_trait]
impl ProjectType for WebsiteProjectType {
    fn type_name(&self) -> &str { "website" }
    fn lib_dir(&self) -> &str { "site_libs" }
    fn output_dir(&self) -> &str { "_site" }

    async fn pre_render(&self, context: &ProjectContext) -> Result<()> {
        let nav_state = init_website_navigation(context).await?;
        *self.navigation_state.write().await = nav_state;
        Ok(())
    }

    async fn format_extras(
        &self,
        project: &ProjectContext,
        source: &Path,
        format: &Format,
    ) -> Result<FormatExtras> {
        let nav_state = self.navigation_state.read().await;
        let href = compute_href(project, source);

        let sidebar = expanded_sidebar(&href, &nav_state);
        let breadcrumbs = compute_breadcrumbs(&href, &sidebar);

        let body_envelope = render_navigation_envelope(
            &nav_state.navbar,
            &sidebar,
            &breadcrumbs,
        )?;

        Ok(FormatExtras {
            html: HtmlExtras {
                body_envelope: Some(body_envelope),
                dependencies: vec![search_dependency()],
                postprocessors: vec![],
            },
            ..Default::default()
        })
    }

    async fn post_render(
        &self,
        context: &ProjectContext,
        incremental: bool,
        output_files: &[ProjectOutputFile],
    ) -> Result<()> {
        update_sitemap(context, output_files, incremental).await?;
        update_search_index(context, output_files, incremental).await?;
        complete_listing_generation(context, output_files).await?;
        update_aliases(context, output_files).await?;
        ensure_index_page(context).await?;
        Ok(())
    }
}
```

3. **Search Index Generation** (HTML parsing with scraper):
```rust
pub async fn update_search_index(
    context: &ProjectContext,
    output_files: &[ProjectOutputFile],
    incremental: bool,
) -> Result<()> {
    let mut search_docs = Vec::new();

    for output_file in output_files {
        let html = std::fs::read_to_string(&output_file.file)?;
        let document = scraper::Html::parse_document(&html);

        let title_selector = Selector::parse("h1.title").unwrap();
        let title = document.select(&title_selector)
            .next()
            .map(|el| el.text().collect::<String>());

        let section_selector = Selector::parse("section.level2, section.footnotes").unwrap();
        for section in document.select(&section_selector) {
            let section_title = section.select(&Selector::parse("h2").unwrap())
                .next()
                .map(|el| el.text().collect::<String>());

            let text = section.text().collect::<Vec<_>>().join(" ");
            let href = format!("{}#{}", output_file.href, section.value().attr("id").unwrap());

            search_docs.push(SearchDoc {
                object_id: href.clone(),
                href,
                title: title.clone(),
                section: section_title,
                text,
                crumbs: compute_crumbs(&output_file.href, context),
            });
        }
    }

    let search_json = serde_json::to_string(&search_docs)?;
    std::fs::write(context.output_dir().join("search.json"), search_json)?;
    Ok(())
}
```

4. **Sitemap Generation** (XML serialization):
```rust
pub async fn update_sitemap(
    context: &ProjectContext,
    output_files: &[ProjectOutputFile],
    incremental: bool,
) -> Result<()> {
    let base_url = context.config().website.base_url
        .as_ref()
        .ok_or_else(|| anyhow!("website.base-url required for sitemap"))?;

    let mut urlset = if incremental {
        read_existing_sitemap(context)?
    } else {
        Vec::new()
    };

    for output_file in output_files {
        let loc = format!("{}{}", base_url, output_file.href);
        let lastmod = file_modified_date(&output_file.file)?;

        urlset.push(SitemapEntry { loc, lastmod });
    }

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
{}
</urlset>"#,
        urlset.iter()
            .map(|entry| format!("  <url>\n    <loc>{}</loc>\n    <lastmod>{}</lastmod>\n  </url>",
                entry.loc, entry.lastmod))
            .collect::<Vec<_>>()
            .join("\n")
    );

    std::fs::write(context.output_dir().join("sitemap.xml"), xml)?;
    Ok(())
}
```

**Timing Estimates** (100-file website):
- Project detection: 10-50ms (filesystem search)
- Pre-render hook (navigation init): 100-500ms (parse all files for navigation)
- Per-file rendering: 100-file website × (1-120s per file) = 100-12000s total
- Post-render hooks:
  - Sitemap: 50-200ms (incremental) / 500ms-2s (full)
  - Search: 500ms-5s (HTML parsing, section extraction)
  - Listings: 100ms-1s (RSS generation)
  - Aliases: 50-200ms (redirect page generation)
- Total project render: 2-200 minutes (highly variable, dominated by individual file rendering)

**Key Difference from Single Document**:
| Aspect | Single Document | Website Project |
|--------|-----------------|-----------------|
| Context | File-level only | Project-level context |
| Navigation | None | Global navbar/sidebar state |
| Search | N/A | Indexed at section level |
| Sitemap | N/A | XML with all pages + lastmod |
| Coordination | Independent file | Shared navigation state across all files |
| Post-processing | File-local only | Cross-file (search index, sitemap) |

**Critical for Rust Port**:
- ProjectType trait with async hooks (pre_render, format_extras, post_render)
- Global navigation state shared across all file renders (Arc<RwLock<NavigationState>>)
- HTML parsing with scraper (for search indexing)
- XML generation (for sitemap)
- EJS-compatible templating (tera recommended) for navigation templates
- Incremental rendering support (diff existing sitemap/search entries)
- Project context detection (walk up directory tree for _quarto.yml)

### 7. Book Project Rendering Analysis

**Document**: `book-project-rendering.md`

**Investigation**:
- Analyzed book project type inheritance from website (`book.ts`)
- Studied chapter management system (`book-chapters.ts`, `book-config.ts`)
- Traced dual rendering modes: multi-file (HTML) vs single-file (PDF/EPUB/DOCX) (`book-render.ts`)
- Examined custom Pandoc renderer implementation
- Documented cross-reference and bibliography post-render fixups
- Analyzed chapter numbering and title formatting

**Key findings**:

**Book projects inherit from website projects**:
```typescript
export const bookProjectType: ProjectType = {
  type: "book",
  inheritsType: websiteProjectType.type,  // KEY: Inheritance

  preRender: bookPreRender,  // Delegates to website
  postRender: bookPostRender,  // Extends website
  formatExtras: async (...) => {
    // Book-specific extras
    let extras = { /* book defaults */ };

    if (isHtmlOutput(format)) {
      // INHERIT website extras
      const websiteExtras = await websiteProjectType.formatExtras!(...);
      extras = mergeConfigs(extras, websiteExtras);
    }

    return extras;
  },
};
```

**Dual rendering modes**:

1. **Multi-file mode** (HTML, AsciiDoc):
   - Each chapter renders as separate HTML file
   - Similar to website pages
   - Post-render fixups for cross-references and bibliography

2. **Single-file mode** (PDF, EPUB, DOCX):
   - All chapters accumulated during execution
   - Merged into single markdown document
   - Single Pandoc render of unified document
   - Cross-references and bibliography handled by Pandoc

**Chapter management**:
```typescript
interface BookRenderItem {
  type: "index" | "chapter" | "appendix" | "part";
  depth: number;
  text?: string;        // Part titles
  file?: string;        // Chapter file paths
  number?: number;      // Chapter numbers (1, 2, 3... or undefined)
}

// Example for 3-chapter book with appendix:
[
  { type: "index", file: "index.md", depth: 0, number: undefined },
  { type: "chapter", file: "intro.md", depth: 0, number: 1 },
  { type: "chapter", file: "methods.md", depth: 0, number: 2 },
  { type: "appendix", text: "Appendices", depth: 0 },
  { type: "chapter", file: "appendix-a.md", depth: 1, number: 1 },  // Letter A
]
```

**Custom Pandoc renderer**:
```typescript
export function bookPandocRenderer(
  options: RenderOptions,
  project: ProjectContext,
): PandocRenderer {
  return {
    onRender: async (format: string, file: ExecutedFile, quiet: boolean) => {
      if (isMultiFileBookFormat(file.context.format)) {
        // MULTI-FILE: Render each chapter immediately
        const chapterInfo = chapterInfoForInput(project, fileRelative);
        file.recipe.format = withChapterMetadata(
          file.recipe.format,
          partitioned.headingText,
          partitioned.headingAttr,
          chapterInfo,  // { number: 2, labelPrefix: "2" }
        );
        await renderPandoc(file, quiet);

      } else {
        // SINGLE-FILE: Accumulate for later merge
        executedFiles[format].push(file);
      }
    },

    onComplete: async (error?: boolean) => {
      // For single-file formats: merge and render
      for (const format of Object.keys(executedFiles)) {
        const files = executedFiles[format];
        const mergedFile = await mergeExecutedFiles(project, options, files);
        const rendered = await renderPandoc(mergedFile, quiet);
        renderedFiles.push(rendered);
      }
      return { files: renderedFiles };
    },
  };
}
```

**Chapter title formatting**:
```typescript
export function formatChapterTitle(
  format: Format,
  label: string,
  info?: ChapterInfo,
) {
  if (!info) {
    return label;  // Unnumbered
  }

  if (info.appendix) {
    // "Appendix A — Technical Details"
    return `Appendix ${info.labelPrefix} — ${label}`;
  } else {
    // "[2]{.chapter-number}  [Introduction]{.chapter-title}"
    return `[${info.labelPrefix}]{.chapter-number}\u00A0 [${label}]{.chapter-title}`;
  }
}
```

**Post-render fixups** (multi-file HTML only):

1. **Cross-reference fixup**:
```typescript
export async function bookCrossrefsPostRender(
  context: ProjectContext,
  websiteFiles: WebsiteProjectOutputFile[],
) {
  // Build map of all IDs to their file locations
  const xrefMap: Map<string, { file: string; href: string }> = new Map();
  for (const file of websiteFiles) {
    const doc = parseHtml(file.file);
    doc.querySelectorAll("[id]").forEach(element => {
      xrefMap.set(element.id, {
        file: file.file,
        href: `${file.href}#${element.id}`,
      });
    });
  }

  // Fix up links that point to other files
  for (const file of websiteFiles) {
    const doc = parseHtml(file.file);
    doc.querySelectorAll("a[href^='#']").forEach(link => {
      const id = link.href.substring(1);
      const xref = xrefMap.get(id);
      if (xref && xref.file !== file.file) {
        link.href = xref.href;  // Update to cross-file link
      }
    });
    writeHtml(file.file, doc);
  }
}
```

2. **Bibliography fixup**:
```typescript
export async function bookBibliographyPostRender(
  context: ProjectContext,
  incremental: boolean,
  websiteFiles: WebsiteProjectOutputFile[],
) {
  const allBibEntries: BibEntry[] = [];

  // Collect bibliography from all chapters
  for (const file of websiteFiles) {
    const doc = parseHtml(file.file);
    const bibSection = doc.querySelector("#refs");
    if (bibSection) {
      bibSection.querySelectorAll(".csl-entry").forEach(entry => {
        allBibEntries.push({ id: entry.id, html: entry.innerHTML });
      });
      bibSection.remove();  // Remove from this chapter
      writeHtml(file.file, doc);
    }
  }

  // Deduplicate and append to references page
  const uniqueEntries = uniqBy(allBibEntries, e => e.id);
  const referencesFile = websiteFiles.find(f => f.href.includes("references.html"));
  if (referencesFile) {
    const doc = parseHtml(referencesFile.file);
    const bibHtml = uniqueEntries.map(e =>
      `<div id="${e.id}" class="csl-entry">${e.html}</div>`
    ).join("\n");
    doc.querySelector("main").innerHTML +=
      `<div id="refs" class="references">${bibHtml}</div>`;
    writeHtml(referencesFile.file, doc);
  }
}
```

**Implications for Rust Port**:

1. **Trait-based inheritance**:
```rust
pub trait ProjectType: Send + Sync {
    fn inherits_from(&self) -> Option<&'static dyn ProjectType> {
        None
    }

    async fn pre_render(&self, context: &ProjectContext) -> Result<()> {
        if let Some(parent) = self.inherits_from() {
            parent.pre_render(context).await
        } else {
            Ok(())
        }
    }
}

pub struct BookProjectType {
    website: &'static WebsiteProjectType,
    chapter_manager: Arc<RwLock<ChapterManager>>,
}

impl BookProjectType {
    fn inherits_from(&self) -> Option<&'static dyn ProjectType> {
        Some(self.website)
    }
}
```

2. **Chapter manager**:
```rust
pub struct ChapterManager {
    render_items: Vec<BookRenderItem>,
    chapter_map: HashMap<PathBuf, ChapterInfo>,
}

impl ChapterManager {
    pub async fn from_config(project: &ProjectContext) -> Result<Self> {
        let mut render_items = Vec::new();
        let mut chapter_number = 1;

        // Parse book.chapters
        for chapter in &project.config.book.chapters {
            let item = BookRenderItem {
                item_type: BookRenderItemType::Chapter,
                file: Some(chapter.clone()),
                number: Some(chapter_number),
                depth: 0,
                text: None,
            };
            render_items.push(item);
            chapter_number += 1;
        }

        // Parse book.appendices (numbered with letters)
        chapter_number = 1;
        for appendix in &project.config.book.appendices {
            let label = std::char::from_u32(64 + chapter_number).unwrap();
            // ... add appendix items with letter labels
        }

        Ok(ChapterManager { render_items, chapter_map })
    }
}
```

3. **Custom Pandoc renderer**:
```rust
pub struct BookPandocRenderer {
    mode: BookRenderMode,
    executed_files: Mutex<Vec<ExecutedFile>>,
}

pub enum BookRenderMode {
    MultiFile,   // Render each chapter
    SingleFile,  // Accumulate and merge
}

#[async_trait]
impl PandocRenderer for BookPandocRenderer {
    async fn on_render(&self, file: ExecutedFile) -> Result<()> {
        match self.mode {
            BookRenderMode::MultiFile => {
                // Add chapter number to title
                let chapter_info = get_chapter_info(&file);
                file.format = with_chapter_metadata(file.format, chapter_info)?;
                render_pandoc(file).await?;
            }
            BookRenderMode::SingleFile => {
                // Just accumulate
                self.executed_files.lock().await.push(file);
            }
        }
        Ok(())
    }

    async fn on_complete(&self) -> Result<Vec<RenderedFile>> {
        match self.mode {
            BookRenderMode::MultiFile => Ok(self.rendered_files()),
            BookRenderMode::SingleFile => {
                let files = std::mem::take(&mut *self.executed_files.lock().await);
                let merged = merge_chapters(files).await?;
                let rendered = render_pandoc(merged).await?;
                Ok(vec![rendered])
            }
        }
    }
}
```

4. **Post-render fixups** (using scraper):
```rust
pub async fn book_crossrefs_post_render(
    context: &ProjectContext,
    html_files: &[&ProjectOutputFile],
) -> Result<()> {
    use scraper::{Html, Selector};

    // Build xref map
    let mut xref_map: HashMap<String, String> = HashMap::new();
    for file in html_files {
        let html = std::fs::read_to_string(&file.file)?;
        let document = Html::parse_document(&html);
        let selector = Selector::parse("[id]").unwrap();
        for element in document.select(&selector) {
            if let Some(id) = element.value().attr("id") {
                xref_map.insert(
                    id.to_string(),
                    format!("{}#{}", file.href, id)
                );
            }
        }
    }

    // Fix up links (requires mutable HTML manipulation)
    for file in html_files {
        // Use lol_html for efficient mutations
        let rewritten = rewrite_html(&file.file, |element| {
            if element.tag_name() == "a" {
                if let Some(href) = element.get_attribute("href") {
                    if href.starts_with('#') {
                        let id = &href[1..];
                        if let Some(target) = xref_map.get(id) {
                            element.set_attribute("href", target)?;
                        }
                    }
                }
            }
            Ok(())
        })?;
        std::fs::write(&file.file, rewritten)?;
    }

    Ok(())
}
```

**Key architecture insight**: Book projects demonstrate Quarto's **type composition** pattern:
- Website provides base functionality (navigation, search, sitemap)
- Book extends with chapter management, dual rendering modes, and cross-file fixups
- Other project types (manuscript, etc.) can similarly extend existing types

## Notes for Next Session

- Configuration merging is more complex than initially apparent
- Tag support is straightforward but critical for Quarto functionality
- Workspace architecture is essential for managing Kyoto's complexity
- All designs are well-aligned with existing Rust infrastructure
- All maintain compatibility with TypeScript while being more robust
- Serialization is a key advantage (enables LSP caching)
- Modular crate structure enables parallel development and testing
- **Website projects use a clean hook-based architecture (pre_render, format_extras, post_render)**
- **Navigation state is global but immutable during rendering (built once, read many times)**
- **Search indexing uses section-level granularity for better UX**
- **Sitemap and search support incremental updates (only reprocess changed files)**
- Ready to begin implementation - architecture decisions are complete
