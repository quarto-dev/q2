# Claude Notes Index

<!-- quarto-error-code-audit-ignore-file -->

This directory contains notes about the Kyoto project - exploring a Rust port of Quarto CLI.

We track work in Beads instead of Markdown. Run `br quickstart` to see how. Keep using markdown files to provide information to the user, but use Beads to track your own work, project dependencies, etc.

**Note:** `br` is non-invasive and never executes git commands. After `br sync --flush-only`, you must manually run `git add .beads/ && git commit`.

## Project Overview

- **[project-overview.md](project-overview.md)** - High-level project goals, current status, architecture findings
- **[quarto-dependencies.dot](quarto-dependencies.dot)** - Graphviz diagram showing all subsystem dependencies (arrows point from dependency → dependent). Visualizes the complete architecture from foundation layer (MappedString, QuartoMD, ErrorReporting) through core infrastructure (YAML, Config) to rendering systems and tools (LSP, CLI, MCP)
- **[rust-cli-organization-patterns.md](rust-cli-organization-patterns.md)** - Survey of popular Rust CLI tools and their source code organization patterns, with recommendations for Kyoto
- **[render-pipeline/single-document/](render-pipeline/single-document/)** - Complete analysis of single document rendering (`quarto render doc.qmd`): 10 stages, data flow, HTML vs PDF comparison, and implications for Rust port. Split into 19 documents for easier navigation
- **[website-project-rendering.md](website-project-rendering.md)** - Website project rendering analysis: project detection, pre-render hooks, navigation state, format extras injection, post-render hooks (sitemap, search, listings), multi-file coordination, and Rust port design
- **[book-project-rendering.md](book-project-rendering.md)** - Book project rendering analysis: inherits from website, dual rendering modes (multi-file HTML vs single-file PDF/EPUB), chapter management, custom Pandoc renderer, cross-reference and bibliography fixups, and trait-based inheritance design for Rust

## Rendering Pipeline Architecture

- **[explicit-workflow-design.md](explicit-workflow-design.md)** - Design for representing rendering dependencies explicitly as DAG workflows: data structures (Step, Artifact, Workflow), execution strategies, parallelization, caching, reconfiguration, and extension APIs for Rust implementation
- **[explicit-dependencies-analysis.md](explicit-dependencies-analysis.md)** - Analysis of implicit vs explicit dependencies in rendering: problem statement, benefits (parallelization, reconfiguration, caching), implementation phases, success metrics, and recommendations for Kyoto
- **[surface-syntax-converter-design.md](surface-syntax-converter-design.md)** - ✅ **Strongly Recommended Design**: Separate surface syntax conversion (.ipynb, percent scripts, R spin scripts → qmd) from execution engines. Creates SourceConverter registry independent of engines. Benefits: cleaner architecture, independent extensibility, better testing, performance gains (caching/parallelization), third-party friendly. Includes Rust API design, migration path, 8-13 week implementation roadmap. Addresses file claiming, metadata preservation, source mapping challenges. Includes future converter example: Rustdoc (enables .rs files → revealjs/PDF/typst/etc via qmd)

## LSP Analysis

- **[lsp-architecture-findings.md](lsp-architecture-findings.md)** - Deep dive into current TypeScript LSP architecture
- **[lsp-feature-catalog.md](lsp-feature-catalog.md)** - Complete catalog of LSP features to port
- **[rust-lsp-implementation-plan.md](rust-lsp-implementation-plan.md)** - Detailed 14-week plan for Rust LSP
- **[current-lsp-implementation-analysis.md](current-lsp-implementation-analysis.md)** - TypeScript LSP feature analysis
- **[rust-lsp-recommendations.md](rust-lsp-recommendations.md)** - Rust crates and architecture recommendations
- **[lsp-rust-port-summary.md](lsp-rust-port-summary.md)** - Executive summary of LSP port

## Quarto Markdown Parser

