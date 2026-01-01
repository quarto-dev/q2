//! XML parser that builds XmlWithSourceInfo trees.

use crate::{
    Error, ParseResult, Result, XmlAttribute, XmlChild, XmlChildren, XmlElement, XmlParseContext,
    XmlWithSourceInfo,
};
use quarto_source_map::{FileId, SourceInfo};
use quick_xml::Reader;
use quick_xml::events::{BytesCData, BytesEnd, BytesStart, BytesText, Event};

/// Parse XML from a string, producing an XmlWithSourceInfo tree.
///
/// # Example
///
/// ```rust
/// use quarto_xml::parse;
///
/// let xml = parse("<root><child/></root>").unwrap();
/// assert_eq!(xml.root.name, "root");
/// ```
///
/// # Errors
///
/// Returns an error if the XML is malformed or if parsing fails.
pub fn parse(content: &str) -> Result<XmlWithSourceInfo> {
    parse_impl(content, None, FileId(0))
}

/// Parse XML from a string with an associated file ID.
///
/// The file ID is used in source location information for error reporting.
pub fn parse_with_file_id(content: &str, file_id: FileId) -> Result<XmlWithSourceInfo> {
    parse_impl(content, None, file_id)
}

/// Parse XML that was extracted from a parent document.
///
/// This function is used when parsing XML that is a substring of a larger
/// document. The resulting XmlWithSourceInfo will have Substring mappings
/// that track back to the parent document.
pub fn parse_with_parent(content: &str, parent: SourceInfo) -> Result<XmlWithSourceInfo> {
    parse_impl(content, Some(parent), FileId(0))
}

/// Parse XML from a string with diagnostic collection.
///
/// This is the preferred function when you need rich error messages with
/// Q-9-* error codes. The context collects any warnings or lints during
/// parsing, and errors are returned as DiagnosticMessage objects.
///
/// # Example
///
/// ```rust
/// use quarto_xml::{parse_with_context, XmlParseContext};
///
/// let mut ctx = XmlParseContext::new();
/// match parse_with_context("<root/>", &mut ctx) {
///     Ok(xml) => {
///         assert_eq!(xml.root.name, "root");
///         // Check for any warnings
///         if ctx.has_diagnostics() {
///             for diag in ctx.diagnostics() {
///                 eprintln!("Warning: {}", diag.title);
///             }
///         }
///     }
///     Err(errors) => {
///         for err in errors {
///             eprintln!("Error [{}]: {}", err.code.as_deref().unwrap_or("?"), err.title);
///         }
///     }
/// }
/// ```
///
/// # Errors
///
/// Returns a vector of DiagnosticMessage objects if parsing fails.
pub fn parse_with_context(
    content: &str,
    ctx: &mut XmlParseContext,
) -> ParseResult<XmlWithSourceInfo> {
    parse_with_context_impl(content, None, FileId(0), ctx)
}

fn parse_with_context_impl(
    content: &str,
    parent: Option<SourceInfo>,
    file_id: FileId,
    ctx: &mut XmlParseContext,
) -> ParseResult<XmlWithSourceInfo> {
    match parse_impl(content, parent, file_id) {
        Ok(xml) => Ok(xml),
        Err(err) => {
            let diagnostic = err.to_diagnostic();
            ctx.add_diagnostic(diagnostic.clone());
            Err(vec![diagnostic])
        }
    }
}

fn parse_impl(
    content: &str,
    parent: Option<SourceInfo>,
    file_id: FileId,
) -> Result<XmlWithSourceInfo> {
    let mut parser = XmlParser::new(content, parent, file_id);
    parser.parse()
}

/// Internal parser state.
struct XmlParser<'a> {
    /// The source content being parsed.
    source: &'a str,

    /// The quick-xml reader.
    reader: Reader<&'a [u8]>,

    /// Optional parent source info for substring tracking.
    parent: Option<SourceInfo>,

    /// File ID for creating SourceInfo.
    file_id: FileId,

    /// Stack of elements being built.
    stack: Vec<BuildNode>,
}

/// A node being constructed during parsing.
struct BuildNode {
    /// Element name.
    name: String,

    /// Source info for the element name.
    name_source: SourceInfo,

