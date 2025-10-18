# Error Reporting and Console Print Subsystem: Design Research

## Executive Summary

This report analyzes three key sources of inspiration for designing a new error-reporting and console print subsystem for Quarto:

1. **Ariadne** - A Rust crate for compiler diagnostics (already in use in quarto-markdown)
2. **R 'cli' package** - A structured text approach to console output
3. **Tidyverse style guide** - Best practices for error messages

The proposed subsystem should combine ariadne's visual error reporting with structured Markdown-based message construction (inspired by cli), following tidyverse-style message guidelines.

---

## 1. Ariadne: Visual Error Reporting

### Overview

Ariadne is a Rust crate for creating sophisticated compiler diagnostics with colorful, visually appealing error messages.

**Official resources:**
- Docs: https://docs.rs/ariadne/latest/ariadne/
- GitHub: https://github.com/zesterer/ariadne

### Core Capabilities

1. **Multi-line and inline error labeling**
   - Can span multiple lines and files
   - Supports arbitrary configurations of spans

2. **Rich visual formatting**
   - 8-bit and 24-bit color support
   - Automatic color generation for distinct elements
   - Handles variable-width characters (tabs, Unicode)

3. **Structured error reports**
   - Title, message, and multiple labeled spans
   - Different report kinds (Error, Warning, etc.)
   - Optional error codes

### API Design

Basic usage pattern:

```rust
Report::build(ReportKind::Error, ("sample.rs", 12..12))
    .with_code(3)
    .with_message("Incompatible types")
    .with_label(
        Label::new(("sample.rs", 32..33))
            .with_message("This is of type Nat")
            .with_color(Color::Red)
    )
    .with_label(
        Label::new(("sample.rs", 50..51))
            .with_message("Expected type Str")
            .with_color(Color::Blue)
    )
    .finish()
    .print(("sample.rs", Source::from(source_code)))
```

**Key design elements:**
- Builder pattern for constructing reports
- Generic over span types (file + byte range)
- Separate construction from rendering
- `Source` abstraction for input text

### Usage in quarto-markdown (Parser-Level Errors)

**Important distinction:** The quarto-markdown error system handles **LALR parser errors** - a specialized, lower-level concern separate from the higher-level Quarto error reporting we're designing here.

Location: `quarto-markdown-pandoc/src/readers/qmd_error_messages.rs`

The markdown parser uses:
- Jeffery's TOPLAS 2003 paper approach for LR parser error messages
- Error corpus mapping parse states to diagnostics
- ariadne for rendering these specialized parser errors

**This is a separate system** - both use ariadne for rendering, but:
- **Parser errors** = syntax errors in markdown (LALR-specific)
- **Quarto errors** = runtime/semantic errors in Quarto operations (general-purpose)

These two systems should **not share design decisions or code** unless there's clear evidence of benefit.

### Key Takeaway: ariadne as Rendering Layer

What we learn from quarto-markdown's usage:

**ariadne provides:**
- Beautiful compiler-quality visual output
- Multiple labeled spans on source code
- Both human-readable and structured output
- Clean separation between error data and rendering

**Example output:**
```
Error: Unclosed Span
  ┌─ document.qmd:1:4
  │
1 │ an [unclosed span
  │    ^ I reached the end before finding a closing ']'
  │    │
  │    This is the opening bracket
```

**For our purposes:** Use ariadne as the **rendering layer** for Quarto's general error messages, not its specialized parser error system.

---

## 2. R 'cli' Package: Structured Text Output

### Overview

The 'cli' R package provides a comprehensive toolkit for building sophisticated command-line interfaces with semantic, structured text output.

**Official resource:** https://cli.r-lib.org/

### Design Philosophy

**Core principle:** Build messages from semantic elements rather than plain strings.

**Key benefits:**
1. Consistent formatting across the application
2. Automatic styling via themes
3. Context-aware text formatting
4. Easier internationalization

### Core Features

#### 1. Semantic Output Elements

Rather than `print()` or `cat()`, use semantic constructors:

- **Headings:** `cli_h1()`, `cli_h2()`, `cli_h3()`
- **Lists:** `cli_ul()`, `cli_ol()` with nesting support
- **Alerts:** `cli_alert_success()`, `cli_alert_info()`, `cli_alert_warning()`, `cli_alert_danger()`
- **Paragraphs:** `cli_text()`, `cli_par()`

