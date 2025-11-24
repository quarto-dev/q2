# Template Evaluator Implementation Plan

**Date**: 2025-11-24
**Related Issues**: k-387, k-389, k-390, k-391, k-392, k-393
**Epic**: k-379 (Port Pandoc template functionality)

## Overview

This document outlines the implementation plan for the template evaluator in `quarto-doctemplate`. The evaluator takes a parsed template AST and a context of variable bindings, and produces rendered output.

## Reference Implementation Analysis

The reference implementation is jgm/doctemplates (Haskell), which powers Pandoc's template system.

### Key Data Types in Reference

```haskell
-- Template AST (simplified)
data Template a =
    Interpolate Variable
  | Conditional Variable (Template a) (Template a)
  | Iterate Variable (Template a) (Template a)  -- body, separator
  | Nested (Template a)
  | Partial [Pipe] (Template a)
  | Literal (Doc a)
  | Concat (Template a) (Template a)
  | Empty

-- Values
data Val a =
    SimpleVal (Doc a)  -- Note: Doc, not String!
  | ListVal [Val a]
  | MapVal (Context a)
  | BoolVal Bool
  | NullVal

-- Variable resolution result
data Resolved a = Resolved Bool [Doc a]  -- truthiness + rendered values
```

### Key Insight: Doc vs String Output

The Haskell implementation uses `Doc a` from the `doclayout` library, NOT `String`. This is critical for:

1. **Breakable spaces**: `Doc` distinguishes between breakable and unbreakable spaces
2. **Nesting**: `Doc` tracks column position for proper indentation
3. **Flexible wrapping**: Output can be reflowed to fit line widths

For simple string rendering, breakable spaces have no effect, and nesting requires manual post-processing.

### Rendering Semantics

From `resolveVariable'`:
```haskell
resolveVariable' v val =
  case applyPipes (varPipes v) $ multiLookup (varParts v) val of
    ListVal xs    -> mconcat $ map (resolveVariable' mempty) xs  -- concatenate
    SimpleVal d
      | DL.isEmpty d -> Resolved False []
      | otherwise    -> Resolved True [removeFinalNl d]
    MapVal _      -> Resolved True ["true"]
    BoolVal True  -> Resolved True ["true"]
    BoolVal False -> Resolved False ["false"]
    NullVal       -> Resolved False []
```

Key observations:
- **List**: Recursively resolve each element and concatenate
- **String**: Remove final newline, truthy if non-empty
- **Map**: Renders as "true", is truthy
- **Bool true**: Renders as "true"
- **Bool false**: Renders as "false" but is falsy (empty for output)
- **Null**: Empty output, falsy

### For Loop Semantics

From `withVariable`:
```haskell
withVariable var ctx f =
  case applyPipes (varPipes var) $ multiLookup (varParts var) (MapVal ctx) of
    NullVal     -> return mempty  -- no iterations
    ListVal xs  -> mapM (\iterval -> f $ setVarVal iterval) xs
    MapVal ctx' -> (:[]) <$> f (setVarVal (MapVal ctx'))
    val' -> (:[]) <$> f (setVarVal val')
 where
  setVarVal x =
    addToContext var x $ Context $ M.insert "it" x $ unContext ctx
```

Key observations:
- **Array**: Multiple iterations, each element bound to var AND "it"
- **Map**: Single iteration, map bound to var AND "it"
- **Scalar**: Single iteration, value bound to var AND "it"
- **Null**: Zero iterations

### Nesting Implementation

From `renderTemp`:
```haskell
renderTemp (Nested t) ctx = do
  n <- S.get  -- get current column
  DL.nest n <$> renderTemp t ctx
```

The `RenderState` monad tracks column position, updated by `updateColumn` after each literal.

## Our Implementation

### Current State

We have:
- `TemplateValue`: `String | Bool | List | Map | Null` (matches reference)
- `TemplateContext`: With parent scoping (matches reference)
- `TemplateValue::is_truthy()`: Implemented correctly
- `TemplateValue::render()`: Basic implementation
- `evaluator.rs`: Skeleton with TODOs

### Design Decision: Phased Approach

**Phase 1 (k-387 scope): Simple String Output**
- Output type: `String`
- No pipes
- No partials
- Nesting: Simplified approach (track column, post-process newlines)
- Breakable spaces: No-op (pass through content)

**Phase 2 (future): Full Doc Output**
- Create `Doc<T>` type with proper breakable space support
- Full nesting with column tracking
- All pipes implemented
- Partial loading

### Phase 1 Implementation Plan

#### 1. Variable Interpolation (k-389)

```rust
fn resolve_variable(var: &VariableRef, context: &TemplateContext) -> Option<&TemplateValue> {
    let path: Vec<&str> = var.path.iter().map(|s| s.as_str()).collect();
    context.get_path(&path)
}

fn render_variable(var: &VariableRef, context: &TemplateContext) -> String {
    match resolve_variable(var, context) {
        Some(value) => {
            // Handle literal separator for arrays: $var[, ]$
            if let Some(sep) = &var.separator {
                if let TemplateValue::List(items) = value {
                    return items.iter()
                        .map(|v| v.render())
                        .collect::<Vec<_>>()
                        .join(sep);
                }
            }
            value.render()
        }
        None => String::new(),
    }
}
```

