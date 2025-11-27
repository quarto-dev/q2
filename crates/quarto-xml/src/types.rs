//! Core types for source-tracked XML parsing.

use quarto_source_map::SourceInfo;

/// An XML document with source location tracking.
///
/// This is the top-level result of parsing an XML document.
/// Analogous to `YamlWithSourceInfo` in quarto-yaml.
#[derive(Debug, Clone)]
pub struct XmlWithSourceInfo {
    /// The root element of the document.
    pub root: XmlElement,

    /// Source location of the entire document.
    pub source_info: SourceInfo,
}

/// An XML element with source location tracking.
///
/// Tracks the element name, attributes, and children, each with their
/// own source location information.
#[derive(Debug, Clone)]
pub struct XmlElement {
    /// The local name of the element (without namespace prefix).
    pub name: String,

    /// Source location of the element name.
    pub name_source: SourceInfo,

    /// Namespace prefix, if any (e.g., "csl" in `<csl:text>`).
    pub prefix: Option<String>,

    /// Attributes of this element.
    pub attributes: Vec<XmlAttribute>,

    /// Child content of this element.
    pub children: XmlChildren,

    /// Source location of the entire element (from `<` to `>`/`/>`).
    ///
    /// For elements with content, this spans from the start tag to the end tag.
    pub source_info: SourceInfo,
}

/// An XML attribute with source location tracking.
///
/// Tracks both the attribute name and value with separate source locations,
/// enabling precise error reporting for invalid attribute names or values.
#[derive(Debug, Clone)]
pub struct XmlAttribute {
    /// The local name of the attribute (without namespace prefix).
    pub name: String,

    /// Source location of the attribute name.
    pub name_source: SourceInfo,

    /// Namespace prefix, if any.
    pub prefix: Option<String>,

    /// The attribute value (after unescaping XML entities).
    pub value: String,

    /// Source location of the attribute value (including quotes in the source).
    pub value_source: SourceInfo,
}

/// Children of an XML element.
///
/// XML elements can contain:
/// - Only child elements (typical in CSL)
/// - Only text content
/// - Mixed content (text and elements interleaved)
/// - Nothing (empty element)
#[derive(Debug, Clone)]
pub enum XmlChildren {
    /// Element contains only child elements.
    Elements(Vec<XmlElement>),

    /// Element contains only text content.
    Text {
        /// The text content (after unescaping XML entities).
        content: String,
        /// Source location of the text.
        source_info: SourceInfo,
    },

    /// Element contains mixed content (text and elements).
    Mixed(Vec<XmlChild>),

    /// Element is empty (no content).
    Empty,
}

/// A single child in mixed content.
#[derive(Debug, Clone)]
pub enum XmlChild {
    /// A child element.
    Element(XmlElement),

    /// Text content.
    Text {
        /// The text content.
        content: String,
        /// Source location of the text.
        source_info: SourceInfo,
    },
}

impl XmlWithSourceInfo {
    /// Create a new XmlWithSourceInfo.
    pub fn new(root: XmlElement, source_info: SourceInfo) -> Self {
        Self { root, source_info }
    }
}

impl XmlElement {
    /// Create a new empty element.
    pub fn new(
        name: String,
        name_source: SourceInfo,
        prefix: Option<String>,
        attributes: Vec<XmlAttribute>,
        source_info: SourceInfo,
    ) -> Self {
        Self {
            name,
            name_source,
            prefix,
            attributes,
            children: XmlChildren::Empty,
            source_info,
        }
    }

    /// Create an element with child elements.
    pub fn with_elements(
        name: String,
        name_source: SourceInfo,
        prefix: Option<String>,
        attributes: Vec<XmlAttribute>,
        children: Vec<XmlElement>,
        source_info: SourceInfo,
    ) -> Self {
        Self {
            name,
            name_source,
            prefix,
            attributes,
            children: XmlChildren::Elements(children),
            source_info,
        }
    }

    /// Create an element with text content.
    pub fn with_text(
        name: String,
        name_source: SourceInfo,
        prefix: Option<String>,
        attributes: Vec<XmlAttribute>,
        text: String,
        text_source: SourceInfo,
        source_info: SourceInfo,
    ) -> Self {
        Self {
            name,
            name_source,
            prefix,
            attributes,
            children: XmlChildren::Text {
                content: text,
                source_info: text_source,
            },
            source_info,
        }
    }

