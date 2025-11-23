# ErrorCollector Trait Redesign Discussion

## Context

We're migrating from custom ErrorCollector trait to use quarto-error-reporting. Before implementing Phase B (the bridge), we should consider whether the current ErrorCollector trait is the right abstraction for a world where all errors are DiagnosticMessage objects.

## Current ErrorCollector API

```rust
pub trait ErrorCollector {
    fn warn(&mut self, message: String, location: Option<&SourceInfo>);
    fn error(&mut self, message: String, location: Option<&SourceInfo>);
    fn has_errors(&self) -> bool;
    fn messages(&self) -> Vec<String>;
    fn into_messages(self) -> Vec<String>;
}
```

### Current Usage Pattern

```rust
error_collector_ref.borrow_mut().error(
    format!("Found attr in postprocess: {:?} - this should have been removed", attr),
    None
);

error_collector_ref.borrow_mut().warn(
    "Caption found without a preceding table".to_string(),
    Some(&SourceInfo::new(row + 1, col + 1))
);
```

## Issues with Current Design

### 1. String-Based API is Limiting

The trait forces conversion to String at call site:
- Loses structured information (can't add hints, details, problem statements)
- Requires `format!()` + `.to_string()` at every call
- Can't incrementally build complex error messages

### 2. Location Handling is Awkward

```rust
Some(&SourceInfo::new(row + 1, col + 1))
```
- Creates temporary just to take reference
- Simple row/column model (will need richer source tracking later)

### 3. Output Format Baked In

```rust
fn into_messages(self) -> Vec<String>
```
- Forces string output
- Can't defer formatting decision
- Can't access structured data after collection

## Design Options

### Option A: Keep Current Trait, Bridge Implementation

**Approach**: Implement ErrorCollector for DiagnosticCollector, converting strings to DiagnosticMessage internally.

```rust
impl ErrorCollector for DiagnosticCollector {
    fn error(&mut self, message: String, location: Option<&SourceInfo>) {
        // Convert string to DiagnosticMessage using generic_error!
        let diag = generic_error!(message);  // Uses file!() line!() from here
        self.messages.push(diag);
    }
}
```

**Pros**:
- ✅ No changes to calling code
- ✅ Drop-in replacement
- ✅ Minimal risk

**Cons**:
- ❌ Loses file/line info (generic_error! tracks bridge code, not call site)
- ❌ Still string-based API
- ❌ Can't leverage DiagnosticMessage features
- ❌ Temporary solution that we'll want to replace anyway

### Option B: Add DiagnosticMessage Methods to Trait

**Approach**: Extend ErrorCollector with methods that accept DiagnosticMessage directly.

```rust
pub trait ErrorCollector {
    // Old API (deprecated but kept for migration)
    fn warn(&mut self, message: String, location: Option<&SourceInfo>);
    fn error(&mut self, message: String, location: Option<&SourceInfo>);

    // New API
    fn add_diagnostic(&mut self, diagnostic: DiagnosticMessage);
    fn add_warning_diagnostic(&mut self, diagnostic: DiagnosticMessage);

    fn has_errors(&self) -> bool;
    fn diagnostics(&self) -> &[DiagnosticMessage];
    fn into_diagnostics(self) -> Vec<DiagnosticMessage>;
}
```

**Usage**:
```rust
// Old style (for migration)
error_collector.error("Simple message".to_string(), None);

// New style
error_collector.add_diagnostic(
    generic_error!("Found unexpected attr")
);

// Future style (when we enhance)
error_collector.add_diagnostic(
    DiagnosticMessageBuilder::error("Unexpected attribute")
        .with_code("Q-2-15")
        .problem("Attributes should have been removed")
        .build()
);
```

**Pros**:
- ✅ Incremental migration path
- ✅ Preserves file/line info with macros
- ✅ Can use full DiagnosticMessage power
- ✅ Old code keeps working

**Cons**:
- ⚠️ Larger trait surface
- ⚠️ Two ways to do things (temporarily)
- ⚠️ Need to update all implementations

### Option C: Replace Trait Entirely

**Approach**: Create new DiagnosticCollector with simpler, more direct API.

```rust
pub struct DiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
    output_format: OutputFormat,
}

impl DiagnosticCollector {
    pub fn new(format: OutputFormat) -> Self { ... }

    pub fn add(&mut self, diagnostic: DiagnosticMessage) { ... }

    pub fn has_errors(&self) -> bool { ... }

    pub fn render(&self) -> Vec<String> {
        self.diagnostics.iter()
            .map(|d| match self.output_format {
                OutputFormat::Text => d.to_text(),
                OutputFormat::Json => d.to_json().to_string(),
            })
            .collect()
    }

    pub fn into_diagnostics(self) -> Vec<DiagnosticMessage> {
        self.diagnostics
    }
}
```

**Migration helpers**:
```rust
impl DiagnosticCollector {
    // Temporary helpers for migration
    pub fn error(&mut self, message: impl Into<String>) {
        self.add(generic_error!(message.into()));
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.add(generic_warning!(message.into()));
    }
}
```

**Pros**:
- ✅ Clean, purpose-built API
- ✅ No trait complexity
- ✅ Direct DiagnosticMessage storage
- ✅ Simple migration helpers

**Cons**:
- ❌ Breaking change (not compatible with ErrorCollector trait)
- ❌ Need to update all call sites that type-constrain to ErrorCollector
- ❌ More upfront work

### Option D: Hybrid - New Collector, Old Trait Adapter

**Approach**: Create DiagnosticCollector (like Option C), but also implement ErrorCollector for backward compatibility.

```rust
pub struct DiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
    output_format: OutputFormat,
}

impl DiagnosticCollector {
    // Primary API
    pub fn add(&mut self, diagnostic: DiagnosticMessage) { ... }

    // Migration helpers
    pub fn error(&mut self, message: impl Into<String>) {
        self.add(generic_error!(message.into()));
    }
    pub fn warn(&mut self, message: impl Into<String>) {
        self.add(generic_warning!(message.into()));
    }
}

// Backward compatibility
impl ErrorCollector for DiagnosticCollector {
    fn error(&mut self, message: String, location: Option<&SourceInfo>) {
        self.error(message);  // Delegates to migration helper
    }
    fn warn(&mut self, message: String, location: Option<&SourceInfo>) {
        self.warn(message);
    }
    // ... other methods
}
```

**Pros**:
- ✅ New, clean API for DiagnosticMessage
- ✅ Backward compatible via trait impl
- ✅ Gradual migration path
- ✅ Can use either API during transition

**Cons**:
- ⚠️ Location parameter ignored in trait impl (need to handle)
- ⚠️ Slightly more complex implementation

## Recommendations

### Recommended: Option D (Hybrid)

I recommend **Option D** for these reasons:

1. **Best migration path**: Code using `&mut dyn ErrorCollector` keeps working
2. **Clean primary API**: New code can use `.add(diagnostic)` directly
3. **Preserves file/line**: Migration helpers use macros, not trait methods
4. **Future-proof**: Easy to deprecate ErrorCollector trait later

### Implementation Strategy

**Phase B (Revised)**:

1. Create `DiagnosticCollector` struct with:
   - Primary API: `add(DiagnosticMessage)`
   - Migration helpers: `error(message)`, `warn(message)` (methods, not trait)
   - `render()` → `Vec<String>` for output

2. Implement `ErrorCollector` trait for backward compat:
   - Delegate to migration helpers
   - Handle location parameter (for now, ignore - we'll add it properly when we have source-map integration)

3. Add deprecation markers to `ErrorCollector` trait methods

**Phase C (Revised)**:

1. Replace `TextErrorCollector`/`JsonErrorCollector` with `DiagnosticCollector`
2. Keep using trait API initially (no call site changes needed)

**Future**:

1. Gradually migrate call sites to use `.add()` with `generic_error!()` macro
2. Eventually migrate to full DiagnosticMessageBuilder API
3. Remove ErrorCollector trait when no longer needed

## Open Questions

1. **Location handling**: Should we add location to DiagnosticMessage now or wait for source-map integration?
   - **Proposal**: Wait. For now, file/line from macro is good enough.

2. **Migration timeline**: How long to keep ErrorCollector trait?
   - **Proposal**: Keep until all call sites use `.add()` directly (could be months)

3. **Generic error codes**: Should every error during migration have Q-0-99? <!-- quarto-error-code-audit-ignore -->
   - **Proposal**: Yes, makes them easy to find and upgrade later

4. **Macro location**: Should macros be in quarto-error-reporting or quarto-markdown-pandoc?
   - **Proposal**: In quarto-error-reporting (they're part of the migration story)

## Next Steps

1. Decide on design option
2. Update Phase B plan if we choose Option D
3. Implement and test
4. Document migration path for future error enhancements
