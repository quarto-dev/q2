# YamlWithSourceInfo: Lifetime-Based Approach (Alternative Design)

## Executive Summary

This document explores the **lifetime-based approach** to `YamlWithSourceInfo` as proposed by the user. This approach uses borrowed references with hierarchical lifetimes instead of owned data.

**Key insight**: We CAN express "the merged object has the shorter of the two lifetimes" in Rust's type system using lifetime bounds.

## The User's Proposal

### Context Hierarchy

```rust
/// Lives for entire project duration
struct ProjectContext {
    project_yaml: Yaml,  // Owned
    // ... project config, paths, etc.
}

/// Lives for single document rendering
struct DocumentContext<'proj> {
    project_ctx: &'proj ProjectContext,
    document_yaml: Yaml,  // Owned
    // ... document-specific data
}

/// Lives for single cell (if needed)
struct CellContext<'doc> {
    document_ctx: &'doc DocumentContext<'???>,  // We'll come back to this
    cell_yaml: Yaml,  // Owned
}
```

### YamlWithSourceInfo with Lifetimes

```rust
pub struct YamlWithSourceInfo<'a> {
    yaml: &'a Yaml,  // Borrowed from context
    source_info: SourceInfo,
    children: Children<'a>,
}

enum Children<'a> {
    None,
    Array(Vec<YamlWithSourceInfo<'a>>),
    Hash(Vec<YamlHashEntry<'a>>),
}

pub struct YamlHashEntry<'a> {
    key: YamlWithSourceInfo<'a>,
    value: YamlWithSourceInfo<'a>,
}
```

### Wrapping YAML from Contexts

```rust
impl ProjectContext {
    fn get_config(&self) -> YamlWithSourceInfo<'_> {
        wrap_yaml_with_source_info(
            &self.project_yaml,
            SourceInfo::original(self.project_file_id, ...),
        )
    }
}

impl<'proj> DocumentContext<'proj> {
    fn get_config(&self) -> YamlWithSourceInfo<'_> {
        wrap_yaml_with_source_info(
            &self.document_yaml,
            SourceInfo::original(self.document_file_id, ...),
        )
    }
}

// Helper to create YamlWithSourceInfo<'a> from &'a Yaml
fn wrap_yaml_with_source_info<'a>(
    yaml: &'a Yaml,
    source_info: SourceInfo,
) -> YamlWithSourceInfo<'a> {
    let children = match yaml {
        Yaml::Array(arr) => {
            let elements = arr.iter()
                .enumerate()
                .map(|(i, child)| {
                    let child_source = source_info.array_element(i);
                    wrap_yaml_with_source_info(child, child_source)
                })
                .collect();
            Children::Array(elements)
        }
        Yaml::Hash(hash) => {
            let entries = hash.iter()
                .map(|(k, v)| {
                    let key_source = source_info.hash_key(k);
                    let value_source = source_info.hash_value(k);
                    YamlHashEntry {
                        key: wrap_yaml_with_source_info(k, key_source),
                        value: wrap_yaml_with_source_info(v, value_source),
                    }
                })
                .collect();
            Children::Hash(entries)
        }
        _ => Children::None,
    };

    YamlWithSourceInfo {
        yaml,
        source_info,
        children,
    }
}
```

## The Key Question: How to Merge Different Lifetimes?

### User's Intuition

> "It would be sufficient for the YamlWithSourceInfo object that came from merging document metadata and project metadata to have the same lifetime bounds as the DocumentContext object that holds the document Yaml object. [...] I'd like to be able to express that the merged object has the shorter of the two lifetimes."

### How to Express This in Rust

The answer: **lifetime bounds** with `where 'longer: 'shorter`

```rust
fn merge<'short, 'long>(
    base: &YamlWithSourceInfo<'long>,
    override_layer: &YamlWithSourceInfo<'short>,
) -> YamlWithSourceInfo<'short>
where
    'long: 'short,  // 'long outlives 'short
{
    // Return value has lifetime 'short (the shorter one)
    // Can contain references to both 'long and 'short data
}
```

This works because:
- If `'long: 'short`, then any reference valid for `'long` is also valid for `'short`
- The merged result lives for `'short`
- Children can point to either project data (`'long`) or document data (`'short`)
- All references remain valid for the result's lifetime

### Usage Example

