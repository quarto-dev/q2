# Rust CLI Organization Patterns

**Date:** 2025-10-11
**Status:** Complete Analysis
**Purpose:** Survey popular open-source Rust CLI tools with similar subcommand organization to quarto-cli and document their source code structure patterns

## Executive Summary

This analysis examines how major Rust CLI tools organize their source code when implementing multiple top-level subcommands (similar to quarto's `render`, `preview`, `create`, etc.). We surveyed cargo, rustup, just, turborepo, git-cliff, and ripgrep to identify best practices.

**Key Finding:** There are four main organizational patterns, with the **Commands Directory + Ops Separation** (cargo-style) and **Workspace with Thin CLI** (turborepo-style) being most suitable for large, complex CLI tools like Kyoto.

## Quarto CLI Command Structure (Baseline)

For reference, quarto-cli has 17+ top-level subcommands:

```
quarto
├── render      - Render files or projects
├── preview     - Render and preview
├── serve       - Serve Shiny documents
├── create      - Create projects/extensions
├── use         - Automate setup tasks
├── add         - Add extensions
├── update      - Update extensions
├── remove      - Remove extensions
├── convert     - Convert documents
├── pandoc      - Run embedded Pandoc
├── typst       - Run embedded Typst
├── run         - Run scripts
├── list        - List extensions
├── install     - Install dependencies
├── uninstall   - Remove tools
├── tools       - Display tool status
├── publish     - Publish to providers
├── check       - Verify installation
├── call        - Access subsystem functions
└── help        - Show help
```

## Major Examples

### 1. cargo - Rust Package Manager

**Similarity to Quarto:** Very high - multiple subcommands with complex operations, config management, workspace handling

**Subcommands:** `build`, `test`, `run`, `check`, `add`, `update`, `publish`, `install`, `doc`, etc.

**Source Organization:**
```
src/
├── bin/cargo/
│   ├── main.rs              # Entry point
│   ├── cli.rs               # CLI argument parsing with clap
│   └── commands/            # All subcommand definitions
│       ├── build.rs
│       ├── test.rs
│       ├── run.rs
│       ├── publish.rs
│       └── ...
└── cargo/
    ├── ops/                 # Actual implementation logic
    │   ├── cargo_compile/
    │   ├── cargo_test/
    │   └── ...
    ├── core/                # Core data structures
    │   ├── package.rs
    │   ├── workspace.rs
    │   └── manifest.rs
    └── util/                # Utility functions
```

**Key Patterns:**

1. **Clear Separation:** CLI parsing in `commands/` vs implementation in `ops/`
2. **Command Responsibilities:** Each command file:
   - Parses CLI flags (using clap)
   - Loads config files
   - Discovers and loads workspace
   - Delegates to `ops/` for actual work
3. **Extensibility:** Designed for subcommands without modifying cargo itself
4. **Single Binary:** All subcommands in one executable

**Rationale:** This separation allows:
- Testing business logic independently of CLI
- Reusing core operations across multiple commands
- Clear ownership boundaries in large codebase

**Source:** [Cargo Contributor Guide - New Subcommands](https://doc.crates.io/contrib/implementation/subcommands.html)

### 2. rustup - Rust Toolchain Installer

**Similarity to Quarto:** Medium - manages installations and configurations, multiple subcommands

**Subcommands:** `update`, `install`, `default`, `toolchain`, `target`, `component`, `override`, `show`, etc.

**Source Organization:**
```
src/
├── main.rs
├── cli/
│   ├── mod.rs
│   ├── common.rs
│   └── [command modules]
├── toolchain.rs
├── config.rs
└── dist/
```

**Key Patterns:**

1. **Standard Rust Structure:** Flat-ish module hierarchy
2. **Config Management:** Central configuration handling
3. **Built-in Help:** Comprehensive help system for each subcommand
4. **Version Management:** Core abstraction is toolchain/component management

**Documentation:**
- [The Rustup Book](https://rust-lang.github.io/rustup/)
- [Rustup Dev Guide](https://rust-lang.github.io/rustup/dev-guide)

### 3. just - Command Runner (by Casey Rodarmor)

**Similarity to Quarto:** Low (single justfile runner) but **excellent architectural lessons**

**Subcommands:** Modes via enum rather than traditional subcommands (`--list`, `--dump`, `--evaluate`, etc.)

**Source Organization (Flat Structure):**
```
src/
├── main.rs                  # Entry point (thin wrapper calling run())
├── lib.rs                   # Library interface
├── run.rs                   # Main run function
├── common.rs                # Centralized use statements
│
├── config.rs                # CLI argument parsing & configuration
├── subcommand.rs            # Subcommand enum definitions
│
├── compiler.rs              # Compilation coordination
├── lexer.rs                 # Tokenization
├── token.rs                 # Token structures
├── token_kind.rs            # Token kinds
├── parser.rs                # Parsing
├── module.rs                # Parsed but unvalidated module
├── item.rs                  # Source constructs (Alias, Assignment, Recipe, Set)
├── analyzer.rs              # Validation & dependency resolution
├── justfile.rs              # Final analyzed justfile
│
├── compilation_error.rs     # Compilation errors
├── runtime_error.rs         # Runtime errors
│
└── [other feature modules]  # ~70 total modules

tests/                       # Integration tests (test binary from outside)
fuzz/                       # Fuzz testing
janus/                      # Regression test framework
```

**Key Patterns:**

1. **Flat Module Structure:** Author's philosophy - no deep nesting, easy navigation
2. **Centralized Imports:** `common.rs` contains `use` statements for entire project
3. **Single Primary Definition per Module:** Most modules export one main type/function
4. **Clear Pipeline Stages:** lexer → parser → analyzer → justfile
5. **Dual Targets:** Both binary (`main.rs`) and library (`lib.rs`)

**Philosophy (from Casey Rodarmor's blog):**

> "The crate consists of an executable target, in src/main.rs, and a library in src/lib.rs. The main function in main.rs is a thin wrapper that calls the run function in src/run.rs."

> "I prefer a flat module structure. Deep module hierarchies are harder to navigate, and often don't reflect how the types and functions are actually used."

> "I centralize use statements in src/common.rs, which makes it easy to see what external crates and standard library modules are being used."

**Why This Matters for Kyoto:** Demonstrates that flat structure can scale to ~70 modules while remaining maintainable. The clear compilation pipeline is similar to Quarto's render pipeline.

**Source:** [Tour de Just - Casey Rodarmor's Blog](https://casey.github.io/blog/tour-de-just/)

### 4. turborepo - Monorepo Build System

**Similarity to Quarto:** High - complex build orchestration, multiple operations, configuration management

**Subcommands:** `run`, `prune`, `generate`, `login`, `link`, `unlink`, `init`, `daemon`, `turbo-ignore`

**Note:** `turbo run` is aliased to `turbo`, so `turbo build` = `turbo run build`

**Source Organization (Workspace Architecture):**
```
crates/
├── turborepo/                    # Main CLI binary (THIN WRAPPER)
│   └── src/
│       ├── main.rs
│       └── [minimal CLI setup]
│
├── turborepo-lib/                # Core logic (THE REAL IMPLEMENTATION)
│   └── src/
│       ├── lib.rs
│       └── [main functionality]
│
├── turborepo-analytics/          # Feature: Analytics
├── turborepo-cache/              # Feature: Caching system
├── turborepo-auth/               # Feature: Authentication
├── turborepo-frameworks/         # Feature: Framework detection
├── turborepo-api-client/         # Feature: API client
├── turborepo-filewatch/          # Feature: File watching
├── turborepo-scm/                # Feature: Source control
├── turborepo-telemetry/          # Feature: Telemetry
├── turborepo-ui/                 # Feature: Terminal UI
├── turborepo-ffi/                # FFI bindings (for Go interop during migration)
└── [many other feature crates]
```

**Key Patterns:**

1. **Thin CLI Wrapper:** Main binary is minimal, delegates to `-lib`
2. **Feature-Based Crates:** Each major feature is separate crate
3. **Modular Architecture:** Crates can be:
   - Tested independently
   - Reused in other tools
   - Versioned separately
   - Compiled in parallel
4. **Migration-Friendly:** Used FFI during Go → Rust port

**Migration Story:**

Turborepo was originally written in Go and is being incrementally ported to Rust:

1. **Phase 1:** Global turbo + CLI argument parsing
2. **Phase 2:** Main commands (`run`, `prune`)
3. **Phase 3:** Auxiliary commands (`login`, `link`, `unlink`) - easier, just HTTP + config
4. **FFI Strategy:** `turborepo-ffi` compiled to C static library, linked to Go binary via CGO

**Why This Matters for Kyoto:**

- Demonstrates workspace architecture scales to 20+ crates
- Shows how to incrementally migrate large codebases
- Proves that "thin CLI + core lib" pattern works for complex tools
- Feature isolation makes testing and maintenance easier

**Sources:**
- [Turborepo GitHub](https://github.com/vercel/turborepo)
- [Turborepo Contributing Guide](https://github.com/vercel/turborepo/blob/main/CONTRIBUTING.md)
- "How Turborepo is porting from Go to Rust" (Hacker News discussion)

### 5. git-cliff - Changelog Generator

**Similarity to Quarto:** Medium - processes files, generates output, configuration-driven

**Subcommands:** Fewer than quarto, but similar pattern

**Source Organization:**
```
git-cliff/              # CLI implementation
git-cliff-core/         # Core library
config/                 # Configuration files
examples/               # Example configurations
website/                # Documentation
npm/, pypi/             # Package distribution
```

**Key Patterns:**

1. **CLI + Core Split:** Same as turborepo but simpler (only 2 main crates)
2. **Configuration-Driven:** Heavy use of `cliff.toml`
3. **Library-First Design:** Core functionality in `-core`, CLI is thin layer
4. **Multi-Language Distribution:** Rust binary wrapped for npm/pypi

**Why This Matters for Kyoto:** Shows minimal viable workspace structure - just CLI + core can be sufficient.

### 6. ripgrep - Fast Search Tool

**Similarity to Quarto:** Low (single command) but **exemplary modular design**

**Command Style:** Single command (`rg`) controlled by arguments (not subcommands)

**Source Organization (Highly Modular):**
```
crates/
├── ripgrep/            # Main CLI (SMALL - mostly argv & output handling)
├── grep/               # Core search algorithms
├── ignore/             # Gitignore processing & parallel directory iteration
├── globset/            # Pattern matching
├── termcolor/          # Cross-platform terminal colors
└── wincolor/           # Windows console coloring
```

**Evolution:**

Started as monolithic tool, evolved into separate reusable crates. Goal was for `ripgrep/` to become "pretty small, limited mostly to argv handling and output handling."

**Key Patterns:**

1. **Extract Reusable Components:** `ignore` crate now used by other projects (tokei)
2. **Platform Abstraction:** `termcolor` handles cross-platform issues
3. **Library-First APIs:** Core functionality available as Rust libraries
4. **Thin CLI:** Main binary just handles interface

**Limitations:**

- APIs are lower-level than CLI abstraction
- Less high-level documentation
- More challenging to use as library

**Why This Matters for Kyoto:**

- Demonstrates value of extracting reusable crates (`kyoto-engines`, `kyoto-filters`)
- Shows how to handle cross-platform concerns
- Proves that modular architecture improves ecosystem

**Source:** [Ripgrep Code Review (mbrt blog)](https://blog.mbrt.dev/posts/ripgrep/)

## Organizational Patterns Summary

### Pattern 1: Commands Directory (Cargo-Style)

**Structure:**
```
src/
├── bin/[tool]/
│   ├── main.rs
│   └── commands/
│       ├── mod.rs
│       ├── command1.rs
│       ├── command2.rs
│       └── ...
└── [tool]/ops/
    ├── operation1.rs
    └── operation2.rs
```

**Best For:**
- CLI tools with many subcommands (10+)
- Clear separation between interface and logic
- Large teams needing ownership boundaries

**Advantages:**
- Very clear where to add new commands
- Easy to test business logic independently
- Scalable to dozens of subcommands
- Parallel development (different developers own different commands)

**Examples:** cargo

### Pattern 2: Flat Module Structure (Just-Style)

**Structure:**
```
src/
├── main.rs           # Thin wrapper
├── lib.rs            # Library interface
├── run.rs            # Main run function
├── common.rs         # Centralized imports
├── config.rs         # CLI parsing
├── subcommand.rs     # Subcommand enum
└── [features].rs     # Feature modules (flat)
```

**Best For:**
- Medium-complexity CLI tools (up to ~70 modules)
- Single developer or small team
- Projects valuing simplicity over deep hierarchies

**Advantages:**
- Easy navigation (no deep nesting)
- Fast to understand entire codebase
- Minimal module boilerplate
- Clear which module does what

**Disadvantages:**
- Can feel cluttered with 50+ modules
- Less clear ownership boundaries
- Harder to parallelize development

**Examples:** just

**Philosophy:** Casey Rodarmor advocates this approach, demonstrating it can scale to ~11,000 LOC across 70 modules.

### Pattern 3: Workspace with Thin CLI Wrapper (Turborepo-Style)

**Structure:**
```
crates/
├── [tool]/              # Main CLI binary (THIN)
│   └── src/main.rs      # Just CLI setup + delegation
├── [tool]-lib/          # Core logic
│   └── src/lib.rs       # Main functionality
├── [tool]-feature1/     # Modular feature
├── [tool]-feature2/     # Modular feature
└── [tool]-feature3/     # Modular feature
```

**Best For:**
- Large, complex tools (20,000+ LOC)
- Projects with reusable components
- Tools that might be used as libraries
- Teams wanting parallel compilation

**Advantages:**
- Strong modularity boundaries
- Parallel compilation (workspace crates compile independently)
- Easy to extract/reuse components
- Clear dependency graph
- Testing isolation
- Can version crates independently

**Disadvantages:**
- More boilerplate (Cargo.toml per crate)
- Slightly more complex for newcomers
- Need to manage inter-crate versions

**Examples:** turborepo, ripgrep, git-cliff

**Why It Works:**
- Rust's workspace system is designed for this
- Cargo handles cross-crate dependencies well
- Enables incremental migration (like turborepo's Go → Rust port)

### Pattern 4: Commands + Ops Separation (Cargo Hybrid)

**Structure:**
```
src/
├── bin/[tool]/
│   └── commands/      # CLI parsing & setup only
└── [tool]/
    ├── ops/           # Implementation logic
    ├── core/          # Data structures
    └── util/          # Utilities
```

**Best For:**
- CLI tools that are also used as libraries
- Projects needing strong separation of concerns
- Tools with complex business logic

**Advantages:**
- Clear separation: interface vs logic
- Easy to use core functionality from code
- Business logic testable without CLI
- Can evolve CLI without changing core

**Examples:** cargo

## Common Technical Choices

### CLI Argument Parsing: clap (v4.x with derive macros)

**Universally Used:** Almost all modern Rust CLIs use clap

**Pattern:**
```rust
use clap::{Parser, Subcommand, Args};

#[derive(Parser)]
#[command(name = "tool")]
#[command(version, about, long_about = None)]
struct Cli {
    /// Global flags
    #[arg(short, long)]
    verbose: bool,

    /// Subcommands
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build the project
    Build(BuildArgs),

    /// Test the project
    Test(TestArgs),

    /// Run the project
    Run(RunArgs),
}

#[derive(Args)]
struct BuildArgs {
    /// Build in release mode
    #[arg(short, long)]
    release: bool,

    /// Target to build
    target: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build(args) => build::run(args),
        Commands::Test(args) => test::run(args),
        Commands::Run(args) => run::run(args),
    }
}
```

**Why Clap:**
- Derive macros reduce boilerplate
- Excellent error messages
- Automatic help generation
- Shell completion support
- Widely used (de facto standard)

**Alternatives:**
- `argh` - Smaller, simpler
- `structopt` - Predecessor to clap derive
- Manual parsing - Almost never done

### Configuration Management: Context Struct Pattern

**Pattern (from Kevin K's CLI Structure blog):**

```rust
/// Single source of truth for runtime configuration
struct Context {
    // Normalized config from all sources:
    // - System config files
    // - User config files
    // - Environment variables
    // - CLI flags
    verbose: bool,
    config_path: PathBuf,
    theme: String,
    // ... more fields
}

impl Context {
    fn new() -> Self {
        // Default values
    }

    fn load_system_config(mut self) -> Result<Self> {
        // Update from /etc/tool/config.toml
    }

    fn load_user_config(mut self) -> Result<Self> {
        // Update from ~/.config/tool/config.toml
    }

    fn load_env(mut self) -> Result<Self> {
        // Update from TOOL_* environment variables
    }

    fn load_cli(mut self, cli: Cli) -> Self {
        // CLI flags override everything
    }
}

// Usage:
let ctx = Context::new()
    .load_system_config()?
    .load_user_config()?
    .load_env()?
    .load_cli(cli);

// Pass to commands:
commands::build::run(&ctx, args)?;
```

**Benefits:**
- Single source of truth
- Clear precedence order
- Easy to test (just construct Context)
- No config-related arguments in every function

**Source:** [CLI Structure in Rust - Kevin K](https://kbknapp.dev/cli-structure-01/)

### Error Handling

**Common Choices:**

1. **anyhow** - For applications (not libraries)
   ```rust
   use anyhow::{Context, Result};

   fn run() -> Result<()> {
       do_thing().context("Failed to do thing")?;
       Ok(())
   }
   ```

2. **thiserror** - For defining custom errors
   ```rust
   use thiserror::Error;

   #[derive(Error, Debug)]
   pub enum ToolError {
       #[error("File not found: {0}")]
       NotFound(PathBuf),

       #[error("Parse error at line {line}: {msg}")]
       Parse { line: usize, msg: String },
   }
   ```

3. **color-eyre** - anyhow with pretty error reports
   - Color output
   - Suggestions
   - Better stack traces

### Logging

**Common Choices:**

1. **tracing** - Structured logging & diagnostics (most modern choice)
2. **env_logger** - Simple, environment-variable configured
3. **log** - Trait-based logging facade

### Configuration Files

**Common Formats:**
- TOML (most common): cargo, just, git-cliff, rustup
- YAML: less common in Rust ecosystem
- JSON: rare for human-edited config

**Libraries:**
- `serde` + `toml` - Universal choice
- `config` - Multi-source configuration management

## Recommendations for Kyoto (Rust Quarto Port)

### Context: Kyoto's Complexity

Quarto is highly complex:
- 17+ subcommands
- Multiple engines (jupyter, knitr, julia)
- Complex render pipeline (input → qmd → engines → handlers → filters → postprocessors → recipes → output)
- Configuration merging from multiple sources
- Large codebase (~100,000+ LOC in TypeScript)

### Recommended Architecture: Hybrid Workspace + Commands

**Primary Structure:** Pattern 3 (Workspace) + Pattern 1 (Commands Directory)

```
crates/
├── kyoto/                          # Main CLI binary
│   └── src/
│       ├── main.rs                 # Entry point
│       ├── config.rs               # Context struct + config loading
│       └── commands/               # Command definitions
│           ├── mod.rs
│           ├── render.rs
│           ├── preview.rs
│           ├── create.rs
│           ├── publish.rs
│           └── ... (17 commands)
│
├── kyoto-core/                     # Core rendering infrastructure
│   └── src/
│       ├── lib.rs
│       ├── pipeline.rs             # Render pipeline orchestration
│       ├── project.rs              # Project detection & loading
│       └── context.rs              # Shared context types
│
├── kyoto-engines/                  # Engine system
│   └── src/
│       ├── lib.rs
│       ├── engine.rs               # Engine trait
│       ├── jupyter/                # Jupyter engine
│       ├── knitr/                  # Knitr engine
│       ├── julia/                  # Julia engine
│       └── markdown/               # Plain markdown
│
├── kyoto-filters/                  # Pandoc filter system
│   └── src/
│       ├── lib.rs
│       ├── filter.rs               # Filter trait & infrastructure
│       ├── lua.rs                  # Lua filter support
│       └── builtin/                # Built-in filters
│
├── kyoto-handlers/                 # Diagram & special content handlers
│   └── src/
│       ├── lib.rs
│       ├── mermaid.rs
│       ├── graphviz.rs
│       └── tikz.rs
│
├── kyoto-formats/                  # Output format definitions
│   └── src/
│       ├── lib.rs
│       ├── html/
│       ├── pdf/
│       ├── docx/
│       └── reveal/
│
├── kyoto-config/                   # Configuration system
│   └── src/
│       ├── lib.rs
│       ├── merge.rs                # Config merging with source tracking
│       ├── schema.rs               # Schema definitions
│       └── validate.rs             # Validation
│
├── kyoto-yaml/                     # YAML infrastructure
│   └── src/
│       ├── lib.rs
│       ├── annotated.rs            # AnnotatedParse
│       ├── mapped.rs               # MappedString equivalent
│       └── schema.rs               # YAML intelligence
│
├── kyoto-lsp/                      # LSP server
│   └── src/
│       ├── main.rs                 # LSP binary
│       ├── handlers/               # LSP request handlers
│       └── features/               # LSP features
│
├── kyoto-extensions/               # Extension system
│   └── src/
│       ├── lib.rs
│       ├── loader.rs
│       └── registry.rs
│
└── kyoto-util/                     # Shared utilities
    └── src/
        ├── lib.rs
        ├── paths.rs
        ├── platform.rs
        └── source_info.rs          # Unified source location system
```

### Rationale for This Structure

**1. Workspace Architecture (Pattern 3)**
- **Parallel Compilation:** 10+ crates compile in parallel → faster builds
- **Clear Boundaries:** Each crate has single responsibility
- **Reusability:** Other tools can use `kyoto-yaml`, `kyoto-engines`, etc.
- **Testing Isolation:** Test engines without CLI, test filters without engines
- **Incremental Migration:** Can port one crate at a time from TypeScript

**2. Commands Directory (Pattern 1)**
- **Many Subcommands:** 17 commands is too many for flat structure
- **Clear Location:** Obvious where to add/modify commands
- **Parallel Development:** Different people can own different commands
- **Separation:** Commands handle CLI → core handles logic

**3. Thin CLI Binary**
- `kyoto/src/main.rs` is minimal (like turborepo)
- Real work happens in crates
- CLI just parses args and delegates

**4. Feature-Based Crates**
- Each major subsystem (engines, filters, formats) is separate crate
- Follows ripgrep's evolution toward modularity
- Enables third-party engine contributions (user requirement)

### Implementation Phases

**Phase 1: Foundation (MVP)**
```
crates/
├── kyoto/              # CLI with basic commands
├── kyoto-core/         # Minimal pipeline
└── kyoto-util/         # Shared types
```

**Phase 2: Core Features**
```
+ kyoto-engines/        # Engine system + markdown engine
+ kyoto-config/         # Config loading
+ kyoto-yaml/           # YAML infrastructure
```

**Phase 3: Advanced Features**
```
+ kyoto-filters/        # Filter system
+ kyoto-handlers/       # Diagram handlers
+ kyoto-formats/        # Output formats
```

**Phase 4: Ecosystem**
```
+ kyoto-lsp/            # LSP server
+ kyoto-extensions/     # Extension system
```

### CLI Command Organization Details

**Main Binary (kyoto/src/main.rs):**
```rust
use clap::Parser;

mod commands;
mod config;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Build context from config sources
    let ctx = config::Context::new()
        .load_system_config()?
        .load_user_config()?
        .load_project_config()?
        .load_env()?
        .load_cli(&cli);

    // Dispatch to command
    match cli.command {
        Commands::Render(args) => commands::render::run(&ctx, args),
        Commands::Preview(args) => commands::preview::run(&ctx, args),
        // ... etc
    }
}
```

**Command Implementation (kyoto/src/commands/render.rs):**
```rust
use clap::Args;
use anyhow::Result;
use kyoto_core::pipeline::Pipeline;

#[derive(Args)]
pub struct RenderArgs {
    /// Input file or project
    pub input: Option<PathBuf>,

    /// Output format
    #[arg(short, long)]
    pub to: Option<String>,

    /// Execute computations
    #[arg(long)]
    pub execute: bool,
}

pub fn run(ctx: &Context, args: RenderArgs) -> Result<()> {
    // 1. Resolve input
    let input = resolve_input(&args.input, ctx)?;

    // 2. Build pipeline (from kyoto-core)
    let pipeline = Pipeline::new(ctx)
        .with_input(input)
        .with_format(args.to)
        .with_execution(args.execute);

    // 3. Execute
    pipeline.run()?;

    Ok(())
}
```

**Core Implementation (kyoto-core/src/pipeline.rs):**
```rust
// This is where the actual work happens
pub struct Pipeline {
    // ...
}

impl Pipeline {
    pub fn run(&self) -> Result<()> {
        // Parse QMD
        let ast = self.parse_input()?;

        // Run engine
        let rendered = self.run_engine(ast)?;

        // Apply handlers
        let processed = self.apply_handlers(rendered)?;

        // Run filters
        let filtered = self.run_filters(processed)?;

        // Postprocess
        let output = self.postprocess(filtered)?;

        // Write output
        self.write_output(output)?;

        Ok(())
    }
}
```

### Why Not Other Patterns?

**Pattern 2 (Flat Structure):**
- ❌ Too many commands (17+) would clutter `src/`
- ❌ No clear ownership boundaries for large team
- ❌ Harder to extract reusable components
- ✅ Would work for smaller tool (5-7 commands)

**Single Crate (No Workspace):**
- ❌ Slower compilation (can't parallelize)
- ❌ Harder to reuse components
- ❌ Testing requires full tool
- ❌ Can't incrementally migrate

**Pattern 4 Only (Commands + Ops):**
- ❌ Single crate gets huge (100,000+ LOC)
- ❌ No parallelization benefits
- ✅ Could work if combined with workspace

### Additional Considerations

**1. Engine Extensibility (User Requirement)**

To allow third-party engines:

```rust
// kyoto-engines/src/engine.rs
pub trait Engine {
    fn name(&self) -> &str;
    fn execute(&self, input: &str) -> Result<String>;
}

// Third-party crate can implement:
impl Engine for CustomEngine { ... }
```

Register via:
- Config file: `engines.custom = "/path/to/engine"`
- Extension API: `kyoto_extensions::register_engine()`

**2. Parallelization (User Requirement)**

With modular crates, can parallelize:
- Multiple file rendering (use rayon)
- Engine execution (async/await)
- Filter application (parallel where safe)

Example:
```rust
use rayon::prelude::*;

files.par_iter()
    .map(|file| pipeline.render(file))
    .collect::<Result<Vec<_>>>()?;
```

**3. Pipeline Flexibility (User Requirement)**

Instead of hardcoded pipeline, use builder pattern:

```rust
Pipeline::new(ctx)
    .stage(ParseStage)
    .stage(EngineStage)
    .stage(HandlerStage)
    .stage(FilterStage)
    .stage(PostprocessStage)
    .run()?;
```

Allow engines/extensions to insert custom stages.

## Comparison: Patterns vs Kyoto Needs

| Pattern | Commands | Modularity | Parallel Build | Reusability | Learning Curve | Verdict for Kyoto |
|---------|----------|------------|----------------|-------------|----------------|-------------------|
| 1: Commands Dir | ⭐⭐⭐ Excellent | ⭐⭐ Good | ⭐ Poor | ⭐⭐ Good | ⭐⭐⭐ Easy | Good but needs workspace |
| 2: Flat | ⭐⭐ OK | ⭐⭐ Good | ⭐ Poor | ⭐ Poor | ⭐⭐⭐ Very Easy | Too simple for Kyoto |
| 3: Workspace | ⭐⭐⭐ Excellent | ⭐⭐⭐ Excellent | ⭐⭐⭐ Excellent | ⭐⭐⭐ Excellent | ⭐⭐ Moderate | **Best for Kyoto** |
| 4: Commands+Ops | ⭐⭐⭐ Excellent | ⭐⭐ Good | ⭐ Poor | ⭐⭐⭐ Excellent | ⭐⭐ Moderate | Good, combine with workspace |
| **Recommendation** | **Pattern 3 + 1** | | | | | **Workspace + Commands** |

## Action Items for Kyoto

### Immediate (Setup)
1. Create workspace structure with initial crates
2. Set up `kyoto/src/commands/` directory
3. Implement basic CLI with clap derive
4. Create Context struct for config management

### Short Term (Foundation)
1. Port 2-3 simple commands to validate pattern
2. Implement basic pipeline in `kyoto-core`
3. Create engine trait in `kyoto-engines`
4. Build config loading in `kyoto-config`

### Medium Term (Core Features)
1. Port all 17 commands
2. Implement markdown engine
3. Build filter system
4. Add handler system

### Long Term (Ecosystem)
1. Extract reusable crates for community
2. Build extension system
3. Implement LSP server
4. Document third-party engine API

## References

### Blog Posts & Articles
- [CLI Structure in Rust - Kevin K](https://kbknapp.dev/cli-structure-01/) - Context struct pattern
- [Tour de Just - Casey Rodarmor](https://casey.github.io/blog/tour-de-just/) - Flat structure philosophy
- [Ripgrep Code Review - mbrt blog](https://blog.mbrt.dev/posts/ripgrep/) - Modular design lessons

### Official Documentation
- [Cargo Contributor Guide - Subcommands](https://doc.crates.io/contrib/implementation/subcommands.html)
- [The Rustup Book](https://rust-lang.github.io/rustup/)
- [Command Line Applications in Rust](https://rust-cli.github.io/book/)

### Source Code Repositories
- [cargo](https://github.com/rust-lang/cargo) - Pattern 1 + 4 example
- [rustup](https://github.com/rust-lang/rustup) - Standard structure
- [just](https://github.com/casey/just) - Pattern 2 example
- [turborepo](https://github.com/vercel/turborepo) - Pattern 3 example
- [ripgrep](https://github.com/BurntSushi/ripgrep) - Modularity example
- [git-cliff](https://github.com/orhun/git-cliff) - Simple workspace

### Rust CLI Ecosystem
- [clap](https://github.com/clap-rs/clap) - CLI argument parsing
- [clap examples](https://github.com/clap-rs/clap/tree/master/examples) - See `git-derive.rs`

## Conclusion

Based on analysis of 6 major Rust CLI tools, the recommended architecture for Kyoto is:

**Workspace Architecture (turborepo/ripgrep style) + Commands Directory (cargo style)**

This provides:
- ✅ Clear organization for 17+ subcommands
- ✅ Parallel compilation via workspace
- ✅ Reusable components for ecosystem
- ✅ Extensibility for third-party engines
- ✅ Testing isolation
- ✅ Incremental migration from TypeScript

The pattern is proven at scale by turborepo (similar complexity) and cargo (similar command count), while maintaining the modularity of ripgrep for ecosystem growth.

**Next Step:** Create initial workspace structure with `kyoto/`, `kyoto-core/`, and `kyoto-util/` crates, then implement 2-3 commands to validate the pattern.
