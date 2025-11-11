# Quarto Markdown CommonMark-Compatible Subset Specification

## Motivation

Define a **well-behaved subset** of markdown syntax where we can **guarantee** identical parsing between qmd and CommonMark. This is NOT about making qmd pass all CommonMark tests, but rather defining a "safe zone" of markdown that works predictably everywhere.

This provides:

1. **Migration confidence**: Users know exactly which markdown constructs are portable
2. **Predictability**: No surprises when moving between parsers
3. **Documentation**: Clear guidance on "write markdown this way and it works everywhere"
4. **Interoperability**: Safe interchange format with other tools

## Core Challenge: The Intersection Problem

We need to find:

```
qmd_safe_subset ⊂ qmd
qmd_safe_subset ⊂ CommonMark
qmd_safe_subset produces identical output in both parsers
```

**NOT** trying to make `qmd ⊃ CommonMark` (supporting all CommonMark features).

### What We Exclude

We intentionally exclude:
- **qmd-specific features**: divs, callouts, shortcodes, executable code
- **CommonMark edge cases**: malformed constructs, weird nesting, tab/space ambiguities
- **Unsupported features**: reference-style links (qmd doesn't support them)
- **Ambiguous constructs**: things that might parse differently
- **HTML complexity**: intricate HTML passthrough rules

### What We Include

Only **clean, well-behaved, universally-understood markdown**:
- Simple headings, paragraphs, lists
- Basic emphasis and strong
- Inline links and images
- Clean code blocks
- Blockquotes
- Horizontal rules

The subset is defined by **whitelist**, not blacklist.

## Target Specification: CommonMark 0.31.2

We target **CommonMark 0.31.2** as the reference because:

1. **Stable specification**: Changes rarely, clearly versioned
2. **Testable**: Formal spec we can validate against (652 test cases)
3. **Universal baseline**: Everyone understands "CommonMark-compatible"
4. **Not Pandoc**: Pandoc's behavior changes frequently and varies by version - tracking it would make our guarantees meaningless

### Reference Implementation: comrak

We use [**comrak**](https://github.com/kivikakk/comrak) (Rust CommonMark parser) as our reference implementation:

- **Full compliance**: Passes **652/652** CommonMark 0.31.2 spec tests
- **Well-maintained**: Active development, good documentation
- **Rust ecosystem**: Easy to integrate in our test suite
- **Pure Rust**: No FFI needed

**Important**: comrak also supports GFM (GitHub Flavored Markdown). We must configure it to **CommonMark-only mode** by disabling all GFM extensions in `ComrakOptions`.

### Our Approach

Our goal is **NOT** to pass all 652 CommonMark spec tests. Many of those tests cover edge cases we don't want (malformed constructs, weird indentation rules, etc.). Instead, we:

1. **Define a clean subset** of well-behaved markdown
2. **Verify it works identically** in both qmd and comrak (configured for pure CommonMark)
3. **Document the guarantees** clearly for users

## Subset Definition: The Safe Zone

This is a **whitelist** approach. Only features explicitly listed here are in the subset.

### ✅ INCLUDED: Clean, Well-Behaved Markdown

**Semantic Grammar** (abstract representation, not tied to implementation):

```
Document     := Block*
Block        := Heading | Paragraph | CodeBlock | Blockquote | List | HorizontalRule
Inline       := Text | Emphasis | Strong | Code | Link | Image | LineBreak

Heading      := Level(1..6) × Inline*
Paragraph    := Inline+
CodeBlock    := Language? × TextContent
Blockquote   := Block+
List         := (Ordered | Unordered) × ListItem+
ListItem     := Block+
HorizontalRule := ∅

Emphasis     := Inline+
Strong       := Inline+
Code         := TextContent
Link         := Inline* × URL × Title?
Image        := AltText × URL × Title?
LineBreak    := ∅
Text         := String
```

**Concrete Syntax** (how you write it):

| Feature | Syntax | Semantic Structure | Notes |
|---------|--------|-------------------|-------|
| ATX Headings | `# H1` through `###### H6` | `Heading(level, inlines)` | Space after `#` required |
| Paragraphs | Plain text, blank-line separated | `Paragraph(inlines)` | Simple, straightforward |
| Fenced Code Blocks | ` ```lang` ... ` ``` ` | `CodeBlock(lang?, content)` | Backticks only, info string optional |
| Blockquotes | `> text` | `Blockquote(blocks)` | Can nest with `> >` |
| Unordered Lists | `- item` or `* item` | `List(Unordered, items)` | Consistent marker per list |
| Ordered Lists | `1. item`, `2. item` | `List(Ordered, items)` | Start at 1, increment by 1 |
| Horizontal Rules | `---` or `***` (3+) | `HorizontalRule` | Own line, no other content |
| Emphasis | `*text*` or `_text_` | `Emphasis(inlines)` | Produces `<em>` |
| Strong Emphasis | `**text**` or `__text__` | `Strong(inlines)` | Produces `<strong>` |
| Inline Code | `` `code` `` | `Code(content)` | Single backticks |
| Links | `[text](url)` or `[text](url "title")` | `Link(inlines, url, title?)` | Inline only |
| Images | `![alt](url)` or `![alt](url "title")` | `Image(alt, url, title?)` | Inline only |
| Autolinks | `<http://example.com>` | `Link([Text(url)], url, None)` | Angle brackets required |
| Hard Line Breaks | `\` at end of line | `LineBreak` | Backslash only (not spaces) |
| Backslash Escapes | `\*`, `\[`, `\\`, etc. | `Text(escaped_char)` | Standard escaping |

**Guarantee**: For any markdown using only the above syntax, qmd produces a semantic structure that is equivalent to CommonMark's structure, where equivalence is verified via:
1. **Primary**: Pandoc AST comparison (structural)
2. **Secondary**: HTML output comparison (rendering)

### ❌ EXCLUDED: Everything Else

**qmd-specific features** (obviously excluded):
- Fenced divs (`:::`)
- Callouts
- Executable code blocks
- Shortcodes
- Cross-references
- Citations
- Math
- Attributes on blocks

**CommonMark features we intentionally exclude:**

| Feature | Why Excluded |
|---------|-------------|
| Setext headings (`===`, `---`) | Ambiguous, harder to parse reliably. ATX is clearer. |
| Indented code blocks (4 spaces) | Ambiguous with list continuation. Use fenced blocks. |
| Reference-style links `[text][ref]` | qmd doesn't support these at all |
| HTML blocks | Complex rules, security concerns, may parse differently |
| Inline HTML | Complex, may be filtered differently |
| Two-space hard breaks | Too error-prone, hard to see. Use backslash. |
| Tab indentation | Ambiguous (4-space equivalence). Use spaces only. |
| Lazy continuation | Ambiguous behavior. Be explicit. |
| Link reference definitions | Goes with reference links |
| Complex list nesting | Edge cases differ between parsers |

**Features requiring investigation:**
- YAML frontmatter (not in CommonMark, but universally used - likely special case)
- Nested emphasis/strong combinations (need to verify identical parsing)
- List item continuation with blank lines
- Multiple blank lines (normalization differences?)
- Trailing whitespace handling
- Unicode in URLs
- Percent-encoding in URLs

## Technical Approach

### Overview

We take a **bottom-up, whitelist-driven** approach:

1. **Define** the subset explicitly (✅ done above)
2. **Create** comprehensive test cases covering the subset
3. **Verify** identical output in both qmd and CommonMark (comrak)
4. **Validate** that input stays within the subset boundaries
5. **Document** the guarantees clearly for users

### Phase 1: Subset Test Suite (Core)

**Goal**: Create a comprehensive test suite for subset features

**Implementation**:
```rust
// tests/commonmark_subset.rs

#[test]
fn subset_atx_headings() {
    let inputs = vec![
        "# Heading 1\n",
        "## Heading 2\n",
        "### Heading 3 with *emphasis*\n",
        "###### Heading 6\n",
    ];

    for input in inputs {
        let qmd_output = qmd_to_pandoc_ast(input);
        let comrak_output = comrak_to_pandoc_ast(input);

        assert_ast_equivalent!(qmd_output, comrak_output,
            "Input: {:?}", input);
    }
}

#[test]
fn subset_emphasis_and_strong() {
    // Test emphasis combinations within the subset
    // ...
}
```

**Test Coverage**:
- One test module per feature category
- Cover combinations (emphasis in lists, code in headings, etc.)
- Edge cases within the subset (nested blockquotes, etc.)
- NOT trying to cover edge cases outside the subset

**Success Metric**: 100% of subset tests pass with identical output

### Phase 2: Differential Testing with comrak

**Goal**: Use comrak as the reference CommonMark implementation

**Reference**: See [Comrak AST Structure Analysis](./2025-11-06-comrak-ast-structure.md) for detailed documentation of comrak's AST and testing strategies.

**Setup**:
```toml
[dev-dependencies]
comrak = "0.47"  # Rust CommonMark + GFM implementation
```

**Test Infrastructure**:
```rust
use comrak::{markdown_to_html, ComrakOptions};

fn comrak_to_pandoc_ast(markdown: &str) -> Value {
    // Convert markdown -> HTML via comrak
    // Then HTML -> Pandoc AST
    // Or: use comrak's AST directly if we can map it
}

fn qmd_to_pandoc_ast(markdown: &str) -> Value {
    // Our parser -> Pandoc AST
}

fn assert_ast_equivalent(qmd: Value, comrak: Value, msg: &str) {
    // Normalize both ASTs
    let qmd_norm = normalize_pandoc_ast(qmd);
    let comrak_norm = normalize_pandoc_ast(comrak);

    assert_eq!(qmd_norm, comrak_norm, "{}", msg);
}
```

**Normalization Needed**:
- Whitespace in text nodes
- Source position information (strip it)
- Attribute ordering
- Empty text nodes

### Phase 3: Validation Tooling

**Goal**: Help users verify their markdown is in the subset

**Implementation**:
```rust
// In quarto-markdown-pandoc

pub fn validate_commonmark_subset(input: &str) -> Result<(), ValidationError> {
    let tree = parse_tree_sitter(input)?;

    // Query for non-subset features
    let non_subset_query = r#"
        (fenced_div) @error
        (shortcode) @error
        (setext_heading) @error
        (indented_code_block) @error
        (html_block) @error
        (reference_link) @error
    "#;

    let matches = query_tree(&tree, non_subset_query);

    if !matches.is_empty() {
        return Err(ValidationError::NonSubsetFeatures(matches));
    }

    Ok(())
}
```

**CLI Integration**:
```bash
# Validate a file is in the subset
quarto-markdown-pandoc --validate-subset input.md

# Convert and validate
quarto-markdown-pandoc --to html --validate-subset input.md
```

**Error Messages**:
```
Error: Non-subset feature detected
  --> input.md:5:1
   |
 5 | === Heading
   | ^^^^^^^^^^^ Setext headings are not in the CommonMark-compatible subset
   |
   = help: Use ATX headings instead: `## Heading`
   = note: See subset documentation at https://...
```

### Phase 4: Documentation & Specification

**Formal Spec Document**: `COMMONMARK-SUBSET.md`

Contents:
```markdown
# CommonMark-Compatible Subset of Quarto Markdown

## Guarantee

When you write markdown using ONLY the features listed in this document,
we guarantee that:
1. qmd will parse it identically to CommonMark 0.31.2
2. Output will be structurally equivalent (Pandoc AST comparison)
3. This guarantee is tested in CI for every commit

## Included Features
[The tables we defined above]

## Validation
[How to use --validate-subset]

## Examples
[Clean examples of subset-compliant markdown]

## Non-Subset Features
[What's excluded and why]
```

**User Documentation**: `docs/reference/commonmark-subset.qmd`

User-friendly guide:
- "Why use the subset?"
- "Quick reference card"
- "Migration guide"
- "Validation workflow"

### Not Doing: Porting All CommonMark Tests

We are **NOT** porting the full 652-test CommonMark spec suite. Why?

1. **Many tests are for features we exclude**: indented code blocks, setext headings, reference links, HTML blocks, tab handling
2. **Many tests are for edge cases we don't want**: malformed constructs, weird nesting, lazy continuation
3. **We're defining a subset**: we only need tests for what's IN the subset, not what's OUT
4. **Maintenance burden**: tracking CommonMark test evolution is unnecessary

Instead: We create our own focused test suite covering the subset thoroughly.

## Testing Strategy

### 1. Subset Feature Tests (Primary)

**Goal**: Comprehensive coverage of features IN the subset

**Organization**:
```
tests/commonmark_subset/
  ├── mod.rs                    # Test infrastructure
  ├── headings.rs              # ATX headings only
  ├── emphasis.rs              # emphasis and strong
  ├── links.rs                 # inline links and images
  ├── lists.rs                 # ordered and unordered
  ├── code.rs                  # fenced code blocks, inline code
  ├── blockquotes.rs           # blockquotes
  ├── combinations.rs          # emphasis in lists, etc.
  └── edge_cases.rs            # subset-valid edge cases
```

**Example Test**:
```rust
// tests/commonmark_subset/emphasis.rs

#[test]
fn basic_emphasis() {
    verify_subset(&[
        ("*italic*", "emphasis"),
        ("_also italic_", "emphasis"),
        ("**bold**", "strong"),
        ("__also bold__", "strong"),
    ]);
}

#[test]
fn nested_emphasis() {
    verify_subset(&[
        ("***bold italic***", "both"),
        ("**bold with *nested italic***", "nested"),
    ]);
}

fn verify_subset(cases: &[(&str, &str)]) {
    for (input, description) in cases {
        let qmd_ast = qmd_to_pandoc(input);
        let comrak_ast = comrak_to_pandoc(input);

        assert_ast_equivalent!(qmd_ast, comrak_ast,
            "{}: {:?}", description, input);
    }
}
```

**Coverage Target**: Every feature in the subset, plus common combinations

### 2. Differential Testing Corpus

**Goal**: Real-world examples that should be in the subset

**Corpus Sources**:
```
tests/fixtures/subset-corpus/
  ├── simple-blog-post.md       # Typical blog content
  ├── readme-example.md          # Like a GitHub README (no tables/GFM)
  ├── documentation.md           # Technical docs with code blocks
  ├── nested-lists.md            # Complex list structures
  └── mixed-content.md           # All features combined
```

**Test**:
```rust
#[test]
fn corpus_differential_testing() {
    for file in glob("tests/fixtures/subset-corpus/*.md") {
        let input = read_to_string(file)?;

        // Validate it's in the subset first
        validate_commonmark_subset(&input)
            .expect("Corpus file not in subset");

        // Verify identical parsing
        let qmd_ast = qmd_to_pandoc(&input);
        let comrak_ast = comrak_to_pandoc(&input);

        assert_ast_equivalent!(qmd_ast, comrak_ast,
            "File: {}", file.display());
    }
}
```

### 3. Validation Tests

**Goal**: Verify that validation correctly identifies non-subset features

```rust
// tests/subset_validation.rs

#[test]
fn rejects_setext_headings() {
    let input = "Heading\n=======\n";

    let result = validate_commonmark_subset(input);

    assert!(result.is_err());
    assert_contains!(result.unwrap_err(), "Setext heading");
}

#[test]
fn rejects_indented_code() {
    let input = "    code block\n";

    let result = validate_commonmark_subset(input);

    assert!(result.is_err());
    assert_contains!(result.unwrap_err(), "indented code");
}

#[test]
fn accepts_valid_subset() {
    let input = "# Heading\n\n*emphasis* and **strong**\n";

    validate_commonmark_subset(input).expect("Should be valid");
}
```

### 4. NOT Doing: Full CommonMark Test Port

We explicitly **do not** port the full CommonMark spec test suite because:
- Most tests cover excluded features (HTML, tabs, reference links, etc.)
- Edge cases we intentionally avoid (malformed constructs)
- Maintenance burden of tracking spec evolution
- Our subset is smaller than CommonMark

Instead: Focused tests on what we actually support and guarantee.

## Implementation Considerations

### What Level of "Identical" Do We Promise?

**Critical Question**: CommonMark spec only defines HTML output, not AST structure. What exactly do we guarantee?

#### Option 1: HTML Output Equivalence (CommonMark's Approach)

**Promise**: "Given subset markdown, qmd produces identical HTML to CommonMark"

```rust
fn verify_html_equivalence(markdown: &str) {
    let qmd_html = qmd_to_html(markdown);
    let comrak_html = comrak_to_html(markdown);

    assert_eq!(normalize_html(qmd_html), normalize_html(comrak_html));
}
```

**Pros**:
- Directly matches CommonMark spec methodology
- Concrete, testable, unambiguous
- Users care about output, not internal representation

**Cons**:
- HTML rendering details may vary (attribute order, whitespace, self-closing tags)
- Normalization is tricky (what counts as "same"?)
- Doesn't capture semantic structure directly
- Tight coupling to HTML output format

#### Option 2: Pandoc AST Equivalence

**Promise**: "Given subset markdown, qmd produces semantically equivalent Pandoc AST to CommonMark"

```rust
fn verify_pandoc_ast_equivalence(markdown: &str) {
    let qmd_ast = qmd_to_pandoc_ast(markdown);
    let comrak_ast = comrak_to_pandoc_ast(markdown);  // via HTML roundtrip

    assert_eq!(normalize_pandoc(qmd_ast), normalize_pandoc(comrak_ast));
}
```

**Pros**:
- Semantic-level comparison (structure, not rendering)
- Pandoc AST is well-defined and stable
- Independent of output format (HTML, LaTeX, etc.)
- qmd already produces Pandoc AST natively

**Cons**:
- Pandoc AST is not part of CommonMark spec
- Requires converting CommonMark output to Pandoc AST (lossy?)
- More abstract than what CommonMark promises

#### Option 3: Abstract Semantic Grammar

**Promise**: "Given subset markdown, qmd produces structure matching this abstract grammar"

Define a simplified grammar like:
```
Document := Block*
Block := Heading | Paragraph | List | CodeBlock | Blockquote | HorizontalRule
Heading := Level × Inline*
Paragraph := Inline*
Inline := Text | Emph | Strong | Code | Link | Image
...
```

**Pros**:
- Clean abstraction independent of implementation
- Can be verified at multiple levels (tree-sitter, Pandoc AST, HTML)
- Describes semantics clearly
- Flexible - multiple conforming implementations

**Cons**:
- Need to design and maintain this grammar
- More work to specify formally
- Less concrete than direct comparison
- Verification requires mapping both parsers to this grammar

#### Option 4: Multi-Level Guarantees

**Promise**: Equivalence at multiple levels, each with different strength

1. **Strong**: HTML output matches (byte-for-byte after normalization)
2. **Semantic**: Pandoc AST matches (structural equivalence)
3. **Grammatical**: Satisfies abstract grammar (semantic correctness)

**Pros**:
- Most comprehensive
- Different guarantees for different use cases
- Can test at multiple levels for defense-in-depth

**Cons**:
- Complex to specify and maintain
- May discover conflicts between levels
- Which level is "the" guarantee?

### Recommendation: Hybrid Approach

**Primary Promise**: **Pandoc AST semantic equivalence**
- This is what qmd natively produces
- Captures semantic structure
- Independent of rendering details

**Validation**: **HTML output verification**
- Use HTML comparison as secondary check
- Following CommonMark's own methodology
- Helps catch rendering bugs

**Specification**: **Abstract semantic description**
- Document the subset in terms of semantic structure
- Not tied to tree-sitter internals
- Human-readable, clear promises

### Implementation Strategy

**See also**: [Comrak AST Structure Analysis](./2025-11-06-comrak-ast-structure.md) for detailed information about comrak's AST and comparison strategies.

**Recommended approach** (from comrak analysis):

1. **Primary testing**: HTML output comparison
   - Simple, direct, matches CommonMark spec methodology
   - Use `comrak::markdown_to_html()` vs `qmd::to_html()`
   - Normalize HTML before comparison (whitespace, attribute order, etc.)

2. **Secondary testing**: Pandoc AST comparison
   - Semantic verification independent of rendering
   - comrak → HTML → pandoc → AST vs qmd → AST
   - Requires pandoc CLI in test environment

```rust
// Primary test: HTML output equivalence
#[test]
fn subset_test_html_output(markdown: &str) {
    use comrak::{markdown_to_html, ComrakOptions};

    let comrak_html = markdown_to_html(markdown, &ComrakOptions::default());
    let qmd_html = qmd::to_html(markdown);

    assert_html_equivalent!(
        normalize_html(&qmd_html),
        normalize_html(&comrak_html)
    );
}

// Secondary test: Pandoc AST equivalence
#[test]
fn subset_test_pandoc_ast(markdown: &str) {
    // qmd → Pandoc AST (native)
    let qmd_ast = qmd::parse(markdown).to_pandoc_json();

    // comrak → HTML → Pandoc AST (via CLI)
    let comrak_html = comrak::markdown_to_html(markdown, &ComrakOptions::default());
    let comrak_ast = html_to_pandoc_via_cli(&comrak_html);

    assert_ast_equivalent!(
        normalize_pandoc(qmd_ast),
        normalize_pandoc(comrak_ast)
    );
}

fn normalize_html(html: &str) -> String {
    // Collapse whitespace
    // Sort attributes
    // Normalize self-closing tags
    // etc.
}

fn normalize_pandoc(ast: serde_json::Value) -> serde_json::Value {
    // Strip source positions
    // Normalize whitespace in text nodes
    // etc.
}
```

### Abstract Semantic Specification

In the formal subset document, describe features semantically:

```markdown
## Heading (ATX Style)

**Syntax**: 1-6 `#` characters, followed by space, followed by inline content

**Semantics**:
- Creates a heading element at the specified level (1-6)
- Content is parsed as inline elements
- Level determined by number of `#` characters

**Example**:
Input: `## Hello *world*`
Structure: Heading(level=2, [Text("Hello "), Emph([Text("world")])])

**Not Included**:
- Closing `#` characters (allowed but not required)
- Setext-style headings (`===` or `---`)
```

This way:
- **Users** see semantic descriptions (what it means)
- **Tests** verify Pandoc AST equivalence (structural)
- **Validation** can use HTML output as additional check (rendering)

### Open Question: How to Handle HTML Output Differences?

Even with identical semantics, HTML rendering can differ:

```html
<!-- Semantically equivalent but textually different -->
<a href="url" title="title">text</a>
<a title="title" href="url">text</a>

<!-- Self-closing tags -->
<img src="url" alt="alt" />
<img src="url" alt="alt">

<!-- Whitespace -->
<p>text</p>
<p>
  text
</p>
```

**Options**:
1. **HTML normalization**: Parse HTML to DOM, compare DOM trees
2. **Regex normalization**: Strip/normalize whitespace, sort attributes
3. **Don't promise HTML equivalence**: Only promise semantic (AST) equivalence
4. **Document known differences**: "Semantically identical, may differ in rendering details"

**Recommendation**: Primary promise is **semantic (Pandoc AST) equivalence**, with HTML comparison as validation but documented that rendering details may vary.

### Example: Different Levels of Representation

To illustrate how the different levels relate, consider this subset markdown:

```markdown
# Hello *world*

This is a [link](https://example.com).
```

**Level 1: Syntax (what you write)**
```
# Hello *world*\n\nThis is a [link](https://example.com).
```

**Level 2: Tree-sitter CST (implementation detail - NOT what we promise)**
```
(document
  (atx_heading
    (heading_content
      (text) (emphasis (text))))
  (paragraph
    (text) (inline_link (text) (url))))
```

**Level 3: Abstract Semantic Structure (what we promise)**
```
Document([
  Heading(1, [Text("Hello "), Emphasis([Text("world")])]),
  Paragraph([
    Text("This is a "),
    Link([Text("link")], "https://example.com", None),
    Text(".")
  ])
])
```

**Level 4: Pandoc AST (how we verify promise)**
```json
{
  "blocks": [
    {
      "t": "Header",
      "c": [1, ["", [], []], [
        {"t": "Str", "c": "Hello"},
        {"t": "Space"},
        {"t": "Emph", "c": [{"t": "Str", "c": "world"}]}
      ]]
    },
    {
      "t": "Para",
      "c": [
        {"t": "Str", "c": "This"}, {"t": "Space"},
        {"t": "Str", "c": "is"}, {"t": "Space"},
        {"t": "Str", "c": "a"}, {"t": "Space"},
        {"t": "Link", "c": [
          ["", [], []],
          [{"t": "Str", "c": "link"}],
          ["https://example.com", ""]
        ]},
        {"t": "Str", "c": "."}
      ]
    }
  ]
}
```

**Level 5: HTML Output (secondary verification)**
```html
<h1>Hello <em>world</em></h1>
<p>This is a <a href="https://example.com">link</a>.</p>
```

**What We Promise**:
- ✅ Level 3 (Abstract Semantic): The semantic structure matches
- ✅ Level 4 (Pandoc AST): When normalized, ASTs are equivalent
- ⚠️ Level 5 (HTML): Output should be equivalent (may differ in whitespace/attributes)
- ❌ Level 2 (Tree-sitter CST): Implementation detail, no promises

### Tree-Sitter Queries for Validation

Create tree-sitter queries to detect non-subset features:

```scheme
; queries/non-subset-features.scm

; qmd-specific features
(fenced_div) @error.qmd-feature
(callout_block) @error.qmd-feature
(shortcode) @error.qmd-feature
(executable_code_block) @error.qmd-feature

; Excluded CommonMark features
(setext_heading) @error.excluded
(indented_code_block) @error.excluded
(html_block) @error.excluded
(reference_link) @error.excluded
(link_reference_definition) @error.excluded
```

Usage in validation:
```rust
pub fn validate_commonmark_subset(input: &str) -> Result<(), Vec<ValidationError>> {
    let tree = parse_tree_sitter(input)?;
    let query = load_query("queries/non-subset-features.scm");

    let matches = execute_query(&tree, &query);

    if matches.is_empty() {
        Ok(())
    } else {
        Err(matches.into_iter().map(|m| {
            ValidationError::NonSubsetFeature {
                feature: m.capture_name,
                location: m.range,
                suggestion: get_suggestion(&m.capture_name),
            }
        }).collect())
    }
}
```

### Error Messages

When validation fails, provide clear guidance:

```
Error: Non-CommonMark feature detected
  --> input.qmd:5:1
   |
 5 | ::: {.callout-note}
   | ^^^^^^^^^^^^^^^^^^^ fenced div is not part of CommonMark
   |
   = note: fenced divs are a Quarto extension
   = help: for CommonMark compatibility, use standard blockquotes instead
```

## Documentation Requirements

### 1. User-Facing Documentation

**Location**: `docs/commonmark-compatibility.qmd`

**Contents:**
- What is the CommonMark-compatible subset?
- Why use it?
- Complete feature list (in/out)
- Migration guide from pure markdown
- Validation tools
- Examples

### 2. Technical Specification

**Location**: `COMMONMARK-SUBSET.md` in repo root

**Contents:**
- Formal grammar subset definition
- Testing methodology
- Version compatibility matrix
- Deviation notes (if any intentional differences)

### 3. API Documentation

```rust
/// Parse input in strict CommonMark compatibility mode.
///
/// This mode guarantees that:
/// 1. Input is validated against the CommonMark subset
/// 2. Output matches CommonMark 0.31.2 reference implementation
/// 3. Non-subset features are rejected with clear errors
///
/// # Errors
/// Returns `Error::NonCommonMarkFeature` if input contains
/// qmd-specific extensions.
pub fn parse_commonmark_strict(input: &str) -> Result<Document>;
```

## Open Questions & Design Decisions

### 1. YAML Frontmatter
**Status**: ⚠️ Needs Investigation

- **Issue**: Not in CommonMark spec, but ubiquitous in practice
- **Options**:
  - Exclude entirely (purist approach)
  - Include as documented extension (practical approach)
  - Make it opt-in to subset
- **Recommendation**: Include with clear documentation as "universal extension" - too useful to exclude
- **Action**: Test if qmd and CommonMark parse frontmatter identically (probably need to strip it before comparison)

### 2. HTML Passthrough
**Status**: ⚠️ Needs Decision

- **Issue**: CommonMark allows raw HTML, qmd might filter/sanitize it
- **Options**:
  - Allow all HTML (follow CommonMark)
  - Exclude all HTML (safest)
  - Allow only inline HTML (partial)
- **Recommendation**: **Exclude all HTML** from subset - too complex and varies by renderer
- **Rationale**: HTML handling differs wildly (security filtering, tag allowlists), impossible to guarantee identical output

### 3. Whitespace Normalization
**Status**: ✅ Decided

- **Decision**: Semantic equivalence via AST comparison, NOT byte-for-byte
- **Rationale**:
  - Trailing whitespace shouldn't matter
  - Multiple blank lines may normalize differently
  - AST captures semantic meaning
- **Implementation**: Normalize whitespace in text nodes during comparison

### 4. Hard Line Breaks
**Status**: ✅ Decided

- **Decision**: Backslash only (`\` at EOL), NOT two spaces
- **Rationale**:
  - Two spaces are invisible, error-prone
  - Backslash is explicit and visible
  - Subset should favor clarity
- **Note**: CommonMark supports both, but subset excludes two-space variant

### 5. Nested Emphasis/Strong
**Status**: ⚠️ Needs Testing

- **Issue**: Complex nesting like `**bold *and italic* bold**` has subtle parsing rules
- **Action**: Comprehensive tests to verify identical parsing
- **Edge cases**:
  - `***text***` - both or nested?
  - `**bold *italic**` - mismatched?
  - `_emphasis *mixed* emphasis_` - different delimiters?

### 6. List Markers
**Status**: ✅ Decided

- **Decision**: Allow `-` or `*` for unordered, but must be consistent within a list
- **Rationale**: Both are standard, but mixing can be ambiguous
- **Validation**: Check for consistent markers in validation mode

### 7. URL Encoding
**Status**: ⚠️ Needs Investigation

- **Issue**: How do parsers handle spaces, unicode, and special chars in URLs?
- **Examples**:
  - `[link](url with spaces.html)` - percent-encode?
  - `[link](http://example.com/文档.html)` - unicode handling?
- **Action**: Test and document expected behavior, possibly require percent-encoding

## Success Metrics

### Phase 1: Subset Definition & Initial Testing (2-3 weeks)
- [ ] Formal subset specification document complete
- [ ] Test infrastructure set up (comrak integration, AST comparison)
- [ ] Initial test suite covering all subset features (50+ test cases)
- [ ] All tests pass with identical qmd/comrak output

### Phase 2: Comprehensive Testing (2-3 weeks)
- [ ] Comprehensive test coverage (200+ test cases)
- [ ] Differential testing corpus (5+ real-world examples)
- [ ] Edge case testing (nested structures, combinations)
- [ ] 100% pass rate maintained

### Phase 3: Validation Tooling (1-2 weeks)
- [ ] `--validate-subset` flag implemented
- [ ] Tree-sitter queries for non-subset detection
- [ ] Clear error messages with suggestions
- [ ] Validation test suite (50+ cases)

### Phase 4: Documentation & Release (1 week)
- [ ] User-facing documentation published
- [ ] API documentation complete
- [ ] Examples and migration guide
- [ ] Announced to community

**Total Estimate**: 6-9 weeks

## Implementation Plan

### Epic: CommonMark-Compatible Subset (k-333)
- **Priority**: Medium (P2) - important for maturity, not blocking current work
- **Time Estimate**: 6-9 weeks total
- **Dependencies**: None (can work in parallel with other efforts)

### Task Breakdown

#### Phase 1: Definition & Infrastructure (2-3 weeks)

1. **Write formal subset specification** (2-3 days)
   - Finalize COMMONMARK-SUBSET.md document
   - Get review/approval on included features
   - Resolve open questions (frontmatter, HTML, etc.)

2. **Set up test infrastructure** (3-4 days)
   - Add comrak dev dependency (see comrak AST analysis document)
   - Implement HTML normalization and comparison
   - Implement Pandoc AST normalization (via pandoc CLI)
   - Create test helper macros and assertions
   - Determine correct ComrakOptions for pure CommonMark

3. **Create initial test suite** (5-7 days)
   - Write tests for each subset feature category
   - Headings, emphasis, links, lists, code, blockquotes
   - Run tests, document any failures
   - Fix obvious parser bugs if found

#### Phase 2: Comprehensive Testing (2-3 weeks)

4. **Expand test coverage** (5-7 days)
   - Test feature combinations
   - Nested structures
   - Edge cases within subset
   - Target: 200+ test cases

5. **Create differential testing corpus** (3-4 days)
   - Write 5+ realistic markdown documents
   - Blog posts, READMEs, documentation
   - Verify they're in subset
   - Add as regression tests

6. **Investigate and resolve discrepancies** (5-7 days)
   - Debug any test failures
   - Fix parser bugs
   - Document intentional differences (if any)
   - Update subset definition if needed

#### Phase 3: Validation Tooling (1-2 weeks)

7. **Implement subset validation** (3-4 days)
   - Create tree-sitter query for non-subset features
   - Implement validation function
   - Add `--validate-subset` CLI flag
   - Return helpful error messages

8. **Error messages and suggestions** (2-3 days)
   - Map each non-subset feature to suggestion
   - Format errors nicely (file:line:col)
   - Test error output for all excluded features

9. **Validation test suite** (2-3 days)
   - Tests verifying validation rejects non-subset
   - Tests verifying validation accepts subset
   - Edge cases in validation logic

#### Phase 4: Documentation & Polish (1 week)

10. **Write user documentation** (2-3 days)
    - docs/reference/commonmark-subset.qmd
    - Why use it, what's included, how to validate
    - Examples and migration guide

11. **API documentation** (1-2 days)
    - Rustdoc for validation functions
    - CLI flag documentation
    - Code examples

12. **Final review and announcement** (1-2 days)
    - Internal review
    - Update CHANGELOG
    - Community announcement

### Optional Future Work

13. **Extended subsets** (future, if needed)
    - "CommonMark + GFM" (tables, task lists, strikethrough)
    - "CommonMark + Pandoc basics" (footnotes, definition lists)
    - Separate testing for each

## Related Work

### Prior Art
- **cmark**: C reference implementation of CommonMark
- **comrak**: Rust CommonMark parser with GFM extensions (our reference implementation)
  - See [Comrak AST Structure Analysis](./2025-11-06-comrak-ast-structure.md) for detailed documentation
- **markdown-it**: JavaScript parser with strict mode
- **Pandoc**: Multiple input formats with clear compatibility documentation

### Inspiration
- **rustc**: Multiple edition modes (2015, 2018, 2021) with clear guarantees
- **eslint**: Configurable rule sets and strict mode
- **prettier**: Strict formatting mode with guarantees

## Future Extensions

Once CommonMark subset is solid:

1. **Pandoc Subset**: CommonMark + selected Pandoc extensions
   - Attributes
   - Definition lists
   - Footnotes
   - Citations
   - Math

2. **GFM Subset**: CommonMark + GitHub extensions
   - Tables
   - Task lists
   - Strikethrough
   - Mentions/autolinks

3. **Progressive Enhancement Model**:
   ```
   CommonMark ⊂ Pandoc ⊂ QMD
   CommonMark ⊂ GFM ⊂ QMD
   ```

## Risks and Mitigations

### Risk: Parser Bugs Found During Testing
**Concern**: Tests may reveal significant qmd parser bugs

**Mitigation**:
- Start with small subset, expand gradually
- Fix bugs incrementally
- Update subset definition if needed (remove problematic features)
- Document intentional differences clearly

### Risk: Subset Too Restrictive
**Concern**: Users find subset too limiting for practical use

**Mitigation**:
- Start with core features, expand based on feedback
- Document clearly what's excluded and why
- Provide migration examples
- Consider "extended subsets" (CommonMark + GFM, etc.)

### Risk: Maintenance Burden
**Concern**: Keeping tests and validation in sync as qmd evolves

**Mitigation**:
- CI enforcement (tests must pass)
- Clear ownership of subset feature
- Version lock to CommonMark 0.31.2 (stable target)
- Automated testing prevents regressions

### Risk: AST Comparison Complexity
**Concern**: Normalizing and comparing ASTs is tricky

**Mitigation**:
- Start with simple comparison, refine as needed
- Use Pandoc JSON as common format
- Accept some normalization differences (whitespace, etc.)
- Document comparison methodology

### Risk: Performance Impact of Validation
**Concern**: Validation adds overhead

**Mitigation**:
- Make validation opt-in (explicit flag)
- Tree-sitter queries are fast
- No runtime overhead unless requested
- Consider caching validation results

## Key Insights & Takeaways

### What This Is
- **Whitelist approach**: Define clean, well-behaved markdown that works everywhere
- **Intersection**: `qmd_subset ⊂ qmd` AND `qmd_subset ⊂ CommonMark`
- **Testing-driven**: Verify identical output via differential testing
- **Practical**: Focus on features people actually use

### What This Is NOT
- **Not maximalist**: NOT trying to support all of CommonMark
- **Not perfect**: Some edge cases will differ (intentionally excluded)
- **Not comprehensive**: Won't cover tables, GFM, Pandoc extensions (initially)
- **Not mandatory**: Users can still use full qmd - this is an optional guarantee

### Value Proposition

For users:
- **Confidence**: "If I write markdown this way, it works everywhere"
- **Portability**: Safe interchange with other tools
- **Validation**: Check compliance automatically
- **Documentation**: Clear boundaries

For the project:
- **Quality**: Ensures parser correctness on common features
- **Maturity**: Shows we understand compatibility
- **Testing**: Reference implementation for validation
- **Differentiation**: Clear value prop vs "just another markdown parser"

## Conclusion

A well-defined CommonMark-compatible subset is about **defining a safe zone** where we guarantee identical behavior. This is fundamentally different from trying to pass all CommonMark tests - we're being selective and intentional about what we promise.

**Core principle**: Better to have a small, rock-solid guarantee than a large, shaky one.

**Recommended next step**: Start with Phase 1 - formal specification and test infrastructure. Once we have concrete tests, we'll discover what actually works and can refine the subset definition accordingly.

---

*This plan is a living document. As we implement and test, we'll update based on findings.*