```rust
impl<'proj> DocumentContext<'proj> {
    fn get_merged_config(&self) -> YamlWithSourceInfo<'_> {
        // Project config has lifetime 'proj
        let project_config: YamlWithSourceInfo<'proj> =
            self.project_ctx.get_config();

        // Document config has lifetime tied to &self borrow (call it 'doc)
        // The compiler knows 'proj: 'doc (project outlives this borrow)
        let document_config: YamlWithSourceInfo<'_> =
            self.get_config();

        // Merge returns YamlWithSourceInfo<'doc> (the shorter lifetime)
        merge(&project_config, &document_config)
    }
}
```

## The Hybrid Ownership Problem

### Challenge: Merged Containers

When we merge two hashes, we create a NEW hash that combines keys from both:

```
Project YAML:     {theme: "cosmo", toc: true}
Document YAML:    {toc: false, css: "style.css"}
Merged:           {theme: "cosmo", toc: false, css: "style.css"}
```

**Problem**: The merged hash doesn't exist in either the project or document YAML. It's a new container.

**But**: The leaf values (strings, integers) CAN still be borrowed from the original YAML.

### Solution: Hybrid Ownership

```rust
pub enum YamlRef<'a> {
    /// Borrowed from original context
    Borrowed(&'a Yaml),

    /// Owned (for merged containers)
    Owned(Yaml),
}

pub struct YamlWithSourceInfo<'a> {
    yaml: YamlRef<'a>,  // Can be borrowed OR owned
    source_info: SourceInfo,
    children: Children<'a>,
}
```

### Merge Implementation

```rust
fn merge<'short, 'long>(
    base: &YamlWithSourceInfo<'long>,
    override_layer: &YamlWithSourceInfo<'short>,
) -> YamlWithSourceInfo<'short>
where
    'long: 'short,
{
    match (base.yaml.as_yaml(), override_layer.yaml.as_yaml()) {
        (Yaml::Hash(base_hash), Yaml::Hash(override_hash)) => {
            // Merge hash entries
            let mut merged_entries = Vec::new();

            // Process all keys from both hashes
            // ...

            // Create NEW owned hash (doesn't exist in either source)
            let merged_hash = build_merged_hash(&merged_entries);

            YamlWithSourceInfo {
                yaml: YamlRef::Owned(Yaml::Hash(merged_hash)),
                source_info: SourceInfo::concat(...),
                children: Children::Hash(merged_entries),
            }
        }

        // Scalar override: can borrow from override_layer
        (_, _) => YamlWithSourceInfo {
            yaml: YamlRef::Borrowed(override_layer.yaml.as_yaml()),
            source_info: override_layer.source_info.clone(),
            children: override_layer.children.clone(),
        }
    }
}
```

**Key insight**:
- Merged containers (hashes, arrays) must be owned
- Leaf values and non-merged nodes can be borrowed
- This is a **hybrid** approach: some nodes owned, some borrowed

## Lifetime Propagation Through the Codebase

### How Far Do Lifetimes Propagate?

**Functions that accept config:**

```rust
fn validate_config<'a>(
    config: &YamlWithSourceInfo<'a>,
    schema: &Schema,
) -> Vec<ValidationError> {
    // Lifetime parameter required
}

fn render_document<'proj>(
    project_ctx: &'proj ProjectContext,
    document_path: &Path,
) -> Result<()> {
    let doc_ctx = DocumentContext::new(project_ctx, document_path)?;
    let merged_config = doc_ctx.get_merged_config();  // Inferred lifetime

    validate_config(&merged_config)?;  // Passes lifetime implicitly
    render_with_config(&merged_config)?;

    Ok(())
}
```

**Not too bad!** Most code uses elided lifetimes (`'_` or implicit).

**But**: Every function signature that touches YAML configs needs the lifetime parameter, even if just `<'a>`.

### Viral Lifetimes Example

```rust
// Before (owned data):
fn get_format_theme(config: &YamlWithSourceInfo) -> Option<&str> {
    config.get_path(&["format", "html", "theme"])?.as_str()
}

// After (lifetimes):
fn get_format_theme<'a>(config: &YamlWithSourceInfo<'a>) -> Option<&str> {
    config.get_path(&["format", "html", "theme"])?.as_str()
}
```

**Impact**: Every function that touches config needs `<'a>` parameter.

This is **viral** - once you add a lifetime parameter to one function, all callers and callees need it too.

## LSP Caching: The Serialization Problem

### The Challenge

LSP needs to cache parsed configs to disk for performance. With references, we can't serialize directly.

```rust
// Can't derive Serialize with references
#[derive(Serialize, Deserialize)]
pub struct YamlWithSourceInfo<'a> {
    yaml: &'a Yaml,  // ❌ Can't serialize references
    ...
}
```

