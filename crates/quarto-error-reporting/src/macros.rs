//! Macros for creating diagnostic messages.

/// Create a generic error with automatic file and line information.
///
/// This macro is for migration purposes - it creates an error with code Q-0-99
/// and automatically includes the file and line number where the error was created.
///
/// # Example
///
/// ```
/// use quarto_error_reporting::generic_error;
///
/// let error = generic_error!("Found unexpected attribute");
/// assert_eq!(error.code, Some("Q-0-99".to_string()));
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
/// This macro is for migration purposes - it creates a warning with code Q-0-99
/// and automatically includes the file and line number where the warning was created.
///
/// # Example
///
/// ```
/// use quarto_error_reporting::generic_warning;
///
/// let warning = generic_warning!("Caption found without table");
/// assert_eq!(warning.code, Some("Q-0-99".to_string()));
/// assert!(warning.title.contains("Caption found without table"));
/// assert!(warning.title.contains(file!()));
/// ```
#[macro_export]
macro_rules! generic_warning {
    ($message:expr) => {
        $crate::DiagnosticMessageBuilder::generic_warning($message, file!(), line!())
    };
}
