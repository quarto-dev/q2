# Template System Dependency Architecture

**Date**: 2025-11-23
**Context**: Analysis of how Pandoc's template system interacts with metadata types

## Question

How should the `quarto-templates` crate relate to Pandoc AST types? Should it depend on them, or remain independent?

## Analysis of Haskell Implementation

### Key Finding: Complete Independence

After studying the `doctemplates` Haskell library, the answer is **clear**: The template engine is **completely independent** of Pandoc's types.

### Architecture Layers

```
┌─────────────────────────────────────────────────────────┐
│  Pandoc Types Layer                                     │
│  - Meta (map of MetaValue)                             │
│  - MetaValue (String | Bool | Inlines | Blocks | ...)  │
│  - Blocks, Inlines (AST nodes)                         │
└────────────────┬────────────────────────────────────────┘
                 │
                 │ Conversion via ToContext trait
                 │ (requires writer functions)
                 ↓
┌─────────────────────────────────────────────────────────┐
│  Template Types Layer (doctemplates)                    │
│  - Context a (map of Val a)                            │
│  - Val a (SimpleVal | ListVal | MapVal | BoolVal | ...)│
│  - Template a (AST of template nodes)                  │
└────────────────┬────────────────────────────────────────┘
                 │
                 │ Template evaluation
                 ↓
┌─────────────────────────────────────────────────────────┐
│  Output Layer                                           │
│  - Doc a (layout-aware document)                       │
│  - Rendered to Text/String                             │
└─────────────────────────────────────────────────────────┘
```

### Generic Type Parameter

The doctemplates library is parameterized by type `a`, which is the underlying string/document type:

```haskell
data Val a =
    SimpleVal  (Doc a)
  | ListVal    [Val a]
  | MapVal     (Context a)
  | BoolVal    Bool
  | NullVal
```

The constraint `TemplateTarget a` requires:
```haskell
type TemplateTarget a = (HasChars a, ToText a, FromText a)
```

So `a` can be `Text`, `String`, or any other text-like type. **It has nothing to do with Pandoc.**

### ToContext Typeclass

The bridge between any data type and templates is the `ToContext` typeclass:

```haskell
class ToContext a b where
  toContext :: b -> Context a
  toVal     :: b -> Val a
```

This allows **any** type `b` to be converted to template values. Instances exist for:
- `ToContext a Value` - Aeson JSON values
- `ToContext a Bool`
- `ToContext a b => ToContext a [b]` - Lists
- `ToContext a b => ToContext a (M.Map Text b)` - Maps
- etc.

**Crucially, there is NO instance like `ToContext a MetaValue` in doctemplates itself!**

### Conversion in Pandoc

Pandoc provides the conversion in `Text.Pandoc.Writers.Shared`:

```haskell
metaValueToVal :: (Monad m, TemplateTarget a)
               => ([Block] -> m (Doc a))    -- ^ block writer
               -> ([Inline] -> m (Doc a))   -- ^ inline writer
               -> MetaValue
               -> m (Val a)
metaValueToVal blockWriter inlineWriter (MetaMap metamap) =
  MapVal . Context <$> mapM (metaValueToVal blockWriter inlineWriter) metamap
metaValueToVal blockWriter inlineWriter (MetaList xs) =
  ListVal <$> mapM (metaValueToVal blockWriter inlineWriter) xs
metaValueToVal _ _ (MetaBool b) = return $ BoolVal b
metaValueToVal _ inlineWriter (MetaString s) =
  SimpleVal <$> inlineWriter (Builder.toList (Builder.text s))
metaValueToVal blockWriter _ (MetaBlocks bs) =
  SimpleVal <$> blockWriter bs
metaValueToVal _ inlineWriter (MetaInlines is) =
  SimpleVal <$> inlineWriter is
```

**Key insights:**
1. This is NOT in doctemplates - it's in Pandoc itself
2. It requires **writer functions** to convert `MetaBlocks` and `MetaInlines`
3. The conversion is monadic because writers can fail or do I/O

## Rust Port Architecture

### Proposed Design: Independent Template Crate

Based on the Haskell analysis, our architecture should be:

