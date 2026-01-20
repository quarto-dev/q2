//! Core types for LSP-like functionality.
//!
//! These types are designed to be:
//! - Transport-agnostic (no LSP protocol dependencies)
//! - Easily serializable to JSON (for WASM/hub-client)
//! - Easily convertible to `lsp-types` (for native LSP)
//!
//! All positions use 0-based line and character indices, matching the LSP specification.

use serde::{Deserialize, Serialize};

/// A position in a text document, expressed as zero-based line and character offset.
///
/// Character offsets are measured in UTF-16 code units to match the LSP specification.
/// For ASCII text, this is equivalent to the character index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Position {
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based character offset (UTF-16 code units).
    pub character: u32,
}

impl Position {
    /// Create a new position.
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.line.cmp(&other.line) {
            std::cmp::Ordering::Equal => self.character.cmp(&other.character),
            ord => ord,
        }
    }
}

/// A range in a text document, expressed as start and end positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Range {
    /// The range's start position (inclusive).
    pub start: Position,
    /// The range's end position (exclusive).
    pub end: Position,
}

impl Range {
    /// Create a new range.
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    /// Create a range spanning a single position (zero-width).
    pub fn point(pos: Position) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// Check if this range contains a position.
    pub fn contains(&self, pos: Position) -> bool {
        self.start <= pos && pos < self.end
    }

    /// Check if this range is empty (zero-width).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Diagnostic severity levels, matching LSP DiagnosticSeverity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    /// Reports an error.
    Error = 1,
    /// Reports a warning.
    Warning = 2,
    /// Reports an information.
    Information = 3,
    /// Reports a hint.
    Hint = 4,
}

impl DiagnosticSeverity {
    /// Convert from quarto-error-reporting DiagnosticKind.
    pub fn from_diagnostic_kind(kind: quarto_error_reporting::DiagnosticKind) -> Self {
        match kind {
            quarto_error_reporting::DiagnosticKind::Error => Self::Error,
            quarto_error_reporting::DiagnosticKind::Warning => Self::Warning,
            quarto_error_reporting::DiagnosticKind::Info => Self::Information,
            quarto_error_reporting::DiagnosticKind::Note => Self::Hint,
        }
    }
}

/// Related information for a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticRelatedInformation {
    /// The location of this related diagnostic information.
    pub range: Range,
    /// The message of this related diagnostic information.
    pub message: String,
}

/// A diagnostic message, such as a compiler error or warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The range at which the diagnostic applies.
    pub range: Range,
    /// The diagnostic's severity.
    pub severity: DiagnosticSeverity,
    /// The diagnostic's code, which might appear in the user interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// A human-readable string describing the source of this diagnostic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The diagnostic's message.
    pub message: String,
    /// Additional metadata about the diagnostic.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub related_information: Vec<DiagnosticRelatedInformation>,
}

impl Diagnostic {
    /// Create a new diagnostic.
    pub fn new(range: Range, severity: DiagnosticSeverity, message: impl Into<String>) -> Self {
        Self {
            range,
            severity,
            code: None,
            source: Some("quarto".to_string()),
            message: message.into(),
            related_information: Vec::new(),
        }
    }

    /// Set the diagnostic code.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Add related information.
    pub fn with_related(mut self, range: Range, message: impl Into<String>) -> Self {
        self.related_information.push(DiagnosticRelatedInformation {
            range,
            message: message.into(),
        });
        self
    }
}

