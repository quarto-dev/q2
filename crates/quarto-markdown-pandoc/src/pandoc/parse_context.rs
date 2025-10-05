/*
 * parse_context.rs
 * Copyright (c) 2025 Posit, PBC
 */

/// Context passed through the parsing pipeline to provide information
/// about the current parse operation (e.g., source filename).
#[derive(Debug, Clone)]
pub struct ParseContext {
    pub filename: Option<String>,
}

impl ParseContext {
    pub fn new(filename: Option<String>) -> Self {
        ParseContext { filename }
    }

    pub fn with_filename(filename: impl Into<String>) -> Self {
        ParseContext {
            filename: Some(filename.into()),
        }
    }

    pub fn anonymous() -> Self {
        ParseContext { filename: None }
    }
}
