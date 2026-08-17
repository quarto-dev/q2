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
            quarto_error_reporting::DetailKind::Note
            | quarto_error_reporting::DetailKind::Faded => Self::Note,
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

// ============================================================================
// Semantic tokens (Monaco DocumentSemanticTokensProvider)
// ============================================================================

/// One semantic token in the LSP/Monaco model: a single-line range plus a
/// type index into [`QMD_TOKEN_LEGEND`].
///
/// `line`/`character`/`length` are **absolute** (not delta-encoded) and in
/// UTF-16 code units on a single line — the delta encoding into Monaco's
/// 5-tuple `Uint32Array` happens client-side. A token never spans a newline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticToken {
    /// Zero-based line.
    pub line: u32,
    /// Zero-based start character (UTF-16 code units).
    pub character: u32,
    /// Token length in UTF-16 code units, on this one line.
    pub length: u32,
    /// Index into [`QMD_TOKEN_LEGEND`].
    pub token_type: u32,
    /// Token modifier bitset (unused — always 0).
    pub modifiers: u32,
}

/// Serializable envelope for the WASM boundary: a flat token list. The legend
/// is **not** shipped per-response (it is a compile-time constant on both
/// sides; see [`QMD_TOKEN_LEGEND`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticTokensJson {
    /// The document's semantic tokens, sorted and non-overlapping.
    pub tokens: Vec<SemanticToken>,
}

/// The ordered Monaco token-type legend — the contract the editor theme
/// targets. Index into this array is a token's `token_type`.
///
/// **Every entry carries a `qmd.` sentinel super-prefix** on top of its family
/// namespace (`qmd.markup.*` / `qmd.punctuation.*` / `qmd.attribute.*` for
/// structural, `qmd.code.*` for embedded). No other grammar emits anything
/// under `qmd.`, so a theme rule keyed on a legend entry can never prefix-match
/// a scope another language emits (Phase 7, Defence 1). The `.scm` capture
/// names stay unprefixed; [`capture_to_token_type`] adds the prefix.
///
/// The `qmd.code.*` group **must** mirror the `.hl-*` roots in
/// `resources/scss/html/templates/highlight-default.scss` (24 roots) so editor and
/// render colour the same captures — pinned by the `code_legend_covers_render_css`
/// test (Phase 7, Defence 3).
pub static QMD_TOKEN_LEGEND: &[&str] = &[
    // --- structural (qmd `highlights.scm`) ---
    "qmd.markup.heading",
    "qmd.markup.emphasis",
    "qmd.markup.strong",
    "qmd.markup.strikethrough",
    "qmd.markup.link.label",
    "qmd.markup.link.url",
    "qmd.markup.link.title",
    "qmd.markup.image.label",
    "qmd.markup.image.url",
    "qmd.markup.raw.inline",
    "qmd.markup.raw",
    "qmd.markup.raw.info",
    "qmd.markup.math",
    "qmd.markup.shortcode",
    "qmd.markup.list",
    "qmd.markup.quote",
    "qmd.markup.comment",
    "qmd.punctuation.special",
    "qmd.punctuation.special.image",
    "qmd.punctuation.bracket",
    "qmd.punctuation.delimiter.fence",
    "qmd.punctuation.delimiter.frontmatter",
    "qmd.attribute.specifier",
    // --- embedded code (the 24 `hl-*` roots in highlight-default.scss) ---
    "qmd.code.attribute",
    "qmd.code.boolean",
    "qmd.code.character",
    "qmd.code.comment",
    "qmd.code.constant",
    "qmd.code.constructor",
    "qmd.code.embedded",
    "qmd.code.error",
    "qmd.code.escape",
    "qmd.code.function",
    "qmd.code.keyword",
    "qmd.code.label",
    "qmd.code.markup",
    "qmd.code.module",
    "qmd.code.namespace",
    "qmd.code.number",
    "qmd.code.operator",
    "qmd.code.property",
    "qmd.code.punctuation",
    "qmd.code.special",
    "qmd.code.string",
    "qmd.code.tag",
    "qmd.code.type",
    "qmd.code.variable",
];

