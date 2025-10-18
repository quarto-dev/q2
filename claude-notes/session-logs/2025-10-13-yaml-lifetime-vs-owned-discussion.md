# Session Log: YamlWithSourceInfo Lifetime vs Owned Data Discussion (2025-10-13)

## Session Overview

This session involved a deep technical discussion about whether to use lifetime-based or owned-data approach for `YamlWithSourceInfo` (renamed from `AnnotatedParse`). The user challenged my initial recommendation of owned data by proposing a lifetime-based design with context hierarchies.

## Key Participants & Perspectives

**User's Position**:
- Proposed lifetime-based design with ProjectContext/DocumentContext hierarchy
- Wanted to use references to avoid data duplication
- Asked: "Can we express that the merged object has the shorter of the two lifetimes?"

**My Initial Position**:
- Recommended owned data (3x memory overhead)
- Cited config merging challenges with lifetimes
- May have dismissed lifetime approach too quickly

**Final Outcome**:
- Both approaches are technically feasible
- Provided concrete rust-analyzer examples showing their owned-data choice
- User will review evidence and decide

## Chronological Discussion

### 1. User's Initial Challenge

The user read my owned-data design and pushed back:

> "Your concerns about lifetime make sense, but let me try to argue just a bit further for my idea. I expect Quarto to have a data structure like ProjectContext that will exist for the lifetime of a project, which could serve as the "witness" for references that only live as long as a project do."

**Key insight proposed**:
```rust
struct ProjectContext {
    project_yaml: Yaml,  // Owned, lives for project lifetime
}

struct DocumentContext<'proj> {
    project_ctx: &'proj ProjectContext,
    document_yaml: Yaml,  // Owned, lives for document lifetime
}

// Merged config has the shorter lifetime
fn merge<'short, 'long>(
    base: &YamlWithSourceInfo<'long>,
    override: &YamlWithSourceInfo<'short>
) -> YamlWithSourceInfo<'short>  // <-- User's key point
where 'long: 'short
```

**User's question**: "Is this kind of thing typically done in larger Rust projects?"

### 2. My Response: Lifetime Approach IS Feasible

I created `yaml-with-source-info-lifetime-approach.md` acknowledging:

**Yes, you can express "shorter of two lifetimes"** using `where 'long: 'short`

**But requires hybrid ownership**:
```rust
pub enum YamlRef<'a> {
    Borrowed(&'a Yaml),  // For leaves from original contexts
    Owned(Yaml),         // For merged containers
}
```

**Trade-offs analysis**:

