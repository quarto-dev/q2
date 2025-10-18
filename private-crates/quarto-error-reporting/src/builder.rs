//! Builder API for diagnostic messages.
//!
//! This module provides a builder pattern that encodes tidyverse-style error message
//! guidelines directly in the API, making it easy to construct well-structured error messages.

use crate::diagnostic::{DetailItem, DetailKind, DiagnosticKind, DiagnosticMessage, MessageContent};

/// Builder for creating diagnostic messages following tidyverse guidelines.
///
/// The builder API naturally encourages the tidyverse four-part error structure:
/// 1. **Title**: Brief error message (via `.error()`, `.warning()`, etc.)
/// 2. **Problem**: What went wrong - the "must" or "can't" statement (via `.problem()`)
/// 3. **Details**: Specific information - max 5 bulleted items (via `.add_detail()`, `.add_info()`)
/// 4. **Hints**: Optional guidance (via `.add_hint()`)
///
/// # Example
///
/// ```
/// use quarto_error_reporting::DiagnosticMessageBuilder;
///
/// let error = DiagnosticMessageBuilder::error("Incompatible types")
///     .with_code("Q-1-2")
///     .problem("Cannot combine date and datetime types")
///     .add_detail("`x`{.arg} has type `date`{.type}")
///     .add_detail("`y`{.arg} has type `datetime`{.type}")
///     .add_hint("Convert both to the same type?")
///     .build();
///
/// assert_eq!(error.title, "Incompatible types");
/// assert_eq!(error.code, Some("Q-1-2".to_string()));
/// assert!(error.problem.is_some());
/// assert_eq!(error.details.len(), 2);
/// assert_eq!(error.hints.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct DiagnosticMessageBuilder {
    /// The kind of diagnostic (Error, Warning, Info)
    kind: DiagnosticKind,

    /// Brief title for the error
    title: String,

    /// Optional error code (e.g., "Q-1-1")
    code: Option<String>,

    /// The problem statement (the "what")
    problem: Option<MessageContent>,

    /// Specific error details (the "where/why")
    details: Vec<DetailItem>,

    /// Optional hints for fixing
    hints: Vec<MessageContent>,

    // Future: Source spans for pointing to specific code locations
    // source_spans: Vec<SourceSpan>,
}

