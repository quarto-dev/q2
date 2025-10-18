# rust-analyzer: Owned Data Patterns (Concrete Examples)

## Overview

This document shows **concrete code examples** from rust-analyzer demonstrating how they use owned data with reference counting instead of lifetimes for their tree structures and configurations.

**Key finding**: rust-analyzer and rowan (its syntax tree library) use **owned data with reference counting** throughout, with **zero lifetimes in the public API**.

## Example 1: Configuration - Pure Owned Data

### Location
`rust-analyzer/crates/rust-analyzer/src/config.rs`

### The Config Struct

```rust
pub struct Config {
    /// Projects that have a Cargo.toml or a rust-project.json
    discovered_projects_from_filesystem: Vec<ProjectManifest>,  // Owned Vec
    discovered_projects_from_command: Vec<ProjectJsonFromCommand>,  // Owned Vec
    workspace_roots: Vec<AbsPathBuf>,  // Owned Vec
    caps: ClientCapabilities,
    root_path: AbsPathBuf,  // Owned path
    snippets: Vec<Snippet>,  // Owned Vec
    client_info: Option<ClientInfo>,

    default_config: &'static DefaultConfigData,  // Only static lifetime!
    client_config: (FullConfigInput, ConfigErrors),
    user_config: Option<(GlobalWorkspaceLocalConfigInput, ConfigErrors)>,

    ratoml_file: FxHashMap<SourceRootId, (RatomlFile, ConfigErrors)>,  // Owned HashMap

    // Arc for shared ownership
    source_root_parent_map: Arc<FxHashMap<SourceRootId, SourceRootId>>,

    validation_errors: ConfigErrors,
    detached_files: Vec<AbsPathBuf>,  // Owned Vec
}
```

**Key observations**:
- **All owned data**: `Vec`, `HashMap`, `Arc` - no lifetime parameters
- **Only `'static` lifetime**: For compile-time defaults
- **Arc for sharing**: `Arc<FxHashMap<...>>` when data needs to be shared
- **Clone-based updates**: Line 1068: `let mut config = self.clone();`

### Config Fields with Owned Collections

```rust
// From the config_data! macro expansion:
completion_snippets_custom: FxIndexMap<String, SnippetDef> = Config::completion_snippets_default(),
files_exclude: Vec<Utf8PathBuf> = vec![],  // Owned Vec of owned paths
completion_autoimport_exclude: Vec<AutoImportExclusion> = vec![...],
completion_excludeTraits: Vec<String> = Vec::new(),  // Owned Vec<String>
diagnostics_disabled: FxHashSet<String> = FxHashSet::default(),  // Owned HashSet
diagnostics_remapPrefix: FxHashMap<String, String> = FxHashMap::default(),  // Owned HashMap
diagnostics_warningsAsHint: Vec<String> = vec![],
```

**Pattern**: Every configuration field is owned. No lifetimes.

### How They Merge Configs

Looking at the `apply_change_with_sink` method:

```rust
fn apply_change_with_sink(&self, change: ConfigChange) -> (Config, bool) {
    let mut config = self.clone();  // <-- Clone the entire config!
    config.validation_errors = ConfigErrors::default();

    let mut should_update = false;

    if let Some(change) = change.user_config_change {
        // Parse and merge...
    }

    (config, should_update)
}
```

**Key observation**: They **clone the entire Config** structure. No attempt to use lifetimes to avoid cloning.

## Example 2: Syntax Trees - Reference Counted Owned Data