### Solution 1: Separate Owned Type

```rust
/// Runtime type with references
pub struct YamlWithSourceInfo<'a> {
    yaml: &'a Yaml,
    ...
}

/// Serializable type with owned data
#[derive(Serialize, Deserialize)]
pub struct YamlWithSourceInfoOwned {
    yaml: Yaml,  // Owned
    ...
}

impl<'a> YamlWithSourceInfo<'a> {
    /// Convert to owned for serialization
    fn to_owned(&self) -> YamlWithSourceInfoOwned {
        YamlWithSourceInfoOwned {
            yaml: self.yaml.clone(),  // Clone the Yaml
            source_info: self.source_info.clone(),
            children: self.children.to_owned(),
        }
    }
}
```

**Impact**:
- Need two parallel types
- Conversion requires cloning (same cost as owned approach!)
- More API surface area

### Solution 2: Don't Cache

Just re-parse on every LSP request.

**Impact**:
- Slower LSP (but maybe acceptable if parsing is fast enough)
- Simpler code (no serialization)

## Comparison: Lifetime vs Owned

### Memory Usage

| Scenario | Lifetime Approach | Owned Approach |
|----------|-------------------|----------------|
| Single file parse | **1x** (references only) | 3x (duplication) |
| After merge | **~1.2x** (merged containers owned, leaves borrowed) | 3x (all owned) |
| LSP cache | **3x** (must convert to owned) | 3x (already owned) |

**Winner for memory**: Lifetime approach (when not caching)

### Code Complexity

| Aspect | Lifetime Approach | Owned Approach |
|--------|-------------------|----------------|
| Core types | Moderate (`<'a>` everywhere) | Simple (no parameters) |
| Merge implementation | Complex (hybrid ownership) | Simple (clone everything) |
| API surface | Moderate (lifetime parameters) | Simple (no parameters) |
| Serialization | Complex (separate type or no cache) | Simple (derive Serialize) |
| Viral propagation | High (every function needs `<'a>`) | None |

**Winner for simplicity**: Owned approach

### Performance

| Operation | Lifetime Approach | Owned Approach |
|-----------|-------------------|----------------|
| Initial parse | **Fast** (just wrap) | Slower (duplicate data) |
| Config access | Same | Same |
| Merge | Moderate (hybrid clone) | Slower (clone everything) |
| LSP cache write | **Slow (convert to owned)** | Fast (already owned) |
| LSP cache read | Same | Same |

**Winner**: Depends on workload. Lifetime wins for one-off renders, owned wins for LSP.

## Precedents in Large Rust Projects

### rustc (Rust Compiler)

- Uses **arena allocation** with lifetimes extensively
- Types are parameterized with `'tcx` (type context lifetime)
- Very memory efficient
- **But**: Extremely complex lifetime management
- Example: `struct TyCtxt<'tcx>`, `type Ty<'tcx> = &'tcx TyS<'tcx>`

**Verdict**: Feasible but complex. rustc is one of the most complex Rust codebases.

### rust-analyzer (Rust IDE)

- Uses **owned data** with extensive use of `Arc<T>` and interning
- Types are often `Arc<FooData>` instead of `Foo<'a>`
- Less memory efficient but simpler
- Easier to cache and serialize

**Quote from rust-analyzer architecture docs**:
> "We prefer to use owned data and Arc liberally. Explicit lifetimes are avoided where possible."

**Verdict**: Chose simplicity over memory efficiency.

### salsa (rust-analyzer's incremental computation framework)

- Uses **owned data** for all inputs and intermediate values
- Everything is `Clone` or `Arc`
- Caching is trivial (already owned)
- Trade memory for simplicity and cacheability

### ripgrep

- Uses **lifetimes** for zero-copy parsing
- Very memory efficient
- **But**: Doesn't have complex data structures or merging
- Processes files one at a time

**Verdict**: Lifetimes work well for simple, linear workflows.

### serde_json::Value

- Uses **owned data** (`String`, `Vec`, etc.)
- No lifetimes in public API
- Easy to use but not memory efficient

**Verdict**: Prioritizes ease of use.

## Recommendation

### Both Approaches Are Viable

The user is correct: **the lifetime-based approach is feasible**. We CAN express the required lifetime relationships in Rust.

### Trade-offs Summary

**Choose lifetimes if**:
- Memory efficiency is critical
- You're comfortable with complex lifetime management
- LSP caching is not required or can be sacrificed
- You want the "most Rusty" solution