#### 2. Markup Dialect

Inline markup for styling within text:

```r
cli_text("Installing {.pkg packagename} version {.val 1.2.3}")
cli_text("See {.file ~/.config/app/settings.json} for configuration")
cli_text("Run {.code install.packages('pkg')} to install")
```

**Semantic classes:**
- `.pkg` - package names
- `.file` - file paths
- `.code` - code snippets
- `.val` - values
- `.arg` - arguments
- `.fun` - function names
- `.url` - URLs

#### 3. CSS-like Theming

Allows customization of how semantic elements appear:

```r
theme <- list(
  ".pkg" = list(color = "blue"),
  ".file" = list(color = "cyan", "font-style" = "italic")
)
```

#### 4. Advanced Text Features

- **Glue-style interpolation:** Variables automatically interpolated
- **Pluralization:** Automatic singular/plural handling
- **Progress bars:** Built-in progress indicators
- **Unicode/ASCII compatibility:** Graceful fallback

### Example Usage

```r
# Semantic alerts
cli_alert_success("Package installed successfully")
cli_alert_danger("Installation failed: network timeout")

# Structured lists
cli_h2("Installation steps:")
cli_ol(c(
  "Download package source",
  "Verify checksums",
  "Install dependencies",
  "Build from source"
))

# Rich formatting
cli_text("Found {.val {n_errors}} error{?s} in {.file {filename}}")
```

### Why This Matters

**Contrast with string concatenation:**

```r
# Traditional approach (bad)
message(paste0("Error: ", error_msg, " in file ", filename, " at line ", line_num))

# cli approach (good)
cli_alert_danger("Error: {error_msg}")
cli_text("Location: {.file {filename}}:{.val {line_num}}")
```

Benefits:
- Consistent formatting
- Easier to maintain
- Theme-able
- Better internationalization
- Self-documenting code

---

## 3. Tidyverse Style Guide: Error Message Best Practices

### Overview

