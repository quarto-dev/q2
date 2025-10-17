/// Source location information for errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    pub row: usize,
    pub column: usize,
}

impl SourceInfo {
    pub fn new(row: usize, column: usize) -> Self {
        Self { row, column }
    }
}

/// Trait for collecting errors and warnings during parsing/processing
pub trait ErrorCollector {
    /// Add a warning message (non-fatal)
    fn warn(&mut self, message: String, location: Option<&SourceInfo>);

    /// Add an error message (fatal)
    fn error(&mut self, message: String, location: Option<&SourceInfo>);

    /// Check if any errors were collected
    fn has_errors(&self) -> bool;

    /// Get a copy of collected messages (without consuming the collector)
    fn messages(&self) -> Vec<String>;

    /// Convert collected errors into final format (consumes the collector)
    fn into_messages(self) -> Vec<String>;
}

/// Text-based error collector that produces human-readable messages
#[derive(Debug, Default)]
pub struct TextErrorCollector {
    messages: Vec<String>,
    has_errors: bool,
}

impl TextErrorCollector {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            has_errors: false,
        }
    }
}

impl ErrorCollector for TextErrorCollector {
    fn warn(&mut self, message: String, location: Option<&SourceInfo>) {
        let formatted = if let Some(loc) = location {
            format!("Warning: {} at {}:{}", message, loc.row, loc.column)
        } else {
            format!("Warning: {}", message)
        };
        self.messages.push(formatted);
    }

    fn error(&mut self, message: String, location: Option<&SourceInfo>) {
        let formatted = if let Some(loc) = location {
            format!("Error: {} at {}:{}", message, loc.row, loc.column)
        } else {
            format!("Error: {}", message)
        };
        self.messages.push(formatted);
        self.has_errors = true;
    }

    fn has_errors(&self) -> bool {
        self.has_errors
    }

    fn messages(&self) -> Vec<String> {
        self.messages.clone()
    }

    fn into_messages(self) -> Vec<String> {
        self.messages
    }
}

/// JSON-based error collector that produces structured JSON messages
#[derive(Debug, Default)]
pub struct JsonErrorCollector {
    messages: Vec<String>,
    has_errors: bool,
}

impl JsonErrorCollector {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            has_errors: false,
        }
    }

    fn format_json_message(title: &str, message: String, location: Option<&SourceInfo>) -> String {
        use serde_json::json;

        let json_obj = if let Some(loc) = location {
            json!({
                "title": title,
                "message": message,
                "location": {
                    "row": loc.row,
                    "column": loc.column
                }
            })
        } else {
            json!({
                "title": title,
                "message": message
            })
        };

        json_obj.to_string()
    }
}

impl ErrorCollector for JsonErrorCollector {
    fn warn(&mut self, message: String, location: Option<&SourceInfo>) {
        let formatted = Self::format_json_message("Warning", message, location);
        self.messages.push(formatted);
    }

    fn error(&mut self, message: String, location: Option<&SourceInfo>) {
        let formatted = Self::format_json_message("Error", message, location);
        self.messages.push(formatted);
        self.has_errors = true;
    }

    fn has_errors(&self) -> bool {
        self.has_errors
    }

    fn messages(&self) -> Vec<String> {
        self.messages.clone()
    }

    fn into_messages(self) -> Vec<String> {
        self.messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_collector_warning_without_location() {
        let mut collector = TextErrorCollector::new();
        collector.warn("This is a warning".to_string(), None);

        assert!(!collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0], "Warning: This is a warning");
    }