### Location
`rowan/src/cursor.rs` (rowan is rust-analyzer's syntax tree library)

### The Core Type: SyntaxNode

```rust
pub struct SyntaxNode {
    ptr: ptr::NonNull<NodeData>,  // <-- Raw pointer, not a lifetime!
}

impl Clone for SyntaxNode {
    #[inline]
    fn clone(&self) -> Self {
        self.data().inc_rc();  // <-- Just increment refcount
        SyntaxNode { ptr: self.ptr }
    }
}

impl Drop for SyntaxNode {
    #[inline]
    fn drop(&mut self) {
        if self.data().dec_rc() {
            unsafe { free(self.ptr) }
        }
    }
}
```

**Key observations**:
- **No lifetime parameters**: `SyntaxNode` has no `<'a>`
- **Manual reference counting**: `inc_rc()` / `dec_rc()`
- **Clone is cheap**: Just increments a counter
- **Drop handles cleanup**: Automatic memory management

### The Internal Data: NodeData

```rust
struct NodeData {
    _c: Count<_SyntaxElement>,

    rc: Cell<u32>,  // <-- Reference count
    parent: Cell<Option<ptr::NonNull<NodeData>>>,
    index: Cell<u32>,
    green: Green,  // Pointer to actual data

    mutable: bool,
    offset: TextSize,
    // Mutable tree links
    first: Cell<*const NodeData>,
    next: Cell<*const NodeData>,
    prev: Cell<*const NodeData>,
}
```

**Key observations**:
- **Reference counted**: `rc: Cell<u32>`
- **Interior mutability**: Uses `Cell` for refcount updates
- **Pointer-based tree**: Parent/sibling links are raw pointers
- **No lifetimes**: All ownership is managed through refcounts

### Creating Nodes

```rust
impl SyntaxNode {
    pub fn new_root(green: GreenNode) -> SyntaxNode {
        let green = GreenNode::into_raw(green);  // Take ownership
        let green = Green::Node { ptr: Cell::new(green) };
        SyntaxNode { ptr: NodeData::new(None, 0, 0.into(), green, false) }
    }

    fn new_child(
        green: &GreenNodeData,
        parent: SyntaxNode,  // <-- Takes owned parent, not &'a parent
        index: u32,
        offset: TextSize,
    ) -> SyntaxNode {
        let mutable = parent.data().mutable;
        let green = Green::Node { ptr: Cell::new(green.into()) };
        SyntaxNode { ptr: NodeData::new(Some(parent), index, offset, green, mutable) }
    }
}
```

**Key observation**: Child nodes take **owned** `parent: SyntaxNode`, not `&'a SyntaxNode`. The refcount is managed internally.

### Traversal Methods

```rust
impl SyntaxNode {
    pub fn parent(&self) -> Option<SyntaxNode> {
        self.data().parent_node()  // <-- Returns owned SyntaxNode
    }

    pub fn children(&self) -> SyntaxNodeChildren {
        SyntaxNodeChildren::new(self.clone())  // <-- Clone for iterator
    }

    pub fn first_child(&self) -> Option<SyntaxNode> {
        // Returns owned node, increments refcount
    }
}
```

**Key observations**:
- **All methods return owned types**: `Option<SyntaxNode>`, not `Option<&'a SyntaxNode>`
- **Clone is cheap**: Just refcount increment
- **No lifetime propagation**: Iterators don't need lifetime parameters

## Example 3: Public API - Zero Lifetimes

### rust-analyzer's SyntaxNode wrapper

```rust
// From rust-analyzer/crates/syntax/src/syntax_node.rs
pub type SyntaxNode = rowan::SyntaxNode<RustLanguage>;  // <-- No lifetime parameter!
pub type SyntaxToken = rowan::SyntaxToken<RustLanguage>;  // <-- No lifetime parameter!
```

### Using it in code

```rust
// From rowan/src/api.rs
pub struct SyntaxNode<L: Language> {
    raw: cursor::SyntaxNode,  // The refcounted node
    _p: PhantomData<L>,  // Language marker
}

impl<L: Language> SyntaxNode<L> {
    pub fn parent(&self) -> Option<SyntaxNode<L>> {  // <-- Returns owned!
        self.raw.parent().map(Self::from)
    }

    pub fn children(&self) -> SyntaxNodeChildren<L> {  // <-- No lifetime!
        SyntaxNodeChildren { raw: self.raw.children(), _p: PhantomData }
    }

    pub fn ancestors(&self) -> impl Iterator<Item = SyntaxNode<L>> + use<L> {
        self.raw.ancestors().map(SyntaxNode::from)  // <-- Owned nodes
    }
}
```

**Key observations**:
- **Zero lifetime parameters**: Neither `<'a>` nor `use<'a, L>`
- **Owned returns**: `Option<SyntaxNode<L>>`, not `Option<&'a SyntaxNode<L>>`
- **Clean API**: No lifetime complexity for users

## Example 4: Arc for Shared Ownership

### When They Need Sharing

```rust
// From config.rs
pub struct Config {
    /// Clone of the value that is stored inside a `GlobalState`.
    source_root_parent_map: Arc<FxHashMap<SourceRootId, SourceRootId>>,
}

impl Config {
    pub fn same_source_root_parent_map(
        &self,
        other: &Arc<FxHashMap<SourceRootId, SourceRootId>>,
    ) -> bool {
        Arc::ptr_eq(&self.source_root_parent_map, other)  // <-- Pointer equality check
    }
}
```

**Pattern**: When data needs to be shared across multiple contexts, use `Arc<T>` instead of lifetimes.

### Another Arc example

```rust
pub struct ConfigErrors(Vec<Arc<ConfigErrorInner>>);  // <-- Arc'd error data
```

**Why Arc?**
- Errors can be shared across multiple contexts
- Avoids lifetime complexity
- Allows errors to outlive the context that created them

## Comparison: What If They Used Lifetimes?

### Hypothetical Lifetime-Based Design

```rust
// THIS IS NOT HOW RUST-ANALYZER DOES IT
pub struct SyntaxNode<'tree> {
    data: &'tree NodeData,
    _p: PhantomData<&'tree ()>,
}

impl<'tree> SyntaxNode<'tree> {
    pub fn parent(&self) -> Option<SyntaxNode<'tree>> {  // Lifetime propagates
        // ...
    }

    pub fn children(&self) -> SyntaxNodeChildren<'tree> {  // Lifetime propagates
        // ...
    }
}

// Every function that touches syntax trees needs lifetimes
pub fn analyze_function<'tree>(func: &SyntaxNode<'tree>) -> Analysis {  // Viral!
    // ...
}
```

**Problems they avoided**:
1. **Viral lifetimes**: Every function needs `<'tree>`
2. **Can't return owned trees**: What if you want to extract a subtree?
3. **Complex with multiple trees**: Can't easily combine nodes from different parses
4. **Iterator hell**: Iterators need lifetime bounds everywhere

## Why This Works for rust-analyzer

### 1. Clone is Cheap

Because it's just refcount increment:
```rust
impl Clone for SyntaxNode {
    fn clone(&self) -> Self {
        self.data().inc_rc();  // Just a counter increment!
        SyntaxNode { ptr: self.ptr }
    }
}
```

### 2. Memory Overhead is Acceptable

- Syntax trees are large, but refcount is just `u32` (4 bytes)
- Multiple SyntaxNodes can point to same NodeData
- Trade: 4 bytes per node vs. lifetime complexity

### 3. Enables Key Features

**Incremental reparsing**:
```rust
pub fn replace_with(&self, replacement: GreenNode) -> GreenNode {
    // Can return a whole new tree
}
```

**Cached subtrees**:
```rust
pub fn clone_subtree(&self) -> SyntaxNode<L> {
    SyntaxNode::from(self.raw.clone_subtree())  // Owned subtree
}
```

**No lifetime constraints**:
- Can store nodes in collections
- Can return nodes from functions
- Can cache nodes across requests

## Lessons for Quarto

### What rust-analyzer teaches us:

1. **Owned data with Arc/refcounting works at scale**
   - rust-analyzer handles huge Rust codebases
   - Performance is excellent
   - API is simple

2. **Lifetimes aren't necessary for tree structures**
   - Even though it seems like the "perfect" use case
   - The complexity cost outweighs memory savings

3. **Config merging is simpler with owned data**
   - Just `clone()` and modify
   - No lifetime juggling across config layers

4. **Reference counting is a proven pattern**
   - rowan uses manual refcounting
   - We could use `Rc<T>` or `Arc<T>` (even simpler)

5. **Zero lifetime parameters in public API**
   - Makes the library much easier to use
   - Prevents lifetime propagation across codebase

## Applying to YamlWithSourceInfo

### rust-analyzer's pattern applied to YAML:

```rust
// Similar to rust-analyzer's SyntaxNode
pub struct YamlWithSourceInfo {
    yaml: Yaml,  // Owned (yaml-rust2::Yaml is Clone)
    source_info: SourceInfo,  // Owned
    children: Children,  // Owned
}

// Clean API, no lifetimes
impl YamlWithSourceInfo {
    pub fn get_hash_value(&self, key: &str) -> Option<&YamlWithSourceInfo> {
        // Returns reference with anonymous lifetime (like rust-analyzer)
    }

    pub fn parent(&self) -> Option<YamlWithSourceInfo> {
        // Could return owned if we add parent links (like rust-analyzer)
    }
}

// For sharing across contexts (like rust-analyzer's Arc usage)
pub struct MergedConfig {
    config: Arc<YamlWithSourceInfo>,  // Shared ownership when needed
}
```

### When to use Arc vs. owned:

**Use owned (clone) for**:
- Config merging (rust-analyzer does this)
- Short-lived traversals
- Local modifications

**Use Arc for**:
- Sharing across threads
- Sharing across long-lived contexts
- When clone would be expensive (large trees)

## Code Locations for Further Study

### rust-analyzer
- **Config handling**: `crates/rust-analyzer/src/config.rs` (lines 980-1069)
- **Config merging**: Search for `apply_change_with_sink` (line 1067)
- **Arc usage**: Search for `Arc<` in config.rs

### rowan
- **SyntaxNode core**: `src/cursor.rs` (lines 146-974)
- **Reference counting**: Lines 150-165 (Clone/Drop impl)
- **NodeData structure**: Lines 112-130
- **Public API**: `src/api.rs` (lines 15-304)

### Suggested exploration:

```bash
cd /Users/cscheid/repos/github/cscheid/kyoto/tmp/rust-analyzer
# Look at config cloning
grep -A 20 "fn apply_change_with_sink" crates/rust-analyzer/src/config.rs

cd /Users/cscheid/repos/github/cscheid/kyoto/tmp/rowan
# Look at reference counting
grep -A 10 "impl Clone for SyntaxNode" src/cursor.rs
grep -A 10 "impl Drop for SyntaxNode" src/cursor.rs

# Look at the NodeData structure
grep -A 20 "struct NodeData" src/cursor.rs
```

## Conclusion

rust-analyzer provides concrete evidence that **owned data with reference counting** works excellently for tree structures and configurations in large-scale Rust projects. They explicitly chose this over lifetimes despite the seeming "perfect fit" of lifetimes for trees.

**Key takeaway**: If rust-analyzer (one of the most complex and performance-critical Rust projects) uses owned data for their syntax trees, it's a proven pattern for similar use cases like YamlWithSourceInfo.

**The precedent is strong**: Choose simplicity and proven patterns over theoretical optimality.
