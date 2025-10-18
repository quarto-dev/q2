## Implications for Rust Port

### 1. Metadata Merging is Critical

The TypeScript implementation uses lodash's `mergeWith` extensively. The Rust port needs:

```rust
// From config-merging-analysis.md
pub fn merge_configs(base: Metadata, override: Metadata) -> Metadata {
  // Strategy 4: AnnotatedParse merge (recommended)
  // Eagerly merge while preserving source info
}
```

**See:** `claude-notes/config-merging-analysis.md` for full design.

### 2. Engine Architecture Should Be Trait-Based

```rust
pub trait Engine {
  fn name(&self) -> &str;
  fn claims_file(&self, path: &Path, ext: &str) -> bool;
  fn claims_language(&self, lang: &str) -> bool;

  async fn target(&self, file: &Path, project: &Project) -> Result<ExecutionTarget>;
  async fn execute(&self, options: ExecuteOptions) -> Result<ExecuteResult>;

  // Optional hooks
  async fn dependencies(&self, options: DependenciesOptions) -> Result<Dependencies> {
    Ok(Dependencies::default())
  }

  async fn postprocess(&self, options: PostprocessOptions) -> Result<()> {
    Ok(())
  }
}

// Registration
pub fn register_engine(engine: Box<dyn Engine>) {
  ENGINES.lock().unwrap().insert(engine.name().to_string(), engine);
}
```

**See:** `claude-notes/rust-cli-organization-patterns.md` for workspace architecture.

### 3. Pandoc Invocation Needs Careful Translation

The TypeScript code builds complex defaults files and filter chains. The Rust port should:

- Generate YAML defaults files (using `serde_yaml`)
- Pass base64-encoded filter parameters via environment
- Handle temp file management carefully

### 4. HTML Postprocessing Requires DOM Manipulation

TypeScript uses `deno-dom`. Rust options:

```rust
// Option 1: html5ever + scraper (recommended)
use scraper::{Html, Selector};

let document = Html::parse_document(&html_string);
let selector = Selector::parse("div.output").unwrap();

for element in document.select(&selector) {
  // Modify element
}

// Option 2: kuchiki
// Option 3: tl (fast but less featured)
```

**See:** `claude-notes/js-runtime-dependencies.md` for HTML handling analysis.

### 5. Source Tracking is Essential

`MappedString` equivalent needed in Rust:

```rust
pub struct MappedString {
  pub value: String,
  pub source_info: SourceInfo,  // From unified-source-location-design.md
}

impl MappedString {
  pub fn substring(&self, start: usize, end: usize) -> MappedString {
    MappedString {
      value: self.value[start..end].to_string(),
      source_info: self.source_info.transform(Transformation::Substring { start, end }),
    }
  }

  pub fn concat(&self, other: &MappedString) -> MappedString {
    MappedString {
      value: format!("{}{}", self.value, other.value),
      source_info: self.source_info.transform(Transformation::Concat {
        other: Box::new(other.source_info.clone())
      }),
    }
  }
}
```

**See:** `claude-notes/unified-source-location-design.md` for complete design.

### 6. Modular Crate Structure

Based on the analysis, recommended crate structure:

```
crates/
├── kyoto/                      # CLI binary
│   └── src/commands/
│       ├── render.rs
│       └── ...
├── kyoto-core/                 # Core rendering
│   └── src/
│       ├── pipeline.rs
│       └── ...
├── kyoto-engines/              # Engine system
│   └── src/
│       ├── engine.rs           # Engine trait
│       ├── jupyter/
│       ├── knitr/
│       └── markdown/
├── kyoto-formats/              # Format definitions
├── kyoto-filters/              # Filter system
├── kyoto-handlers/             # Language handlers
├── kyoto-yaml/                 # YAML infrastructure
└── kyoto-util/                 # Shared utilities (SourceInfo, etc.)
```

**See:** `claude-notes/rust-cli-organization-patterns.md` for complete architecture.