    #[test]
    fn test_text_collector_warning_with_location() {
        let mut collector = TextErrorCollector::new();
        let location = SourceInfo::new(35, 1);
        collector.warn(
            "Caption found without a preceding table".to_string(),
            Some(&location),
        );

        assert!(!collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0],
            "Warning: Caption found without a preceding table at 35:1"
        );
    }

    #[test]
    fn test_text_collector_error_without_location() {
        let mut collector = TextErrorCollector::new();
        collector.error("This is an error".to_string(), None);

        assert!(collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0], "Error: This is an error");
    }

    #[test]
    fn test_text_collector_error_with_location() {
        let mut collector = TextErrorCollector::new();
        let location = SourceInfo::new(42, 10);
        collector.error("Found attr in postprocess".to_string(), Some(&location));

        assert!(collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0], "Error: Found attr in postprocess at 42:10");
    }

    #[test]
    fn test_text_collector_multiple_messages() {
        let mut collector = TextErrorCollector::new();
        let loc1 = SourceInfo::new(10, 5);
        let loc2 = SourceInfo::new(20, 15);

        collector.warn("First warning".to_string(), Some(&loc1));
        collector.error("First error".to_string(), Some(&loc2));
        collector.warn("Second warning".to_string(), None);

        assert!(collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0], "Warning: First warning at 10:5");
        assert_eq!(messages[1], "Error: First error at 20:15");
        assert_eq!(messages[2], "Warning: Second warning");
    }

    #[test]
    fn test_json_collector_warning_without_location() {
        let mut collector = JsonErrorCollector::new();
        collector.warn("This is a warning".to_string(), None);

        assert!(!collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 1);

        // Parse and verify JSON structure
        let parsed: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(parsed["title"], "Warning");
        assert_eq!(parsed["message"], "This is a warning");
        assert!(parsed.get("location").is_none());
    }

    #[test]
    fn test_json_collector_warning_with_location() {
        let mut collector = JsonErrorCollector::new();
        let location = SourceInfo::new(35, 1);
        collector.warn(
            "Caption found without a preceding table".to_string(),
            Some(&location),
        );

        assert!(!collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 1);

        // Parse and verify JSON structure
        let parsed: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(parsed["title"], "Warning");
        assert_eq!(parsed["message"], "Caption found without a preceding table");
        assert_eq!(parsed["location"]["row"], 35);
        assert_eq!(parsed["location"]["column"], 1);
    }

    #[test]
    fn test_json_collector_error_without_location() {
        let mut collector = JsonErrorCollector::new();
        collector.error("This is an error".to_string(), None);

        assert!(collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 1);

        // Parse and verify JSON structure
        let parsed: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(parsed["title"], "Error");
        assert_eq!(parsed["message"], "This is an error");
        assert!(parsed.get("location").is_none());
    }

    #[test]
    fn test_json_collector_error_with_location() {
        let mut collector = JsonErrorCollector::new();
        let location = SourceInfo::new(42, 10);
        collector.error("Found attr in postprocess".to_string(), Some(&location));

        assert!(collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 1);

        // Parse and verify JSON structure
        let parsed: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(parsed["title"], "Error");
        assert_eq!(parsed["message"], "Found attr in postprocess");
        assert_eq!(parsed["location"]["row"], 42);
        assert_eq!(parsed["location"]["column"], 10);
    }

    #[test]
    fn test_json_collector_multiple_messages() {
        let mut collector = JsonErrorCollector::new();
        let loc1 = SourceInfo::new(10, 5);
        let loc2 = SourceInfo::new(20, 15);

        collector.warn("First warning".to_string(), Some(&loc1));
        collector.error("First error".to_string(), Some(&loc2));
        collector.warn("Second warning".to_string(), None);

        assert!(collector.has_errors());
        let messages = collector.into_messages();
        assert_eq!(messages.len(), 3);

        // Verify each message is valid JSON
        let parsed1: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
        assert_eq!(parsed1["title"], "Warning");
        assert_eq!(parsed1["message"], "First warning");
        assert_eq!(parsed1["location"]["row"], 10);

        let parsed2: serde_json::Value = serde_json::from_str(&messages[1]).unwrap();
        assert_eq!(parsed2["title"], "Error");
        assert_eq!(parsed2["message"], "First error");
        assert_eq!(parsed2["location"]["row"], 20);

        let parsed3: serde_json::Value = serde_json::from_str(&messages[2]).unwrap();
        assert_eq!(parsed3["title"], "Warning");
        assert_eq!(parsed3["message"], "Second warning");
        assert!(parsed3.get("location").is_none());
    }

    #[test]
    fn test_empty_collector_has_no_errors() {
        let collector = TextErrorCollector::new();
        assert!(!collector.has_errors());

        let collector = JsonErrorCollector::new();
        assert!(!collector.has_errors());
    }

    #[test]
    fn test_collector_with_only_warnings_has_no_errors() {
        let mut collector = TextErrorCollector::new();
        collector.warn("Warning 1".to_string(), None);
        collector.warn("Warning 2".to_string(), None);
        assert!(!collector.has_errors());

        let mut collector = JsonErrorCollector::new();
        collector.warn("Warning 1".to_string(), None);
        collector.warn("Warning 2".to_string(), None);
        assert!(!collector.has_errors());
    }
}
