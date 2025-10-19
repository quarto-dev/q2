# YAML Metadata Serialization: Current State and Plan

## Key Finding: quarto-yaml NOT YET INTEGRATED

**Current state**: `rawblock_to_meta()` uses **yaml-rust2 directly**, NOT the new quarto-yaml crate.

```rust
// Current implementation in meta.rs:160
pub fn rawblock_to_meta(block: RawBlock) -> Meta {
    let content = extract_between_delimiters(&block.text).unwrap();
    let mut parser = Parser::new_from_str(content);  // <-- yaml-rust2!
    let mut handler = YamlEventHandler::new();
    parser.load(&mut handler, false);
    handler.result.unwrap()  // Returns Meta (not YamlWithSourceInfo!)
}
```

## Current Data Structures

```rust
// MetaBlock (the quarto-markdown extension)
pub struct MetaBlock {
    pub meta: Meta,                    // LinkedHashMap<String, MetaValue>
    pub source_info: SourceInfo,       // Old pandoc::location type
}

// MetaValue (Pandoc standard)
pub enum MetaValue {
    MetaString(String),
    MetaBool(bool),
    MetaInlines(Inlines),
    MetaBlocks(Blocks),
    MetaList(Vec<MetaValue>),
    MetaMap(LinkedHashMap<String, MetaValue>),
    // NO YamlWithSourceInfo!
}
```

**Problem**: No source tracking for YAML metadata keys/values!

## User's Request: Test Serialization Blowup

The user wants to:
1. Parse a .qmd with deeply nested YAML metadata
2. Get PandocAST with MetadataBlock containing **YamlWithSourceInfo**
3. Serialize to JSON
4. Deserialize back
5. Verify SourceInfo still works
6. Measure the JSON size (observe the duplication problem)

**But we can't do this yet** because YamlWithSourceInfo isn't integrated!

## What Needs to Happen (Phase 4 of Migration Plan)

### Step 1: Decide on MetaValue Design

**Option A: Add YamlWithSourceInfo variant**
```rust
pub enum MetaValue {
    MetaString(String),
    MetaBool(bool),
    MetaInlines(Inlines),
    MetaBlocks(Blocks),
    MetaList(Vec<MetaValue>),
    MetaMap(LinkedHashMap<String, MetaValue>),
    MetaYaml(YamlWithSourceInfo),  // NEW - preserve full YAML with source info
}
```

**Option B: Replace Meta entirely**
```rust
pub struct MetaBlock {
    pub yaml: YamlWithSourceInfo,  // Store the raw YAML with source info
    pub source_info: SourceInfo,
}
```

**Option C: Parallel structure**
```rust
pub struct MetaBlock {
    pub meta: Meta,                        // Keep for backward compat
    pub yaml: Option<YamlWithSourceInfo>,  // Add source-tracked YAML
    pub source_info: SourceInfo,
}
```

### Step 2: Update rawblock_to_meta

```rust
use quarto_yaml;

pub fn rawblock_to_meta(block: RawBlock) -> (Meta, YamlWithSourceInfo) {
    let content = extract_between_delimiters(&block.text).unwrap();

    // Get offsets for substring mapping
    let yaml_start = /* offset of content within block.text */;
    let yaml_end = yaml_start + content.len();

    // Create parent SourceInfo for the RawBlock
    let parent = SourceInfo::original(FileId(?), block.source_info.range);
    let yaml_parent = SourceInfo::substring(parent, yaml_start, yaml_end);

    // Parse with quarto-yaml
    let yaml = quarto_yaml::parse_with_parent(content, yaml_parent)?;

    // Convert YamlWithSourceInfo -> MetaValue (for compatibility)
    let meta = yaml_to_meta_value(&yaml);

    (meta, yaml)
}
```

### Step 3: Add Serialization

```rust
impl Serialize for MetaValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // ... existing variants
            MetaValue::MetaYaml(yaml) => {
                // Serialize YamlWithSourceInfo
                yaml.serialize(serializer)
            }
        }
    }
}
```

YamlWithSourceInfo already has Serialize/Deserialize!

### Step 4: Test Roundtrip

```rust
#[test]
fn test_deeply_nested_yaml_roundtrip() {
    let qmd = r#"---
title: "Test"
deeply:
  nested:
    yaml:
      structure:
        with:
          many:
            levels:
              foo: bar
              baz: qux
---
# Content
"#;

    // Parse
    let ast = read_qmd(qmd);
    let meta_block = /* extract MetaBlock */;

    // Serialize
    let json = serde_json::to_string_pretty(&ast)?;
    println!("JSON size: {} bytes", json.len());

    // Analyze duplication
    // Count how many times "deeply.nested.yaml..." appears

    // Deserialize
    let ast2: PandocAST = serde_json::from_str(&json)?;

    // Verify SourceInfo works
    let yaml = &ast2.meta_block.yaml;
    // Walk the structure, verify offsets map back correctly
}
```

## Immediate Next Steps

1. **Check Phase 4 tasks in migration plan** - are they in beads?
2. **Create tasks if needed**:
   - Integrate quarto-yaml into rawblock_to_meta
   - Design MetaValue/MetaBlock changes
   - Add YamlWithSourceInfo serialization test
   - Create deeply nested YAML roundtrip test
3. **Implement integration** (this is the blocker for serialization testing)
4. **Then test serialization blowup** (the user's actual request)

## Timeline

This is **Phase 4** work according to the migration plan:
- Phase 1: ✅ Done (quarto-yaml with substring support)
- Phase 2: ✅ Done (quarto-markdown-pandoc infrastructure)
- Phase 3: Not started (systematic migration of all types)
- **Phase 4: Integrate YAML with SourceMapping** ← We need this!
- Phase 5: Testing
- Phase 6: Cleanup

We can't test the serialization until Phase 4 is done.

## Recommendation

Before implementing Phase 4, we should:
1. Discuss the design choice (MetaValue variant vs separate field)
2. Understand the compatibility requirements (Pandoc compatibility?)
3. Plan the migration strategy

**The user is right** - we need this to observe the serialization problem. But we need to do Phase 4 first.
