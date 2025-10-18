//! YAML parser that builds YamlWithSourceInfo trees.

use crate::{Error, Result, SourceInfo, YamlHashEntry, YamlWithSourceInfo};
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;
use yaml_rust2::Yaml;

/// Parse YAML from a string, producing a YamlWithSourceInfo tree.
///
/// This parses a single YAML document. If the input contains multiple documents,
/// only the first one will be parsed.
///
/// # Example
///
/// ```rust
/// use quarto_yaml::parse;
///
/// let yaml = parse("title: My Document").unwrap();
/// assert!(yaml.is_hash());
/// ```
///
/// # Errors
///
/// Returns an error if the YAML is invalid or if parsing fails.
pub fn parse(content: &str) -> Result<YamlWithSourceInfo> {
    parse_impl(content, None)
}

/// Parse YAML from a string with an associated filename.
///
/// The filename is included in source location information for better
/// error reporting.
///
/// # Example
///
/// ```rust
/// use quarto_yaml::parse_file;
///
/// let yaml = parse_file("title: My Document", "config.yaml").unwrap();
/// assert_eq!(yaml.source_info.file, Some("config.yaml".into()));
/// ```
///
/// # Errors
///
/// Returns an error if the YAML is invalid or if parsing fails.
pub fn parse_file(content: &str, filename: &str) -> Result<YamlWithSourceInfo> {
    parse_impl(content, Some(filename))
}

fn parse_impl(content: &str, filename: Option<&str>) -> Result<YamlWithSourceInfo> {
    let mut parser = Parser::new_from_str(content);
    let mut builder = YamlBuilder::new(content, filename);

    parser
        .load(&mut builder, false)  // false = single document only
        .map_err(Error::from)?;

    builder.result()
}

/// Builder that implements MarkedEventReceiver to construct YamlWithSourceInfo.
struct YamlBuilder<'a> {
    /// The source text being parsed (reserved for future use in accurate scalar length computation)
    _source: &'a str,

    /// Optional filename for source info
    filename: Option<String>,

    /// Stack of nodes being constructed
    stack: Vec<BuildNode>,

    /// The completed root node
    root: Option<YamlWithSourceInfo>,
}

/// A node being constructed during parsing.
enum BuildNode {
    /// Building a sequence
    Sequence {
        start_marker: Marker,
        items: Vec<YamlWithSourceInfo>,
    },

    /// Building a mapping
    Mapping {
        start_marker: Marker,
        entries: Vec<(YamlWithSourceInfo, Option<YamlWithSourceInfo>)>,
    },
}

impl<'a> YamlBuilder<'a> {
    fn new(source: &'a str, filename: Option<&str>) -> Self {
        Self {
            _source: source,
            filename: filename.map(|s| s.to_string()),
            stack: Vec::new(),
            root: None,
        }
    }

    fn result(self) -> Result<YamlWithSourceInfo> {
        self.root
            .ok_or_else(|| Error::ParseError {
                message: "No YAML document found".into(),
                location: None,
            })
    }

    fn push_complete(&mut self, node: YamlWithSourceInfo) {
        if self.stack.is_empty() {
            // This is the root
            self.root = Some(node);
            return;
        }

        // Add to the parent node
        match self.stack.last_mut().unwrap() {
            BuildNode::Sequence { items, .. } => {
                items.push(node);
            }
            BuildNode::Mapping { entries, .. } => {
                if let Some((_, value)) = entries.last_mut() {
                    if value.is_none() {
                        *value = Some(node);
                    } else {
                        // This is a new key
                        entries.push((node, None));
                    }
                } else {
                    // First key
                    entries.push((node, None));
                }
            }
        }
    }

    fn make_source_info(&self, marker: &Marker, len: usize) -> SourceInfo {
        let mut info = SourceInfo::from_marker(marker, len);
        if let Some(ref filename) = self.filename {
            info = info.with_file(filename.clone());
        }
        info
    }

    fn compute_scalar_len(&self, _marker: &Marker, value: &str) -> usize {
        // For now, use the value length
        // TODO: This should be computed more accurately from the source
        // considering quotes, escapes, etc.
        value.len()
    }
}