The tidyverse style guide (https://style.tidyverse.org/errors.html) provides clear, well-tested guidelines for writing effective error messages.

### Core Philosophy

**Principle:** Help users quickly understand and resolve issues.

**Goals:**
1. Clear problem statement
2. Specific error details
3. Actionable hints when appropriate
4. Consistent formatting

### The Four-Part Error Structure

#### 1. Problem Statement

**Rules:**
- Start with a general, concise statement
- Use sentence case, end with full stop
- Use "must" for requirements or "can't" for impossibilities
- Be specific about types/expectations

**Examples:**

```r
# Good
"`n` must be a numeric vector, not a character vector."
"Can't combine date and datetime types."

# Bad
"Invalid input."  # Too vague
"Error: Wrong type"  # Not specific enough
```

#### 2. Error Location

**Rule:** Mention the specific function or expression causing the error when possible.

**Example:**
```r
"Error in `mutate()`:"
"! `x` must be numeric, not character."
```

#### 3. Error Details

**Format:** Use bulleted lists with symbols:
- `x` (cross bullet) - Problems/errors
- `i` (info bullet) - Additional information

**Rules:**
- Keep sentences short and specific
- Reveal location, name, or content of problematic input
- Limit to 5 problems (avoid overwhelming users)

**Example:**

```r
"! Incompatible lengths:
✖ `x` has length 3.
✖ `y` has length 5."
```

#### 4. Hints (Optional)

**When to include:**
- Problem source is clear and common
- Fix is straightforward

**Format:**
- Use info bullet (`i`)
- End with a question mark if suggesting action

**Example:**

```r
"! Could not find function `summarise()`.
i Did you mean `summarize()`?"
```

### Formatting Guidelines

1. **Sentence case** throughout
2. **Prefer singular** forms
3. **Backticks** around code/argument names
4. **Under 80 characters** per component
5. **Let CLI handle wrapping** (don't pre-wrap)
6. **Simple language** for easier translation

### Complete Example

```r
map_int(1:5, ~ "x")

# Error output:
Error:
! Each result must be a single integer.
✖ Result 1 is a character vector.
```

Better yet, with context:

```r
# Error in map_int():
! Each result must be a single integer.
✖ Result 1 is a character vector.
i Did you mean to use `map_chr()` instead?
```

### Recommended Tool: `cli::cli_abort()`

```r
cli_abort(c(
  "Each result must be a single integer.",
  x = "Result {i} is a {type}.",
  i = "Did you mean to use `map_chr()` instead?"
))
```

**Benefits:**
- Automatic bullet formatting
- Glue-style interpolation
- Inline markup support
- Error chaining

---

## 4. Synthesis: Design Considerations for Quarto

### Proposed Architecture

#### Layer 1: Message Construction (Markdown-based)

**Inspiration:** R 'cli' package's structured approach, but using Markdown instead of function calls.

**Concept:** Build messages from Markdown (or Pandoc AST) rather than strings.

**Example structure:**

```rust
ErrorMessage::new()
    .with_title("Unclosed code block")
    .with_problem_statement(md!("Code block started but never closed."))
    .with_detail(md!("The code block starting with `` ```python `` was never closed."))
    .with_hint(md!("Did you forget the closing `` ``` ``?"))
    .with_location(source_span)
```

Or using Pandoc AST from quarto-markdown:

```rust
let message_ast = parse_markdown_inline(
    "The code block starting with `{python}` was never closed."
);

ErrorMessage::new()
    .with_title("Unclosed code block")
    .with_problem_content(message_ast)
    .with_location(source_span)
```

#### Layer 2: Error Reporting (ariadne-based)

**Inspiration:** Current quarto-markdown approach

**Concept:** Render structured messages using ariadne for visual output.

```rust
// Convert ErrorMessage to ariadne Report
let report = error_message.to_ariadne_report();
report.print(sources);

// Or to JSON for programmatic use
let json = error_message.to_json();
```

#### Layer 3: Console Output (Semantic Elements)

**Inspiration:** R 'cli' package's semantic elements

**Concept:** Provide high-level console output primitives.

```rust
console.success("Package installed successfully");
console.warning("Using experimental feature");
console.info("Processing {} files...", count);

console.heading1("Build Results");
console.bullet_list(&[
    "✓ TypeScript compiled",
    "✓ Tests passed",
    "⚠ 2 warnings",
]);
```

### Key Design Questions

#### Question 1: Markdown Dialect for Messages ✓ DECIDED

**Decision:** Use Markdown (not a custom mini-dialect)

- **API surface**: Accept plain Markdown strings
- **Internal representation**: Pandoc AST

**Potential optimization**: Compile-time macro that parses Markdown to Pandoc AST

**Open question**: Can we use Rust procedural macros to parse Markdown at compile time?

```rust
// Proposed macro usage
.with_message(md!("The `x` parameter must be numeric"))
// Expands at compile time to:
.with_message(PandocAst { /* constructed AST */ })
```

**Considerations:**
- Avoids runtime parsing overhead for static messages
- Type-safe at compile time
- Potential bootstrapping complexity (see analysis below)

---

### Compile-Time Markdown Macro: Technical Analysis

**Answer: Yes, Rust macros can run code at compile time!**

#### Macro Types

Rust has two types of macros:

1. **Declarative macros** (`macro_rules!`) - Pattern matching, limited
2. **Procedural macros** - Can run arbitrary Rust code during compilation

For parsing Markdown to Pandoc AST, we'd use a **procedural macro**.

#### How It Would Work

```rust
// In quarto-markdown-macros crate
use proc_macro::TokenStream;
use quote::quote;

#[proc_macro]
pub fn md(input: TokenStream) -> TokenStream {
    // 1. Extract the string literal at compile time
    let markdown_str = parse_string_literal(input);

    // 2. Parse Markdown to Pandoc AST (at compile time!)
    let ast = quarto_markdown::parse_inline(&markdown_str)
        .expect("Invalid Markdown in md! macro");

    // 3. Generate Rust code that constructs the AST
    let generated_code = ast_to_rust_code(&ast);

    // 4. Return the generated code
    generated_code.into()
}
```

**Usage:**

```rust
use quarto_markdown_macros::md;

let message = md!("The `x` parameter must be **numeric**");
// Expands to:
let message = Inlines(vec![
    Inline::Str(Str { text: "The ".into(), source_info: ... }),
    Inline::Code(Code { text: "x".into(), attr: ..., source_info: ... }),
    Inline::Str(Str { text: " parameter must be ".into(), ... }),
    Inline::Strong(Strong {
        content: vec![Inline::Str(Str { text: "numeric".into(), ... })],
        ...
    }),
]);
```

#### Benefits

1. **Zero runtime overhead** - Parsing happens during compilation
2. **Compile-time validation** - Invalid Markdown = compilation error
3. **Type safety** - Directly constructs typed AST
4. **Developer experience** - Write natural Markdown strings

#### Potential Issues

##### 1. **Bootstrapping Complexity**

The macro crate needs quarto-markdown as a *build dependency*:

```toml
# quarto-markdown-macros/Cargo.toml
[dependencies]
proc-macro2 = "1.0"
quote = "1.0"

[build-dependencies]
quarto-markdown-pandoc = { path = "../quarto-markdown-pandoc" }
```

**Circular dependency risk:**
```
quarto-core → depends on → quarto-markdown
quarto-markdown → depends on → quarto-core (for error types)
                               ↑
                               This creates a cycle!
```

**Solutions:**

A. **Separate error types crate**
   ```
   quarto-types (error types, basic AST)
   ├── quarto-markdown (parser)
   └── quarto-core (uses both)
       └── quarto-markdown-macros (uses quarto-markdown at build time)
   ```

B. **Macro in separate leaf crate**
   ```
   quarto-markdown-pandoc (parser, no dependencies on quarto-core)
   quarto-core (main functionality)
   quarto-markdown-macros (depends on quarto-markdown-pandoc only)
   quarto (top-level, uses all)
   ```

##### 2. **Build Time Impact**

- Procedural macros run during compilation
- Each `md!()` invocation runs the parser
- For many messages, could slow down builds
- *Mitigation*: Parser is fast, likely negligible impact

##### 3. **Error Messages**

When the macro fails (invalid Markdown), error points to macro invocation:

```rust
let msg = md!("Unclosed `code");
//        ^^^^^^^^^^^^^^^^^^^ error: Unclosed code span at position 13
```

This is actually quite good for developer experience!

##### 4. **Source Information**

Generated AST nodes won't have meaningful `SourceInfo`:

```rust
// The SourceInfo will be synthetic, not pointing to real file locations
Inline::Code(Code {
    text: "x".into(),
    source_info: SourceInfo::synthetic(), // Not a real location
    ...
})
```

This is fine for error *messages*, but not for error *locations*.

#### Alternative: Runtime Parsing with Caching

If compile-time macros prove too complex, we can:

```rust
// Parse once at first use, cache the result
lazy_static! {
    static ref ERROR_MESSAGES: HashMap<&'static str, Inlines> = {
        let mut map = HashMap::new();
        map.insert(
            "invalid_type",
            parse_markdown_inline("The `x` parameter must be numeric")
        );
        // ... more messages
        map
    };
}
```

Or even simpler:

```rust
// Parse on first access, cache thereafter
pub fn error_message(key: &str) -> &'static Inlines {
    static CACHE: OnceLock<HashMap<String, Inlines>> = OnceLock::new();
    CACHE.get_or_init(|| {
        // Parse all error messages once
    }).get(key).unwrap()
}
```

#### Recommendation ✓ DECIDED

**Start without the macro**, use runtime parsing:

1. Keep it simple initially
2. Measure if parsing overhead is actually a problem
3. Add the `md!()` macro later if needed

**Advantages of waiting:**
- No bootstrapping complexity
- Simpler build process
- Can use dynamic error messages (runtime variables)
- Still fast enough (modern parsers are quick)

**When to add macro:**
- If profiling shows Markdown parsing is a bottleneck
- If we want compile-time validation of all error messages
- Once crate structure is stable (avoid circular deps)

**Decision: Defer compile-time macros. Use Markdown strings with runtime parsing initially.**

---

#### Question 2: Scope ✓ DECIDED

**Decision: Rust-only**

- Design for Rust idioms and ergonomics
- No need for cross-language compatibility in the API
- If cross-language integration is needed, use WASM (same approach as quarto-markdown)

**Implications:**
- Can use Rust-specific features freely (traits, type system, etc.)
- Simpler design without lowest-common-denominator constraints
- TypeScript/Lua can call via WASM bindings if needed

#### Question 3: Semantic Markup for Inline Content ✓ DECIDED

**Decision: Use Pandoc spans with classes and attributes**

Messages will use standard Pandoc Markdown syntax for semantic inline markup:

```rust
error_message(
    "Could not find file `config.yaml`{.file} in directory `/home/user/.config`{.path}"
)
```

**Why this works well:**

1. **Native Pandoc syntax** - Already supported by quarto-markdown parser
2. **Familiar to Quarto users** - Same syntax used throughout Quarto documents
3. **Parsed to structured AST**:
   ```rust
   Inline::Code(Code {
       text: "config.yaml",
       attr: Attr { classes: vec!["file"], ... }
   })
   ```
4. **Extensible** - Can define semantic classes as needed
5. **Backward compatible** - Plain backticks work fine, classes are optional

**Example semantic classes (TBD):**
- `.file` - filenames and paths
- `.engine` - engine names (jupyter, knitr, julia)
- `.format` - output formats (html, pdf, docx)
- `.option` - YAML option names
- `.extension` - Quarto extensions
- `.code` - generic code (default if no class specified)

**Benefits:**
- Writers (ANSI, HTML) can style based on semantic class
- Consistent styling across all Quarto error messages
- Can add highlighting, colors, or even interactivity (clickable paths in HTML)
- Same infrastructure Quarto already uses for documents

#### Question 4: Theming and Styling ✓ DECIDED

**Decision: Defer theming decision**

**Approach:**
- For themable output needs: Build HTML writer from Pandoc AST
- For terminal output: Build ANSI writer that works directly from Markdown/Pandoc AST

**Benefits:**
- Markdown/AST representation enables multiple output formats
- HTML writer allows CSS theming naturally
- ANSI writer produces nice terminal output directly
- Keeps core system simple
- Theming can be added later through output format plugins

**Implementation path:**
1. Start with ANSI terminal output (via ariadne for errors, custom writer for console)
2. Add HTML writer when needed for themable contexts
3. Future: Add more output formats as needed (JSON, XML, etc.)

#### Question 4: Integration with Existing Systems

**How should this relate to current error handling?**

1. **quarto-markdown's ariadne errors** - Already working well
2. **quarto-cli's TypeScript errors** - Currently ad-hoc
3. **Lua filter errors** - Currently opaque
4. **Engine errors (Jupyter, Knitr)** - Need better formatting

**Question for you:** Should this subsystem:
- Replace all error output? (ambitious)
- Start with Rust code only? (incremental)
- Provide a library that all systems can use? (modular)

#### Question 5: Error Message Guidelines ✓ DECIDED

**Decision: Encode tidyverse guidelines in the API design**

Use builder methods that naturally encourage the tidyverse four-part structure:

```rust
DiagnosticMessage::builder()
    .error("Incompatible types")           // Title
    .problem("Cannot combine date and datetime types")  // Problem statement
    .add_detail("`x`{.arg} has type `date`{.type}")    // Error details
    .add_detail("`y`{.arg} has type `datetime`{.type}")
    .add_hint("Convert both to the same type?")         // Hint
    .at_location(span)                     // Source location
    .build()
