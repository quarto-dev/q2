# Comrak AST Structure Analysis

**Related to**: CommonMark-compatible subset design (k-333)
**Purpose**: Document comrak's AST structure to inform our testing strategy
**Date**: 2025-11-06

## Overview

[comrak](https://github.com/kivikakk/comrak) is a Rust implementation of CommonMark and GitHub Flavored Markdown.

**CommonMark Compliance**: comrak claims "Compliant with CommonMark 0.31.2 by default" and **passes all 652/652 CommonMark spec tests**, making it a reliable reference implementation for our differential testing.

Understanding its AST structure is critical for our differential testing strategy, as we need to know how to extract and compare semantic information.

## Source References

- **Repository**: https://github.com/kivikakk/comrak
- **Primary AST source**: [`src/nodes.rs`](https://github.com/kivikakk/comrak/blob/main/src/nodes.rs)
- **Documentation**: https://docs.rs/comrak/latest/comrak/
- **Version**: 0.47.0 (as of this writing)

## Core Type Definitions

### Type Aliases

From `src/nodes.rs`:

```rust
pub type AstNode<'a> = arena_tree::Node<'a, RefCell<Ast>>;
pub type Node<'a> = &'a AstNode<'a>;
```

**Key points**:
- Uses `arena_tree` crate for memory-efficient tree storage
- `RefCell<Ast>` provides interior mutability
- Nodes are bound to arena lifetime `'a`
- Most APIs use `Node<'a>` (a reference to an arena node)

### The Ast Struct

From `src/nodes.rs`:

```rust
#[derive(Clone, PartialEq, Eq)]
pub struct Ast {
    pub value: NodeValue,              // The actual node type
    pub sourcepos: Sourcepos,          // Source location information
    pub(crate) content: String,        // Internal content buffer
    pub(crate) open: bool,             // Parse state flag
    pub(crate) last_line_blank: bool,  // Parse state flag
    pub(crate) table_visited: bool,    // Parse state flag
    pub(crate) line_offsets: Vec<usize>, // Line offset tracking
}
```

**Public fields**:
- `value: NodeValue` - The semantic node type (see below)
- `sourcepos: Sourcepos` - Line/column position in source markdown

**Private fields**: Used during parsing, not relevant for our comparison

Reference: [`src/nodes.rs` - struct Ast](https://github.com/kivikakk/comrak/blob/main/src/nodes.rs)

### Sourcepos Struct

```rust
pub struct Sourcepos {
    pub start: LineColumn,
    pub end: LineColumn,
}

pub struct LineColumn {
    pub line: usize,
    pub column: usize,
}
```

Tracks the position in the original markdown source.

**For our purposes**: We'll need to **strip or ignore** sourcepos when comparing ASTs, as our parser may have different position tracking.

## NodeValue Enum - Complete Taxonomy

The `NodeValue` enum defines all possible AST node types. This is the **semantic heart** of comrak's AST.

Reference: [`src/nodes.rs` - enum NodeValue](https://github.com/kivikakk/comrak/blob/main/src/nodes.rs) and [API docs](https://docs.rs/comrak/latest/comrak/nodes/enum.NodeValue.html)

### Block-Level Nodes

| Variant | Fields | In Our Subset? | Notes |
|---------|--------|----------------|-------|
| `Document` | None | ✅ Yes | Root node |
| `BlockQuote` | None | ✅ Yes | Simple blockquotes |
| `List(NodeList)` | Metadata | ✅ Yes | Ordered/unordered lists |
| `Item(NodeList)` | Metadata | ✅ Yes | List items |
| `CodeBlock(Box<NodeCodeBlock>)` | Metadata | ✅ Yes (fenced only) | Both fenced and indented |
| `HtmlBlock(NodeHtmlBlock)` | Metadata | ❌ No | HTML passthrough |
| `Paragraph` | None | ✅ Yes | Text paragraphs |
| `Heading(NodeHeading)` | Level, style | ✅ Yes (ATX only) | ATX and Setext |
| `ThematicBreak` | None | ✅ Yes | Horizontal rules (`---`) |
| `FrontMatter(String)` | Content | ⚠️ TBD | YAML frontmatter |
| `FootnoteDefinition` | None | ❌ No | Not in CommonMark core |
| `Table(Box<NodeTable>)` | Metadata | ❌ No | GFM extension |
| `TableRow(bool)` | Is header? | ❌ No | GFM extension |
| `TableCell` | None | ❌ No | GFM extension |
| `DescriptionList` | None | ❌ No | Extension |
| `DescriptionItem` | None | ❌ No | Extension |
| `DescriptionTerm` | None | ❌ No | Extension |
| `DescriptionDetails` | None | ❌ No | Extension |
| `MultilineBlockQuote` | None | ❌ No | Extension |
| `Alert(Box<NodeAlert>)` | Metadata | ❌ No | GitHub extension |
| `Subtext` | None | ❌ No | Extension |

### Inline Nodes

| Variant | Fields | In Our Subset? | Notes |
|---------|--------|----------------|-------|
| `Text(Cow<'static, str>)` | Content | ✅ Yes | Plain text |
| `SoftBreak` | None | ✅ Yes | Line break in source |
| `LineBreak` | None | ✅ Yes | Hard line break |
| `Code(NodeCode)` | Content | ✅ Yes | Inline code spans |
| `HtmlInline(String)` | Content | ❌ No | Inline HTML |
| `Emph` | None | ✅ Yes | Emphasis (`*text*`) |
| `Strong` | None | ✅ Yes | Strong (`**text**`) |
| `Strikethrough` | None | ❌ No | GFM extension |
| `Superscript` | None | ❌ No | Extension |
| `Subscript` | None | ❌ No | Extension |
| `Link(Box<NodeLink>)` | URL, title | ✅ Yes | Links |
| `Image(Box<NodeLink>)` | URL, title | ✅ Yes | Images |
| `FootnoteReference` | None | ❌ No | Not in CommonMark core |
| `WikiLink(NodeWikiLink)` | Metadata | ❌ No | Extension |
| `Math(NodeMath)` | Content | ❌ No | Extension |
| `Underline` | None | ❌ No | Extension |
| `SpoileredText` | None | ❌ No | Extension |
| `EscapedTag(String)` | Content | ❌ No | Extension |
| `Escaped` | None | ✅ Yes | Backslash escapes |
| `ShortCode(String)` | Code | ❌ No | Extension (feature-gated) |
| `TaskItem(Option<char>)` | Checkbox state | ❌ No | GFM extension |

## Detailed Node Structures

### NodeCodeBlock

From [API docs](https://docs.rs/comrak/latest/comrak/nodes/struct.NodeCodeBlock.html):

```rust
pub struct NodeCodeBlock {
    pub fenced: bool,          // true = fenced (```), false = indented (4 spaces)
    pub fence_char: u8,        // '`' or '~'
    pub fence_length: usize,   // Number of fence chars
    pub fence_offset: usize,   // Indentation level
    pub info: String,          // Info string after opening fence (language)
    pub literal: String,       // The actual code content
    pub closed: bool,          // Whether closing fence was present
}
```

**For our subset**: We only support `fenced: true` with backticks. When comparing:
- Check `fenced == true`
- Compare `info` (language string)
- Compare `literal` (code content)
- Ignore `fence_char`, `fence_length`, `fence_offset`, `closed`

### NodeHeading

From [API docs](https://docs.rs/comrak/latest/comrak/nodes/struct.NodeHeading.html):

```rust
pub struct NodeHeading {
    pub level: u8,    // 1-6 for ATX, 1-2 for Setext
    pub setext: bool, // true = Setext (===, ---), false = ATX (#, ##)
    pub closed: bool, // For ATX, whether trailing # was present
}
```

**For our subset**: We only support `setext: false` (ATX headings). When comparing:
- Check `setext == false`
- Compare `level` (1-6)
- Ignore `closed`

### NodeList

From [API docs](https://docs.rs/comrak/latest/comrak/nodes/struct.NodeList.html):

```rust
pub struct NodeList {
    pub list_type: ListType,          // Bullet or Ordered
    pub marker_offset: usize,         // Spaces before marker
    pub padding: usize,               // Spacing from marker to content
    pub start: usize,                 // Starting number (ordered lists)
    pub delimiter: ListDelimType,     // '.' or ')' for ordered lists
    pub bullet_char: u8,              // '-', '*', or '+' for bullet lists
    pub tight: bool,                  // Tight vs loose list
    pub is_task_list: bool,           // GFM task list
}
```

**Supporting enums**:
```rust
pub enum ListType {
    Bullet,
    Ordered,
}

pub enum ListDelimType {
    Period,    // '.'
    Paren,     // ')'
}
```

**For our subset**: When comparing:
- Compare `list_type` (Bullet or Ordered)
- For ordered lists: compare `start` (must be 1 in our subset)
- Compare `tight` (affects rendering)
- Ignore `marker_offset`, `padding`, `delimiter`, `bullet_char`
- Check `is_task_list == false` (not in our subset)

### NodeLink

From [API docs](https://docs.rs/comrak/latest/comrak/nodes/struct.NodeLink.html):

```rust
pub struct NodeLink {
    pub url: String,    // Destination URL (link) or source (image)
    pub title: String,  // Title attribute (empty string if none)
}
```

**Note**: Used for both `Link` and `Image` variants. For images, the `alt` text is in the inline text children, not in this struct.

**For our subset**: When comparing:
- Compare `url`
- Compare `title` (may be empty)

### NodeCode (Inline Code)

From source:

```rust
pub struct NodeCode {
    pub num_backticks: usize,  // Number of backticks used (`, ``, etc.)
    pub literal: String,       // The code content
}
```

**For our subset**: When comparing:
- Compare `literal` (code content)
- Can ignore `num_backticks` (affects rendering edge cases)

## Tree Structure and Traversal

### Arena-Based Tree

From `src/nodes.rs`:

```rust
pub type AstNode<'a> = arena_tree::Node<'a, RefCell<Ast>>;
```

The tree is stored in a [`typed_arena`](https://docs.rs/typed_arena/) with relationships managed by [`arena_tree`](https://docs.rs/arena_tree/).

**Tree navigation methods** (from `arena_tree::Node`):
- `node.parent()` → `Option<Node<'a>>` - Get parent node
- `node.children()` → `Children<'a, RefCell<Ast>>` - Iterator over children
- `node.first_child()` → `Option<Node<'a>>` - First child
- `node.last_child()` → `Option<Node<'a>>` - Last child
- `node.next_sibling()` → `Option<Node<'a>>` - Next sibling
- `node.previous_sibling()` → `Option<Node<'a>>` - Previous sibling

**Accessing node data**:
```rust
let node: Node<'a>; // &AstNode<'a>
let ast: &RefCell<Ast> = node.data;
let ast_borrowed: Ref<Ast> = ast.borrow();
let node_value: &NodeValue = &ast_borrowed.value;
```

## Implications for Our Differential Testing

### Strategy 1: Direct AST Comparison

**Approach**: Parse with comrak, extract its AST, compare to qmd's AST

**Challenges**:
1. **Different tree representations**: comrak uses arena with `RefCell`, qmd likely has different structure
2. **Need AST walker**: Traverse comrak's tree and build comparable structure
3. **Field filtering**: Ignore parse-state fields, sourcepos, rendering hints

**Implementation sketch**:
```rust
fn comrak_ast_to_comparable(node: &comrak::Node) -> ComparableAst {
    let ast = node.data.borrow();
    let children: Vec<_> = node.children()
        .map(|child| comrak_ast_to_comparable(child))
        .collect();

    ComparableAst {
        value: normalize_node_value(&ast.value),
        children,
    }
}

fn normalize_node_value(value: &NodeValue) -> NormalizedValue {
    match value {
        NodeValue::Heading(h) if h.setext => panic!("Setext not in subset"),
        NodeValue::Heading(h) => NormalizedValue::Heading(h.level),
        NodeValue::CodeBlock(cb) if !cb.fenced => panic!("Indented code not in subset"),
        NodeValue::CodeBlock(cb) => NormalizedValue::CodeBlock {
            lang: cb.info.clone(),
            content: cb.literal.clone(),
        },
        // ... etc
    }
}
```

### Strategy 2: HTML Output Comparison (Recommended Easier)

**Approach**: Use comrak's HTML output, compare to qmd's HTML output

**Advantages**:
1. **No AST mapping needed**: Both produce HTML
2. **Direct use of comrak API**: `comrak::markdown_to_html()`
3. **Matches CommonMark spec methodology**: Spec defines HTML output

**Implementation**:
```rust
use comrak::{markdown_to_html, ComrakOptions};

fn test_subset_via_html(markdown: &str) {
    // Comrak output
    let comrak_html = markdown_to_html(markdown, &ComrakOptions::default());

    // qmd output
    let qmd_html = qmd::to_html(markdown);

    // Normalize and compare
    assert_html_equivalent(normalize_html(&qmd_html), normalize_html(&comrak_html));
}
```

**Normalization needed**:
- Whitespace (collapse, trim)
- Attribute ordering (sort alphabetically)
- Self-closing tags (`<br>` vs `<br />`)
- Empty attributes
- Entity encoding (`&quot;` vs `&#34;`)

### Strategy 3: Hybrid - Pandoc AST as Common Format

**Approach**: Convert both comrak and qmd to Pandoc JSON AST, compare those

**Steps**:
1. comrak → HTML → pandoc CLI → Pandoc JSON
2. qmd → Pandoc JSON (native)
3. Normalize both Pandoc JSONs
4. Compare

**Advantages**:
- Semantic comparison (structure, not rendering)
- qmd natively produces Pandoc JSON
- Pandoc AST is well-defined and stable

**Disadvantages**:
- Requires pandoc CLI in test environment
- HTML → Pandoc conversion may be lossy
- More moving parts

**Implementation**:
```rust
fn comrak_to_pandoc_ast(markdown: &str) -> serde_json::Value {
    // comrak → HTML
    let html = comrak::markdown_to_html(markdown, &ComrakOptions::default());

    // HTML → Pandoc AST via CLI
    let output = Command::new("pandoc")
        .arg("-f").arg("html")
        .arg("-t").arg("json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?
        .stdin.unwrap().write_all(html.as_bytes())?;

    let ast_json = output.stdout.read_to_string()?;
    serde_json::from_str(&ast_json)?
}

fn qmd_to_pandoc_ast(markdown: &str) -> serde_json::Value {
    let doc = qmd::parse(markdown)?;
    doc.to_pandoc_json()
}

#[test]
fn test_subset_via_pandoc_ast() {
    let markdown = "# Hello *world*";

    let comrak_ast = comrak_to_pandoc_ast(markdown);
    let qmd_ast = qmd_to_pandoc_ast(markdown);

    assert_ast_equivalent(normalize_pandoc(comrak_ast), normalize_pandoc(qmd_ast));
}
```

## Recommended Testing Approach

Based on comrak's AST structure, **Strategy 2 (HTML comparison)** is recommended for **initial implementation**:

### Why HTML Comparison First?

1. **Simplicity**: No AST mapping needed, direct API usage
2. **Matches spec**: CommonMark spec defines HTML output, not AST
3. **Fast to implement**: Can start testing immediately
4. **Catches rendering bugs**: Verifies end-to-end output

### Then Add Pandoc AST Comparison (Strategy 3)

1. **Semantic verification**: Confirms structural equivalence
2. **Format-independent**: Not tied to HTML rendering decisions
3. **Native to qmd**: qmd produces Pandoc JSON directly
4. **Defense in depth**: Two levels of verification

### Save Direct AST Comparison (Strategy 1) for Later

1. **More complex**: Requires AST walker and normalization
2. **Less value**: Pandoc AST already provides semantic comparison
3. **Possible future use**: For debugging or deep analysis

## Open Questions

### 1. How does comrak handle YAML frontmatter?

From the enum: `FrontMatter(String)` variant exists.

**Need to investigate**:
- Is it enabled by default or needs a flag?
- How does it parse the YAML? (as opaque string or structured?)
- Does it appear in HTML output?

### 2. What are comrak's default options?

`ComrakOptions::default()` might not match "pure CommonMark" - comrak supports both CommonMark and GFM.

**CRITICAL**: comrak passes 652/652 CommonMark tests AND 670/670 GFM tests. We need to ensure we're testing against **CommonMark mode only**, not GFM extensions.

**Check**:
- Are GFM extensions enabled by default in `ComrakOptions::default()`?
- Table parsing?
- Strikethrough?
- Task lists?
- Autolinks?

**Action**:
1. Review [`ComrakOptions`](https://docs.rs/comrak/latest/comrak/struct.ComrakOptions.html) documentation
2. Create a "strict CommonMark only" option set that disables all GFM extensions
3. Document this configuration in our test code
4. Use this config consistently across all subset tests

**Example**:
```rust
fn commonmark_only_options() -> ComrakOptions {
    let mut options = ComrakOptions::default();
    options.extension.strikethrough = false;
    options.extension.table = false;
    options.extension.autolink = false;
    options.extension.tasklist = false;
    // ... etc for all GFM extensions
    options
}
```

### 3. How does comrak differentiate SoftBreak vs LineBreak?

- `SoftBreak`: Line break in source (doesn't create `<br>`)
- `LineBreak`: Hard line break (creates `<br>`)

**In CommonMark**:
- Hard breaks: two spaces at EOL, or backslash at EOL
- Soft breaks: single newline

**Our subset**: Only backslash, not two spaces

**Testing consideration**: Verify comrak creates `LineBreak` for backslash breaks

### 4. Normalization of Pandoc AST from HTML

When converting `comrak HTML → Pandoc AST`, will we lose information?

**Potential issues**:
- List tightness (tight vs loose)
- Emphasis vs strong (nested combinations)
- Link titles

**Action**: Run experiments to see how well HTML → Pandoc roundtrips

## Next Steps

1. **Configure comrak for pure CommonMark** ⚠️ **CRITICAL FIRST STEP**:
   - Review `ComrakOptions` documentation
   - Identify and disable ALL GFM extensions
   - Create `commonmark_only_options()` helper function
   - Test that it produces pure CommonMark output (not GFM)
   - Document the configuration

2. **Experiment with comrak API**:
   - Parse simple subset markdown
   - Generate HTML output with CommonMark-only options
   - Verify output matches expected CommonMark behavior
   - Test edge cases (emphasis nesting, list continuation, etc.)

3. **Build test harness**:
   - Implement HTML normalization (whitespace, attribute order, etc.)
   - Implement HTML comparison with clear diff output
   - Create test helper macros (`assert_html_equivalent!`, etc.)

4. **HTML → Pandoc pipeline**:
   - Verify pandoc CLI is available in CI
   - Test HTML → Pandoc AST roundtrip quality
   - Implement Pandoc AST normalization
   - Compare against qmd's native Pandoc output

5. **Validate the approach**:
   - Run a few tests on simple subset examples
   - Ensure comrak (CommonMark-only) and qmd produce identical output
   - Debug any discrepancies
   - Refine normalization if needed

## References

- **comrak repository**: https://github.com/kivikakk/comrak
- **comrak API docs**: https://docs.rs/comrak/latest/comrak/
- **CommonMark spec**: https://spec.commonmark.org/0.31.2/
- **Pandoc AST**: https://hackage.haskell.org/package/pandoc-types/docs/Text-Pandoc-Definition.html
- **arena_tree**: https://docs.rs/arena_tree/
- **typed_arena**: https://docs.rs/typed_arena/

---

*This document will be updated as we discover more about comrak's behavior during implementation.*