- **[quarto-markdown-analysis.md](quarto-markdown-analysis.md)** - Analysis of quarto-markdown Rust parser and LSP integration strategy
- **[performance-strategy.md](performance-strategy.md)** - Comprehensive performance measurement and optimization strategy
- **[unified-source-location-design.md](unified-source-location-design.md)** - Unified, serializable source location system integrating SourceInfo and MappedString

## Mapped-Text and YAML System

- **[mapped-text-analysis.md](mapped-text-analysis.md)** - MappedString data structure analysis (~1000 LOC)
- **[yaml-validator-analysis.md](yaml-validator-analysis.md)** - YAML validation system analysis (~7600 LOC)
- **[mapped-text-yaml-port-plan.md](mapped-text-yaml-port-plan.md)** - Combined 6-8 week port plan
- **[yaml-annotated-parse-rust-plan.md](yaml-annotated-parse-rust-plan.md)** - Plan to build AnnotatedParse using yaml-rust2's MarkedEventReceiver API
- **[config-merging-analysis.md](config-merging-analysis.md)** - Analysis of mergeConfigs function and source-location-aware merge strategy for Rust port
- **[yaml-tags-analysis.md](yaml-tags-analysis.md)** - YAML tags (like !expr) support analysis and integration strategy with AnnotatedParse
- **[mapped-string-cell-yaml-design.md](mapped-string-cell-yaml-design.md)** - Comprehensive design for location tracking in YAML parsing across three scenarios: standalone files, metadata blocks, and code cell options (non-contiguous extraction). Includes unified SourceInfo design with Concat strategy, yaml-rust2 integration, complete API, implementation plan, and testing strategy
- **[yaml-with-source-info-design.md](yaml-with-source-info-design.md)** - Complete design for YamlWithSourceInfo (renamed from AnnotatedParse): uses yaml-rust2's Yaml directly with owned data and parallel children structure. Addresses lifetime management, config merging, and dual access patterns (raw Yaml + source-tracked). Includes full API, parsing, validation, ~3x memory overhead analysis, and 3-4 week implementation plan
- **[yaml-with-source-info-lifetime-approach.md](yaml-with-source-info-lifetime-approach.md)** - Alternative lifetime-based design analysis: shows how to express "shorter of two lifetimes" using lifetime bounds, requires hybrid ownership (merged containers owned, leaves borrowed), compares complexity/memory trade-offs, discusses LSP caching implications, reviews rust-analyzer and rustc precedents, recommends owned data for simplicity but acknowledges lifetime approach is feasible
- **[rust-analyzer-owned-data-patterns.md](rust-analyzer-owned-data-patterns.md)** - Concrete code examples from rust-analyzer showing their owned data approach: Config struct uses Vec/HashMap/Arc with zero lifetimes, rowan's SyntaxNode uses manual refcounting (inc_rc/dec_rc), Clone is cheap (just refcount++), all public APIs return owned types not references, demonstrates that owned data works at scale for large codebases
- **`crates/quarto-yaml/`** - ✅ **Implemented and Benchmarked!** YAML parsing with source location tracking. Wraps yaml-rust2::Yaml with owned data approach (6.38x memory overhead, verified linear scaling). Provides parse() API, dual access (raw Yaml + source-tracked), complete test coverage (14 tests + 2 benchmarks). See crate's README and claude-notes/ for details
- **[yaml-validation-rust-design.md](yaml-validation-rust-design.md)** - Comprehensive design for YAML validation crate: analyzed TypeScript validator (~7000 LOC), designed Rust equivalents (Schema enum with 13 variants, ValidationContext, navigate function, type-specific validators), addressed error collection/pruning/improvement, schema compilation from YAML, and 6-8 week implementation plan (6 phases)
- **`crates/quarto-yaml-validation/`** - ✅ **Phase 1 Complete!** YAML validation with schema-based validation. Implements Schema enum (13 types), ValidationError with source tracking, ValidationContext with path tracking, critical navigate() function for error reporting, all type-specific validators (boolean, number, string, null, enum, any, anyOf, allOf, array, object, ref). Complete test coverage (12 tests passing). ~1150 LOC. Error pruning deferred to Phase 2