**Considerations**:
- The `it` keyword needs special handling in loop contexts
- Variable paths like `employee.salary` are handled by `get_path`

#### 2. Conditional Evaluation (k-390)

```rust
fn evaluate_conditional(
    branches: &[(VariableRef, Vec<TemplateNode>)],
    else_branch: &Option<Vec<TemplateNode>>,
    context: &TemplateContext,
) -> TemplateResult<String> {
    for (condition, body) in branches {
        if let Some(value) = resolve_variable(condition, context) {
            if value.is_truthy() {
                return evaluate(body, context);
            }
        }
    }

    // No branch matched, try else
    if let Some(else_body) = else_branch {
        evaluate(else_body, context)
    } else {
        Ok(String::new())
    }
}
```

**Note**: Our `is_truthy()` already matches Pandoc semantics.

#### 3. For Loop Evaluation (k-391)

```rust
fn evaluate_for_loop(
    var: &VariableRef,
    body: &[TemplateNode],
    separator: &Option<Vec<TemplateNode>>,
    context: &TemplateContext,
) -> TemplateResult<String> {
    let value = resolve_variable(var, context);

    let items: Vec<&TemplateValue> = match value {
        Some(TemplateValue::List(items)) => items.iter().collect(),
        Some(TemplateValue::Map(_)) => vec![value.unwrap()],  // Single iteration
        Some(v) if v.is_truthy() => vec![v],  // Single iteration for truthy scalars
        _ => vec![],  // No iterations
    };

    let mut results = Vec::new();
    let var_name = var.path.last().unwrap_or(&String::new()).clone();

    for item in &items {
        let mut child_ctx = context.child();

        // Bind to variable name AND "it"
        child_ctx.insert(&var_name, (*item).clone());
        child_ctx.insert("it", (*item).clone());

        results.push(evaluate(body, &child_ctx)?);
    }

    // Join with separator
    if let Some(sep_nodes) = separator {
        let sep = evaluate(sep_nodes, context)?;
        Ok(results.join(&sep))
    } else {
        Ok(results.concat())
    }
}
```

**Key points**:
- Both the variable name AND `it` are bound
- Child context for scoping
- Separator is rendered once, used between iterations

#### 4. Nesting (k-392) - Simplified Approach

For Phase 1, we use a simpler post-processing approach rather than the full Doc monad:

```rust
struct EvalState {
    column: usize,  // Current column position
}

fn apply_nesting(content: &str, indent: usize) -> String {
    // Add indentation to all lines after the first
    content
        .lines()
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                line.to_string()
            } else {
                format!("{}{}", " ".repeat(indent), line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
```

**Limitation**: This simplified approach doesn't handle all edge cases that the Doc-based approach handles (e.g., nested nesting directives, interaction with breakable spaces).

#### 5. Breakable Spaces (k-393) - No-op

For String output, breakable spaces have no effect:

```rust
// In evaluate_node
TemplateNode::BreakableSpace(BreakableSpace { children, .. }) => {
    // For String output, just render children as-is
    evaluate(children, context)
}
```

### Testing Strategy

1. **Unit tests** for each component:
   - Variable resolution with various paths
   - Truthiness edge cases
   - For loop iteration modes
   - Nested conditionals

2. **Integration tests**:
   - Parse and evaluate complete templates
   - Compare against expected output
   - Use test templates from `crates/tree-sitter-doctemplate/test-templates/`

3. **Pandoc comparison tests**:
   - Run same templates through Pandoc
   - Verify matching output (where our semantics match)

### Implementation Order

1. **k-389: Variable interpolation** - Foundation for everything else
2. **k-390: Conditionals** - Depends on variable resolution for truthiness
3. **k-391: For loops** - Most complex, depends on variable resolution
4. **k-392: Nesting** - Can be simplified initially
5. **k-393: Breakable spaces** - No-op for Phase 1

### Alternatives Considered

#### Alternative A: Implement Doc type from the start

**Pros**:
- Full compatibility with Pandoc
- Proper breakable space and nesting support

**Cons**:
- Significantly more complex
- Requires implementing doclayout equivalent
- Overkill for initial use cases

**Decision**: Defer to Phase 2. String output is sufficient for initial integration.

#### Alternative B: Use existing doclayout port

**Pros**:
- Proven implementation

**Cons**:
- No Rust port exists
- Would need to port from Haskell

**Decision**: Not viable for Phase 1.

#### Alternative C: Return enum instead of String

```rust
enum RenderOutput {
    Text(String),
    Doc(Doc),
}
```

**Pros**:
- Could support both modes

**Cons**:
- Adds complexity throughout
- Need to decide at call site which mode

**Decision**: Consider for Phase 2 if needed.

## Open Questions

1. **Pipe priority**: k-387 says "no pipes" but pipes affect rendering in subtle ways. Should we implement basic pipes (uppercase, lowercase) in Phase 1?

2. **Partial loading**: How will partials be loaded? Need a trait similar to Haskell's `TemplateMonad`:
   ```rust
   trait PartialLoader {
       fn get_partial(&self, name: &str) -> Option<String>;
   }
   ```

3. **Error handling**: Should we fail on undefined variables or silently return empty? Pandoc returns empty. We should match.

## Next Steps

1. Implement k-389 (variable interpolation)
2. Add tests as we go
3. Iterate through remaining issues in order
4. Validate against Pandoc output