```

**Why this works:**

1. **Structured API** - Separate methods for problem/detail/hint guide developers
2. **Type-safe** - Rust compiler enforces structure
3. **Flexible output** - Everything becomes Pandoc AST, which can be:
   - Processed programmatically before rendering
   - Transformed or filtered
   - Rendered to multiple formats
4. **Documentation by API** - Method names teach the pattern
5. **Optional enforcement** - `.build()` can validate message structure

**Additional support:**
- Style guide documentation (CONTRIBUTING-ERRORS.md)
- Examples in API docs
- Optional linting in CI (check for problem statements, etc.)

**Key insight:** Since the final representation is Pandoc AST, we can post-process messages programmatically (add context, transform for locale, etc.) before rendering to ANSI/HTML/JSON.

#### Question 6: Machine-Readable Output

**Current quarto-markdown supports both visual and JSON output.**

Should the broader system:
- Always generate both formats?
- Support other formats (XML, LSP diagnostics)?
- Allow custom formatters?

**Question for you:** What's the priority for machine-readable error formats beyond JSON?

### Proposed Minimal Implementation

#### Phase 1: Core Error Types (Rust)

```rust
pub struct DiagnosticMessage {
    title: String,
    kind: DiagnosticKind,  // Error, Warning, Info
    problem: MessageContent,
    details: Vec<DetailItem>,
    hints: Vec<MessageContent>,
    source_spans: Vec<SourceSpan>,
}