## Error Reporting and Console Output

- **[error-reporting-design-research.md](error-reporting-design-research.md)** - Comprehensive design for error reporting and console print subsystem: ariadne (visual errors), R cli (structured output), tidyverse style guide (message best practices), Markdown-based API with Pandoc AST, multiple output formats (ANSI/HTML/JSON)
- **[error-id-system-design.md](error-id-system-design.md)** - TypeScript-style error code system for Quarto. Format: `Q-<subsystem>-<number>` (e.g., Q-1-1). JSON catalog, optional but encouraged, enables Googleable error codes <!-- quarto-error-code-audit-ignore-file -->
- **`crates/quarto-error-reporting/`** - ✅ **Phase 1 Complete!** Error reporting with TypeScript-style error codes. Includes DiagnosticMessage types, builder API, error catalog (JSON), Q-<subsystem>-<number> format. Phase 2-4 planned (rendering, console helpers)

## YAML and Validation

- **[yaml-schema-from-yaml-design.md](yaml-schema-from-yaml-design.md)** - **[REVISED FOR YAML 1.2]** Design for loading Quarto schemas from YAML files. **Critical change**: Uses YamlWithSourceInfo instead of serde to ensure YAML 1.2 compatibility and source tracking. Required for Quarto extensions support. Includes complete implementation plan for `validate-yaml` binary. See YAML-1.2-REQUIREMENT.md in both quarto-yaml and quarto-yaml-validation crates

## JavaScript Runtime Dependencies

- **[js-runtime-dependencies.md](js-runtime-dependencies.md)** - Comprehensive analysis of JS runtime dependencies (HTML/DOM, EJS, OJS, Puppeteer) and Rust porting strategy

## Implications and Strategy

- **[rust-port-implications.md](rust-port-implications.md)** - Strategic analysis of what breaks when porting CLI to Rust
- **[rust-ecosystem-risks-analysis.md](rust-ecosystem-risks-analysis.md)** - Comprehensive analysis of long-term risks: dependency abandonment (62% single-maintainer), supply chain security, edition migrations (track record excellent), vulnerability communication (good tools, slow disclosure ~2yrs), practical recommendations for Kyoto
- **[versioning-strategy.md](versioning-strategy.md)** - Dual versioning approach: Cargo.toml (0.x.y) vs CLI reported (99.9.9-dev) for extension compatibility
- **[machine-readable-io-design.md](machine-readable-io-design.md)** - Comprehensive design for machine-readable I/O: global `--format` flag, `Outputable` trait, line-delimited JSON streaming, and config file integration

## MCP Integration

- **[quarto-mcp-server-plan.md](quarto-mcp-server-plan.md)** - Complete design and implementation plan for `quarto mcp` server
- **[quarto-mcp-technical-spec.md](quarto-mcp-technical-spec.md)** - Technical specification with API schemas, code examples, and integration patterns

## Key Findings

### 1. LSP Must Move to Rust
The current TypeScript LSP has tight runtime coupling with the CLI (loads JS modules from CLI installation). When CLI → Rust, this breaks. Solution: Implement "quarto lsp" command in Rust.

### 1.5. quarto-markdown Enables AST-Based LSP
The quarto-markdown Rust parser (~11K LOC) converts QMD to typed Pandoc AST with full source tracking. This allows the LSP to work with structured AST instead of fragile string parsing, sharing the exact same parser with the CLI.

### 2. Mapped-Text is Critical Infrastructure
~450 LOC that tracks source positions through text transformations. Essential for error reporting. Used by YAML validation, LSP, error formatting, code cell processing.

