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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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

/// A detail item in a diagnostic message.
///
/// Matches `quarto_error_reporting::DetailItem` for compatibility.
/// Details provide specific information about errors (what went wrong,
/// where, with what values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticDetail {
    /// The kind of detail (error, info, note) - determines bullet style.
    pub kind: DetailKind,
    /// The content of the detail.
    pub content: MessageContent,
    /// Optional source location for this detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

impl DiagnosticDetail {
    /// Create a new diagnostic detail.
    pub fn new(kind: DetailKind, content: impl Into<MessageContent>) -> Self {
        Self {
            kind,
            content: content.into(),
            range: None,
        }
    }

    /// Create a diagnostic detail with a range.
    pub fn with_range(kind: DetailKind, content: impl Into<MessageContent>, range: Range) -> Self {
        Self {
            kind,
            content: content.into(),
            range: Some(range),
        }
    }

    /// Set the range for this detail.
    pub fn set_range(mut self, range: Range) -> Self {
        self.range = Some(range);
        self
    }
}

/// A rich diagnostic message matching `quarto_error_reporting::DiagnosticMessage`.
///
/// This preserves the tidyverse-style structure:
/// - `title`: Brief error description
/// - `problem`: What went wrong (the "must" or "can't" statement)
/// - `details`: Specific information (bulleted, max 5 per tidyverse)
/// - `hints`: Suggestions for fixing
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// The range at which the diagnostic applies (primary location).
    pub range: Range,
    /// The diagnostic's severity.
    pub severity: DiagnosticSeverity,
    /// Optional error code (e.g., "Q-1-1") for searchability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// A human-readable string describing the source of this diagnostic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Brief title for the error.
    pub title: String,
    /// The problem statement - what went wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<MessageContent>,
    /// Specific error details with optional locations.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub details: Vec<DiagnosticDetail>,
    /// Suggestions for fixing the issue.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub hints: Vec<MessageContent>,
}

impl Diagnostic {
    /// Create a new diagnostic with just a title.
    pub fn new(range: Range, severity: DiagnosticSeverity, title: impl Into<String>) -> Self {
        Self {
            range,
            severity,
            code: None,
            source: Some("quarto".to_string()),
            title: title.into(),
            problem: None,
            details: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// Set the diagnostic code.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Set the problem statement.
    pub fn with_problem(mut self, problem: impl Into<MessageContent>) -> Self {
        self.problem = Some(problem.into());
        self
    }

    /// Add a detail item.
    pub fn with_detail(mut self, detail: DiagnosticDetail) -> Self {
        self.details.push(detail);
        self
    }

    /// Add a hint.
    pub fn with_hint(mut self, hint: impl Into<MessageContent>) -> Self {
        self.hints.push(hint.into());
        self
    }

    /// Get a combined message for simplified display (title + problem).
    ///
    /// This is useful for contexts that only support a single message string,
    /// like the LSP protocol.
    pub fn combined_message(&self) -> String {
        if let Some(problem) = &self.problem {
            format!("{}: {}", self.title, problem.as_str())
        } else {
            self.title.clone()
        }
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
#[serde(rename_all = "camelCase")]
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

/// The kind of a folding range, matching LSP FoldingRangeKind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FoldingRangeKind {
    /// Folding range for a comment.
    Comment,
    /// Folding range for imports or includes.
    Imports,
    /// Folding range for a region (e.g., `#region`).
    Region,
}

/// A folding range for code folding in editors.
///
/// Folding ranges are identified by start and end line numbers (0-based).
/// The client should fold from `start_line` to `end_line` inclusive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldingRange {
    /// Zero-based start line.
    pub start_line: u32,
    /// Zero-based end line (inclusive).
    pub end_line: u32,
    /// The kind of folding range (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<FoldingRangeKind>,
}

impl FoldingRange {
    /// Create a new folding range.
    pub fn new(start_line: u32, end_line: u32) -> Self {
        Self {
            start_line,
            end_line,
            kind: None,
        }
    }

    /// Create a folding range with a specific kind.
    pub fn with_kind(start_line: u32, end_line: u32, kind: FoldingRangeKind) -> Self {
        Self {
            start_line,
            end_line,
            kind: Some(kind),
        }
    }

    /// Set the kind of this folding range.
    pub fn set_kind(mut self, kind: FoldingRangeKind) -> Self {
        self.kind = Some(kind);
        self
    }
}

// ============================================================================
// Rich Diagnostic Types (matching quarto-error-reporting::DiagnosticMessage)
// ============================================================================

/// The content type for message text.
///
/// Matches `quarto_error_reporting::MessageContent` for compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type", content = "content")]
pub enum MessageContent {
    /// Plain text content.
    Plain(String),
    /// Markdown content (may be parsed for rich formatting).
    Markdown(String),
}

impl MessageContent {
    /// Create plain text content.
    pub fn plain(text: impl Into<String>) -> Self {
        Self::Plain(text.into())
    }

    /// Create markdown content.
    pub fn markdown(text: impl Into<String>) -> Self {
        Self::Markdown(text.into())
    }

    /// Get the raw string content.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Plain(s) | Self::Markdown(s) => s,
        }
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        Self::Markdown(s)
    }
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        Self::Markdown(s.to_string())
    }
}

/// Convert from quarto-error-reporting MessageContent.
impl From<&quarto_error_reporting::MessageContent> for MessageContent {
    fn from(content: &quarto_error_reporting::MessageContent) -> Self {
        match content {
            quarto_error_reporting::MessageContent::Plain(s) => Self::Plain(s.clone()),
            quarto_error_reporting::MessageContent::Markdown(s) => Self::Markdown(s.clone()),
        }
    }
}

