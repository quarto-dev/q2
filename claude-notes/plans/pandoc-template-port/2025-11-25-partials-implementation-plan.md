# Partials Implementation Plan

**Date**: 2025-11-25
**Related Issue**: k-394
**Epic**: k-379 (Port Pandoc template functionality)
**Depends On**: k-387 (Basic template evaluator) - now closed

## Overview

Partials are subtemplates stored in different files that can be included and composed with the main template. They are heavily used in Quarto projects for modularity and are essential for a minimal viable template implementation.

## Reference Implementation Analysis

### Haskell Data Types

From `doctemplates/src/Text/DocTemplates/Internal.hs`:

```haskell
data Template a =
    ...
  | Partial [Pipe] (Template a)  -- Pipes applied AFTER partial evaluation
    ...

-- During parsing, partials become nested Template values
-- The partial file is loaded, parsed, and inlined as a Template
```

Key insight: **Partials are resolved at template compilation time**, not render time. The partial file is loaded and parsed during `Template::compile()` (which happens at Rust runtime, not Rust compilation), and the resulting AST is inlined into the parent template's AST.

### TemplateMonad Abstraction

```haskell
class Monad m => TemplateMonad m where
  getPartial :: FilePath -> m Text

instance TemplateMonad Identity where
  getPartial _ = return mempty  -- No file loading

instance TemplateMonad IO where
  getPartial = TIO.readFile    -- Load from filesystem
```

The `TemplateMonad` class abstracts partial loading, allowing:
- IO: Read from filesystem
- Identity: Return empty (for testing without files)
- Custom: Database, network, in-memory cache, etc.

### Path Resolution

From `pPartial` in `Parser.hs`:

```haskell
pPartial mbvar fp = do
  ...
  tp <- templatePath <$> P.getState  -- Original template path
  let fp' = case takeExtension fp of
               "" -> replaceBaseName tp fp     -- No extension: use template's
               _  -> replaceFileName tp fp     -- Has extension: keep it
  partial <- lift $ removeFinalNewline <$> getPartial fp'
```

Path resolution rules:
1. If partial name has no extension: Use the main template's extension
   - Main template: `templates/doc.html`, Partial: `header` → `templates/header.html`
2. If partial name has extension: Keep it
   - Main template: `templates/doc.html`, Partial: `header.tex` → `templates/header.tex`
3. Directory is always the main template's directory

### Recursion Protection

```haskell
nesting <- partialNesting <$> P.getState
t <- if nesting > 50
        then return $ Literal "(loop)"
        else do
          ...
          P.updateState $ \st -> st{ partialNesting = nesting + 1 }
          res' <- pTemplate <* P.eof
          P.updateState $ \st -> st{ partialNesting = nesting }
          ...
```

Haskell approach: Prevents infinite recursion with a depth limit of 50. When exceeded, returns literal "(loop)".

**Our approach**: Since we track `SourceInfo` on all AST nodes, we will emit a proper error message with source location pointing to the partial reference that caused the recursion limit to be exceeded. This provides better diagnostics than silently returning "(loop)".

### Final Newline Handling

```haskell
removeFinalNewline :: Text -> Text
removeFinalNewline t =
  case T.unsnoc t of
    Just (t', '\n') -> t'
    _ -> t
```

Final newlines are stripped from partials to avoid extra blank lines when composing.

### Partial Syntax Variations

1. **Bare partial**: `$boilerplate()$`
   - Simply includes the partial content

2. **Applied to variable**: `$date:fancy()$`
   - Evaluates partial with variable as context
   - If variable is array, iterates over it

3. **With separator**: `$articles:bibentry()[; ]$`
   - For arrays, uses literal separator between iterations

4. **With pipes**: `$employee:name()/uppercase$`
   - Pipes applied AFTER partial evaluation

### Rendering Semantics

From `renderTemp`:

```haskell
renderTemp (Partial fs t) ctx = do
    val' <- renderTemp t ctx      -- Evaluate the partial template
    return $ case applyPipes fs (SimpleVal val') of
      SimpleVal x -> x
      _           -> mempty
```

Partials are evaluated as templates, then pipes are applied to the result.

## Our Current State

### AST (ast.rs)

```rust
pub struct Partial {
    pub name: String,              // Partial template name
    pub var: Option<VariableRef>,  // Optional variable to apply partial to
    pub separator: Option<String>, // Literal separator for arrays
    pub pipes: Vec<Pipe>,          // Pipes to apply to output
    pub source_info: SourceInfo,
}
```

### Parser (parser.rs)

Parsing works but creates an AST node with just the partial name. The actual partial loading/inlining doesn't happen.

### Evaluator (evaluator.rs)

```rust
TemplateNode::Partial(Partial { name, var, separator, pipes, .. }) => {
    // TODO: Implement partial loading and evaluation
    let _ = (name, var, separator, pipes);
    Ok(Doc::Empty)
}
```

Currently returns empty - needs implementation.

## Design Decisions

### Key Decision: Compile-time vs. Runtime Loading

**Haskell approach**: Partials loaded at template compilation time, inlined into AST
- Pros: Errors caught early, single traversal at render time
- Cons: Can't change partials without recompiling the template