pub enum MessageContent {
    Plain(String),
    Markdown(String),
    // Future: PandocAst(Box<Inlines>)
}

pub struct DetailItem {
    kind: DetailKind,  // Error, Info, Note
    content: MessageContent,
    span: Option<SourceSpan>,
}
```

#### Phase 2: ariadne Integration

```rust
impl DiagnosticMessage {
    pub fn to_ariadne_report(&self) -> ariadne::Report {
        let mut report = Report::build(
            self.kind.to_ariadne_kind(),
            self.primary_source_file(),
            self.primary_byte_offset()
        )
        .with_message(&self.title)
        .with_label(self.primary_label());

        for detail in &self.details {
            if let Some(span) = &detail.span {
                report = report.with_label(detail.to_ariadne_label(span));
            }
        }

        report.finish()
    }

    pub fn to_json(&self) -> serde_json::Value {
        // Similar to current quarto-markdown approach
    }
}
```

#### Phase 3: Console Output Helpers

```rust
pub struct Console {
    color_enabled: bool,
}

impl Console {
    pub fn error(&self, message: &DiagnosticMessage) {
        // Use ariadne for error messages with source context
        message.to_ariadne_report().print(self.sources());
    }

    pub fn success(&self, text: &str) {
        // Simple ANSI output
        println!("{} {}", "✓".green(), text);
    }