### 3. YAML System is Large but Well-Structured
~8,600 LOC total:
- YAML intelligence (IDE features): ~2,500 LOC
- YAML validation: ~1,500 LOC
- YAML schemas: ~4,000 LOC
- Mapped-text: ~450 LOC + utilities

### 4. Clear Dependencies
```
quarto-markdown (Markdown → Pandoc AST with SourceInfo)
    ↓
Unified SourceInfo (serializable, multi-file, transformation-aware)
    ↓ powers both
MappedString (YAML extraction) + AnnotatedParse (YAML parsing)
    ↓
YAML Validation + Schemas
    ↓
LSP Features + CLI Validation
```

**Key insight**: Unified SourceInfo replaces closure-based MappedString, enabling disk caching for LSP while preserving full transformation history.

## Estimated Timelines

- **Rust LSP**: 14 weeks (7 phases from infrastructure to production)
- **MappedString + YAML**: 6-8 weeks (6 phases from foundation to integration)
- **JS Runtime Dependencies**: 9-14 weeks (HTML postprocessing: 4-6w, templating: 2-3w, browser: 2-3w, OJS: 1-2w)
- **Total for LSP + YAML**: ~4-5 months (some parallel work possible)

## Technical Decisions

### MappedString
- **Choice**: Enum-based mapping (vs closures)
- **Rationale**: More Rust-idiomatic, easier to debug

### YAML Parsing and Source Tracking
- **Choice**: yaml-rust2 with MarkedEventReceiver for YamlWithSourceInfo (renamed from AnnotatedParse)
- **Rationale**: Provides position tracking for all events, already in use, single parser (strict), optional tree-sitter-yaml for lenient mode if needed later
- **Data Structure**: Owned yaml-rust2::Yaml + parallel Children with source tracking (~3x memory overhead)
- **Rationale**: Enables config merging across different lifetimes, provides dual access (raw Yaml + source-tracked), simpler API than lifetime-based alternatives

### LSP Framework
- **Choice**: tower-lsp
- **Rationale**: Well-maintained, used by rust-analyzer

### Markdown Parser
- **Choice**: quarto-markdown-pandoc
- **Rationale**: Typed AST, source tracking, Quarto-aware, shared with CLI

### Source Location System
- **Choice**: Unified SourceInfo (serializable enum-based design)
- **Rationale**: Replaces closure-based MappedString, enables caching, supports multi-file, preserves transformation chains

### Schema Definition
- **Choice**: Start in Rust code, migrate to JSON Schema later
- **Rationale**: Type safety during port, flexibility after

### HTML Postprocessing
- **Choice**: html5ever + scraper
- **Rationale**: Industry standard (powers Servo), CSS selector support, excellent performance

### Templating
- **Choice**: tera (Jinja2-like)
- **Rationale**: Runtime template loading like EJS, minimal syntax changes, mature ecosystem

### OJS Compilation
- **Choice**: Keep JavaScript parser (shell to Node.js)
- **Rationale**: OJS is inherently JS, parser maintained by Observable, minimal CLI impact

### Browser Automation
- **Choice**: headless_chrome
- **Rationale**: Native Rust CDP bindings, similar to Puppeteer API, well-maintained

### MCP Server
- **Choice**: rmcp (official Rust SDK for Model Context Protocol)
- **Rationale**: Anthropic's official SDK, JSON-RPC 2.0 transport, stdio/SSE/WebSocket support

### Configuration Merging
- **Choice**: YamlWithSourceInfo merge (eager merging with source tracking)
- **Rationale**: Leverages YamlWithSourceInfo infrastructure, preserves source locations through merge, integrates naturally with validation, serializable for caching

### YAML Tags
- **Choice**: Full tag support via yaml-rust2's Event API, with YamlWithSourceInfo tag field
- **Rationale**: yaml-rust2 provides complete tag support through Option<Tag> in Event::Scalar/SequenceStart/MappingStart, compatible with TypeScript's tagged value representation, enables !expr for R/Python expressions

