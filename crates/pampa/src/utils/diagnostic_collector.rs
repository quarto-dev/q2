///! DiagnosticCollector - collects DiagnosticMessage objects and renders them to text or JSON
use quarto_error_reporting::{DiagnosticKind, DiagnosticMessage};

/// Collector for diagnostic messages
#[derive(Debug)]
pub struct DiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DiagnosticCollector {
    /// Create a new diagnostic collector
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    /// Add a diagnostic message
    pub fn add(&mut self, diagnostic: DiagnosticMessage) {
        self.diagnostics.push(diagnostic);
    }

    /// Helper: Add an error message (uses generic_error! macro for file/line tracking)
    ///
    /// For migration from ErrorCollector. Creates a DiagnosticMessage with code Q-0-99. (quarto-error-code-audit-ignore)
    pub fn error(&mut self, message: impl Into<String>) {
        self.add(quarto_error_reporting::generic_error!(message.into()));
    }

    /// Helper: Add a warning message (uses generic_warning! macro for file/line tracking)
    ///
    /// For migration from ErrorCollector. Creates a DiagnosticMessage with code Q-0-99. (quarto-error-code-audit-ignore)
    pub fn warn(&mut self, message: impl Into<String>) {
        self.add(quarto_error_reporting::generic_warning!(message.into()));
    }

    /// Add an error message with source location
    ///
    /// Use this when you have source location information available.
    pub fn error_at(
        &mut self,
        message: impl Into<String>,
        location: quarto_source_map::SourceInfo,
    ) {
        let mut diagnostic = quarto_error_reporting::generic_error!(message.into());
        diagnostic.location = Some(location);
        self.add(diagnostic);
    }

    /// Add a warning message with source location
    ///
    /// Use this when you have source location information available.
    pub fn warn_at(&mut self, message: impl Into<String>, location: quarto_source_map::SourceInfo) {
        let mut diagnostic = quarto_error_reporting::generic_warning!(message.into());
        diagnostic.location = Some(location);
        self.add(diagnostic);
    }

    /// Check if any errors were collected (warnings don't count)
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Error)
    }

    /// Get a reference to the collected diagnostics
    pub fn diagnostics(&self) -> &[DiagnosticMessage] {
        &self.diagnostics
    }

    /// Render all diagnostics to text strings
    pub fn to_text(&self) -> Vec<String> {
        self.diagnostics.iter().map(|d| d.to_text(None)).collect()
    }

    /// Render all diagnostics to JSON strings
    pub fn to_json(&self) -> Vec<String> {
        self.diagnostics
            .iter()
            .map(|d| d.to_json().to_string())
            .collect()
    }

    /// Consume the collector and return the diagnostics
    pub fn into_diagnostics(mut self) -> Vec<DiagnosticMessage> {
        // Sort diagnostics by file position (start offset)
        self.diagnostics
            .sort_by_key(|diag| diag.location.as_ref().map_or(0, |loc| loc.start_offset()));
        self.diagnostics
    }
}

