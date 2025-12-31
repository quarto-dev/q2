//! Core diagnostic message types.
//!
//! This module defines the fundamental structures for representing diagnostic messages
//! (errors, warnings, info) following tidyverse-style guidelines.

use serde::{Deserialize, Serialize};

/// The kind of diagnostic message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticKind {
    /// An error that prevents completion
    Error,
    /// A warning that doesn't prevent completion but indicates a problem
    Warning,
    /// Informational message
    Info,
    /// A note providing additional context
    Note,
}

/// How detail items should be presented (tidyverse x/i bullet style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetailKind {
    /// Error detail (✖ bullet in tidyverse style)
    Error,
    /// Info detail (i bullet in tidyverse style)
    Info,
    /// Note detail (plain bullet)
    Note,
}

/// Options for rendering diagnostic messages to text.
///
/// This struct controls various aspects of text rendering, such as whether
/// to include terminal hyperlinks for clickable file paths.
#[derive(Debug, Clone)]
pub struct TextRenderOptions {
    /// Enable OSC 8 hyperlinks for clickable file paths in terminals.
    ///
    /// When enabled, file paths in error messages will include terminal
    /// escape codes for clickable links (supported by iTerm2, VS Code, etc.).
    /// Disable for snapshot testing to avoid absolute path differences.
    pub enable_hyperlinks: bool,
}

impl Default for TextRenderOptions {
    fn default() -> Self {
        Self {
            enable_hyperlinks: true,
        }
    }
}

/// The content of a message or detail item.
///
/// This will eventually support Pandoc AST for rich formatting, but starts
/// with simpler string-based content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageContent {
    /// Plain text content
    Plain(String),
    /// Markdown content (will be parsed to Pandoc AST in later phases)
    Markdown(String),
    // Future: PandocAst(Box<Inlines>)
}

impl MessageContent {
    /// Get the raw string content for display
    pub fn as_str(&self) -> &str {
        match self {
            MessageContent::Plain(s) => s,
            MessageContent::Markdown(s) => s,
        }
    }

    /// Convert to JSON value with type information
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            MessageContent::Plain(s) => json!({
                "type": "plain",
                "content": s
            }),
            MessageContent::Markdown(s) => json!({
                "type": "markdown",
                "content": s
            }),
        }
    }
}

impl From<String> for MessageContent {
    fn from(s: String) -> Self {
        MessageContent::Markdown(s)
    }
}

impl From<&str> for MessageContent {
    fn from(s: &str) -> Self {
        MessageContent::Markdown(s.to_string())
    }
}

/// A detail item in a diagnostic message.
///
/// Following tidyverse guidelines, details provide specific information about
/// the error (what went wrong, where, with what values).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetailItem {
    /// The kind of detail (error, info, note)
    pub kind: DetailKind,
    /// The content of the detail
    pub content: MessageContent,
    /// Optional source location for this detail
    ///
    /// When present, this identifies where in the source code this detail applies.
    /// This allows error messages to highlight multiple related locations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<quarto_source_map::SourceInfo>,
}

/// A diagnostic message following tidyverse-style structure.
///
/// Structure:
/// 1. **Code**: Optional error code (e.g., "Q-1-1") for searchability
/// 2. **Title**: Brief error message
/// 3. **Kind**: Error, Warning, Info
/// 4. **Problem**: What went wrong (the "must" or "can't" statement)
/// 5. **Details**: Specific information (bulleted, max 5 per tidyverse)
/// 6. **Hints**: Optional guidance for fixing (ends with ?)
///
/// # Example
///
/// ```ignore
/// let msg = DiagnosticMessage {
///     code: Some("Q-1-2".to_string()), // quarto-error-code-audit-ignore
///     title: "Incompatible types".to_string(),
///     kind: DiagnosticKind::Error,
///     problem: Some("Cannot combine date and datetime types".into()),
///     details: vec![
///         DetailItem {
///             kind: DetailKind::Error,
///             content: "`x`{.arg} has type `date`{.type}".into(),
///         },
///         DetailItem {
///             kind: DetailKind::Error,
///             content: "`y`{.arg} has type `datetime`{.type}".into(),
///         },
///     ],
///     hints: vec!["Convert both to the same type?".into()],
///     source_spans: vec![],
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticMessage {
    /// Optional error code (e.g., "Q-1-1")
    ///
    /// Error codes are optional but encouraged. They provide:
    /// - Searchability (users can Google "Q-1-1")
    /// - Stability (codes don't change even if message wording improves)
    /// - Documentation (each code maps to a detailed explanation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Brief title for the error
    pub title: String,

    /// The kind of diagnostic (Error, Warning, Info)
    pub kind: DiagnosticKind,

    /// The problem statement (the "what" - using "must" or "can't")
    pub problem: Option<MessageContent>,

    /// Specific error details (the "where/why" - max 5 per tidyverse)
    pub details: Vec<DetailItem>,

    /// Optional hints for fixing (ends with ?)
    pub hints: Vec<MessageContent>,

    /// Source location for this diagnostic
    ///
    /// When present, this identifies where in the source code the issue occurred.
    /// The location may track transformation history, allowing the error to be
    /// mapped back through multiple processing steps to the original source file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<quarto_source_map::SourceInfo>,
}