**Alternative (render-time loading)**: Load partials during evaluation
- Pros: More flexible, can change partials without recompiling
- Cons: Runtime errors, repeated parsing

**Decision**: Follow Haskell approach (template compilation time) for now. This matches Pandoc behavior and catches errors early. We can add render-time option later if needed.

**Clarification**: "Template compilation time" means when `Template::compile()` is called at Rust runtime. This is distinct from Rust compilation time (when `rustc` runs).

### PartialResolver Trait

```rust
/// Trait for loading partial templates.
pub trait PartialResolver {
    /// Load a partial template by name.
    /// Returns the template source, or None if not found.
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String>;
}

/// Resolver that loads partials from the filesystem.
pub struct FileSystemResolver;

impl PartialResolver for FileSystemResolver {
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String> {
        let partial_path = resolve_partial_path(name, base_path);
        std::fs::read_to_string(&partial_path).ok()
    }
}

/// Resolver that returns empty (for testing).
pub struct NullResolver;

impl PartialResolver for NullResolver {
    fn get_partial(&self, _name: &str, _base_path: &Path) -> Option<String> {
        None
    }
}
```

### Path Resolution Function

```rust
fn resolve_partial_path(partial_name: &str, template_path: &Path) -> PathBuf {
    let partial_path = Path::new(partial_name);
    let base_dir = template_path.parent().unwrap_or(Path::new("."));

    if partial_path.extension().is_some() {
        // Partial has explicit extension: use it
        base_dir.join(partial_name)
    } else {
        // No extension: use template's extension
        let ext = template_path.extension().unwrap_or_default();
        base_dir.join(partial_name).with_extension(ext)
    }
}
```

### Compilation with Partials

New compile signature:

```rust
impl Template {
    /// Compile without partial support (for testing)
    pub fn compile(source: &str) -> TemplateResult<Self> {
        Self::compile_with_resolver(source, "<template>", &NullResolver, 0)
    }

    /// Compile with partial loading from filesystem
    pub fn compile_from_file(path: &Path) -> TemplateResult<Self> {
        let source = std::fs::read_to_string(path)?;
        Self::compile_with_resolver(&source, path, &FileSystemResolver, 0)
    }

    /// Compile with custom resolver
    pub fn compile_with_resolver(
        source: &str,
        template_path: impl AsRef<Path>,
        resolver: &impl PartialResolver,
        nesting_depth: usize,
    ) -> TemplateResult<Self> {
        // ... parse, then resolve partials recursively
    }
}
```

### Partial Resolution (Post-Parse)

After parsing produces AST with `Partial` nodes, we need a second pass to resolve them:

```rust
fn resolve_partials(
    nodes: &mut Vec<TemplateNode>,
    template_path: &Path,
    resolver: &impl PartialResolver,
    depth: usize,
) -> TemplateResult<()> {
    const MAX_DEPTH: usize = 50;

    if depth > MAX_DEPTH {
        // Emit error with source location pointing to the partial that exceeded the limit
        // Find the first Partial node to get its source_info for the error
        for node in nodes.iter() {
            if let TemplateNode::Partial(partial) = node {
                return Err(TemplateError::RecursionLimitExceeded {
                    name: partial.name.clone(),
                    source_info: partial.source_info.clone(),
                    depth: MAX_DEPTH,
                });
            }
        }
        // Shouldn't reach here, but fallback
        return Err(TemplateError::RecursionLimitExceeded {
            name: "<unknown>".to_string(),
            source_info: SourceInfo::default(),
            depth: MAX_DEPTH,
        });
    }

    for node in nodes.iter_mut() {
        match node {
            TemplateNode::Partial(partial) => {
                let partial_source = resolver
                    .get_partial(&partial.name, template_path)
                    .ok_or_else(|| TemplateError::PartialNotFound {
                        name: partial.name.clone(),
                    })?;

                // Remove final newline
                let partial_source = partial_source.trim_end_matches('\n');

                // Parse the partial
                let partial_path = resolve_partial_path(&partial.name, template_path);
                let mut partial_template = Template::compile_with_resolver(
                    partial_source,
                    &partial_path,
                    resolver,
                    depth + 1,
                )?;

                // Store the parsed partial in the AST
                partial.resolved = Some(Box::new(partial_template));
            }

            // Recurse into nested structures
            TemplateNode::Conditional(cond) => {
                for (_, body) in &mut cond.branches {
                    resolve_partials(body, template_path, resolver, depth)?;
                }
                if let Some(else_branch) = &mut cond.else_branch {
                    resolve_partials(else_branch, template_path, resolver, depth)?;
                }
            }
            // ... similar for ForLoop, Nesting, BreakableSpace
            _ => {}
        }
    }
    Ok(())
}
```

### Modified AST

```rust
pub struct Partial {
    pub name: String,
    pub var: Option<VariableRef>,
    pub separator: Option<String>,
    pub pipes: Vec<Pipe>,
    pub source_info: SourceInfo,
    /// Resolved partial template (populated during compilation)
    pub resolved: Option<Box<Template>>,
}
```