**Choose owned data if**:
- API simplicity is important
- Serialization/caching is important (LSP!)
- This is a port and you want to minimize risk
- You want to follow rust-analyzer's precedent

### My Recommendation: Owned Data

**For Quarto specifically**, I still recommend owned data because:

1. **LSP is important** - caching will be critical for responsiveness
2. **This is a port** - simplicity reduces risk, speeds development
3. **Memory cost is acceptable** - configs are small (<10KB), ~30KB overhead is negligible
4. **rust-analyzer precedent** - similar use case (IDE tool), chose owned data
5. **Serialization matters** - we'll need it for caching and debugging

But I acknowledge the lifetime approach is more memory-efficient and could work.

### Middle Ground: Start Owned, Optimize Later?

Could start with owned data for:
- Faster initial development
- Proven design pattern
- LSP caching works out of the box

Then profile and consider converting to lifetimes if memory becomes an issue.

**Benefit**: Don't pay complexity cost unless we need to.

## Detailed Lifetime Design (If We Choose It)

If you want to proceed with the lifetime approach, here's the complete design:

### Core Types

```rust
pub enum YamlRef<'a> {
    Borrowed(&'a Yaml),
    Owned(Yaml),
}

pub struct YamlWithSourceInfo<'a> {
    yaml: YamlRef<'a>,
    source_info: SourceInfo,
    children: Children<'a>,
}

pub enum Children<'a> {
    None,
    Array(Vec<YamlWithSourceInfo<'a>>),
    Hash(Vec<YamlHashEntry<'a>>),
}

pub struct YamlHashEntry<'a> {
    pub key: YamlWithSourceInfo<'a>,
    pub value: YamlWithSourceInfo<'a>,
}
```

### Access API

```rust
impl<'a> YamlWithSourceInfo<'a> {
    pub fn as_yaml(&self) -> &Yaml {
        self.yaml.as_yaml()
    }

    pub fn get_hash_value(&self, key: &str) -> Option<&YamlWithSourceInfo<'a>> {
        // ...
    }

    pub fn iter_array(&self) -> Option<impl Iterator<Item = &YamlWithSourceInfo<'a>>> {
        // ...
    }
}
```

### Merge Signature

```rust
pub fn merge<'short, 'long>(
    base: &YamlWithSourceInfo<'long>,
    override_layer: &YamlWithSourceInfo<'short>,
) -> YamlWithSourceInfo<'short>
where
    'long: 'short,
{
    // Hybrid implementation
}
```

### Context Integration

```rust
impl<'proj> DocumentContext<'proj> {
    pub fn get_merged_config(&self) -> YamlWithSourceInfo<'_> {
        let project_config = self.project_ctx.get_config();
        let document_config = self.get_config();
        merge(&project_config, &document_config)
    }
}
```

### Serialization Strategy

```rust
// Option 1: Separate owned type
#[derive(Serialize, Deserialize)]
pub struct YamlWithSourceInfoOwned {
    yaml: Yaml,
    source_info: SourceInfo,
    children: ChildrenOwned,
}

impl<'a> YamlWithSourceInfo<'a> {
    pub fn to_owned(&self) -> YamlWithSourceInfoOwned {
        // Clone everything
    }
}

// Option 2: Skip caching (just re-parse)
// Simpler but slower LSP
```

## Open Questions for Lifetime Approach

1. **How much does lifetime complexity slow development?**
   - Hard to estimate without trying
   - Could add weeks to implementation

2. **Is LSP caching negotiable?**
   - Maybe parsing is fast enough that we don't need caching?
   - Would need to benchmark

3. **How maintainable is this for future contributors?**
   - Lifetimes are a Rust learning curve
   - Might make contributions harder

4. **What about error messages?**
   - Lifetime errors can be cryptic
   - Might complicate debugging

## Conclusion

**You're absolutely right**: the lifetime approach is feasible and would be more memory-efficient.

**The choice comes down to**: Memory efficiency vs. complexity/cacheability

**For Quarto**, I still lean toward owned data because:
- LSP caching is valuable
- Simplicity reduces port risk
- Memory cost is small (KB not MB)
- Follows rust-analyzer's precedent

**But**: If memory efficiency is a priority and you're comfortable with lifetime complexity, the lifetime approach is a solid choice.

**My suggestion**: Start with owned data for the MVP, measure memory usage, and reconsider if needed?

**Or**: If you feel strongly about lifetimes, we can prototype both and compare real-world performance.

What are your thoughts?