impl DiagnosticMessage {
    /// Access the diagnostic message builder API.
    ///
    /// This is the recommended way to create diagnostic messages, as the builder API
    /// encodes tidyverse-style guidelines and makes it easy to construct well-structured
    /// error messages.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
    ///
    /// let error = DiagnosticMessageBuilder::error("Incompatible types")
    ///     .with_code("Q-1-2") // quarto-error-code-audit-ignore
    ///     .problem("Cannot combine date and datetime types")
    ///     .add_detail("`x` has type `date`")
    ///     .add_detail("`y` has type `datetime`")
    ///     .add_hint("Convert both to the same type?")
    ///     .build();
    /// ```
    pub fn builder() -> crate::builder::DiagnosticMessageBuilder {
        // This is just a convenience for accessing the builder type
        // Users should call DiagnosticMessageBuilder::error() etc directly
        crate::builder::DiagnosticMessageBuilder::error("")
    }

    /// Create a new diagnostic message with just a title and kind.
    ///
    /// Note: Consider using `DiagnosticMessage::builder()` instead for better structure.
    pub fn new(kind: DiagnosticKind, title: impl Into<String>) -> Self {
        Self {
            code: None,
            title: title.into(),
            kind,
            problem: None,
            details: Vec::new(),
            hints: Vec::new(),
            location: None,
        }
    }

    /// Create an error diagnostic.
    ///
    /// Note: Consider using `DiagnosticMessage::builder().error()` instead for better structure.
    pub fn error(title: impl Into<String>) -> Self {
        Self::new(DiagnosticKind::Error, title)
    }

    /// Create a warning diagnostic.
    ///
    /// Note: Consider using `DiagnosticMessage::builder().warning()` instead for better structure.
    pub fn warning(title: impl Into<String>) -> Self {
        Self::new(DiagnosticKind::Warning, title)
    }

    /// Create an info diagnostic.
    ///
    /// Note: Consider using `DiagnosticMessage::builder().info()` instead for better structure.
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
    /// use quarto_error_reporting::DiagnosticMessage;
    ///
    /// let msg = DiagnosticMessage::error("YAML Syntax Error")
    ///     .with_code("Q-1-1");
    /// ```
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Get the documentation URL for this error, if it has an error code.
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessage;
    ///
    /// let msg = DiagnosticMessage::error("Internal Error")
    ///     .with_code("Q-0-1");
    ///
    /// assert!(msg.docs_url().is_some());
    /// ```
    pub fn docs_url(&self) -> Option<&str> {
        self.code
            .as_ref()
            .and_then(|code| crate::catalog::get_docs_url(code))
    }

    /// Render this diagnostic message as text following tidyverse style.
    ///
    /// This is a convenience method that uses default rendering options.
    /// For more control over rendering, use [`Self::to_text_with_options`].
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessageBuilder;
    ///
    /// let msg = DiagnosticMessageBuilder::error("Invalid input")
    ///     .problem("Values must be numeric")
    ///     .add_detail("Found text in column 3")
    ///     .add_hint("Convert to numbers first?")
    ///     .build();
    /// let text = msg.to_text(None);
    /// assert!(text.contains("Error: Invalid input"));
    /// assert!(text.contains("Values must be numeric"));
    /// ```
    pub fn to_text(&self, ctx: Option<&quarto_source_map::SourceContext>) -> String {
        self.to_text_with_options(ctx, &TextRenderOptions::default())
    }