/// Symbol kinds for document outline, matching LSP SymbolKind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    /// A file symbol.
    File = 1,
    /// A module symbol.
    Module = 2,
    /// A namespace symbol.
    Namespace = 3,
    /// A package symbol.
    Package = 4,
    /// A class symbol.
    Class = 5,
    /// A method symbol.
    Method = 6,
    /// A property symbol.
    Property = 7,
    /// A field symbol.
    Field = 8,
    /// A constructor symbol.
    Constructor = 9,
    /// An enum symbol.
    Enum = 10,
    /// An interface symbol.
    Interface = 11,
    /// A function symbol.
    Function = 12,
    /// A variable symbol.
    Variable = 13,
    /// A constant symbol.
    Constant = 14,
    /// A string symbol.
    String = 15,
    /// A number symbol.
    Number = 16,
    /// A boolean symbol.
    Boolean = 17,
    /// An array symbol.
    Array = 18,
    /// An object symbol.
    Object = 19,
    /// A key symbol.
    Key = 20,
    /// A null symbol.
    Null = 21,
    /// An enum member symbol.
    EnumMember = 22,
    /// A struct symbol.
    Struct = 23,
    /// An event symbol.
    Event = 24,
    /// An operator symbol.
    Operator = 25,
    /// A type parameter symbol.
    TypeParameter = 26,
}

/// A symbol representing a document element for outline/navigation.
///
/// This corresponds to LSP's DocumentSymbol, using a hierarchical structure
/// where symbols can contain children.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    /// The name of this symbol.
    pub name: String,
    /// More detail for this symbol, e.g., the signature of a function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The kind of this symbol.
    pub kind: SymbolKind,
    /// The range enclosing this symbol (including leading/trailing whitespace).
    pub range: Range,
    /// The range that should be selected when this symbol is selected.
    pub selection_range: Range,
    /// Children of this symbol, e.g., nested headers.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<Symbol>,
}

impl Symbol {
    /// Create a new symbol.
    pub fn new(
        name: impl Into<String>,
        kind: SymbolKind,
        range: Range,
        selection_range: Range,
    ) -> Self {
        Self {
            name: name.into(),
            detail: None,
            kind,
            range,
            selection_range,
            children: Vec::new(),
        }
    }

    /// Set the detail for this symbol.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Add a child symbol.
    pub fn with_child(mut self, child: Symbol) -> Self {
        self.children.push(child);
        self
    }

    /// Add multiple child symbols.
    pub fn with_children(mut self, children: impl IntoIterator<Item = Symbol>) -> Self {
        self.children.extend(children);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_ordering() {
        let p1 = Position::new(0, 5);
        let p2 = Position::new(0, 10);
        let p3 = Position::new(1, 0);

        assert!(p1 < p2);
        assert!(p2 < p3);
        assert!(p1 < p3);
    }

    #[test]
    fn range_contains() {
        let range = Range::new(Position::new(1, 0), Position::new(1, 10));

        assert!(range.contains(Position::new(1, 0)));
        assert!(range.contains(Position::new(1, 5)));
        assert!(!range.contains(Position::new(1, 10))); // End is exclusive
        assert!(!range.contains(Position::new(0, 5)));
        assert!(!range.contains(Position::new(2, 0)));
    }

    #[test]
    fn diagnostic_serialization() {
        let diag = Diagnostic::new(
            Range::new(Position::new(0, 0), Position::new(0, 10)),
            DiagnosticSeverity::Error,
            "Test error",
        )
        .with_code("Q-1-1");

        let json = serde_json::to_string(&diag).unwrap();
        assert!(json.contains("\"severity\":\"error\""));
        assert!(json.contains("\"code\":\"Q-1-1\""));
    }

    #[test]
    fn symbol_hierarchy() {
        let child = Symbol::new(
            "Subsection",
            SymbolKind::String,
            Range::new(Position::new(2, 0), Position::new(3, 0)),
            Range::new(Position::new(2, 0), Position::new(2, 12)),
        );

        let parent = Symbol::new(
            "Section",
            SymbolKind::String,
            Range::new(Position::new(0, 0), Position::new(5, 0)),
            Range::new(Position::new(0, 0), Position::new(0, 9)),
        )
        .with_child(child);

        assert_eq!(parent.children.len(), 1);
        assert_eq!(parent.children[0].name, "Subsection");
    }
}