### CLI Architecture
- **Choice**: Workspace architecture (turborepo-style) + Commands directory (cargo-style)
- **Rationale**: 17+ subcommands need clear organization; workspace enables parallel compilation, reusable crates, and third-party engine extensibility; proven at scale by cargo and turborepo

### Versioning
- **Choice**: Dual versioning (Cargo: 0.1.0, CLI: 99.9.9-dev)
- **Rationale**: Cargo version signals instability (Rust convention); CLI version ensures extension compatibility and compares > all v1.x versions

### Rust Edition
- **Choice**: Edition 2024 (with nightly toolchain)
- **Rationale**: Improved temporary lifetimes, better unsafe ergonomics, RPIT capture improvements, reserves `gen` keyword; stable as of Rust 1.85.0 (Feb 2025)

### Machine-Readable I/O
- **Choice**: Hybrid approach - global `--format` flag + per-command `--json` for backward compatibility
- **Rationale**: Follows cargo/ripgrep patterns; line-delimited JSON to stdout, human messages to stderr; `Outputable` trait separates business logic from presentation; supports streaming for long operations

### Error Reporting and Console Output
- **Choice**: Markdown strings → Pandoc AST → Multiple outputs (ANSI/HTML/JSON)
- **Rationale**:
  - Uses ariadne for visual error rendering (proven in quarto-markdown)
  - Markdown with Pandoc spans (`` `text`{.class} ``) for semantic markup
  - Builder API (`.problem()`, `.add_detail()`, `.add_hint()`) encodes tidyverse guidelines
  - Rust-only (WASM for cross-language if needed)
  - Defer compile-time macros and theming

## Next Steps

### Completed
1. ✅ Understand architecture (done)
2. ✅ MCP server design and planning (done)
3. ✅ Workspace structure created (5 crates: quarto, quarto-core, quarto-util, quarto-yaml, quarto-yaml-validation)
4. ✅ CLI skeleton with all 18 commands and complete option parsing
5. ✅ Versioning strategy implemented (dual version: Cargo 0.1.0, CLI 99.9.9-dev)
6. ✅ Upgraded to Rust Edition 2024
7. ✅ Machine-readable I/O design (global --format, Outputable trait, streaming)
8. ✅ Error reporting and console output design (ariadne + Markdown + Pandoc AST)
9. ✅ **quarto-yaml crate implemented** (YAML parsing with source tracking, owned data approach, 14 tests passing)
10. ✅ **quarto-yaml-validation Phase 1 implemented** (Schema types, ValidationContext, navigate function, all type-specific validators, 12 tests passing, ~1150 LOC)

### Proposed Priorities
1. **Option A: MCP Server Spike** (2 days)
   - Validate rmcp/mcp-core with basic server
   - Test Claude Desktop integration
   - Prove concept before committing

2. **Option B: Continue Core Infrastructure**
   - Begin MappedString implementation
   - Prototype YAML parsing with yaml-rust2
   - Start basic LSP server with tower-lsp
   - Parallel work: Schema porting + Validator implementation

3. **Option C: Hybrid Approach**
   - 2-day MCP spike first
   - If successful, add to roadmap
   - Continue core infrastructure in parallel

## Session Logs