    /// Render this diagnostic message as text following tidyverse style with custom options.
    ///
    /// Format:
    /// ```text
    /// Error: title
    /// Problem statement here
    /// ✖ Error detail 1
    /// ✖ Error detail 2
    /// ℹ Info detail
    /// • Note detail
    /// ? Hint 1
    /// ? Hint 2
    /// ```
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::{DiagnosticMessageBuilder, TextRenderOptions};
    ///
    /// let msg = DiagnosticMessageBuilder::error("Invalid input")
    ///     .problem("Values must be numeric")
    ///     .add_detail("Found text in column 3")
    ///     .add_hint("Convert to numbers first?")
    ///     .build();
    ///
    /// // Disable hyperlinks for snapshot testing
    /// let options = TextRenderOptions { enable_hyperlinks: false };
    /// let text = msg.to_text_with_options(None, &options);
    /// assert!(text.contains("Error: Invalid input"));
    /// ```
    pub fn to_text_with_options(
        &self,
        ctx: Option<&quarto_source_map::SourceContext>,
        options: &TextRenderOptions,
    ) -> String {
        use std::fmt::Write;

        let mut result = String::new();

        // Check if we have any location info that could be displayed with ariadne
        // This includes the main diagnostic location OR any detail with a location
        let has_any_location =
            self.location.is_some() || self.details.iter().any(|d| d.location.is_some());

        // If we have location info and source context, render ariadne source display
        let has_ariadne = if has_any_location && ctx.is_some() {
            // Use main location if available, otherwise use first detail location
            let location = self
                .location
                .as_ref()
                .or_else(|| self.details.iter().find_map(|d| d.location.as_ref()));

            if let Some(loc) = location {
                if let Some(ariadne_output) =
                    self.render_ariadne_source_context(loc, ctx.unwrap(), options.enable_hyperlinks)
                {
                    result.push_str(&ariadne_output);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // If we don't have ariadne output, show full tidyverse-style content
        // If we do have ariadne, only show details without locations and hints
        // (ariadne already shows: title, code, problem, and details with locations)
        if !has_ariadne {
            // No ariadne - show everything in tidyverse style

            // Title with kind prefix and error code (e.g., "Error [Q-1-1]: Invalid input")
            let kind_str = match self.kind {
                DiagnosticKind::Error => "Error",
                DiagnosticKind::Warning => "Warning",
                DiagnosticKind::Info => "Info",
                DiagnosticKind::Note => "Note",
            };
            if let Some(code) = &self.code {
                writeln!(result, "{} [{}]: {}", kind_str, code, self.title).unwrap();
            } else {
                writeln!(result, "{}: {}", kind_str, self.title).unwrap();
            }

            // Show location info if available (but no ariadne rendering)
            if let Some(loc) = &self.location {
                // Try to map with context if available
                if let Some(ctx) = ctx {
                    if let Some(mapped) = loc.map_offset(loc.start_offset(), ctx)
                        && let Some(file) = ctx.get_file(mapped.file_id) {
                            writeln!(
                                result,
                                "  at {}:{}:{}",
                                file.path,
                                mapped.location.row + 1,
                                mapped.location.column + 1
                            )
                            .unwrap();
                        }
                } else {
                    // No context: show immediate location (1-indexed for display)
                    // Note: Without context, we can't get row/column from offsets
                    // We could map_offset with ctx to get Location, but ctx is None here
                    writeln!(result, "  at offset {}", loc.start_offset()).unwrap();
                }
            }

            // Problem statement (optional additional context)
            if let Some(problem) = &self.problem {
                writeln!(result, "{}", problem.as_str()).unwrap();
            }

            // All details with appropriate bullets
            for detail in &self.details {
                let bullet = match detail.kind {
                    DetailKind::Error => "✖",
                    DetailKind::Info => "ℹ",
                    DetailKind::Note => "•",
                };
                writeln!(result, "{} {}", bullet, detail.content.as_str()).unwrap();
            }

            // All hints
            for hint in &self.hints {
                writeln!(result, "ℹ {}", hint.as_str()).unwrap();
            }
        } else {
            // Have ariadne - only show details without locations and hints
            // (ariadne shows title, code, problem, and located details)

            // Details without locations (ariadne can't show these)
            for detail in &self.details {
                if detail.location.is_none() {
                    let bullet = match detail.kind {
                        DetailKind::Error => "✖",
                        DetailKind::Info => "ℹ",
                        DetailKind::Note => "•",
                    };
                    writeln!(result, "{} {}", bullet, detail.content.as_str()).unwrap();
                }
            }

            // All hints (ariadne doesn't show hints)
            for hint in &self.hints {
                writeln!(result, "ℹ {}", hint.as_str()).unwrap();
            }
        }

        result
    }

    /// Render this diagnostic message as a JSON value.
    ///
    /// Returns a structured JSON object with all fields:
    /// ```json
    /// {
    ///   "kind": "error",
    ///   "title": "Invalid input",
    ///   "code": "Q-1-2", // quarto-error-code-audit-ignore
    ///   "problem": "Values must be numeric",
    ///   "details": [{"kind": "error", "content": "Found text in column 3"}],
    ///   "hints": ["Convert to numbers first?"]
    /// }
    /// ```
    ///
    /// # Example
    ///
    /// ```
    /// use quarto_error_reporting::DiagnosticMessage;
    ///
    /// let msg = DiagnosticMessage::error("Something went wrong");
    /// let json = msg.to_json();
    /// assert_eq!(json["kind"], "error");
    /// assert_eq!(json["title"], "Something went wrong");
    /// ```
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;

        let kind_str = match self.kind {
            DiagnosticKind::Error => "error",
            DiagnosticKind::Warning => "warning",
            DiagnosticKind::Info => "info",
            DiagnosticKind::Note => "note",
        };

        let mut obj = json!({
            "kind": kind_str,
            "title": self.title,
        });

        // Add optional fields
        if let Some(code) = &self.code {
            obj["code"] = json!(code);
        }

        if let Some(problem) = &self.problem {
            obj["problem"] = problem.to_json();
        }

        if !self.details.is_empty() {
            let details: Vec<_> = self
                .details
                .iter()
                .map(|d| {
                    let detail_kind = match d.kind {
                        DetailKind::Error => "error",
                        DetailKind::Info => "info",
                        DetailKind::Note => "note",
                    };
                    let mut detail_obj = json!({
                        "kind": detail_kind,
                        "content": d.content.to_json()
                    });
                    if let Some(location) = &d.location {
                        detail_obj["location"] = json!(location);
                    }
                    detail_obj
                })
                .collect();
            obj["details"] = json!(details);
        }

        if !self.hints.is_empty() {
            let hints: Vec<_> = self.hints.iter().map(|h| h.to_json()).collect();
            obj["hints"] = json!(hints);
        }

        if let Some(location) = &self.location {
            obj["location"] = json!(location); // quarto-source-map::SourceInfo is Serialize
        }

        obj
    }

    /// Extract the original file_id from a SourceInfo by traversing the mapping chain
    fn extract_file_id(
        source_info: &quarto_source_map::SourceInfo,
    ) -> Option<quarto_source_map::FileId> {
        match source_info {
            quarto_source_map::SourceInfo::Original { file_id, .. } => Some(*file_id),
            quarto_source_map::SourceInfo::Substring { parent, .. } => {
                Self::extract_file_id(parent)
            }
            quarto_source_map::SourceInfo::Concat { pieces } => {
                // For concatenated sources, use the first piece's file_id
                pieces
                    .first()
                    .and_then(|p| Self::extract_file_id(&p.source_info))
            }
            quarto_source_map::SourceInfo::FilterProvenance { .. } => {
                // Filter provenance doesn't have a traditional file_id
                None
            }
        }
    }

    /// Wrap a file path with OSC 8 ANSI hyperlink codes for clickable terminal links.
    ///
    /// OSC 8 is a terminal escape sequence that creates clickable hyperlinks:
    /// `\x1b]8;;URI\x1b\\TEXT\x1b\\`
    ///
    /// Only adds hyperlinks if:
    /// - Hyperlinks are enabled via the `enable_hyperlinks` parameter
    /// - The file exists on disk (not an ephemeral in-memory file)
    /// - The path can be converted to an absolute path
    ///
    /// The `url` crate handles:
    /// - Platform differences (Windows drive letters vs Unix paths)
    /// - Percent-encoding of special characters
    /// - Proper file:// URL construction
    ///
    /// Line and column numbers are added to the URL as a fragment identifier
    /// (e.g., `file:///path#line:column`), which is supported by iTerm2 3.4+
    /// and other terminal emulators for opening files at specific positions.
    ///
    /// Returns the wrapped path if conditions are met, otherwise returns the original path.
    #[cfg(not(target_family = "wasm"))]
    fn wrap_path_with_hyperlink(
        path: &str,
        has_disk_file: bool,
        line: Option<usize>,
        column: Option<usize>,
        enable_hyperlinks: bool,
    ) -> String {
        // Don't add hyperlinks if disabled (e.g., for snapshot testing)
        if !enable_hyperlinks {
            return path.to_string();
        }

        // Only add hyperlinks for real files on disk (not ephemeral in-memory files)
        if !has_disk_file {
            return path.to_string();
        }

        // Canonicalize to absolute path
        let abs_path = match std::fs::canonicalize(path) {
            Ok(p) => p,
            Err(_) => return path.to_string(), // Can't canonicalize, skip hyperlink
        };

        // Convert to file:// URL (handles Windows/Unix + percent-encoding)
        let mut file_url = match url::Url::from_file_path(&abs_path) {
            Ok(url) => url.as_str().to_string(),
            Err(_) => return path.to_string(), // Conversion failed, skip hyperlink
        };

        // Add line and column as fragment identifier (e.g., #line:column)
        // This format is supported by iTerm2 3.4+ semantic history
        if let Some(line_num) = line {
            if let Some(col_num) = column {
                file_url.push_str(&format!("#{}:{}", line_num, col_num));
            } else {
                file_url.push_str(&format!("#{}", line_num));
            }
        }

        // Wrap with OSC 8 codes: \x1b]8;;URI\x1b\\TEXT\x1b]8;;\x1b\\
        format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", file_url, path)
    }

    /// WASM version: hyperlinks don't make sense in WASM environments (no file system).
    /// Just return the path unmodified.
    #[cfg(target_family = "wasm")]
    fn wrap_path_with_hyperlink(
        path: &str,
        _has_disk_file: bool,
        _line: Option<usize>,
        _column: Option<usize>,
        _enable_hyperlinks: bool,
    ) -> String {
        path.to_string()
    }

    /// Render source context using ariadne (private helper for to_text).
    ///
    /// This produces the visual source code snippet with highlighting.
    /// The tidyverse-style problem/details/hints are added separately by to_text().
    fn render_ariadne_source_context(
        &self,
        main_location: &quarto_source_map::SourceInfo,
        ctx: &quarto_source_map::SourceContext,
        enable_hyperlinks: bool,
    ) -> Option<String> {
        use ariadne::{Color, Config, IndexType, Label, Report, ReportKind, Source};

        // Extract file_id from the source mapping by traversing the chain
        let file_id = Self::extract_file_id(main_location)?;

        let file = ctx.get_file(file_id)?;

        // Get file content: use stored content for ephemeral files, or read from disk
        let content = match &file.content {
            Some(c) => c.clone(), // Ephemeral file: use stored content
            None => {
                // Disk-backed file: read from disk
                std::fs::read_to_string(&file.path)
                    .unwrap_or_else(|e| panic!("Failed to read file '{}': {}", file.path, e))
            }
        };

        // Map the location offsets back to original file positions
        // map_offset expects relative offsets (0 = start of this SourceInfo's range)
        let start_mapped = main_location.map_offset(0, ctx)?;
        // For end offset, try the full length first. If that fails (e.g., when the span
        // extends past EOF), clamp to the last valid position. This handles edge cases
        // like errors pointing to EOF or diagnostics with off-by-one end offsets.
        let end_mapped = main_location
            .map_offset(main_location.length(), ctx)
            .or_else(|| {
                // Clamp: if length() fails, try length()-1, which should be the last valid byte
                if main_location.length() > 0 {
                    main_location.map_offset(main_location.length() - 1, ctx)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| start_mapped.clone());

        // Create display path with OSC 8 hyperlink for clickable file paths
        // Check if this path refers to a real file on disk (vs an ephemeral in-memory file)
        let is_disk_file = std::path::Path::new(&file.path).exists();
        // Line and column numbers are 1-indexed for display (start_mapped.location uses 0-indexed)
        let line = Some(start_mapped.location.row + 1);
        let column = Some(start_mapped.location.column + 1);
        let display_path = Self::wrap_path_with_hyperlink(
            &file.path,
            is_disk_file,
            line,
            column,
            enable_hyperlinks,
        );

        // Determine report kind and color
        let (report_kind, main_color) = match self.kind {
            DiagnosticKind::Error => (ReportKind::Error, Color::Red),
            DiagnosticKind::Warning => (ReportKind::Warning, Color::Yellow),
            DiagnosticKind::Info => (ReportKind::Advice, Color::Cyan),
            DiagnosticKind::Note => (ReportKind::Advice, Color::Blue),
        };

        // Build the report using the mapped offset for proper line:column display
        // IMPORTANT: Use IndexType::Byte because our offsets are byte offsets, not character offsets
        let mut report = Report::build(
            report_kind,
            display_path.clone(),
            start_mapped.location.offset,
        )
        .with_config(Config::default().with_index_type(IndexType::Byte));

        // Add title with error code
        if let Some(code) = &self.code {
            report = report.with_message(format!("[{}] {}", code, self.title));
        } else {
            report = report.with_message(&self.title);
        }

        // Add main location label using mapped offsets
        let main_span = start_mapped.location.offset..end_mapped.location.offset;
        let main_message = if let Some(problem) = &self.problem {
            problem.as_str()
        } else {
            &self.title
        };

        report = report.with_label(
            Label::new((display_path.clone(), main_span))
                .with_message(main_message)
                .with_color(main_color),
        );

        // Add detail locations as additional labels (only those with locations)
        for detail in &self.details {
            if let Some(detail_loc) = &detail.location {
                // Extract file_id from detail location
                let detail_file_id = match Self::extract_file_id(detail_loc) {
                    Some(fid) => fid,
                    None => continue, // Skip if we can't extract file_id
                };

                if detail_file_id == file_id {
                    // Map detail offsets to original file positions
                    // map_offset expects relative offsets (0 = start of SourceInfo's range)
                    if let (Some(detail_start), Some(detail_end)) = (
                        detail_loc.map_offset(0, ctx),
                        detail_loc.map_offset(detail_loc.length(), ctx),
                    ) {
                        let detail_span = detail_start.location.offset..detail_end.location.offset;
                        let detail_color = match detail.kind {
                            DetailKind::Error => Color::Red,
                            DetailKind::Info => Color::Cyan,
                            DetailKind::Note => Color::Blue,
                        };

                        report = report.with_label(
                            Label::new((display_path.clone(), detail_span))
                                .with_message(detail.content.as_str())
                                .with_color(detail_color),
                        );
                    }
                }
            }
        }

        // Render to string
        let report = report.finish();
        let mut output = Vec::new();
        report
            .write(
                (display_path.clone(), Source::from(content.as_str())),
                &mut output,
            )
            .ok()?;

        let output_str = String::from_utf8(output).ok()?;

        // Post-process to extend hyperlinks to include line:column numbers
        // Ariadne adds :line:column after our hyperlinked path, so we need to
        // move the hyperlink end marker to include those numbers
        if is_disk_file && enable_hyperlinks {
            Some(Self::extend_hyperlink_to_include_line_column(
                &output_str,
                &file.path,
            ))
        } else {
            Some(output_str)
        }
    }

    /// Extend OSC 8 hyperlinks to include the :line:column suffix that ariadne adds.
    ///
    /// Ariadne formats file references as `path:line:column`, but since we wrap the path
    /// with OSC 8 codes, the structure becomes: `[hyperlink:path]:line:column`
    /// We want: `[hyperlink:path:line:column]`
    ///
    /// This function finds patterns like `path]8;;\:line:column` and moves the hyperlink
    /// end marker to after the line:column part.
    fn extend_hyperlink_to_include_line_column(output: &str, original_path: &str) -> String {
        // Pattern: original_path followed by ]8;;\ then :numbers:numbers
        // We want to move the ]8;;\ to after the :numbers:numbers part
        let end_marker = "\x1b]8;;\x1b\\";
        let search_pattern = format!("{}{}", original_path, end_marker);

        let mut result = output.to_string();
        while let Some(pos) = result.find(&search_pattern) {
            let after_marker = pos + search_pattern.len();
            // Check if what follows is :line:column pattern
            if let Some(rest) = result.get(after_marker..) {
                // Match :digits:digits pattern
                if let Some(colon_end) = Self::find_line_column_end(rest) {
                    // Move the end marker to after the :line:column
                    let before = &result[..pos + original_path.len()];
                    let line_col = &rest[..colon_end];
                    let after = &rest[colon_end..];
                    result = format!("{}{}{}{}", before, line_col, end_marker, after);
                    continue;
                }
            }
            break;
        }
        result
    }

    /// Find the end position of a :line:column pattern at the start of the string.
    /// Returns None if the pattern doesn't match.
    fn find_line_column_end(s: &str) -> Option<usize> {
        let bytes = s.as_bytes();
        if bytes.is_empty() || bytes[0] != b':' {
            return None;
        }

        let mut pos = 1;
        // Read digits for line number
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos == 1 || pos >= bytes.len() || bytes[pos] != b':' {
            return None; // No digits or no second colon
        }

        pos += 1; // Skip second colon
        let col_start = pos;
        // Read digits for column number
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos == col_start {
            return None; // No digits for column
        }

        Some(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_kind() {
        assert_eq!(DiagnosticKind::Error, DiagnosticKind::Error);
        assert_ne!(DiagnosticKind::Error, DiagnosticKind::Warning);
    }

    #[test]
    fn test_message_content_from_str() {
        let content: MessageContent = "test".into();
        assert_eq!(content.as_str(), "test");
    }

    #[test]
    fn test_diagnostic_message_new() {
        let msg = DiagnosticMessage::new(DiagnosticKind::Error, "Test error");
        assert_eq!(msg.title, "Test error");
        assert_eq!(msg.kind, DiagnosticKind::Error);
        assert!(msg.code.is_none());
        assert!(msg.problem.is_none());
        assert!(msg.details.is_empty());
        assert!(msg.hints.is_empty());
    }

    #[test]
    fn test_diagnostic_message_constructors() {
        let error = DiagnosticMessage::error("Error");
        assert_eq!(error.kind, DiagnosticKind::Error);
        assert!(error.code.is_none());

        let warning = DiagnosticMessage::warning("Warning");
        assert_eq!(warning.kind, DiagnosticKind::Warning);

        let info = DiagnosticMessage::info("Info");
        assert_eq!(info.kind, DiagnosticKind::Info);
    }

    #[test]
    fn test_with_code() {
        let msg = DiagnosticMessage::error("Test error").with_code("Q-1-1");
        assert_eq!(msg.code, Some("Q-1-1".to_string()));
    }

    #[test]
    fn test_docs_url() {
        let msg = DiagnosticMessage::error("Internal Error").with_code("Q-0-1");
        assert!(msg.docs_url().is_some());
        assert!(msg.docs_url().unwrap().contains("Q-0-1"));
    }

    #[test]
    fn test_docs_url_without_code() {
        let msg = DiagnosticMessage::error("Test error");
        assert!(msg.docs_url().is_none());
    }

    #[test]
    fn test_docs_url_invalid_code() {
        let msg = DiagnosticMessage::error("Test error").with_code("Q-999-999"); // quarto-error-code-audit-ignore
        assert!(msg.docs_url().is_none());
    }

    #[test]
    fn test_to_text_simple_error() {
        let msg = DiagnosticMessage::error("Something went wrong");
        assert_eq!(msg.to_text(None), "Error: Something went wrong\n");
    }

    #[test]
    fn test_to_text_with_code() {
        let msg = DiagnosticMessage::error("Something went wrong").with_code("Q-1-1");
        assert_eq!(msg.to_text(None), "Error [Q-1-1]: Something went wrong\n");
    }

    #[test]
    fn test_to_text_full_message() {
        use crate::builder::DiagnosticMessageBuilder;

        let msg = DiagnosticMessageBuilder::error("Invalid input")
            .problem("Values must be numeric")
            .add_detail("Found text in column 3")
            .add_info("Columns should contain only numbers")
            .add_hint("Convert to numbers first?")
            .build();

        let text = msg.to_text(None);
        assert!(text.contains("Error: Invalid input"));
        assert!(text.contains("Values must be numeric"));
        assert!(text.contains("✖ Found text in column 3"));
        assert!(text.contains("ℹ Columns should contain only numbers"));
        assert!(text.contains("ℹ Convert to numbers first?"));
    }

    #[test]
    fn test_to_json_simple() {
        let msg = DiagnosticMessage::error("Something went wrong");
        let json = msg.to_json();

        assert_eq!(json["kind"], "error");
        assert_eq!(json["title"], "Something went wrong");
        assert!(json.get("code").is_none());
        assert!(json.get("problem").is_none());
    }

    #[test]
    fn test_to_json_with_code() {
        let msg = DiagnosticMessage::error("Something went wrong").with_code("Q-1-1");
        let json = msg.to_json();

        assert_eq!(json["kind"], "error");
        assert_eq!(json["title"], "Something went wrong");
        assert_eq!(json["code"], "Q-1-1");
    }

    #[test]
    fn test_to_json_full_message() {
        use crate::builder::DiagnosticMessageBuilder;

        let msg = DiagnosticMessageBuilder::error("Invalid input")
            .with_code("Q-1-2") // quarto-error-code-audit-ignore
            .problem("Values must be numeric")
            .add_detail("Found text in column 3")
            .add_info("Expected numbers")
            .add_hint("Convert to numbers first?")
            .build();

        let json = msg.to_json();
        assert_eq!(json["kind"], "error");
        assert_eq!(json["title"], "Invalid input");
        assert_eq!(json["code"], "Q-1-2"); // quarto-error-code-audit-ignore
        assert_eq!(json["problem"]["type"], "markdown");
        assert_eq!(json["problem"]["content"], "Values must be numeric");
        assert_eq!(json["details"][0]["kind"], "error");
        assert_eq!(json["details"][0]["content"]["type"], "markdown");
        assert_eq!(
            json["details"][0]["content"]["content"],
            "Found text in column 3"
        );
        assert_eq!(json["details"][1]["kind"], "info");
        assert_eq!(json["details"][1]["content"]["type"], "markdown");
        assert_eq!(json["details"][1]["content"]["content"], "Expected numbers");
        assert_eq!(json["hints"][0]["type"], "markdown");
        assert_eq!(json["hints"][0]["content"], "Convert to numbers first?");
    }

    #[test]
    fn test_to_json_warning() {
        let msg = DiagnosticMessage::warning("Be careful");
        let json = msg.to_json();

        assert_eq!(json["kind"], "warning");
        assert_eq!(json["title"], "Be careful");
    }

    #[test]
    fn test_location_in_to_text_without_context() {
        use crate::builder::DiagnosticMessageBuilder;

        // Create a location at offsets 100-110
        let location =
            quarto_source_map::SourceInfo::original(quarto_source_map::FileId(0), 100, 110);

        let msg = DiagnosticMessageBuilder::error("Invalid syntax")
            .with_location(location)
            .build();

        let text = msg.to_text(None);

        // Without context, should show offset (we can't get row/column without context)
        assert!(text.contains("Invalid syntax"));
        assert!(text.contains("at offset 100"));
    }

    #[test]
    fn test_location_in_to_text_with_context() {
        use crate::builder::DiagnosticMessageBuilder;

        // Create a source context with a file
        let mut ctx = quarto_source_map::SourceContext::new();
        let file_id = ctx.add_file(
            "test.qmd".to_string(),
            Some("line 1\nline 2\nline 3\nline 4".to_string()),
        );

        // Create a location in that file (offset 7 is start of "line 2")
        let location = quarto_source_map::SourceInfo::original(
            file_id, 7,  // Start of "line 2"
            13, // End of "line 2"
        );

        let msg = DiagnosticMessageBuilder::error("Invalid syntax")
            .with_location(location)
            .build();

        let text = msg.to_text(Some(&ctx));

        // With context, should show file path and 1-indexed location
        assert!(text.contains("Invalid syntax"));
        assert!(text.contains("test.qmd"));
        assert!(text.contains("2:1")); // row 1 + 1, column 0 + 1
    }

    #[test]
    fn test_location_in_to_json() {
        use crate::builder::DiagnosticMessageBuilder;

        let location =
            quarto_source_map::SourceInfo::original(quarto_source_map::FileId(0), 100, 110);

        let msg = DiagnosticMessageBuilder::error("Invalid syntax")
            .with_location(location)
            .build();

        let json = msg.to_json();

        // Should have location field with Original variant
        assert!(json.get("location").is_some());
        let loc = &json["location"];

        // Verify the SourceInfo is serialized correctly (as Original enum variant)
        assert!(loc.get("Original").is_some());
        let original = &loc["Original"];
        assert_eq!(original["file_id"], 0);
        assert_eq!(original["start_offset"], 100);
        assert_eq!(original["end_offset"], 110);
    }

    #[test]
    fn test_location_optional_in_to_json() {
        let msg = DiagnosticMessage::error("No location");
        let json = msg.to_json();

        // Should not have location field when not provided
        assert!(json.get("location").is_none());
    }

    #[test]
    fn test_text_render_options_disable_hyperlinks() {
        use crate::builder::DiagnosticMessageBuilder;

        let mut ctx = quarto_source_map::SourceContext::new();
        let file_id = ctx.add_file("test.qmd".to_string(), Some("test content".to_string()));

        let location = quarto_source_map::SourceInfo::original(file_id, 0, 4);

        let msg = DiagnosticMessageBuilder::error("Test error")
            .with_location(location)
            .build();

        // With hyperlinks enabled (default)
        let with_hyperlinks = msg.to_text(Some(&ctx));

        // With hyperlinks disabled
        let options = TextRenderOptions {
            enable_hyperlinks: false,
        };
        let without_hyperlinks = msg.to_text_with_options(Some(&ctx), &options);

        // When hyperlinks are disabled, output should be different
        // (specifically, no OSC 8 escape sequences)
        if with_hyperlinks.contains("\x1b]8;") {
            assert!(
                !without_hyperlinks.contains("\x1b]8;"),
                "Disabled hyperlinks should not contain OSC 8 codes"
            );
        }
    }

    #[test]
    fn test_text_render_options_default() {
        let options = TextRenderOptions::default();
        assert!(
            options.enable_hyperlinks,
            "Default should enable hyperlinks"
        );
    }

    #[test]
    fn test_render_with_custom_options() {
        use crate::builder::DiagnosticMessageBuilder;

        let msg = DiagnosticMessageBuilder::error("Test")
            .problem("Something went wrong")
            .add_detail("Detail 1")
            .add_hint("Try this")
            .build();

        let options = TextRenderOptions {
            enable_hyperlinks: false,
        };

        let text = msg.to_text_with_options(None, &options);

        // Should still render properly without hyperlinks
        assert!(text.contains("Error: Test"));
        assert!(text.contains("Something went wrong"));
        assert!(text.contains("Detail 1"));
        assert!(text.contains("Try this"));
    }
}