    pub fn print_markdown(&self, md: &str) {
        // Parse Markdown and render with ANSI writer
        let ast = parse_markdown_inline(md);
        let output = AnsiWriter::new(self.color_enabled).render(&ast);
        println!("{}", output);
    }

    pub fn heading(&self, level: u8, text: &str) {
        // Styled headings via ANSI
    }

    pub fn list(&self, items: &[&str]) {
        for item in items {
            println!("  • {}", item);
        }
    }
}

// Custom ANSI writer for Pandoc AST
pub struct AnsiWriter {
    color_enabled: bool,
}

impl AnsiWriter {
    pub fn render(&self, inlines: &Inlines) -> String {
        // Convert Pandoc AST to ANSI-styled text
        // - Code → monospace/color
        // - Strong → bold
        // - Emph → italic
        // - Link → underline + color
        // etc.
    }

    pub fn render_blocks(&self, blocks: &Blocks) -> String {
        // For rendering full documents (console messages with structure)
    }
}
```

**Output formats:**
- **ANSI terminal** (via AnsiWriter) - for console output
- **HTML** (via HtmlWriter) - for themable contexts, web views
- **JSON** (already supported by ariadne) - for machine-readable errors
- **Plain text** (strip formatting) - for non-TTY contexts

#### Phase 4: Message Builder with Guidelines

```rust
// Builder that encodes tidyverse guidelines in the API
pub struct DiagnosticMessageBuilder {
    kind: DiagnosticKind,
    title: String,
    problem: Option<MessageContent>,
    details: Vec<DetailItem>,
    hints: Vec<MessageContent>,
    source_spans: Vec<SourceSpan>,
}

impl DiagnosticMessageBuilder {
    // Create with error kind and title
    pub fn error(title: impl Into<String>) -> Self { /* ... */ }
    pub fn warning(title: impl Into<String>) -> Self { /* ... */ }

    // Problem statement - the "what" (must/can't)
    pub fn problem(mut self, stmt: impl Into<MessageContent>) -> Self {
        self.problem = Some(stmt.into());
        self
    }

    // Error details - the "where/why" (bulleted)
    pub fn add_detail(mut self, detail: impl Into<MessageContent>) -> Self {
        self.details.push(DetailItem {
            kind: DetailKind::Error,
            content: detail.into(),
            span: None,
        });
        self
    }

    // Info details (i bullets in tidyverse style)
    pub fn add_info(mut self, info: impl Into<MessageContent>) -> Self {
        self.details.push(DetailItem {
            kind: DetailKind::Info,
            content: info.into(),
            span: None,
        });
        self
    }

    // Hints - optional guidance (ends with ?)
    pub fn add_hint(mut self, hint: impl Into<MessageContent>) -> Self {
        self.hints.push(hint.into());
        self
    }