Detailed notes from design sessions:
- **[session-logs/2025-10-12-workspace-setup.md](session-logs/2025-10-12-workspace-setup.md)** - Initial workspace setup, CLI skeleton, versioning
- **[session-logs/2025-10-12-error-reporting-design.md](session-logs/2025-10-12-error-reporting-design.md)** - Error reporting subsystem design research and decisions
- **[session-logs/2025-10-12-rust-ecosystem-research.md](session-logs/2025-10-12-rust-ecosystem-research.md)** - Comprehensive research on Rust ecosystem risks: dependencies (62% single-maintainer, medium-high risk), editions (low risk, excellent track record), security communication (good tools, 2yr disclosure lag)
- **[session-logs/2025-10-13-dependency-diagram.md](session-logs/2025-10-13-dependency-diagram.md)** - Created comprehensive Graphviz dependency diagram showing all subsystem relationships; visualizes architecture from foundation layer through tools; arrows show dependency → dependent flow
- **[session-logs/2025-10-13-mapped-string-cell-yaml-design.md](session-logs/2025-10-13-mapped-string-cell-yaml-design.md)** - Comprehensive design for MappedString/SourceInfo handling all three YAML scenarios (standalone files, metadata blocks, code cell options). Key innovation: Concat strategy with per-piece SourceInfo for non-contiguous text extraction. Includes complete API, yaml-rust2 integration, implementation plan (10 weeks), and testing strategy
- **[session-logs/2025-10-13-yaml-with-source-info-design.md](session-logs/2025-10-13-yaml-with-source-info-design.md)** - YamlWithSourceInfo design session: resolved lifetime tension (lifetimes vs owned data for config merging), decided on owned yaml-rust2::Yaml with parallel Children structure, analyzed ~3x memory overhead trade-off, designed dual access API (raw Yaml + source-tracked), full parsing/validation/merging implementation with 3-4 week timeline
- **[session-logs/2025-10-13-yaml-lifetime-vs-owned-discussion.md](session-logs/2025-10-13-yaml-lifetime-vs-owned-discussion.md)** - User challenged owned-data recommendation with lifetime-based alternative; analyzed both approaches; explored rust-analyzer source code for concrete evidence; both approaches viable; user chose owned data approach
- **[session-logs/2025-10-13-quarto-yaml-implementation.md](session-logs/2025-10-13-quarto-yaml-implementation.md)** - Implemented quarto-yaml crate: created workspace, implemented SourceInfo/YamlWithSourceInfo/YamlHashEntry, MarkedEventReceiver parser, parse() API, 14 tests (all passing), documentation (README + claude-notes), ~2-3 hours total
- **[session-logs/2025-10-13-quarto-yaml-continued.md](session-logs/2025-10-13-quarto-yaml-continued.md)** - Memory overhead validation: created benchmarks, discovered 6.38x overhead (not 3x estimated), verified linear scaling (no superlinear growth), proved production-ready with stable overhead ratios across 100x size increases, explained Rust documentation format
- **[session-logs/2025-10-13-surface-syntax-converter-design.md](session-logs/2025-10-13-surface-syntax-converter-design.md)** - Surface syntax converter architecture design: analyzed current engine coupling (file claiming, conversion, execution), researched quarto-cli implementations (.ipynb/percent/spin converters), designed independent SourceConverter trait/registry, evaluated pros/cons, created Rust API with ConvertedSource/SourceMap, addressed challenges (file claiming, metadata preservation, source mapping), strongly recommended with 8-13 week roadmap
- **[session-logs/2025-10-13-yaml-validation-phase1-implementation.md](session-logs/2025-10-13-yaml-validation-phase1-implementation.md)** - Phase 1 implementation of quarto-yaml-validation crate: implemented Schema enum (13 types), ValidationError with source tracking, ValidationContext with path tracking, critical navigate() function, all type-specific validators (boolean, number, string, null, enum, any, anyOf, allOf, array, object, ref). Resolved YamlWithSourceInfo API challenges, fixed yaml-rust2 integration. 12 tests passing. ~1150 LOC in ~2 hours. Phase 1 complete, ready for Phase 2 (schema compilation)

## Notes for Future Claude Instances

- Use `pwd` frequently when running shell commands
- These notes are living documents - update as understanding evolves
- External sources are in `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/`
- Current focus is **planning and understanding**, not yet writing production code
- When drawing diagrams, always use graphviz, never, *ever* use mermaidjs.
- Use q- as the prefix for Beads issues.