| Aspect | Lifetime Approach | Owned Approach |
|--------|-------------------|----------------|
| Memory | 1x single file, ~1.2x merged | 3x always |
| Complexity | High (viral lifetimes) | Low (no parameters) |
| LSP Caching | Hard (can't serialize refs) | Easy (already owned) |
| API Ergonomics | Every function needs `<'a>` | No lifetime parameters |

**Precedents reviewed**:
- **rustc**: Uses lifetimes extensively (very complex)
- **rust-analyzer**: Uses owned data (simpler)
- **salsa**: Uses owned data (simpler)
- **ripgrep**: Uses lifetimes (but simpler use case)

**Conclusion**: Both viable, but recommended owned for Quarto due to LSP importance and API simplicity.

### 3. User Asks for Concrete Examples

> "Thank you for providing concrete precedents. Can you guide me through a bit of the rust-analyzer source code where this owned data approach is used?"

### 4. Exploring rust-analyzer Code

I cloned rust-analyzer and rowan, then created detailed analysis with concrete examples.

#### Example 1: Config Structure (config.rs:980)

```rust
pub struct Config {
    discovered_projects_from_filesystem: Vec<ProjectManifest>,
    workspace_roots: Vec<AbsPathBuf>,
    ratoml_file: FxHashMap<SourceRootId, (RatomlFile, ConfigErrors)>,
    source_root_parent_map: Arc<FxHashMap<SourceRootId, SourceRootId>>,
    // 20+ more fields, ZERO lifetime parameters
}

// Config "merging" - just clone everything
fn apply_change_with_sink(&self, change: ConfigChange) -> (Config, bool) {
    let mut config = self.clone();  // <-- Clone entire config!
    // ...
    (config, should_update)
}
```

**Key finding**: Zero lifetime parameters, clone-based updates.

#### Example 2: Syntax Tree Structure (rowan/cursor.rs:112-165)

```rust
struct NodeData {
    rc: Cell<u32>,  // <-- Manual reference counting
    parent: Cell<Option<ptr::NonNull<NodeData>>>,
    green: Green,
    // ...
}

pub struct SyntaxNode {
    ptr: ptr::NonNull<NodeData>,  // Raw pointer, NO lifetime
}

impl Clone for SyntaxNode {
    fn clone(&self) -> Self {
        self.data().inc_rc();  // Just increment counter!
        SyntaxNode { ptr: self.ptr }
    }
}
```

**Key finding**: Manual refcounting (like C++ `shared_ptr`), no lifetimes.

#### Example 3: Public API (rowan/api.rs:97-304)

```rust
impl<L: Language> SyntaxNode<L> {
    // No <'a> parameter!
    pub fn parent(&self) -> Option<SyntaxNode<L>> {  // Returns OWNED
        self.raw.parent().map(Self::from)
    }

    pub fn children(&self) -> SyntaxNodeChildren<L> {  // No <'a>
        SyntaxNodeChildren { raw: self.raw.children(), _p: PhantomData }
    }

    pub fn ancestors(&self) -> impl Iterator<Item = SyntaxNode<L>> {
        // Returns owned nodes, no lifetime parameters
    }
}
```

**Key finding**: Every public method returns owned types, zero lifetimes in public API.

## Documents Created

### 1. yaml-with-source-info-design.md (700+ lines)
- Initial owned-data design
- Complete API with construction, access, parsing, validation, merging
- Memory overhead analysis (~3x)
- 3-4 week implementation plan

### 2. yaml-with-source-info-lifetime-approach.md (800+ lines)
- Response to user's lifetime proposal
- Shows how to express "shorter of two lifetimes"
- Hybrid ownership design (YamlRef enum)
- Detailed comparison table
- Precedent analysis (rustc, rust-analyzer, salsa, ripgrep)
- Acknowledges lifetime approach is feasible
- Still recommends owned for LSP caching and simplicity

### 3. rust-analyzer-owned-data-patterns.md (500+ lines)
- Concrete code examples with line numbers
- Config struct analysis
- SyntaxNode/NodeData analysis
- Public API patterns
- Why it works (cheap Clone, small overhead, enables features)
- Lessons for Quarto
- Code locations for further exploration

### 4. This session log

## Key Technical Insights

### 1. Lifetime Bounds Work

User's intuition was correct: `where 'long: 'short` expresses "merged object has shorter lifetime"

```rust
fn merge<'short, 'long>(
    base: &YamlWithSourceInfo<'long>,
    override: &YamlWithSourceInfo<'short>
) -> YamlWithSourceInfo<'short>
where 'long: 'short  // <-- This works!
```

### 2. But Requires Hybrid Ownership

Merged containers don't exist in either source, must be owned:
```rust
enum YamlRef<'a> {
    Borrowed(&'a Yaml),  // Original nodes
    Owned(Yaml),         // Merged nodes
}
```

### 3. rust-analyzer Precedent is Strong

- One of the most complex Rust projects
- Handles massive codebases
- Explicitly chose owned data over lifetimes for trees
- Zero lifetimes in Config struct
- Manual refcounting for SyntaxNode
- All public APIs return owned types

### 4. Why rust-analyzer Chose Owned

From the code patterns observed:
1. **Clone is cheap**: Just refcount increment
2. **Memory overhead acceptable**: 4 bytes per node
3. **Enables caching**: Can store nodes in collections
4. **Simplifies API**: No lifetime propagation
5. **LSP-friendly**: Can serialize owned data

## Open Questions Left for User

1. **Is LSP caching important enough** to warrant owned data?
   - Lifetime approach makes caching harder (can't serialize refs)
   - Would need separate owned type or skip caching

2. **How important is API simplicity?**
   - Lifetime approach: every function needs `<'a>`
   - Owned approach: no lifetime parameters

3. **Is memory efficiency a concern?**
   - Lifetime: 1x single file, ~1.2x merged
   - Owned: 3x always
   - For Quarto configs (<10KB), overhead is ~20-30KB

4. **Comfort with lifetime complexity?**
   - Lifetime approach is more "Rusty" but more complex
   - Owned approach follows rust-analyzer precedent

## Recommendations Made

### Primary Recommendation: Owned Data

**Reasons**:
1. LSP caching is important (rust-analyzer does this)
2. Follows proven rust-analyzer pattern
3. Simpler API for contributors
4. Memory cost acceptable (KB not MB)

### Alternative: Lifetime Approach

**When to consider**:
1. Memory efficiency is critical
2. LSP caching not needed or acceptable to skip
3. Comfortable with viral lifetime parameters
4. Want the "most Rusty" solution

### Suggested Path

**Option A**: Start owned, measure, optimize if needed
**Option B**: Prototype both, benchmark, choose based on data
**Option C**: Go lifetime from start (more efficient, more complex)

## What User Will Do Next

User will:
1. Read `rust-analyzer-owned-data-patterns.md`
2. Review concrete code examples
3. Decide on approach based on evidence
4. May prototype both to compare

## Code Locations Referenced

### rust-analyzer
- Config struct: `crates/rust-analyzer/src/config.rs:980-1069`
- Config merging: `apply_change_with_sink` method (line 1067)
- Arc usage: Search for `Arc<` in config.rs

### rowan
- SyntaxNode: `src/cursor.rs:146-974`
- Reference counting: Lines 150-165 (Clone/Drop)
- NodeData: Lines 112-130
- Public API: `src/api.rs:15-304`

### Commands for exploration
```bash
cd tmp/rust-analyzer
grep -A 20 "fn apply_change_with_sink" crates/rust-analyzer/src/config.rs

cd tmp/rowan
grep -A 10 "impl Clone for SyntaxNode" src/cursor.rs
grep -A 20 "struct NodeData" src/cursor.rs
```

## Key Quotes

### User's Challenge
> "I'd like to be able to express that the merged object has the shorter of the two lifetimes. Is this kind of thing typically done in larger Rust projects?"

### My Acknowledgment
> "You're absolutely right, and I should have explored this more carefully! Your proposed design IS viable in Rust."

### User's Request
> "Can you guide me through a bit of the rust-analyzer source code where this owned data approach is used?"

### Final User Message
> "While I read your document, can you summarize this session into the notes so we can start a new one? Thanks, and bye!"

## Session Outcome

**Status**: User reviewing evidence, will decide on approach

**Artifacts Delivered**:
- Complete owned-data design
- Complete lifetime-based design analysis
- Concrete rust-analyzer code examples
- Detailed comparison of trade-offs

**Next Steps**: User will decide on approach and we can proceed with implementation

## Technical Correctness

### What I Got Right
- Owned data is simpler and proven
- rust-analyzer precedent is strong
- LSP caching consideration is important

### What I Initially Missed
- Lifetime approach IS feasible with context hierarchies
- Can express "shorter of two lifetimes" with bounds
- Should have explored both options more thoroughly initially

### Correction Process
- User challenged my recommendation
- I acknowledged and explored lifetime approach properly
- Provided balanced analysis of both approaches
- Let user decide based on complete information

## Documentation Updates

- Added `yaml-with-source-info-design.md` to index
- Added `yaml-with-source-info-lifetime-approach.md` to index
- Added `rust-analyzer-owned-data-patterns.md` to index
- Updated Technical Decisions section in index
- Created this session log

## Conclusion

This was a productive technical discussion where the user's challenge led to a more thorough analysis. Both approaches are viable:

**Lifetime approach**: More memory-efficient, more complex
**Owned approach**: Simpler, proven at scale, better for LSP

The concrete rust-analyzer examples provide strong evidence that owned data is a battle-tested approach for similar use cases. User will make final decision based on project priorities.