    /// Create an element with mixed content.
    pub fn with_mixed(
        name: String,
        name_source: SourceInfo,
        prefix: Option<String>,
        attributes: Vec<XmlAttribute>,
        children: Vec<XmlChild>,
        source_info: SourceInfo,
    ) -> Self {
        Self {
            name,
            name_source,
            prefix,
            attributes,
            children: XmlChildren::Mixed(children),
            source_info,
        }
    }

    /// Get an attribute value by name.
    pub fn get_attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }

    /// Get an attribute by name, returning the full attribute with source info.
    pub fn get_attribute_full(&self, name: &str) -> Option<&XmlAttribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    /// Check if this element has child elements.
    pub fn has_elements(&self) -> bool {
        matches!(
            &self.children,
            XmlChildren::Elements(e) if !e.is_empty()
        )
    }

    /// Check if this element has text content.
    pub fn has_text(&self) -> bool {
        matches!(&self.children, XmlChildren::Text { .. })
    }

    /// Check if this element is empty.
    pub fn is_empty(&self) -> bool {
        matches!(&self.children, XmlChildren::Empty)
    }

    /// Get child elements, if this element contains only elements.
    pub fn elements(&self) -> Option<&[XmlElement]> {
        match &self.children {
            XmlChildren::Elements(elements) => Some(elements),
            _ => None,
        }
    }

    /// Get text content, if this element contains only text.
    pub fn text(&self) -> Option<&str> {
        match &self.children {
            XmlChildren::Text { content, .. } => Some(content),
            _ => None,
        }
    }

    /// Get child elements by name.
    pub fn get_children(&self, name: &str) -> Vec<&XmlElement> {
        match &self.children {
            XmlChildren::Elements(elements) => {
                elements.iter().filter(|e| e.name == name).collect()
            }
            XmlChildren::Mixed(children) => children
                .iter()
                .filter_map(|c| match c {
                    XmlChild::Element(e) if e.name == name => Some(e),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        }
    }

    /// Get all child elements (ignoring text in mixed content).
    pub fn all_children(&self) -> Vec<&XmlElement> {
        match &self.children {
            XmlChildren::Elements(elements) => elements.iter().collect(),
            XmlChildren::Mixed(children) => children
                .iter()
                .filter_map(|c| match c {
                    XmlChild::Element(e) => Some(e),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        }
    }
}

impl XmlAttribute {
    /// Create a new attribute.
    pub fn new(
        name: String,
        name_source: SourceInfo,
        prefix: Option<String>,
        value: String,
        value_source: SourceInfo,
    ) -> Self {
        Self {
            name,
            name_source,
            prefix,
            value,
            value_source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_get_attribute() {
        let attr = XmlAttribute::new(
            "name".to_string(),
            SourceInfo::default(),
            None,
            "value".to_string(),
            SourceInfo::default(),
        );

        let element = XmlElement::new(
            "test".to_string(),
            SourceInfo::default(),
            None,
            vec![attr],
            SourceInfo::default(),
        );

        assert_eq!(element.get_attribute("name"), Some("value"));
        assert_eq!(element.get_attribute("missing"), None);
    }

    #[test]
    fn test_element_children() {
        let child = XmlElement::new(
            "child".to_string(),
            SourceInfo::default(),
            None,
            vec![],
            SourceInfo::default(),
        );

        let parent = XmlElement::with_elements(
            "parent".to_string(),
            SourceInfo::default(),
            None,
            vec![],
            vec![child],
            SourceInfo::default(),
        );

        assert!(parent.has_elements());
        assert!(!parent.has_text());
        assert_eq!(parent.elements().unwrap().len(), 1);
    }

    #[test]
    fn test_element_text() {
        let element = XmlElement::with_text(
            "text".to_string(),
            SourceInfo::default(),
            None,
            vec![],
            "Hello, world!".to_string(),
            SourceInfo::default(),
            SourceInfo::default(),
        );

        assert!(element.has_text());
        assert!(!element.has_elements());
        assert_eq!(element.text(), Some("Hello, world!"));
    }
}