### Evaluation

```rust
fn evaluate_partial(
    partial: &Partial,
    context: &TemplateContext,
) -> TemplateResult<Doc> {
    let template = partial.resolved.as_ref()
        .ok_or_else(|| TemplateError::UnresolvedPartial {
            name: partial.name.clone()
        })?;

    match &partial.var {
        None => {
            // Bare partial: evaluate with current context
            template.evaluate(context)
        }
        Some(var) => {
            // Applied partial: evaluate with variable as context
            let value = resolve_variable(var, context);

            match value {
                Some(TemplateValue::List(items)) => {
                    // Iterate over array
                    let mut results = Vec::new();
                    for item in items {
                        let mut child_ctx = context.child();
                        child_ctx.insert("it", item.clone());
                        results.push(template.evaluate(&child_ctx)?);
                    }

                    // Join with separator
                    if let Some(sep) = &partial.separator {
                        Ok(intersperse_docs(results, Doc::text(sep)))
                    } else {
                        Ok(concat_docs(results))
                    }
                }
                Some(val) => {
                    // Single value
                    let mut child_ctx = context.child();
                    child_ctx.insert("it", val.clone());
                    template.evaluate(&child_ctx)
                }
                None => Ok(Doc::Empty),
            }
        }
    }
    // TODO: Apply pipes to result
}
```

## Implementation Plan

### Phase 1: Core Infrastructure

1. **Add `PartialResolver` trait** (new file: `resolver.rs`)
   - Define trait
   - Implement `FileSystemResolver`
   - Implement `NullResolver`
   - Path resolution function

2. **Extend `Partial` AST**
   - Add `resolved: Option<Box<Template>>` field

3. **Add recursive compilation**
   - New `compile_with_resolver` method
   - Post-parse partial resolution
   - Recursion depth tracking

### Phase 2: Evaluation

4. **Implement `evaluate_partial`**
   - Bare partial evaluation
   - Applied partial (with variable)
   - Array iteration with separator

### Phase 3: Pipes (Deferred)

5. **Apply pipes to partial output**
   - This depends on pipe implementation (separate issue)
   - For now, ignore pipes on partials

### Phase 4: Testing

6. **Unit tests**
   - Path resolution
   - Bare partials
   - Applied partials
   - Array iteration
   - Recursion limit

7. **Integration tests**
   - Port tests from doctemplates (`partials.test`, `pipe-and-partial.test`, `loop-in-partial.test`)

## Error Handling

**Error code namespace**: Template errors use **q-9-*** codes in quarto-error-reporting.

New error variants:

```rust
pub enum TemplateError {
    // ... existing variants

    /// Partial file not found (q-9-001)
    PartialNotFound {
        name: String,
        source_info: SourceInfo,
        searched_path: PathBuf,
    },

    /// Partial contains parse error (q-9-002)
    PartialParseError {
        name: String,
        source_info: SourceInfo,
        error: String,
    },

    /// Recursion limit exceeded (q-9-003)
    RecursionLimitExceeded {
        name: String,
        source_info: SourceInfo,
        depth: usize,
    },

    /// Unresolved partial - internal error (q-9-004)
    UnresolvedPartial {
        name: String,
        source_info: SourceInfo,
    },
}
```

All errors include `SourceInfo` to enable proper error reporting with source locations via `quarto-error-reporting`.

## Test Cases from doctemplates

### partials.test

```
# Input context
{ "employee": [
    { "name": { "first": "John", "last": "Doe" } },
    { "name": { "first": "Omar", "last": "Smith" }, "salary": "30000" },
    { "name": { "first": "Sara", "last": "Chen" }, "salary": "60000" }
  ]
}

# Template
$for(employee)$
$it:name()$
$endfor$

$employee:name()[, ]$

# Expected output
(John) Doe
(Omar) Smith
(Sara) Chen

(John) Doe, (Omar) Smith, (Sara) Chen
```

### loop-in-partial.test

Tests recursion limit:
```
# Template: loop1.txt contains $loop2()$
#           loop2.txt contains $loop1()$

$loop1()$

# Haskell expected output: (loop)
# Our expected output: Error with source location
#   error[q-9-003]: partial recursion limit exceeded
#     --> loop1.txt:1:1
#       |
#     1 | $loop2()$
#       | ^^^^^^^^^ partial 'loop2' exceeded maximum nesting depth of 50
```

## Open Questions

1. **Should we support render-time partial loading?**
   - Could add `TemplateWithPartials` that defers loading
   - For now: template compilation time only

2. ~~**How to handle missing partials?**~~
   - **Resolved**: Emit error with source location (q-9-001)

3. **Caching?**
   - Currently: each partial parsed fresh
   - Could cache compiled partials by path
   - Deferred to future optimization

## Dependencies

- k-387 must be complete (evaluator infrastructure)
- Pipes implementation not required (can defer pipe application)

## Estimated Scope

- Infrastructure (resolver, path resolution): Small
- AST extension: Trivial
- Recursive compilation: Medium
- Evaluation: Medium
- Testing: Medium

Total: Medium-sized task, can be broken into subtasks if needed.