/// The kind of a diagnostic detail item.
///
/// Matches `quarto_error_reporting::DetailKind` for compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetailKind {
    /// Error detail (✖ bullet in tidyverse style).
    Error,
    /// Info detail (ℹ bullet in tidyverse style).
    Info,
    /// Note detail (• bullet in tidyverse style).
    Note,
}

impl From<quarto_error_reporting::DetailKind> for DetailKind {
    fn from(kind: quarto_error_reporting::DetailKind) -> Self {
        match kind {
            quarto_error_reporting::DetailKind::Error => Self::Error,
            quarto_error_reporting::DetailKind::Info => Self::Info,
            quarto_error_reporting::DetailKind::Note => Self::Note,
        }
    }
}

// ============================================================================
// Document Analysis Result
// ============================================================================

use quarto_source_map::SourceContext;

/// The result of analyzing a document.
///
/// This struct contains all intelligence data extracted from a single parse:
/// - Symbols for document outline and navigation
/// - Folding ranges for code folding
/// - Diagnostics for errors and warnings
/// - Source context for location mapping (internal use)
///
/// Using this struct is more efficient than calling separate functions,
/// as it requires only one parse of the document.
#[derive(Debug)]
pub struct DocumentAnalysis {
    /// Document symbols for outline/navigation.
    pub symbols: Vec<Symbol>,
    /// Folding ranges for code folding.
    pub folding_ranges: Vec<FoldingRange>,
    /// Diagnostics (errors and warnings).
    pub diagnostics: Vec<Diagnostic>,
    /// Source context for byte offset → line/column mapping.
    /// This is for internal use and is not serialized.
    pub source_context: SourceContext,
}

impl DocumentAnalysis {
    /// Create a new empty document analysis with the given source context.
    pub fn new(source_context: SourceContext) -> Self {
        Self {
            symbols: Vec::new(),
            folding_ranges: Vec::new(),
            diagnostics: Vec::new(),
            source_context,
        }
    }

    /// Create a document analysis with all fields populated.
    pub fn with_data(
        symbols: Vec<Symbol>,
        folding_ranges: Vec<FoldingRange>,
        diagnostics: Vec<Diagnostic>,
        source_context: SourceContext,
    ) -> Self {
        Self {
            symbols,
            folding_ranges,
            diagnostics,
            source_context,
        }
    }
}

/// A serializable version of DocumentAnalysis (without SourceContext).
///
/// This is used for JSON serialization to WASM/hub-client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentAnalysisJson {
    /// Document symbols for outline/navigation.
    pub symbols: Vec<Symbol>,
    /// Folding ranges for code folding.
    pub folding_ranges: Vec<FoldingRange>,
    /// Diagnostics (errors and warnings).
    pub diagnostics: Vec<Diagnostic>,
}

impl From<&DocumentAnalysis> for DocumentAnalysisJson {
    fn from(analysis: &DocumentAnalysis) -> Self {
        Self {
            symbols: analysis.symbols.clone(),
            folding_ranges: analysis.folding_ranges.clone(),
            diagnostics: analysis.diagnostics.clone(),
        }
    }
}

impl From<DocumentAnalysis> for DocumentAnalysisJson {
    fn from(analysis: DocumentAnalysis) -> Self {
        Self {
            symbols: analysis.symbols,
            folding_ranges: analysis.folding_ranges,
            diagnostics: analysis.diagnostics,
        }
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

    #[test]
    fn camel_case_serialization() {
        // Verify Symbol uses camelCase for selection_range
        let symbol = Symbol::new(
            "Test",
            SymbolKind::String,
            Range::new(Position::new(0, 0), Position::new(1, 0)),
            Range::new(Position::new(0, 0), Position::new(0, 4)),
        );
        let json = serde_json::to_string(&symbol).unwrap();
        assert!(
            json.contains("\"selectionRange\""),
            "Symbol should serialize selection_range as selectionRange"
        );
        assert!(
            !json.contains("\"selection_range\""),
            "Symbol should not use snake_case"
        );

        // Verify Diagnostic uses camelCase for details
        let diag = Diagnostic::new(
            Range::new(Position::new(0, 0), Position::new(0, 10)),
            DiagnosticSeverity::Error,
            "Test error",
        )
        .with_detail(DiagnosticDetail::with_range(
            DetailKind::Error,
            "Detail info",
            Range::new(Position::new(1, 0), Position::new(1, 5)),
        ));
        let json = serde_json::to_string(&diag).unwrap();
        // Check for camelCase field names
        assert!(
            json.contains("\"details\""),
            "Diagnostic should have details array"
        );
        // The title field is already lowercase, no change needed
        assert!(
            json.contains("\"title\""),
            "Diagnostic should have title field"
        );

        // Verify FoldingRange uses camelCase
        let folding_range = FoldingRange::with_kind(0, 10, FoldingRangeKind::Region);
        let json = serde_json::to_string(&folding_range).unwrap();
        assert!(
            json.contains("\"startLine\""),
            "FoldingRange should serialize start_line as startLine"
        );
        assert!(
            json.contains("\"endLine\""),
            "FoldingRange should serialize end_line as endLine"
        );
        assert!(
            !json.contains("\"start_line\""),
            "FoldingRange should not use snake_case"
        );
    }

    #[test]
    fn message_content_serialization() {
        // Verify MessageContent uses tagged format
        let plain = MessageContent::plain("hello");
        let json = serde_json::to_string(&plain).unwrap();
        assert_eq!(json, r#"{"type":"plain","content":"hello"}"#);

        let markdown = MessageContent::markdown("**bold**");
        let json = serde_json::to_string(&markdown).unwrap();
        assert_eq!(json, r#"{"type":"markdown","content":"**bold**"}"#);

        // Verify DetailKind serialization
        let kind = DetailKind::Error;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, r#""error""#);
    }
}