```
┌─────────────────────────────────────────────────────────┐
│  crates/quarto-markdown-pandoc                          │
│  - Meta, MetaValue types                               │
│  - Blocks, Inlines AST types                           │
└────────────────┬────────────────────────────────────────┘
                 │
                 │ NO DIRECT DEPENDENCY
                 ↓
┌─────────────────────────────────────────────────────────┐
│  crates/quarto-templates (NEW)                          │
│  - TemplateValue enum (String | Bool | List | Map)    │
│  - TemplateContext (map of TemplateValue)             │
│  - Template (parsed AST)                               │
│  - TemplateEngine                                      │
└─────────────────────────────────────────────────────────┘
                 ↑
                 │ Depends on (for shared types)
                 │
┌─────────────────────────────────────────────────────────┐
│  crates/quarto-error-reporting                          │
│  crates/quarto-source-map                               │
└─────────────────────────────────────────────────────────┘
```

### Conversion Layer in quarto-markdown-pandoc

The conversion happens in the **writer layer** of `quarto-markdown-pandoc`:

```rust
// In crates/quarto-markdown-pandoc/src/writers/template_context.rs

use quarto_templates::{TemplateValue, TemplateContext};

/// Convert MetaValue to TemplateValue
/// Requires writer functions to handle MetaInlines and MetaBlocks
pub fn meta_value_to_template_value<W>(
    meta: &MetaValue,
    inline_writer: &mut W,
    block_writer: &mut W,
) -> Result<TemplateValue, WriterError>
where
    W: Write
{
    match meta {
        MetaValue::MetaString(s) => Ok(TemplateValue::String(s.clone())),
        MetaValue::MetaBool(b) => Ok(TemplateValue::Bool(*b)),

        MetaValue::MetaInlines(inlines) => {
            // Write inlines to a buffer and capture as string
            let mut buf = Vec::new();
            write_inlines(inlines, &mut buf)?;
            Ok(TemplateValue::String(String::from_utf8(buf)?))
        },

        MetaValue::MetaBlocks(blocks) => {
            // Write blocks to a buffer and capture as string
            let mut buf = Vec::new();
            write_blocks(blocks, &mut buf)?;
            Ok(TemplateValue::String(String::from_utf8(buf)?))
        },

        MetaValue::MetaList(items) => {
            let converted = items.iter()
                .map(|item| meta_value_to_template_value(item, inline_writer, block_writer))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(TemplateValue::List(converted))
        },

        MetaValue::MetaMap(map) => {
            let mut converted = HashMap::new();
            for (key, value) in map {
                converted.insert(
                    key.clone(),
                    meta_value_to_template_value(value, inline_writer, block_writer)?
                );
            }
            Ok(TemplateValue::Map(converted))
        },
    }
}

/// Build a complete template context from Pandoc metadata
pub fn build_template_context(
    pandoc: &Pandoc,
    ast_context: &AstContext,
    body_html: String,
) -> Result<TemplateContext, WriterError> {
    let mut ctx = TemplateContext::new();

    // Add the rendered body
    ctx.insert("body", TemplateValue::String(body_html));

    // Convert all metadata
    for (key, value) in &pandoc.meta {
        let template_val = meta_value_to_template_value(
            value,
            &mut html_inline_writer,
            &mut html_block_writer
        )?;
        ctx.insert(key, template_val);
    }

    // Add default variables
    ctx.insert("toc", TemplateValue::Bool(false));
    // ... etc

    Ok(ctx)
}
```

### Template Value Type

The `quarto-templates` crate defines its own value type:

```rust
// In crates/quarto-templates/src/value.rs

#[derive(Debug, Clone, PartialEq)]
pub enum TemplateValue {
    String(String),
    Bool(bool),
    List(Vec<TemplateValue>),
    Map(HashMap<String, TemplateValue>),
    Null,
}

impl TemplateValue {
    /// Check if this value is "truthy" for conditionals
    pub fn is_truthy(&self) -> bool {
        match self {
            TemplateValue::Bool(b) => *b,
            TemplateValue::String(s) => !s.is_empty(),
            TemplateValue::List(items) => items.iter().any(|v| v.is_truthy()),
            TemplateValue::Map(_) => true,
            TemplateValue::Null => false,
        }
    }

    /// Get a nested field like "employee.salary"
    pub fn get_path(&self, path: &[&str]) -> Option<&TemplateValue> {
        if path.is_empty() {
            return Some(self);
        }

        match self {
            TemplateValue::Map(map) => {
                map.get(path[0])?.get_path(&path[1..])
            }
            _ => None,
        }
    }
}
```

