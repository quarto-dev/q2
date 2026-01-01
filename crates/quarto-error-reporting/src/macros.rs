//! Macros for creating diagnostic messages.

#[cfg(test)]
mod tests {
    use crate::{DiagnosticKind, generic_error, generic_warning};

    #[test]
    fn test_generic_error_macro() {
        let error = generic_error!("Test error message");

        assert_eq!(error.kind, DiagnosticKind::Error);
        assert_eq!(error.code, Some("Q-0-99".to_string())); // quarto-error-code-audit-ignore
        assert!(error.title.contains("Test error message"));
        assert!(error.title.contains(file!()));
        // Line number is included but varies depending on where macro is called
        assert!(error.title.contains(':'));
    }

    #[test]
    fn test_generic_warning_macro() {
        let warning = generic_warning!("Test warning message");

        assert_eq!(warning.kind, DiagnosticKind::Warning);
        assert_eq!(warning.code, Some("Q-0-99".to_string())); // quarto-error-code-audit-ignore
        assert!(warning.title.contains("Test warning message"));
        assert!(warning.title.contains(file!()));
    }

    #[test]
    fn test_macro_with_format() {
        let value = 42;
        let error = generic_error!(format!("Invalid value: {}", value));

        assert!(error.title.contains("Invalid value: 42"));
    }

    #[test]
    fn test_macro_error_can_be_rendered() {
        let error = generic_error!("Render test");
        let text = error.to_text(None);

        assert!(text.contains("[Q-0-99]")); // quarto-error-code-audit-ignore
        assert!(text.contains("Render test"));
    }

    #[test]
    fn test_macro_warning_can_be_rendered() {
        let warning = generic_warning!("Warning test");
        let text = warning.to_text(None);

        assert!(text.contains("[Q-0-99]")); // quarto-error-code-audit-ignore
        assert!(text.contains("Warning test"));
    }
}

/// Create a generic error with automatic file and line information.
///
/// This macro is for migration purposes - it creates an error with code Q-0-99 (quarto-error-code-audit-ignore)
/// and automatically includes the file and line number where the error was created.
///
/// # Example
///
/// ```
/// use quarto_error_reporting::generic_error;
///
/// let error = generic_error!("Found unexpected attribute");
/// assert_eq!(error.code, Some("Q-0-99".to_string())); // quarto-error-code-audit-ignore
/// assert!(error.title.contains("Found unexpected attribute"));
/// assert!(error.title.contains(file!()));
/// ```
#[macro_export]
macro_rules! generic_error {
    ($message:expr) => {
        $crate::DiagnosticMessageBuilder::generic_error($message, file!(), line!())
    };
}

/// Create a generic warning with automatic file and line information.
///
/// This macro is for migration purposes - it creates a warning with code Q-0-99 (quarto-error-code-audit-ignore)
/// and automatically includes the file and line number where the warning was created.
///
/// # Example
///
/// ```
/// use quarto_error_reporting::generic_warning;
///
/// let warning = generic_warning!("Caption found without table");
/// assert_eq!(warning.code, Some("Q-0-99".to_string())); // quarto-error-code-audit-ignore
/// assert!(warning.title.contains("Caption found without table"));
/// assert!(warning.title.contains(file!()));
/// ```
#[macro_export]
macro_rules! generic_warning {
    ($message:expr) => {
        $crate::DiagnosticMessageBuilder::generic_warning($message, file!(), line!())
    };
}