    /// Namespace prefix, if any.
    prefix: Option<String>,

    /// Attributes of this element.
    attributes: Vec<XmlAttribute>,

    /// Byte offset where this element started (the `<` character).
    start_offset: usize,

    /// Child elements and text accumulated so far.
    children: Vec<XmlChild>,
}

impl<'a> XmlParser<'a> {
    fn new(source: &'a str, parent: Option<SourceInfo>, file_id: FileId) -> Self {
        let mut reader = Reader::from_str(source);
        reader.config_mut().trim_text_start = false;
        reader.config_mut().trim_text_end = false;

        Self {
            source,
            reader,
            parent,
            file_id,
            stack: Vec::new(),
        }
    }

    fn parse(&mut self) -> Result<XmlWithSourceInfo> {
        let mut root: Option<XmlElement> = None;
        let doc_start = 0usize;

        loop {
            // Capture position before reading the event
            let event_start = self.reader.buffer_position() as usize;

            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    self.handle_start(e, event_start)?;
                }
                Ok(Event::End(e)) => {
                    let element = self.handle_end(e)?;

                    if self.stack.is_empty() {
                        // This is the root element
                        if root.is_some() {
                            return Err(Error::MultipleRoots {
                                location: Some(element.source_info.clone()),
                            });
                        }
                        root = Some(element);
                    } else {
                        // Add to parent's children
                        self.stack
                            .last_mut()
                            .unwrap()
                            .children
                            .push(XmlChild::Element(element));
                    }
                }
                Ok(Event::Empty(e)) => {
                    let element = self.handle_empty(e, event_start)?;

                    if self.stack.is_empty() {
                        if root.is_some() {
                            return Err(Error::MultipleRoots {
                                location: Some(element.source_info.clone()),
                            });
                        }
                        root = Some(element);
                    } else {
                        self.stack
                            .last_mut()
                            .unwrap()
                            .children
                            .push(XmlChild::Element(element));
                    }
                }
                Ok(Event::Text(e)) => {
                    self.handle_text(e, event_start)?;
                }
                Ok(Event::CData(e)) => {
                    self.handle_cdata(e, event_start)?;
                }
                Ok(Event::Comment(_) | Event::PI(_) | Event::Decl(_)) => {
                    // Skip comments, processing instructions, and XML declarations
                }
                Ok(Event::DocType(_)) => {
                    // Skip DOCTYPE declarations
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(Error::XmlSyntax {
                        message: e.to_string(),
                        position: Some(self.reader.error_position()),
                    });
                }
            }
        }

        // Check for unclosed elements
        if !self.stack.is_empty() {
            let node = self.stack.last().unwrap();
            return Err(Error::UnexpectedEof {
                expected: format!("closing tag </{}>", node.name),
                location: Some(node.name_source.clone()),
            });
        }

        let root = root.ok_or(Error::EmptyDocument)?;

        let doc_end = self.source.len();
        let doc_source_info = self.make_source_info(doc_start, doc_end);

        Ok(XmlWithSourceInfo::new(root, doc_source_info))
    }

    fn handle_start(&mut self, e: BytesStart<'_>, event_start: usize) -> Result<()> {
        let (name, prefix) = self.parse_name(&e)?;
        let name_start = event_start + 1; // Skip '<'
        let name_end = name_start + e.name().as_ref().len();
        let name_source = self.make_source_info(name_start, name_end);

        let attributes = self.parse_attributes(&e, event_start)?;

        self.stack.push(BuildNode {
            name,
            name_source,
            prefix,
            attributes,
            start_offset: event_start,
            children: Vec::new(),
        });

        Ok(())
    }

    fn handle_end(&mut self, e: BytesEnd<'_>) -> Result<XmlElement> {
        let end_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
        let end_local_name = end_name.split(':').next_back().unwrap_or(&end_name);

        let node = self.stack.pop().ok_or_else(|| Error::InvalidStructure {
            message: format!("Unexpected closing tag </{}>", end_name),
            location: None,
        })?;

        // Verify tag names match
        if node.name != end_local_name {
            return Err(Error::MismatchedEndTag {
                expected: node.name.clone(),
                found: end_local_name.to_string(),
                location: Some(node.name_source.clone()),
            });
        }

        let end_offset = self.reader.buffer_position() as usize;
        let source_info = self.make_source_info(node.start_offset, end_offset);

        let children = self.finalize_children(node.children);

        Ok(XmlElement {
            name: node.name,
            name_source: node.name_source,
            prefix: node.prefix,
            attributes: node.attributes,
            children,
            source_info,
        })
    }

    fn handle_empty(&mut self, e: BytesStart<'_>, event_start: usize) -> Result<XmlElement> {
        let (name, prefix) = self.parse_name(&e)?;
        let name_start = event_start + 1;
        let name_end = name_start + e.name().as_ref().len();
        let name_source = self.make_source_info(name_start, name_end);

        let attributes = self.parse_attributes(&e, event_start)?;

        let end_offset = self.reader.buffer_position() as usize;
        let source_info = self.make_source_info(event_start, end_offset);

        Ok(XmlElement {
            name,
            name_source,
            prefix,
            attributes,
            children: XmlChildren::Empty,
            source_info,
        })
    }

    fn handle_text(&mut self, e: BytesText<'_>, event_start: usize) -> Result<()> {
        let text = e.unescape().map_err(|err| Error::XmlSyntax {
            message: format!("Invalid text content: {}", err),
            position: Some(event_start as u64),
        })?;

        // Compute everything before taking mutable borrow
        let end_offset = self.reader.buffer_position() as usize;
        let source_info = self.make_source_info(event_start, end_offset);
        let text_owned = text.into_owned();

        if let Some(node) = self.stack.last_mut() {
            // Skip whitespace-only text between elements
            if text_owned.trim().is_empty() && !node.children.is_empty() {
                return Ok(());
            }

            node.children.push(XmlChild::Text {
                content: text_owned,
                source_info,
            });
        }
        Ok(())
    }

    fn handle_cdata(&mut self, e: BytesCData<'_>, event_start: usize) -> Result<()> {
        let text = String::from_utf8_lossy(e.as_ref()).to_string();
        let end_offset = self.reader.buffer_position() as usize;
        let source_info = self.make_source_info(event_start, end_offset);

        if let Some(node) = self.stack.last_mut() {
            node.children.push(XmlChild::Text {
                content: text,
                source_info,
            });
        }
        Ok(())
    }

    fn parse_name(&self, e: &BytesStart<'_>) -> Result<(String, Option<String>)> {
        let full_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

        if let Some(pos) = full_name.find(':') {
            let prefix = full_name[..pos].to_string();
            let local_name = full_name[pos + 1..].to_string();
            Ok((local_name, Some(prefix)))
        } else {
            Ok((full_name, None))
        }
    }

    fn parse_attributes(&self, e: &BytesStart<'_>, tag_start: usize) -> Result<Vec<XmlAttribute>> {
        let mut attributes = Vec::new();

        // The tag content starts at tag_start + 1 (after '<')
        let content_start = tag_start + 1;

        // Get the raw tag content for position calculation
        let tag_content = e.as_ref();
        let tag_str = String::from_utf8_lossy(tag_content);

        // Position of attributes section within the tag content (after element name)
        let attrs_offset = e.name().as_ref().len();

        for attr_result in e.attributes() {
            let attr = attr_result?;

            let key_bytes = attr.key.as_ref();
            let key_str = String::from_utf8_lossy(key_bytes);
            let full_name = key_str.to_string();

            let (name, prefix) = if let Some(pos) = full_name.find(':') {
                let prefix = full_name[..pos].to_string();
                let local_name = full_name[pos + 1..].to_string();
                (local_name, Some(prefix))
            } else {
                (full_name.clone(), None)
            };

            let value = attr.unescape_value().map_err(|err| Error::XmlSyntax {
                message: format!("Invalid attribute value: {}", err),
                position: Some(tag_start as u64),
            })?;

            // Find the attribute in the tag content to get precise positions
            // Search for the pattern: name="value" or name='value'
            let (name_source, value_source) =
                self.find_attribute_positions(&tag_str, attrs_offset, &full_name, content_start);

            attributes.push(XmlAttribute {
                name,
                name_source,
                prefix,
                value: value.into_owned(),
                value_source,
            });
        }

        Ok(attributes)
    }

    /// Find precise positions of an attribute name and value within the tag content.
    fn find_attribute_positions(
        &self,
        tag_str: &str,
        search_start: usize,
        attr_name: &str,
        content_start: usize,
    ) -> (SourceInfo, SourceInfo) {
        // Search for the attribute name in the tag content
        let search_area = &tag_str[search_start..];

        if let Some(rel_pos) = search_area.find(attr_name) {
            let name_start = content_start + search_start + rel_pos;
            let name_end = name_start + attr_name.len();
            let name_source = self.make_source_info(name_start, name_end);

            // Find the value after the name
            // Pattern: name="value" or name='value' or name=value
            let after_name = &search_area[rel_pos + attr_name.len()..];

            // Skip whitespace and '='
            let mut chars = after_name.chars().peekable();
            let mut offset = 0;

            // Skip whitespace
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    offset += c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }

            // Skip '='
            if chars.peek() == Some(&'=') {
                offset += 1;
                chars.next();
            }

            // Skip whitespace after '='
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    offset += c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }

            // Determine quote character and find value bounds
            let value_start_rel = rel_pos + attr_name.len() + offset;
            let (value_start, value_end) = if let Some(&quote) = chars.peek() {
                if quote == '"' || quote == '\'' {
                    // Quoted value
                    let value_content_start = value_start_rel + 1;
                    // Find closing quote
                    if let Some(end_pos) = search_area[value_content_start..].find(quote) {
                        // Include quotes in the source info for context
                        (
                            content_start + search_start + value_start_rel,
                            content_start + search_start + value_content_start + end_pos + 1,
                        )
                    } else {
                        // No closing quote found, use what we have
                        (
                            content_start + search_start + value_start_rel,
                            content_start + search_start + value_start_rel + 1,
                        )
                    }
                } else {
                    // Unquoted value (find end at whitespace or >)
                    let value_end_rel = search_area[value_start_rel..]
                        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                        .map_or(search_area.len(), |p| value_start_rel + p);
                    (
                        content_start + search_start + value_start_rel,
                        content_start + search_start + value_end_rel,
                    )
                }
            } else {
                // No value found
                (name_start, name_end)
            };

            let value_source = self.make_source_info(value_start, value_end);
            (name_source, value_source)
        } else {
            // Attribute not found in content (shouldn't happen)
            // Fall back to tag start
            let fallback = self.make_source_info(content_start, content_start + 1);
            (fallback.clone(), fallback)
        }
    }

    fn finalize_children(&self, children: Vec<XmlChild>) -> XmlChildren {
        if children.is_empty() {
            return XmlChildren::Empty;
        }

        // Check if all children are elements
        let all_elements = children.iter().all(|c| matches!(c, XmlChild::Element(_)));

        // Check if there's exactly one text child
        let single_text = children.len() == 1 && matches!(&children[0], XmlChild::Text { .. });

        if all_elements {
            let elements = children
                .into_iter()
                .filter_map(|c| match c {
                    XmlChild::Element(e) => Some(e),
                    _ => None,
                })
                .collect();
            XmlChildren::Elements(elements)
        } else if single_text {
            match children.into_iter().next().unwrap() {
                XmlChild::Text {
                    content,
                    source_info,
                } => XmlChildren::Text {
                    content,
                    source_info,
                },
                _ => unreachable!(),
            }
        } else {
            XmlChildren::Mixed(children)
        }
    }

    fn make_source_info(&self, start: usize, end: usize) -> SourceInfo {
        if let Some(ref parent) = self.parent {
            SourceInfo::substring(parent.clone(), start, end)
        } else {
            SourceInfo::original(self.file_id, start, end)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_element() {
        let xml = parse("<root/>").unwrap();
        assert_eq!(xml.root.name, "root");
        assert!(xml.root.is_empty());
    }

    #[test]
    fn test_parse_nested_elements() {
        let xml = parse("<root><child/></root>").unwrap();
        assert_eq!(xml.root.name, "root");
        assert!(xml.root.has_elements());

        let children = xml.root.elements().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "child");
    }

    #[test]
    fn test_parse_text_content() {
        let xml = parse("<root>Hello, world!</root>").unwrap();
        assert_eq!(xml.root.name, "root");
        assert!(xml.root.has_text());
        assert_eq!(xml.root.text(), Some("Hello, world!"));
    }

    #[test]
    fn test_parse_attributes() {
        let xml = parse(r#"<root attr="value"/>"#).unwrap();
        assert_eq!(xml.root.get_attribute("attr"), Some("value"));
    }

    #[test]
    fn test_parse_namespace_prefix() {
        let xml = parse(r#"<csl:style xmlns:csl="http://example.org"/>"#).unwrap();
        assert_eq!(xml.root.name, "style");
        assert_eq!(xml.root.prefix, Some("csl".to_string()));
    }

    #[test]
    fn test_parse_csl_style() {
        let xml = parse(
            r#"<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0">
  <info>
    <title>Test Style</title>
  </info>
  <citation>
    <layout>
      <text variable="title"/>
    </layout>
  </citation>
</style>"#,
        )
        .unwrap();

        assert_eq!(xml.root.name, "style");
        assert_eq!(xml.root.get_attribute("version"), Some("1.0"));

        let info = xml.root.get_children("info");
        assert_eq!(info.len(), 1);

        let title = info[0].get_children("title");
        assert_eq!(title.len(), 1);
        assert_eq!(title[0].text(), Some("Test Style"));
    }

    #[test]
    fn test_source_info_tracking() {
        let content = "<root/>";
        let xml = parse(content).unwrap();

        // Root element should span the entire content
        assert_eq!(xml.root.source_info.start_offset(), 0);
        assert_eq!(xml.root.source_info.end_offset(), content.len());
    }

    #[test]
    fn test_empty_document_error() {
        let result = parse("");
        assert!(matches!(result, Err(Error::EmptyDocument)));
    }

    #[test]
    fn test_mismatched_tags_error() {
        let result = parse("<root></wrong>");
        // quick-xml catches mismatched tags itself when check_end_names is enabled (default)
        // It returns an Error::IllFormed(MismatchedEndTag), which we convert to XmlSyntax
        assert!(
            matches!(
                result,
                Err(Error::MismatchedEndTag { .. } | Error::XmlSyntax { .. })
            ),
            "Expected MismatchedEndTag or XmlSyntax error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_attribute_position_tracking() {
        //          0         1         2         3
        //          0123456789012345678901234567890123456789
        let xml = r#"<root attr="value" other='test'/>"#;
        let parsed = parse(xml).unwrap();

        assert_eq!(parsed.root.attributes.len(), 2);

        // First attribute: attr="value"
        let attr1 = &parsed.root.attributes[0];
        assert_eq!(attr1.name, "attr");
        assert_eq!(attr1.value, "value");

        // Check name position: "attr" starts at position 6 (after "<root ")
        assert_eq!(attr1.name_source.start_offset(), 6);
        assert_eq!(attr1.name_source.end_offset(), 10); // "attr" is 4 chars

        // Check value position: includes quotes "value" starts at 11
        assert_eq!(attr1.value_source.start_offset(), 11);
        assert_eq!(attr1.value_source.end_offset(), 18); // "value" with quotes

        // Second attribute: other='test'
        let attr2 = &parsed.root.attributes[1];
        assert_eq!(attr2.name, "other");
        assert_eq!(attr2.value, "test");

        // Check name position: "other" starts at position 19
        assert_eq!(attr2.name_source.start_offset(), 19);
        assert_eq!(attr2.name_source.end_offset(), 24); // "other" is 5 chars

        // Check value position: includes quotes 'test' starts at 25
        assert_eq!(attr2.value_source.start_offset(), 25);
        assert_eq!(attr2.value_source.end_offset(), 31); // 'test' with quotes
    }

    #[test]
    fn test_attribute_position_with_namespaces() {
        //          0         1         2         3         4         5
        //          012345678901234567890123456789012345678901234567890123456789
        let xml = r#"<csl:style xmlns:csl="http://example.org" version="1.0"/>"#;
        let parsed = parse(xml).unwrap();

        assert_eq!(parsed.root.name, "style");
        assert_eq!(parsed.root.prefix, Some("csl".to_string()));
        assert_eq!(parsed.root.attributes.len(), 2);

        // Check version attribute
        let version_attr = parsed.root.get_attribute_full("version").unwrap();
        assert_eq!(version_attr.value, "1.0");

        // The version attribute name should be tracked correctly
        // It appears after xmlns:csl="http://example.org"
    }

    // ==================== Error Behavior Tests ====================

    #[test]
    fn test_empty_document_diagnostic() {
        use crate::XmlParseContext;

        let mut ctx = XmlParseContext::new();
        let result = parse_with_context("", &mut ctx);

        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);

        let err = &errors[0];
        assert_eq!(err.code.as_deref(), Some("Q-9-5"));
        assert_eq!(err.title, "Empty XML Document");
    }

    #[test]
    fn test_unclosed_element_diagnostic() {
        use crate::XmlParseContext;

        let mut ctx = XmlParseContext::new();
        let result = parse_with_context("<root>", &mut ctx);

        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);

        let err = &errors[0];
        assert_eq!(err.code.as_deref(), Some("Q-9-2"));
        assert_eq!(err.title, "Unexpected End of XML Input");
        assert!(ctx.has_diagnostics());
    }

    #[test]
    fn test_multiple_roots_diagnostic() {
        use crate::XmlParseContext;

        let mut ctx = XmlParseContext::new();
        let result = parse_with_context("<root/><another/>", &mut ctx);

        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);

        let err = &errors[0];
        assert_eq!(err.code.as_deref(), Some("Q-9-6"));
        assert_eq!(err.title, "Multiple XML Root Elements");
    }

    #[test]
    fn test_syntax_error_diagnostic() {
        use crate::XmlParseContext;

        let mut ctx = XmlParseContext::new();
        let result = parse_with_context("<root attr=unquoted/>", &mut ctx);

        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);

        let err = &errors[0];
        // Syntax errors from quick-xml become Q-9-1
        assert_eq!(err.code.as_deref(), Some("Q-9-1"));
        assert_eq!(err.title, "XML Syntax Error");
    }

    #[test]
    fn test_context_collects_diagnostics() {
        use crate::XmlParseContext;

        let mut ctx = XmlParseContext::new();

        // Successful parse should have no diagnostics
        let result = parse_with_context("<root/>", &mut ctx);
        assert!(result.is_ok());
        assert!(!ctx.has_diagnostics());

        // Failed parse should add diagnostic to context
        let mut ctx2 = XmlParseContext::new();
        let result = parse_with_context("", &mut ctx2);
        assert!(result.is_err());
        assert!(ctx2.has_diagnostics());
        assert_eq!(ctx2.diagnostics().len(), 1);
    }

    #[test]
    fn test_error_to_diagnostic_conversion() {
        // Test that Error::to_diagnostic produces the right codes
        let err = Error::EmptyDocument;
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("Q-9-5"));

        let err = Error::MultipleRoots { location: None };
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("Q-9-6"));

        let err = Error::UnexpectedEof {
            expected: "closing tag".to_string(),
            location: None,
        };
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("Q-9-2"));

        let err = Error::MismatchedEndTag {
            expected: "root".to_string(),
            found: "other".to_string(),
            location: None,
        };
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("Q-9-3"));

        let err = Error::InvalidStructure {
            message: "test".to_string(),
            location: None,
        };
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("Q-9-4"));

        let err = Error::XmlSyntax {
            message: "test".to_string(),
            position: None,
        };
        let diag = err.to_diagnostic();
        assert_eq!(diag.code.as_deref(), Some("Q-9-1"));
    }

    // ==================== CSL Integration Tests ====================

    #[test]
    fn test_parse_real_csl_style() {
        // Parse a real CSL style file to verify quarto-xml handles CSL format
        let csl = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <info>
    <title>Test Style</title>
    <id>http://example.org/test</id>
  </info>
  <locale xml:lang="en">
    <terms>
      <term name="chapter" form="short">ch.</term>
    </terms>
  </locale>
  <macro name="author">
    <names variable="author">
      <name initialize-with=". "/>
    </names>
  </macro>
  <citation>
    <layout>
      <text macro="author"/>
    </layout>
  </citation>
  <bibliography>
    <layout>
      <group delimiter=", ">
        <text macro="author"/>
        <date variable="issued">
          <date-part name="year"/>
        </date>
      </group>
    </layout>
  </bibliography>
</style>"#;

        let xml = parse(csl).unwrap();

        // Verify root element
        assert_eq!(xml.root.name, "style");
        assert_eq!(xml.root.get_attribute("class"), Some("in-text"));
        assert_eq!(xml.root.get_attribute("version"), Some("1.0"));

        // Verify we can navigate the structure (use all_children to handle whitespace)
        let elements = xml.root.all_children();
        let element_names: Vec<&str> = elements.iter().map(|e| e.name.as_str()).collect();
        assert!(element_names.contains(&"info"));
        assert!(element_names.contains(&"locale"));
        assert!(element_names.contains(&"macro"));
        assert!(element_names.contains(&"citation"));
        assert!(element_names.contains(&"bibliography"));

        // Verify macro element
        let macros = xml.root.get_children("macro");
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].get_attribute("name"), Some("author"));

        // Verify locale with xml:lang attribute
        let locales = xml.root.get_children("locale");
        assert_eq!(locales.len(), 1);
        // xml:lang is stored as "lang" with prefix "xml"
        let locale = &locales[0];
        let lang_attr = locale.attributes.iter().find(|a| a.name == "lang");
        assert!(lang_attr.is_some());
        assert_eq!(lang_attr.unwrap().value, "en");
        assert_eq!(lang_attr.unwrap().prefix, Some("xml".to_string()));
    }

    #[test]
    fn test_parse_csl_choose_conditionals() {
        // Test parsing CSL choose/if/else-if/else structure
        let csl = r#"<macro name="title">
  <choose>
    <if type="book" match="any">
      <text variable="title" font-style="italic"/>
    </if>
    <else-if type="article">
      <text variable="title" quotes="true"/>
    </else-if>
    <else>
      <text variable="title"/>
    </else>
  </choose>
</macro>"#;

        let xml = parse(csl).unwrap();
        assert_eq!(xml.root.name, "macro");

        let choose = xml.root.get_children("choose");
        assert_eq!(choose.len(), 1);

        // Use all_children to handle whitespace between elements
        let choose_children = choose[0].all_children();

        let choose_names: Vec<&str> = choose_children.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(choose_names, vec!["if", "else-if", "else"]);

        // Verify if element attributes
        let if_el = choose_children[0];
        assert_eq!(if_el.get_attribute("type"), Some("book"));
        assert_eq!(if_el.get_attribute("match"), Some("any"));
    }

    #[test]
    fn test_parse_csl_names_element() {
        // Test parsing CSL names element structure
        let csl = r#"<names variable="author">
  <name initialize-with=". " delimiter=", " and="text"/>
  <label form="short" prefix=", "/>
  <substitute>
    <names variable="editor"/>
  </substitute>
</names>"#;

        let xml = parse(csl).unwrap();
        assert_eq!(xml.root.name, "names");
        assert_eq!(xml.root.get_attribute("variable"), Some("author"));

        let children = xml.root.all_children();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].name, "name");
        assert_eq!(children[1].name, "label");
        assert_eq!(children[2].name, "substitute");

        // Verify name element attributes
        let name_el = children[0];
        assert_eq!(name_el.get_attribute("initialize-with"), Some(". "));
        assert_eq!(name_el.get_attribute("delimiter"), Some(", "));
        assert_eq!(name_el.get_attribute("and"), Some("text"));
    }

    #[test]
    fn test_parse_csl_date_element() {
        // Test parsing CSL date element structure
        let csl = r#"<date variable="issued">
  <date-part name="year" form="long"/>
  <date-part name="month" form="short" suffix="-"/>
  <date-part name="day" form="numeric"/>
</date>"#;

        let xml = parse(csl).unwrap();
        assert_eq!(xml.root.name, "date");
        assert_eq!(xml.root.get_attribute("variable"), Some("issued"));

        let date_parts = xml.root.all_children();
        assert_eq!(date_parts.len(), 3);

        assert_eq!(date_parts[0].get_attribute("name"), Some("year"));
        assert_eq!(date_parts[1].get_attribute("name"), Some("month"));
        assert_eq!(date_parts[2].get_attribute("name"), Some("day"));
    }
}
