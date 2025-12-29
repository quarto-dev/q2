# Subplan 05: quarto-core Modules

**Order:** 5th
**Complexity:** MEDIUM
**Dependencies:** 01-core-types through 04-pampa-internal

## Files

| File | Usage | Changes Required |
|------|-------|------------------|
| `quarto-core/src/template.rs` | HEAVY | 25+ match arms for template rendering |
| `quarto-core/src/transforms/metadata_normalize.rs` | MODERATE | Metadata normalization |
| `quarto-core/src/transforms/title_block.rs` | LIGHT | Title extraction |
| `quarto-core/src/transforms/callout.rs` | LIGHT | Callout metadata |
| `quarto-core/src/transforms/callout_resolve.rs` | LIGHT | Callout resolution |
| `quarto-core/src/transforms/resource_collector.rs` | LIGHT | Resource metadata |
| `quarto-core/src/pipeline.rs` | LIGHT | Pipeline metadata access |

## Detailed Changes

### 1. `template.rs` - Template Rendering (HEAVY)

**Key functions:**
```rust
fn meta_value_to_template_value(meta: &MetaValueWithSourceInfo) -> TemplateValue
fn add_metadata_to_context(meta: &MetaValueWithSourceInfo, ctx: &mut Context)
```

**Target:**
```rust
fn config_to_template_value(config: &ConfigValue) -> TemplateValue
fn add_metadata_to_context(config: &ConfigValue, ctx: &mut Context)
```

**Pattern changes (25+ locations):**
```rust
// OLD
match meta {
    MetaValueWithSourceInfo::MetaString { value, .. } => TemplateValue::String(value.clone()),
    MetaValueWithSourceInfo::MetaBool { value, .. } => TemplateValue::Bool(*value),
    MetaValueWithSourceInfo::MetaInlines { content, .. } => render_inlines(content),
    MetaValueWithSourceInfo::MetaMap { entries, .. } => ...
    MetaValueWithSourceInfo::MetaList { items, .. } => ...
    MetaValueWithSourceInfo::MetaBlocks { content, .. } => ...
}

// NEW
match &config.value {
    ConfigValueKind::Scalar(yaml) => match yaml {
        Yaml::String(s) => TemplateValue::String(s.clone()),
        Yaml::Boolean(b) => TemplateValue::Bool(*b),
        Yaml::Integer(i) => TemplateValue::Integer(*i),
        Yaml::Real(r) => TemplateValue::Float(r.parse().unwrap_or(0.0)),
        Yaml::Null => TemplateValue::Null,
        _ => TemplateValue::Null,
    },
    ConfigValueKind::PandocInlines(content) => render_inlines(content),
    ConfigValueKind::PandocBlocks(content) => render_blocks(content),
    ConfigValueKind::Map(entries) => ...
    ConfigValueKind::Array(items) => ...
    ConfigValueKind::Path(s) => TemplateValue::String(s.clone()),
    ConfigValueKind::Glob(s) => TemplateValue::String(s.clone()),
    ConfigValueKind::Expr(s) => TemplateValue::String(s.clone()),
}
```

### 2. `transforms/metadata_normalize.rs` - Normalization (MODERATE)

**Functions:**
- `normalize_metadata()` - Normalize metadata fields
- `add_pagetitle_if_missing()` - Extract title for pagetitle
- `extract_plain_text()` - Get text from metadata value

**Pattern changes:**
```rust
// OLD
fn extract_plain_text(meta: &MetaValueWithSourceInfo) -> Option<String> {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => Some(value.clone()),
        MetaValueWithSourceInfo::MetaInlines { content, .. } => ...
        _ => None,
    }
}

// NEW
fn extract_plain_text(config: &ConfigValue) -> Option<String> {
    match &config.value {
        ConfigValueKind::Scalar(Yaml::String(s)) => Some(s.clone()),
        ConfigValueKind::PandocInlines(content) => ...
        _ => None,
    }
}
```

### 3. `transforms/title_block.rs` - Title Handling (LIGHT)

Update metadata access for title block generation:
```rust
// Access title from metadata
if let Some(title) = config.get("title") {
    // Process title
}
```

### 4-6. Callout and Resource Transforms (LIGHT)

Similar pattern updates for:
- `callout.rs` - Callout div metadata
- `callout_resolve.rs` - Resolving callout types
- `resource_collector.rs` - Collecting resource references

### 7. `pipeline.rs` - Pipeline Access (LIGHT)

Update any direct `pandoc.meta` access patterns.

## Migration Steps

```bash
# 1. Start with template.rs (most complex)
# Update all 25+ match arms

# 2. Test template rendering
cargo nextest run -p quarto-core template

# 3. Update metadata_normalize.rs
# 4. Update title_block.rs
# 5. Update callout transforms
# 6. Update resource_collector.rs
# 7. Update pipeline.rs

# After all changes:
cargo nextest run -p quarto-core
```

## Completion Criteria

- [ ] All 7 files compile with ConfigValue
- [ ] Template rendering tests pass
- [ ] Metadata normalization tests pass
- [ ] Transform tests pass
- [ ] All quarto-core tests pass

## Notes

- Template rendering is the most code-intensive change
- Consider creating a helper function for common ConfigValue → TemplateValue conversions
- The transforms are straightforward pattern updates