impl<'a> MarkedEventReceiver for YamlBuilder<'a> {
    fn on_event(&mut self, ev: Event, marker: Marker) {
        match ev {
            Event::Nothing => {}

            Event::StreamStart => {}
            Event::StreamEnd => {}
            Event::DocumentStart => {}
            Event::DocumentEnd => {}

            Event::Scalar(value, _style, _anchor_id, _tag) => {
                let len = self.compute_scalar_len(&marker, &value);
                let source_info = self.make_source_info(&marker, len);

                // Create the Yaml value
                let yaml = parse_scalar_value(&value);
                let node = YamlWithSourceInfo::new_scalar(yaml, source_info);

                self.push_complete(node);
            }

            Event::SequenceStart(_anchor_id, _tag) => {
                self.stack.push(BuildNode::Sequence {
                    start_marker: marker,
                    items: Vec::new(),
                });
            }

            Event::SequenceEnd => {
                let build_node = self.stack.pop().expect("SequenceEnd without SequenceStart");

                if let BuildNode::Sequence { start_marker, items } = build_node {
                    // Compute the length from start to current marker
                    let len = marker.index().saturating_sub(start_marker.index());
                    let source_info = self.make_source_info(&start_marker, len);

                    // Build the Yaml::Array
                    let yaml_items: Vec<Yaml> = items.iter().map(|n| n.yaml.clone()).collect();
                    let yaml = Yaml::Array(yaml_items);

                    let node = YamlWithSourceInfo::new_array(yaml, source_info, items);
                    self.push_complete(node);
                } else {
                    panic!("Expected Sequence build node");
                }
            }

            Event::MappingStart(_anchor_id, _tag) => {
                self.stack.push(BuildNode::Mapping {
                    start_marker: marker,
                    entries: Vec::new(),
                });
            }

            Event::MappingEnd => {
                let build_node = self.stack.pop().expect("MappingEnd without MappingStart");

                if let BuildNode::Mapping { start_marker, entries } = build_node {
                    // Compute the length from start to current marker
                    let len = marker.index().saturating_sub(start_marker.index());
                    let source_info = self.make_source_info(&start_marker, len);

                    // Build the hash entries
                    let mut hash_entries = Vec::new();
                    let mut yaml_pairs = Vec::new();

                    for (key, value) in entries {
                        let value = value.expect("Mapping entry without value");

                        // Create YamlHashEntry
                        let key_span = key.source_info.clone();
                        let value_span = value.source_info.clone();

                        // Entry span from key start to value end
                        let entry_start = key_span.offset;
                        let entry_end = value_span.end_offset();
                        let entry_len = entry_end.saturating_sub(entry_start);
                        let entry_span = SourceInfo::new(
                            self.filename.clone(),
                            entry_start,
                            key_span.line,
                            key_span.col,
                            entry_len,
                        );

                        hash_entries.push(YamlHashEntry::new(
                            key.clone(),
                            value.clone(),
                            key_span,
                            value_span,
                            entry_span,
                        ));

                        yaml_pairs.push((key.yaml.clone(), value.yaml.clone()));
                    }

                    // Build the Yaml::Hash
                    let yaml = Yaml::Hash(yaml_pairs.into_iter().collect());

                    let node = YamlWithSourceInfo::new_hash(yaml, source_info, hash_entries);
                    self.push_complete(node);
                } else {
                    panic!("Expected Mapping build node");
                }
            }

            Event::Alias(_anchor_id) => {
                // For now, we don't support aliases
                // We could add support later by tracking anchors
                let source_info = self.make_source_info(&marker, 0);
                let node = YamlWithSourceInfo::new_scalar(Yaml::Null, source_info);
                self.push_complete(node);
            }
        }
    }
}

/// Parse a scalar string value into the appropriate Yaml type.
///
/// This handles type inference: integers, floats, booleans, null, and strings.
fn parse_scalar_value(value: &str) -> Yaml {
    // Try to parse as integer
    if let Ok(i) = value.parse::<i64>() {
        return Yaml::Integer(i);
    }

    // Try to parse as float
    if let Ok(_f) = value.parse::<f64>() {
        return Yaml::Real(value.to_string());
    }

    // Check for boolean
    match value {
        "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => {
            return Yaml::Boolean(true);
        }
        "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => {
            return Yaml::Boolean(false);
        }
        "null" | "Null" | "NULL" | "~" | "" => {
            return Yaml::Null;
        }
        _ => {}
    }

    // Default to string
    Yaml::String(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_scalar() {
        let yaml = parse("hello").unwrap();
        assert!(yaml.is_scalar());
        assert_eq!(yaml.yaml.as_str(), Some("hello"));
    }

    #[test]
    fn test_parse_integer() {
        let yaml = parse("42").unwrap();
        assert!(yaml.is_scalar());
        assert_eq!(yaml.yaml.as_i64(), Some(42));
    }

    #[test]
    fn test_parse_boolean() {
        let yaml = parse("true").unwrap();
        assert!(yaml.is_scalar());
        assert_eq!(yaml.yaml.as_bool(), Some(true));
    }

    #[test]
    fn test_parse_array() {
        let yaml = parse("[1, 2, 3]").unwrap();
        assert!(yaml.is_array());
        assert_eq!(yaml.len(), 3);

        let items = yaml.as_array().unwrap();
        assert_eq!(items[0].yaml.as_i64(), Some(1));
        assert_eq!(items[1].yaml.as_i64(), Some(2));
        assert_eq!(items[2].yaml.as_i64(), Some(3));
    }

    #[test]
    fn test_parse_hash() {
        let yaml = parse("title: My Document\nauthor: John Doe").unwrap();
        assert!(yaml.is_hash());
        assert_eq!(yaml.len(), 2);

        let title = yaml.get_hash_value("title").unwrap();
        assert_eq!(title.yaml.as_str(), Some("My Document"));

        let author = yaml.get_hash_value("author").unwrap();
        assert_eq!(author.yaml.as_str(), Some("John Doe"));
    }

    #[test]
    fn test_nested_structure() {
        let yaml = parse(r#"
project:
  title: My Project
  authors:
    - Alice
    - Bob
"#).unwrap();

        assert!(yaml.is_hash());

        let project = yaml.get_hash_value("project").unwrap();
        assert!(project.is_hash());

        let authors = project.get_hash_value("authors").unwrap();
        assert!(authors.is_array());
        assert_eq!(authors.len(), 2);
    }

    #[test]
    fn test_source_info_tracking() {
        let yaml = parse("title: My Document").unwrap();

        // Check that source info is present
        assert!(yaml.source_info.line >= 1);  // Line number should be at least 1
        assert!(yaml.source_info.col >= 1);   // Column number should be at least 1
        assert!(yaml.source_info.len > 0);

        let title = yaml.get_hash_value("title").unwrap();
        assert!(title.source_info.line >= 1);
    }

    #[test]
    fn test_parse_with_filename() {
        let yaml = parse_file("title: Test", "config.yaml").unwrap();
        assert_eq!(yaml.source_info.file, Some("config.yaml".into()));
    }
}