### Template Context Type

```rust
// In crates/quarto-templates/src/context.rs

use std::collections::HashMap;
use super::value::TemplateValue;

#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    variables: HashMap<String, TemplateValue>,
}

impl TemplateContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: TemplateValue) {
        self.variables.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<&TemplateValue> {
        self.variables.get(key)
    }

    /// Get a value by path like "employee.salary"
    pub fn get_path(&self, path: &str) -> Option<&TemplateValue> {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return None;
        }

        let value = self.variables.get(parts[0])?;
        value.get_path(&parts[1..])
    }
}
```

## Benefits of Independence

### 1. Clean Separation of Concerns
- Template engine knows nothing about Markdown, AST, or Pandoc
- Can be tested independently with simple data structures
- Clear contract between layers

### 2. Reusability
- Template engine could theoretically be used for other projects
- Not coupled to document processing

### 3. Flexibility
- Can evolve template system without touching Pandoc AST
- Can change metadata representation without affecting templates
- Different writers can convert metadata differently

### 4. Testability
- Template engine tests don't need Pandoc AST setup
- Can use simple `HashMap<String, TemplateValue>` in tests
- Conversion layer tested separately from engine

### 5. Following Best Practices
- Mirrors Haskell's proven architecture
- Adheres to "dependency inversion principle"
- Generic types flow from specific to generic, not vice versa

## Writer Function Challenge

The one complexity is that converting `MetaInlines` and `MetaBlocks` requires **writer functions**. This means:

### Option 1: Format-Specific Conversion
Each writer (HTML, LaTeX, etc.) provides its own conversion:

```rust
// In html writer
let ctx = build_template_context(
    pandoc,
    ast_context,
    &html_inline_writer,
    &html_block_writer,
)?;

// In latex writer
let ctx = build_template_context(
    pandoc,
    ast_context,
    &latex_inline_writer,
    &latex_block_writer,
)?;
```

**Pros**: Accurate - MetaInlines rendered in target format
**Cons**: More complex, requires writer trait

### Option 2: Generic Text Conversion
Provide a "to plain text" converter for metadata:

```rust
fn meta_inlines_to_text(inlines: &Inlines) -> String {
    inlines.iter().map(|inline| {
        match inline {
            Inline::Str(s) => s.text.clone(),
            Inline::Space(_) => " ".to_string(),
            // ... just extract text, ignore formatting
        }
    }).collect()
}
```

**Pros**: Simpler, no writer dependency
**Cons**: Loses formatting, may not match Pandoc exactly

### Recommendation: Option 1 (Format-Specific)

Follow Pandoc's approach for correctness. This means:
- Template conversion happens **within each writer**
- Each writer provides inline/block rendering functions
- Metadata is rendered in the target format

## Updated Phase 1

Given this analysis, Phase 1 needs slight adjustment:

**Phase 1: Core Template Engine (Independent)**
- Create `quarto-templates` crate with NO Pandoc dependencies
- Define `TemplateValue` and `TemplateContext` types
- Implement parser, AST, evaluator
- Test with simple `HashMap` contexts

**Phase 1.5: Conversion Layer**
- Add `template_context.rs` module to `quarto-markdown-pandoc`
- Implement `meta_value_to_template_value` conversion
- Define writer trait for format-specific rendering
- Test conversion with various metadata structures

**Phase 2-7: Continue as planned**

## Conclusion

The `quarto-templates` crate should be **completely independent** of Pandoc AST types, mirroring Haskell's `doctemplates` architecture. The conversion from `MetaValue` to `TemplateValue` happens in the writer layer of `quarto-markdown-pandoc`, using format-specific writer functions to render `MetaInlines` and `MetaBlocks`.

This design:
- ✅ Follows Pandoc's proven architecture
- ✅ Maintains clean separation of concerns
- ✅ Enables independent testing and evolution
- ✅ Supports format-specific metadata rendering
- ✅ Adheres to Rust best practices

## Dependencies Graph

```
quarto-templates          (independent)
        ↑
        │ (uses)
        │
quarto-markdown-pandoc    (provides conversion)
        ↓
     MetaValue → TemplateValue conversion
        ↓
   Template rendering with format-specific writers
```