    // Source location
    pub fn at_location(mut self, span: SourceSpan) -> Self {
        self.source_spans.push(span);
        self
    }

    pub fn build(self) -> Result<DiagnosticMessage, BuildError> {
        // Optional validation:
        // - Has problem statement?
        // - Not too many details (max 5 per tidyverse)?
        // - Hints end with question mark?
        Ok(DiagnosticMessage {
            kind: self.kind,
            title: self.title,
            problem: self.problem.unwrap_or_default(),
            details: self.details,
            hints: self.hints,
            source_spans: self.source_spans,
        })
    }
}

// Convenient trait for converting strings to MessageContent
impl<T: Into<String>> From<T> for MessageContent {
    fn from(s: T) -> Self {
        // Parse Markdown string to Pandoc AST
        MessageContent::Markdown(s.into())
    }
}
```

**Example usage (following tidyverse guidelines):**

```rust
let error = DiagnosticMessage::builder()
    .error("Unclosed code block")
    .problem("Code block started but never closed")
    .add_detail("The code block starting with `` ```{python} `` was never closed")
    .at_location(opening_span)
    .add_hint("Did you forget the closing `` ``` ``?")
    .build()?;

console.error(&error);
```

**Output (via ariadne):**

```
Error: Unclosed code block
  ┌─ document.qmd:15:1
  │
15│ ```{python}
  │ ^^^^^^^^^^^ Code block started but never closed
  │
  = The code block starting with ```{python} was never closed
  = Did you forget the closing ```?
```

**Key features:**
- API naturally follows tidyverse four-part structure
- Method names are self-documenting
- Builder pattern encourages complete messages
- Validation optional but available
- Everything ends up as Pandoc AST for flexible processing

### Benefits of This Approach

1. **Builds on proven work** - ariadne already works well in quarto-markdown
2. **Structured messages** - Inspired by cli's semantic approach
3. **Good practices built-in** - API encourages tidyverse-style messages
4. **Multiple output formats** - Visual (ariadne) and JSON
5. **Markdown-centric** - Can eventually use Pandoc AST for consistency
6. **Incremental adoption** - Can start with Rust, expand to TypeScript/Lua

---

## 5. Open Questions for Discussion

### Decided ✓

1. **Message content format**: Markdown strings (runtime parsing), Pandoc AST internally. Defer compile-time macros.
2. **Scope**: Rust-only. Use WASM if cross-language integration needed.
3. **Theming**: Defer decision. Build HTML writer (CSS theming) and ANSI writer (terminal) from Pandoc AST.
4. **Semantic markup**: Use Pandoc span syntax with classes: `` `text`{.class} ``. Define semantic classes as needed (`.file`, `.engine`, `.format`, etc.)

5. **Guidelines enforcement**: API design! Use builder methods (`.problem()`, `.add_detail()`, `.add_hint()`) that encourage tidyverse-style structure. Since everything becomes Pandoc AST, messages can still be processed programmatically before output.

### Still Open (Lower Priority)

6. **Semantic class vocabulary**: Which specific classes should we standardize? (Can evolve during implementation)
7. **Integration strategy**: Replace existing systems or parallel implementation?
8. **Additional output formats**: Beyond ANSI, HTML, and JSON?
9. **Internationalization**: Design for i18n from the start?
10. **Custom error types**: Domain-specific message builders (e.g., `YAMLError`, `EngineError`)?

---

## 6. Recommended Next Steps

1. **Discuss design questions** - Make decisions on key architectural choices
2. **Prototype minimal version** - Implement Phase 1-2 in Rust
3. **Test with existing errors** - Rewrite some quarto-markdown errors using new system
4. **Evaluate ergonomics** - Is it easier to write good error messages?
5. **Document guidelines** - Create error message style guide for contributors
6. **Expand gradually** - Add Phase 3-4 features based on learnings

---

## 7. References

- **Ariadne documentation**: https://docs.rs/ariadne/latest/ariadne/
- **R cli package**: https://cli.r-lib.org/
- **Tidyverse style guide (errors)**: https://style.tidyverse.org/errors.html
- **Jeffery 2003 paper**: "Generating LR Syntax Error Messages from Examples"
- **quarto-markdown error handling**: `crates/quarto-markdown-pandoc/src/readers/qmd_error_messages.rs`