impl Default for DiagnosticCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_error_reporting::DiagnosticMessageBuilder;

    #[test]
    fn test_new_collector() {
        let collector = DiagnosticCollector::new();
        assert!(collector.diagnostics.is_empty());
        assert!(!collector.has_errors());
    }

    #[test]
    fn test_add_diagnostic() {
        let mut collector = DiagnosticCollector::new();
        let diag = DiagnosticMessageBuilder::error("Test error").build();
        collector.add(diag);

        assert_eq!(collector.diagnostics.len(), 1);
        assert!(collector.has_errors());
    }

    #[test]
    fn test_error_helper() {
        let mut collector = DiagnosticCollector::new();
        collector.error("Something went wrong");

        assert_eq!(collector.diagnostics.len(), 1);
        assert!(collector.has_errors());
        assert_eq!(collector.diagnostics[0].code, Some("Q-0-99".to_string())); // quarto-error-code-audit-ignore
    }

    #[test]
    fn test_warn_helper() {
        let mut collector = DiagnosticCollector::new();
        collector.warn("Be careful");

        assert_eq!(collector.diagnostics.len(), 1);
        assert!(!collector.has_errors()); // Warnings don't count as errors
        assert_eq!(collector.diagnostics[0].code, Some("Q-0-99".to_string())); // quarto-error-code-audit-ignore
    }

    #[test]
    fn test_to_text() {
        let mut collector = DiagnosticCollector::new();
        collector.error("Test error");
        collector.warn("Test warning");

        let messages = collector.to_text();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].contains("Error"));
        assert!(messages[0].contains("Test error"));
        assert!(messages[1].contains("Warning"));
        assert!(messages[1].contains("Test warning"));
    }

    #[test]
    fn test_to_json() {
        let mut collector = DiagnosticCollector::new();
        collector.error("Test error");

        let messages = collector.to_json();
        assert_eq!(messages.len(), 1);

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(parsed["kind"], "error");
        assert!(parsed["title"].as_str().unwrap().contains("Test error"));
    }

    #[test]
    fn test_can_render_both_formats() {
        let mut collector = DiagnosticCollector::new();
        collector.error("Test error");

        // Can render as both text and JSON without needing to decide at construction
        let text = collector.to_text();
        let json = collector.to_json();

        assert_eq!(text.len(), 1);
        assert_eq!(json.len(), 1);
        assert!(text[0].contains("Error"));
        assert!(json[0].contains("\"kind\""));
    }

    #[test]
    fn test_into_diagnostics() {
        let mut collector = DiagnosticCollector::new();
        collector.error("Test error");
        collector.warn("Test warning");

        let diagnostics = collector.into_diagnostics();
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Error);
        assert_eq!(diagnostics[1].kind, DiagnosticKind::Warning);
    }

    #[test]
    fn test_has_errors_with_only_warnings() {
        let mut collector = DiagnosticCollector::new();
        collector.warn("Warning 1");
        collector.warn("Warning 2");

        assert!(!collector.has_errors());
    }

    #[test]
    fn test_has_errors_with_errors() {
        let mut collector = DiagnosticCollector::new();
        collector.warn("Warning");
        collector.error("Error");

        assert!(collector.has_errors());
    }

    #[test]
    fn test_default() {
        let collector: DiagnosticCollector = Default::default();
        assert!(collector.diagnostics.is_empty());
        assert!(!collector.has_errors());
    }

    #[test]
    fn test_error_at() {
        let mut collector = DiagnosticCollector::new();
        let location =
            quarto_source_map::SourceInfo::original(quarto_source_map::FileId(1), 10, 20);
        collector.error_at("Error with location", location);

        assert_eq!(collector.diagnostics.len(), 1);
        assert!(collector.has_errors());
        assert!(collector.diagnostics[0].location.is_some());
    }

    #[test]
    fn test_warn_at() {
        let mut collector = DiagnosticCollector::new();
        let location =
            quarto_source_map::SourceInfo::original(quarto_source_map::FileId(1), 10, 20);
        collector.warn_at("Warning with location", location);

        assert_eq!(collector.diagnostics.len(), 1);
        assert!(!collector.has_errors()); // Warnings don't count as errors
        assert!(collector.diagnostics[0].location.is_some());
    }

    #[test]
    fn test_diagnostics_getter() {
        let mut collector = DiagnosticCollector::new();
        collector.error("Error 1");
        collector.error("Error 2");

        let diagnostics = collector.diagnostics();
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_into_diagnostics_sorts_by_position() {
        let mut collector = DiagnosticCollector::new();

        // Add diagnostics with different locations (out of order)
        let loc2 = quarto_source_map::SourceInfo::original(quarto_source_map::FileId(1), 100, 110);
        let loc1 = quarto_source_map::SourceInfo::original(quarto_source_map::FileId(1), 50, 60);

        collector.error_at("Second", loc2);
        collector.error_at("First", loc1);

        let diagnostics = collector.into_diagnostics();

        // Should be sorted by start offset
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].title.contains("First"));
        assert!(diagnostics[1].title.contains("Second"));
    }

    #[test]
    fn test_into_diagnostics_handles_no_location() {
        let mut collector = DiagnosticCollector::new();

        // Diagnostics without locations should sort to the start
        collector.error("No location");
        let loc = quarto_source_map::SourceInfo::original(quarto_source_map::FileId(1), 100, 110);
        collector.error_at("Has location", loc);

        let diagnostics = collector.into_diagnostics();
        assert_eq!(diagnostics.len(), 2);
        // The one without location (offset 0) should come first
        assert!(diagnostics[0].title.contains("No location"));
    }
}