impl DiagnosticMessageBuilder {
    /// Create a new builder with the specified kind and title.
    ///
    /// Most code should use the convenience methods `.error()`, `.warning()`, or `.info()`
    /// instead of calling this directly.
    pub fn new(kind: DiagnosticKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            code: None,
            problem: None,
            details: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// Create an error diagnostic builder.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let error = DiagnosticMessageBuilder::error("YAML Syntax Error")
    ///     .build();
    /// ```
    pub fn error(title: impl Into<String>) -> Self {
        Self::new(DiagnosticKind::Error, title)
    }

    /// Create a warning diagnostic builder.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let warning = DiagnosticMessageBuilder::warning("Deprecated feature")
    ///     .build();
    /// ```
    pub fn warning(title: impl Into<String>) -> Self {
        Self::new(DiagnosticKind::Warning, title)
    }

    /// Create an info diagnostic builder.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let info = DiagnosticMessageBuilder::info("Processing complete")
    ///     .build();
    /// ```
    pub fn info(title: impl Into<String>) -> Self {
        Self::new(DiagnosticKind::Info, title)
    }

    /// Set the error code.
    ///
    /// Error codes follow the format `Q-<subsystem>-<number>` (e.g., "Q-1-1").
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let error = DiagnosticMessageBuilder::error("YAML Syntax Error")
    ///     .with_code("Q-1-1")
    ///     .build();
    ///
    /// assert_eq!(error.code, Some("Q-1-1".to_string()));
    /// ```
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Set the problem statement.
    ///
    /// Following tidyverse guidelines, the problem statement should:
    /// - Start with a general, concise statement
    /// - Use "must" for requirements or "can't" for impossibilities
    /// - Be specific about types/expectations
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let error = DiagnosticMessageBuilder::error("Invalid input")
    ///     .problem("`n` must be a numeric vector, not a character vector")
    ///     .build();
    /// ```
    pub fn problem(mut self, stmt: impl Into<MessageContent>) -> Self {
        self.problem = Some(stmt.into());
        self
    }

    /// Add an error detail (displayed with error/cross bullet).
    ///
    /// Error details provide specific information about what went wrong.
    /// Following tidyverse guidelines:
    /// - Keep sentences short and specific
    /// - Reveal location, name, or content of problematic input
    /// - Limit to 5 total details (error + info) to avoid overwhelming users
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let error = DiagnosticMessageBuilder::error("Incompatible lengths")
    ///     .add_detail("`x` has length 3")
    ///     .add_detail("`y` has length 5")
    ///     .build();
    ///
    /// assert_eq!(error.details.len(), 2);
    /// ```
    pub fn add_detail(mut self, detail: impl Into<MessageContent>) -> Self {
        self.details.push(DetailItem {
            kind: DetailKind::Error,
            content: detail.into(),
        });
        self
    }

    /// Add an info detail (displayed with info bullet).
    ///
    /// Info details provide additional context or explanatory information.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let error = DiagnosticMessageBuilder::error("Missing file")
    ///     .add_detail("Could not find `config.yaml`")
    ///     .add_info("Default configuration will be used")
    ///     .build();
    /// ```
    pub fn add_info(mut self, info: impl Into<MessageContent>) -> Self {
        self.details.push(DetailItem {
            kind: DetailKind::Info,
            content: info.into(),
        });
        self
    }

    /// Add a note detail (displayed with plain bullet).
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let error = DiagnosticMessageBuilder::error("Parse error")
    ///     .add_note("This is an experimental feature")
    ///     .build();
    /// ```
    pub fn add_note(mut self, note: impl Into<MessageContent>) -> Self {
        self.details.push(DetailItem {
            kind: DetailKind::Note,
            content: note.into(),
        });
        self
    }

    /// Add a hint for fixing the error.
    ///
    /// Following tidyverse guidelines, hints should:
    /// - Only be included when the problem source is clear and common
    /// - Provide straightforward fix suggestions
    /// - End with a question mark if suggesting action
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let error = DiagnosticMessageBuilder::error("Function not found")
    ///     .problem("Could not find function `summarise()`")
    ///     .add_hint("Did you mean `summarize()`?")
    ///     .build();
    ///
    /// assert_eq!(error.hints.len(), 1);
    /// ```
    pub fn add_hint(mut self, hint: impl Into<MessageContent>) -> Self {
        self.hints.push(hint.into());
        self
    }

    /// Build the diagnostic message.
    ///
    /// This consumes the builder and returns the constructed `DiagnosticMessage`.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let error = DiagnosticMessageBuilder::error("Parse error")
    ///     .problem("Invalid syntax")
    ///     .build();
    ///
    /// assert_eq!(error.title, "Parse error");
    /// ```
    pub fn build(self) -> DiagnosticMessage {
        DiagnosticMessage {
            code: self.code,
            title: self.title,
            kind: self.kind,
            problem: self.problem,
            details: self.details,
            hints: self.hints,
        }
    }

    /// Build with validation.
    ///
    /// This validates the message structure according to tidyverse guidelines:
    /// - Warns if there's no problem statement (recommended but not required)
    /// - Warns if there are more than 5 details (overwhelming for users)
    /// - Future: Could check that hints end with '?'
    ///
    /// Returns warnings as a Vec of strings. An empty Vec means validation passed.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let (error, warnings) = DiagnosticMessageBuilder::error("Test error")
    ///     .build_with_validation();
    ///
    /// // Warns because there's no problem statement
    /// assert!(!warnings.is_empty());
    /// ```
    pub fn build_with_validation(self) -> (DiagnosticMessage, Vec<String>) {
        let mut warnings = Vec::new();

        // Check for problem statement
        if self.problem.is_none() {
            warnings.push(
                "Error message missing problem statement. \
                Consider adding .problem() to explain what went wrong."
                    .to_string(),
            );
        }

        // Check detail count (tidyverse recommends max 5)
        if self.details.len() > 5 {
            warnings.push(format!(
                "Error message has {} details. Tidyverse guidelines recommend max 5 to avoid \
                overwhelming users.",
                self.details.len()
            ));
        }

        (self.build(), warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_error() {
        let msg = DiagnosticMessageBuilder::error("Test error").build();
        assert_eq!(msg.title, "Test error");
        assert_eq!(msg.kind, DiagnosticKind::Error);
    }

    #[test]
    fn test_builder_warning() {
        let msg = DiagnosticMessageBuilder::warning("Test warning").build();
        assert_eq!(msg.kind, DiagnosticKind::Warning);
    }

    #[test]
    fn test_builder_info() {
        let msg = DiagnosticMessageBuilder::info("Test info").build();
        assert_eq!(msg.kind, DiagnosticKind::Info);
    }

    #[test]
    fn test_builder_with_code() {
        let msg = DiagnosticMessageBuilder::error("Test")
            .with_code("Q-1-1")
            .build();
        assert_eq!(msg.code, Some("Q-1-1".to_string()));
    }

    #[test]
    fn test_builder_problem() {
        let msg = DiagnosticMessageBuilder::error("Test")
            .problem("Something went wrong")
            .build();
        assert!(msg.problem.is_some());
        assert_eq!(msg.problem.unwrap().as_str(), "Something went wrong");
    }

    #[test]
    fn test_builder_details() {
        let msg = DiagnosticMessageBuilder::error("Test")
            .add_detail("Detail 1")
            .add_info("Info 1")
            .add_note("Note 1")
            .build();

        assert_eq!(msg.details.len(), 3);
        assert_eq!(msg.details[0].kind, DetailKind::Error);
        assert_eq!(msg.details[1].kind, DetailKind::Info);
        assert_eq!(msg.details[2].kind, DetailKind::Note);
    }

    #[test]
    fn test_builder_hints() {
        let msg = DiagnosticMessageBuilder::error("Test")
            .add_hint("Did you mean X?")
            .add_hint("Try Y instead")
            .build();

        assert_eq!(msg.hints.len(), 2);
    }

    #[test]
    fn test_builder_complete_message() {
        let msg = DiagnosticMessageBuilder::error("Incompatible types")
            .with_code("Q-1-2")
            .problem("Cannot combine date and datetime types")
            .add_detail("`x` has type `date`")
            .add_detail("`y` has type `datetime`")
            .add_hint("Convert both to the same type?")
            .build();

        assert_eq!(msg.title, "Incompatible types");
        assert_eq!(msg.code, Some("Q-1-2".to_string()));
        assert!(msg.problem.is_some());
        assert_eq!(msg.details.len(), 2);
        assert_eq!(msg.hints.len(), 1);
    }

    #[test]
    fn test_builder_validation_no_problem() {
        let (msg, warnings) = DiagnosticMessageBuilder::error("Test").build_with_validation();

        assert_eq!(msg.title, "Test");
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("missing problem statement"));
    }

    #[test]
    fn test_builder_validation_too_many_details() {
        let (_msg, warnings) = DiagnosticMessageBuilder::error("Test")
            .problem("Something wrong")
            .add_detail("1")
            .add_detail("2")
            .add_detail("3")
            .add_detail("4")
            .add_detail("5")
            .add_detail("6")
            .build_with_validation();

        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("6 details"));
        assert!(warnings[0].contains("max 5"));
    }

    #[test]
    fn test_builder_validation_passes() {
        let (_msg, warnings) = DiagnosticMessageBuilder::error("Test")
            .problem("Something wrong")
            .add_detail("Detail")
            .build_with_validation();

        assert!(warnings.is_empty());
    }
}