/// Translate a tree-sitter capture name to its [`QMD_TOKEN_LEGEND`] index.
///
/// The single point both capture families map through. Prepend the `qmd.`
/// sentinel (always) and `code.` (embedded only — zones 2/3), then match by
/// **longest dotted prefix** against the legend: try the full namespaced name,
/// then drop trailing `.component`s until a legend entry matches. Returns
/// `None` for an unrecognised capture (the caller skips it — never panics,
/// never emits a garbage index).
///
/// - `markup.heading.3` (structural) → `qmd.markup.heading` (level suffix drops).
/// - `function.builtin` (embedded) → `qmd.code.function`.
/// - `punctuation.bracket` (embedded) → `qmd.code.punctuation` — never collides
///   with the structural `qmd.punctuation.bracket` entry.
pub fn capture_to_token_type(capture: &str, embedded: bool) -> Option<u32> {
    let full = if embedded {
        format!("qmd.code.{capture}")
    } else {
        format!("qmd.{capture}")
    };
    let mut candidate = full.as_str();
    loop {
        if let Some(idx) = QMD_TOKEN_LEGEND.iter().position(|&e| e == candidate) {
            return Some(idx as u32);
        }
        let dot = candidate.rfind('.')?;
        candidate = &candidate[..dot];
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

    fn legend_name(idx: u32) -> &'static str {
        QMD_TOKEN_LEGEND[idx as usize]
    }

    #[test]
    fn translator_longest_prefix_collapses_suffixes() {
        // Structural: heading level suffix drops.
        assert_eq!(
            legend_name(capture_to_token_type("markup.heading.3", false).unwrap()),
            "qmd.markup.heading"
        );
        // Structural: exact match, no collapse.
        assert_eq!(
            legend_name(capture_to_token_type("markup.link.url", false).unwrap()),
            "qmd.markup.link.url"
        );
        // Embedded: function.builtin → code.function.
        assert_eq!(
            legend_name(capture_to_token_type("function.builtin", true).unwrap()),
            "qmd.code.function"
        );
        // Embedded: string.escape → code.string.
        assert_eq!(
            legend_name(capture_to_token_type("string.escape", true).unwrap()),
            "qmd.code.string"
        );
    }

    #[test]
    fn translator_keeps_structural_and_embedded_disjoint() {
        // A code-grammar `punctuation.bracket` lands in the code namespace,
        // never colliding with the structural `qmd.punctuation.bracket`.
        assert_eq!(
            legend_name(capture_to_token_type("punctuation.bracket", true).unwrap()),
            "qmd.code.punctuation"
        );
        assert_eq!(
            legend_name(capture_to_token_type("punctuation.bracket", false).unwrap()),
            "qmd.punctuation.bracket"
        );
    }

    #[test]
    fn translator_skips_unknown_captures() {
        assert_eq!(capture_to_token_type("totally.unknown", false), None);
        assert_eq!(capture_to_token_type("nonsense", true), None);
    }

    #[test]
    fn code_legend_covers_render_css() {
        // Parity-coverage invariant (Phase 7, Defence 3): the shared resolver
        // unifies *which* capture wins each byte, but editor and render use
        // different colour tables — render keys on `.hl-<root>` CSS classes,
        // the editor on the `code.*` legend. A root present in one but not the
        // other is a silent parity break, so lock them together: the set of
        // `code.<root>` legend roots must equal the set of `.hl-<root>` roots.
        //
        // The color rules live in the DEFAULT palette file — the
        // light/dark epic's phase B (bd-0pic6) split the palette out
        // of the structural `highlight.scss` so `highlight-style:`
        // can swap it. Every shipped palette styles the same class
        // vocabulary, so checking the default one suffices.
        const SCSS: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/scss/html/templates/highlight-default.scss"
        ));

        // CSS roots: the first hyphen-segment after `.hl-` in each selector.
        let mut css_roots: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let bytes = SCSS.as_bytes();
        let needle = b".hl-";
        let mut i = 0;
        while i + needle.len() <= bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                let mut j = i + needle.len();
                let start = j;
                while j < bytes.len() && (bytes[j].is_ascii_lowercase() || bytes[j] == b'-') {
                    j += 1;
                }
                if j > start {
                    let ident = &SCSS[start..j];
                    let root = ident.split('-').next().unwrap_or(ident);
                    if !root.is_empty() {
                        css_roots.insert(root.to_string());
                    }
                }
                i = j;
            } else {
                i += 1;
            }
        }

        // Legend code.* roots.
        let legend_roots: std::collections::BTreeSet<String> = QMD_TOKEN_LEGEND
            .iter()
            .filter_map(|e| e.strip_prefix("qmd.code."))
            .map(|s| s.to_string())
            .collect();

        assert_eq!(
            legend_roots,
            css_roots,
            "code.* legend roots and .hl-* CSS roots diverged.\n\
             only in legend: {:?}\nonly in CSS: {:?}",
            legend_roots.difference(&css_roots).collect::<Vec<_>>(),
            css_roots.difference(&legend_roots).collect::<Vec<_>>(),
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
