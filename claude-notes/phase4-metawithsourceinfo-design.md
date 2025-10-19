# Phase 4: MetaWithSourceInfo Design

## Core Insight

**Pandoc's Meta is NOT YAML** - it's an interpreted structure:
- YAML strings are parsed as Markdown → `MetaInlines`
- Special YAML tags are recognized and handled
- It represents semantic content, not raw data

Therefore, we should have source tracking for the **interpreted** structure, not store raw YAML alongside interpreted Meta.

## Architecture

```
┌─────────────┐
│  .qmd file  │
└──────┬──────┘
       │ parse
       ▼
┌─────────────────────┐
│ YAML frontmatter    │  (bytes 10-150 in file)
│ ---                 │
│ title: "My **Doc**" │
│ nested:             │
│   value: foo        │
│ ---                 │
└──────┬──────────────┘
       │ quarto_yaml::parse_with_parent()
       ▼
┌──────────────────────────┐
│ YamlWithSourceInfo       │  Raw YAML with source tracking
│   Hash {                 │
│     "title": String(...) │  Each node knows its offset
│     "nested": Hash(...)  │
│   }                      │
└──────┬───────────────────┘
       │ yaml_to_meta_with_source_info()
       ▼
┌─────────────────────────────────┐
│ MetaValueWithSourceInfo         │  Interpreted Meta with source tracking
│   MetaMap {                     │
│     ("title", source, MetaInlines([  │
│       Strong { source, ... }    │  ← Parsed Markdown has source info!
│     ])),                        │
│     ("nested", source, MetaMap([│
│       ("value", source, MetaString("foo", source))
│     ]))                         │
│   }                             │
└─────────────────────────────────┘
```

## Data Structures

