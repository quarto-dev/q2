//! Source location information for YAML nodes.

use serde::{Deserialize, Serialize};

/// Source location information for a YAML node.
///
/// Tracks the position of a YAML element in the original source text.
/// This enables precise error reporting and source tracking through
/// transformations.
///
/// ## Note on Future Integration
///
/// This is a simplified version for initial implementation. Eventually this
/// will be replaced by the unified SourceInfo type from the main project that
/// supports transformations and non-contiguous mappings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// Optional filename or source identifier
    pub file: Option<String>,

    /// Byte offset from start of source (0-based)
    pub offset: usize,

    /// Line number (1-based)
    pub line: usize,

    /// Column number (1-based, in characters not bytes)
    pub col: usize,

    /// Length in bytes
    pub len: usize,
}

impl SourceInfo {
    /// Create a new SourceInfo with all fields specified.
    pub fn new(
        file: Option<String>,
        offset: usize,
        line: usize,
        col: usize,
        len: usize,
    ) -> Self {
        Self {
            file,
            offset,
            line,
            col,
            len,
        }
    }

    /// Create a SourceInfo from a yaml-rust2::Marker.
    ///
    /// The marker provides the starting position. Length must be computed
    /// separately based on the content.
    pub fn from_marker(marker: &yaml_rust2::scanner::Marker, len: usize) -> Self {
        Self {
            file: None,
            offset: marker.index(),
            line: marker.line() + 1,  // yaml-rust2 uses 0-based, we use 1-based
            col: marker.col() + 1,    // yaml-rust2 uses 0-based, we use 1-based
            len,
        }
    }

    /// Create a SourceInfo spanning from start to end markers.
    pub fn from_span(
        start: &yaml_rust2::scanner::Marker,
        end: &yaml_rust2::scanner::Marker,
    ) -> Self {
        let start_index = start.index();
        let end_index = end.index();
        Self {
            file: None,
            offset: start_index,
            line: start.line() + 1,
            col: start.col() + 1,
            len: end_index.saturating_sub(start_index),
        }
    }

    /// Set the filename for this source location.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Get the end offset (exclusive) of this location.
    pub fn end_offset(&self) -> usize {
        self.offset + self.len
    }
}

impl Default for SourceInfo {
    fn default() -> Self {
        Self {
            file: None,
            offset: 0,
            line: 1,
            col: 1,
            len: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_info_creation() {
        let info = SourceInfo::new(Some("test.yaml".into()), 10, 2, 5, 8);
        assert_eq!(info.file, Some("test.yaml".into()));
        assert_eq!(info.offset, 10);
        assert_eq!(info.line, 2);
        assert_eq!(info.col, 5);
        assert_eq!(info.len, 8);
        assert_eq!(info.end_offset(), 18);
    }

    #[test]
    fn test_with_file() {
        let info = SourceInfo::default().with_file("test.yaml");
        assert_eq!(info.file, Some("test.yaml".into()));
    }

    #[test]
    fn test_default() {
        let info = SourceInfo::default();
        assert_eq!(info.file, None);
        assert_eq!(info.offset, 0);
        assert_eq!(info.line, 1);
        assert_eq!(info.col, 1);
        assert_eq!(info.len, 0);
    }
}