### MetaValueWithSourceInfo

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetaValueWithSourceInfo {
    MetaString {
        value: String,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaBool {
        value: bool,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaInlines {
        content: Inlines,  // Each Inline already has source_info
        source_info: quarto_source_map::SourceInfo,  // Source of whole value
    },
    MetaBlocks {
        content: Blocks,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaList {
        items: Vec<MetaValueWithSourceInfo>,
        source_info: quarto_source_map::SourceInfo,
    },
    MetaMap {
        // Store as Vec to preserve order (Pandoc behavior)
        entries: Vec<MetaMapEntry>,
        source_info: quarto_source_map::SourceInfo,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetaMapEntry {
    pub key: String,
    pub key_source: quarto_source_map::SourceInfo,
    pub value: MetaValueWithSourceInfo,
}
```

### Updated Pandoc Structure

```rust
pub struct Pandoc {
    pub meta: MetaValueWithSourceInfo,  // Changed from Meta
    pub blocks: Blocks,
    pub source_info: SourceInfo,
}
```

## Transformation Logic

### yaml_to_meta_with_source_info()

```rust
fn yaml_to_meta_with_source_info(
    yaml: YamlWithSourceInfo,
    context: &ASTContext,
) -> Result<MetaValueWithSourceInfo> {
    match yaml.yaml {
        Yaml::String(s) => {
            // Check for special YAML tags first
            if let Some(tag) = &yaml.tag {
                match tag.as_str() {
                    "!yaml-tagged-string" => {
                        // Return as MetaString, not parsed
                        return Ok(MetaValueWithSourceInfo::MetaString {
                            value: s.clone(),
                            source_info: yaml.source_info,
                        });
                    }
                    _ => {
                        // Unknown tag - treat as string
                    }
                }
            }

            // Parse as Markdown, creating Substring SourceInfos
            let inlines = parse_markdown_string_with_parent(
                &s,
                yaml.source_info.clone(),
                context
            )?;

            Ok(MetaValueWithSourceInfo::MetaInlines {
                content: inlines,
                source_info: yaml.source_info,
            })
        }

        Yaml::Boolean(b) => {
            Ok(MetaValueWithSourceInfo::MetaBool {
                value: b,
                source_info: yaml.source_info,
            })
        }

        Yaml::Array(items) => {
            let meta_items: Result<Vec<_>> = items
                .into_iter()
                .map(|item| yaml_to_meta_with_source_info(item, context))
                .collect();

            Ok(MetaValueWithSourceInfo::MetaList {
                items: meta_items?,
                source_info: yaml.source_info,
            })
        }

        Yaml::Hash(entries) => {
            let meta_entries: Result<Vec<_>> = entries
                .into_iter()
                .map(|entry| {
                    let key = entry.key.yaml.as_str()
                        .ok_or_else(|| Error::InvalidKey)?
                        .to_string();
                    let key_source = entry.key_span.source_info.clone();
                    let value = yaml_to_meta_with_source_info(entry.value, context)?;

                    Ok(MetaMapEntry { key, key_source, value })
                })
                .collect();

            Ok(MetaValueWithSourceInfo::MetaMap {
                entries: meta_entries?,
                source_info: yaml.source_info,
            })
        }

        // Pandoc doesn't support null or numbers in metadata
        Yaml::Null | Yaml::Real(_) | Yaml::Integer(_) => {
            Err(Error::UnsupportedYamlType)
        }
    }
}
```

### parse_markdown_string_with_parent()

```rust
fn parse_markdown_string_with_parent(
    markdown: &str,
    parent_source: quarto_source_map::SourceInfo,
    context: &ASTContext,
) -> Result<Inlines> {
    // Create a Substring SourceInfo for the markdown content
    let markdown_source = quarto_source_map::SourceInfo::substring(
        parent_source,
        0,
        markdown.len(),
    );

    // Parse as inline markdown
    // Each inline element will have SourceInfo relative to markdown_source
    // which maps back through the chain to the original .qmd file
    parse_inline_markdown(markdown, markdown_source, context)
}
```

## Source Info Chain Example

For this YAML in a .qmd file:

```yaml
---
title: "My **bold** title"
---
```

**SourceInfo chain**:
```
"bold" Strong inline:
  SourceInfo::Substring {
    parent: markdown_string ("My **bold** title"),
    offset: 6,
    length: 4
  }

markdown_string:
  SourceInfo::Substring {
    parent: yaml_value ("My **bold** title"),
    offset: 0,
    length: 19
  }

yaml_value:
  SourceInfo::Substring {
    parent: yaml_frontmatter (entire --- block),
    offset: 8,
    length: 19
  }

yaml_frontmatter:
  SourceInfo::Substring {
    parent: qmd_file,
    offset: 4,
    length: 50
  }

qmd_file:
  SourceInfo::Original {
    file_id: FileId(0),
    range: 0..200
  }
```

**Resolving "bold" back to file**: Walk the chain, adding offsets: 4 + 8 + 0 + 6 = 18

## Integration Points

### 1. rawblock_to_meta()

```rust
pub fn rawblock_to_meta(
    block: RawBlock,
    context: &ASTContext,
) -> Result<MetaValueWithSourceInfo> {
    // Extract YAML content
    let content = extract_between_delimiters(&block.text)?;

    // Calculate offsets within RawBlock
    let yaml_start = block.text.find("---\n").unwrap() + 4;
    let yaml_end = yaml_start + content.len();

    // Create parent SourceInfo from RawBlock
    let parent = block.source_info;  // Already a SourceInfo
    let yaml_parent = quarto_source_map::SourceInfo::substring(
        parent,
        yaml_start,
        yaml_end,
    );

    // Parse with quarto-yaml
    let yaml = quarto_yaml::parse_with_parent(content, yaml_parent)?;

    // Transform to Meta
    yaml_to_meta_with_source_info(yaml, context)
}
```

### 2. Pandoc Construction

```rust
pub struct Pandoc {
    pub meta: MetaValueWithSourceInfo,  // Was: LinkedHashMap<String, MetaValue>
    pub blocks: Blocks,
    pub source_info: SourceInfo,
}
```

### 3. JSON Serialization

```rust
// MetaValueWithSourceInfo already derives Serialize/Deserialize
// JSON will include all SourceInfo chains
let json = serde_json::to_string_pretty(&pandoc)?;

// This is where we'll observe the duplication:
// Each nested meta value serializes its full SourceInfo parent chain
```

## Migration Strategy

### Compatibility

For now, provide conversion helpers:

```rust
impl MetaValueWithSourceInfo {
    /// Convert to old Meta format (loses source info)
    pub fn to_meta_value(&self) -> MetaValue {
        match self {
            MetaValueWithSourceInfo::MetaString { value, .. } => {
                MetaValue::MetaString(value.clone())
            }
            // ... other variants
        }
    }
}

/// Convert old Meta to new format (with dummy source info)
pub fn meta_from_legacy(meta: Meta) -> MetaValueWithSourceInfo {
    let entries = meta.into_iter()
        .map(|(k, v)| MetaMapEntry {
            key: k,
            key_source: quarto_source_map::SourceInfo::default(),
            value: meta_value_from_legacy(v),
        })
        .collect();

    MetaValueWithSourceInfo::MetaMap {
        entries,
        source_info: quarto_source_map::SourceInfo::default(),
    }
}
```

### Gradual Rollout

1. **Add new types** alongside old ones
2. **Update rawblock_to_meta** to use new types
3. **Update Pandoc struct** (breaking change to AST)
4. **Update all construction sites** (readers, tests)
5. **Update traversal code** (filters, writers)
6. **Remove old types** once migration complete

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_yaml_to_meta_preserves_source() {
    let yaml_content = "title: \"Test\"";
    let parent = SourceInfo::original(FileId(0), range_0_100);
    let yaml = parse_with_parent(yaml_content, parent)?;

    let meta = yaml_to_meta_with_source_info(yaml)?;

    // Verify source tracking works
    match meta {
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let title_entry = &entries[0];
            assert_eq!(title_entry.key, "title");
            // Verify key_source points to correct location
            // Verify value has source info
        }
    }
}
```

### Integration Test (The Big One!)

```rust
#[test]
fn test_deeply_nested_yaml_serialization() {
    let qmd = r#"---
title: "Test"
deeply:
  nested:
    yaml:
      structure:
        level5:
          key: "value"
---
# Content
"#;

    // Parse
    let pandoc = parse_qmd(qmd)?;

    // Serialize
    let json = serde_json::to_string_pretty(&pandoc)?;
    println!("JSON size: {} bytes", json.len());

    // Count parent chain duplications
    let yaml_count = json.matches("\"yaml\"").count();
    println!("'yaml' appears {} times in JSON", yaml_count);

    // Deserialize
    let pandoc2: Pandoc = serde_json::from_str(&json)?;

    // Verify source info still works
    // Walk to deeply.nested.yaml.structure.level5.key
    // Resolve SourceInfo back to original offset in qmd
    // Verify it points to the right location
}
```

## Open Questions

1. **Markdown parsing**: Do we have a function to parse just inline markdown? Or do we need to create one?

2. **Error handling**: How should we handle YAML types that Pandoc doesn't support (null, numbers)?

3. **Backward compatibility**: Do we need to support reading old JSON format without source info?

4. **Performance**: Is the Rc optimization (k-43) sufficient, or will we need more optimization after observing the serialization?

## Success Criteria

- ✅ Parse deeply nested YAML with full source tracking
- ✅ MetaInlines have SourceInfo that resolves back to .qmd file
- ✅ Can serialize/deserialize PandocAST with MetaWithSourceInfo
- ✅ Source info remains functional after roundtrip
- ✅ Can observe and measure serialization duplication
- ✅ All existing tests pass (or are updated)
