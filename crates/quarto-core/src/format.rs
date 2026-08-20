/*
 * format.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Output format types and resolution.
 */

//! Output format specification and resolution.
//!
//! Formats determine how documents are rendered. The format includes:
//! - The format identifier (html, pdf, docx, etc.)
//! - Whether to use the native Rust pipeline or Pandoc
//! - Format-specific options

use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;

use crate::extension::discover::parse_format_descriptor;

/// Format identifier enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatIdentifier {
    /// HTML output (native Rust pipeline)
    Html,
    /// PDF output (requires Pandoc + LaTeX)
    Pdf,
    /// Word document (requires Pandoc)
    Docx,
    /// EPUB (requires Pandoc)
    Epub,
    /// Typst (requires typst binary)
    Typst,
    /// RevealJS slides (native Rust pipeline)
    Revealjs,
    /// GitHub-flavored Markdown
    Gfm,
    /// CommonMark
    CommonMark,
}

impl FormatIdentifier {
    /// Get the format name as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            FormatIdentifier::Html => "html",
            FormatIdentifier::Pdf => "pdf",
            FormatIdentifier::Docx => "docx",
            FormatIdentifier::Epub => "epub",
            FormatIdentifier::Typst => "typst",
            FormatIdentifier::Revealjs => "revealjs",
            FormatIdentifier::Gfm => "gfm",
            FormatIdentifier::CommonMark => "commonmark",
        }
    }

    /// Check if this format uses the native Rust pipeline
    pub fn is_native(&self) -> bool {
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }

    /// Check if this is an HTML-based format
    pub fn is_html_based(&self) -> bool {
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }

    /// Check if this format produces multiple output files (e.g., HTML website chapters)
    pub fn is_multi_file(&self) -> bool {
        // HTML is multi-file in project context (each chapter gets a file)
        // PDF, DOCX, EPUB are single-file
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }
}

impl std::fmt::Display for FormatIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<&str> for FormatIdentifier {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "html" => Ok(FormatIdentifier::Html),
            "pdf" => Ok(FormatIdentifier::Pdf),
            "docx" => Ok(FormatIdentifier::Docx),
            "epub" => Ok(FormatIdentifier::Epub),
            "typst" => Ok(FormatIdentifier::Typst),
            "revealjs" => Ok(FormatIdentifier::Revealjs),
            "gfm" => Ok(FormatIdentifier::Gfm),
            "commonmark" => Ok(FormatIdentifier::CommonMark),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
}

/// Builtin pseudo-formats that map to a known base format and an
/// optional pipeline-kind selector.
///
/// `pipeline_kind` is the structured replacement for string-literal
/// `target_format` matches in pipeline-dispatching code. Per Plan 1's
/// §"Multi-plan contract: cleanup owed to Plan 7", `q2-preview` maps
/// to `Some("preview")` so `AstTransformsStage` (and the JS-side
/// data-source switch, eventually) can dispatch on a typed selector
/// instead of grepping on `target_format == "q2-preview"`.
///
/// Temporary bridge until the extensions system provides this
/// mapping. Each entry should become an extension when that system
/// lands.
fn builtin_pseudo_format(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    match name {
        // `q2-slides` is the preview pseudo-format for `format: revealjs`
        // (analogous to `q2-preview` for `html`). It uses the same AST/preview
        // pipeline_kind so `render_page_for_preview` returns AST JSON; the
        // reveal slide construction fires because `build_transform_pipeline`
        // treats `target_format == "q2-slides"` as revealjs (see
        // `is_revealjs_target`). The SPA renders the section AST with a reveal
        // shell. See claude-notes/plans/2026-06-08-revealjs-presentations.md
        // Phase 1P.
        "q2-slides" => Some(("html", Some("preview"))),
        "q2-debug" => Some(("html", None)),
        "q2-preview" => Some(("html", Some("preview"))),
        "q2-sandboxed-preview" => Some(("html", None)),
        _ => None,
    }
}

/// Whether a `target_format` string should build the reveal.js slide tree
/// (`RevealSlidesTransform` replacing the generic title-block + sectionize).
/// True for the native render format (`revealjs`) and its preview
/// pseudo-format (`q2-slides`).
pub fn is_revealjs_target(target_format: &str) -> bool {
    matches!(target_format, "revealjs" | "q2-slides")
}

/// The canonical Pandoc output format a Lua filter or shortcode should see as
/// its `FORMAT` global, given the pipeline's `target_format`.
///
/// The preview pipeline runs with a *pseudo-format* `target_format`
/// (`q2-preview`, `q2-slides`, …) so q2-core can branch on it for AST-vs-HTML
/// output and reveal-tree construction (see [`builtin_pseudo_format`] /
/// [`is_revealjs_target`]). But Lua extensions don't know about those
/// pseudo-formats: their `quarto.doc.is_format("html:js")` /
/// `is_format("revealjs")` checks (e.g. the built-in `video` shortcode) only
/// recognize real Pandoc formats. If we hand the pseudo-format straight to Lua,
/// those checks fail and format-gated shortcodes/filters silently degrade
/// (`{{< video >}}` collapsed to a plain link in preview — bd-5b21rbaq).
///
/// This maps each preview pseudo-format back to the real format it emulates so
/// preview Lua behaves identically to render:
/// - `q2-preview` / `q2-debug` / `q2-sandboxed-preview` → `html`
/// - `q2-slides` → `revealjs` (so `is_format("revealjs")` is true, matching
///   what [`is_revealjs_target`] already does for pipeline decisions; note this
///   intentionally differs from [`builtin_pseudo_format`], which reports the
///   *output writer* base `html`).
///
/// Any non-pseudo `target_format` (`html`, `revealjs`, `latex`, …) passes
/// through unchanged.
pub fn lua_format_for(target_format: &str) -> &str {
    match target_format {
        "q2-preview" | "q2-debug" | "q2-sandboxed-preview" => "html",
        "q2-slides" => "revealjs",
        other => other,
    }
}

/// Extract the chosen output format from a document's leading YAML
/// front-matter: the `format:` scalar, or the first key when `format:` is a
/// map (e.g. `format: {revealjs: {...}}`). Returns `None` when there is no
/// front matter or no `format:` key.
///
/// This is the lightweight, pre-pipeline peek used to resolve a per-document
/// format (e.g. a `format: revealjs` deck inside an otherwise-HTML project)
/// before the render context — and thus the transform pipeline / output
/// extension — is chosen. The CLI's single-input detection delegates here so
/// the two paths agree.
pub fn format_key_from_frontmatter(content: &str) -> Option<String> {
    let yaml = extract_yaml_frontmatter(content)?;
    let value: serde_yaml::Value = serde_yaml::from_str(&yaml).ok()?;
    match value.get("format")? {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Mapping(m) => m.keys().find_map(|k| k.as_str().map(str::to_string)),
        _ => None,
    }
}

/// Return the text of the leading YAML front-matter block (between the opening
/// `---` and the closing `---` / `...` line), if present.
pub fn extract_yaml_frontmatter(content: &str) -> Option<String> {
    let s = content.strip_prefix('\u{feff}').unwrap_or(content);
    let after_open = s
        .strip_prefix("---\n")
        .or_else(|| s.strip_prefix("---\r\n"))?;
    for terminator in ["\n---", "\n..."] {
        if let Some(idx) = after_open.find(terminator) {
            return Some(after_open[..idx].to_string());
        }
    }
    None
}

/// Extract the format key from a `format:` [`ConfigValue`]: the scalar string,
/// or the first key when it is a map (`format: {revealjs: {...}}`).
pub fn format_key_from_config_value(value: &ConfigValue) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    value
        .as_map_entries()
        .and_then(|entries| entries.first().map(|e| e.key.clone()))
}

/// Resolve a document's effective output format key by prefer-merging the
/// `format:` declarations, lowest precedence to highest:
///
/// ```text
///   project config   →   document front matter   →   `--to` override
/// ```
///
/// The `--to` override is modeled as a synthesized `format: !prefer <to>`
/// layer merged on top — uniform with a document that had written that
/// instruction itself — so an explicit `--to` wins over every in-document
/// declaration, while in its absence a per-file or project-level `format:` is
/// honored (the per-file one winning over the project default). Returns
/// `default` when no layer declares a format.
///
/// This runs *before* the render pipeline is built because the format key
/// selects both the transform pipeline (reveal vs. generic) and the output
/// file extension; the full format-specific config flattening still happens
/// later in `MetadataMergeStage`.
pub fn resolve_format_key(
    project_format: Option<&str>,
    document_format: Option<&str>,
    cli_to: Option<&str>,
    default: &str,
) -> String {
    use quarto_config::MergedConfig;
    use quarto_pandoc_types::{ConfigMapEntry, MergeOp};
    use quarto_source_map::{By, SourceInfo};

    // One `{format: !prefer <key>}` layer. All layers use `Prefer` (last
    // wins); ordering project → document → `--to` encodes the precedence.
    let format_layer = |key: &str| -> ConfigValue {
        let si = SourceInfo::generated(By::unknown());
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "format".to_string(),
                key_source: si.clone(),
                value: ConfigValue::new_string(key, si.clone()).with_merge_op(MergeOp::Prefer),
            }],
            si,
        )
    };

    let owned: Vec<ConfigValue> = [project_format, document_format, cli_to]
        .into_iter()
        .flatten()
        .map(format_layer)
        .collect();
    if owned.is_empty() {
        return default.to_string();
    }
    let layers: Vec<&ConfigValue> = owned.iter().collect();
    MergedConfig::new(layers)
        .materialize()
        .ok()
        .as_ref()
        .and_then(|merged| merged.get("format"))
        .and_then(format_key_from_config_value)
        .unwrap_or_else(|| default.to_string())
}

/// Map a FormatIdentifier to its output file extension.
fn output_extension_for(id: FormatIdentifier) -> String {
    match id {
        FormatIdentifier::Html => "html",
        FormatIdentifier::Pdf => "pdf",
        FormatIdentifier::Docx => "docx",
        FormatIdentifier::Epub => "epub",
        FormatIdentifier::Typst => "pdf",
        FormatIdentifier::Revealjs => "html",
        FormatIdentifier::Gfm => "md",
        FormatIdentifier::CommonMark => "md",
    }
    .to_string()
}

/// A complete format specification
#[derive(Debug, Clone)]
pub struct Format {
    /// Format identifier (the base format enum, e.g., Html, Pdf)
    pub identifier: FormatIdentifier,

    /// The original format string (e.g., "q2-slides", "acm-html", "html")
    pub target_format: String,

    /// Extension name, if this is an extension format (e.g., Some("acm") for "acm-html")
    pub extension_name: Option<String>,

    /// Human-readable display name (e.g., "HTML", "q2-slides")
    pub display_name: String,

    /// Output file extension (without leading dot)
    pub output_extension: String,

    /// Whether this format uses the native Rust pipeline
    pub native_pipeline: bool,

    /// Structured pipeline selector. `Some("preview")` for the
    /// `q2-preview` pseudo-format; `None` for everything else
    /// today. Pipeline-dispatching stages (e.g.
    /// `AstTransformsStage::run()`) read this instead of
    /// string-matching on `target_format`.
    pub pipeline_kind: Option<&'static str>,
}

impl Format {
    /// Create an HTML format
    pub fn html() -> Self {
        Self {
            identifier: FormatIdentifier::Html,
            target_format: "html".to_string(),
            extension_name: None,
            display_name: "HTML".to_string(),
            output_extension: "html".to_string(),
            native_pipeline: true,
            pipeline_kind: None,
        }
    }

    /// Create a PDF format
    pub fn pdf() -> Self {
        Self {
            identifier: FormatIdentifier::Pdf,
            target_format: "pdf".to_string(),
            extension_name: None,
            display_name: "PDF".to_string(),
            output_extension: "pdf".to_string(),
            native_pipeline: false,
            pipeline_kind: None,
        }
    }

    /// Create a DOCX format
    pub fn docx() -> Self {
        Self {
            identifier: FormatIdentifier::Docx,
            target_format: "docx".to_string(),
            extension_name: None,
            display_name: "DOCX".to_string(),
            output_extension: "docx".to_string(),
            native_pipeline: false,
            pipeline_kind: None,
        }
    }

    /// The canonical Pandoc output format a Lua filter or shortcode should see
    /// as its `FORMAT` global.
    ///
    /// Lua extensions branch on real Pandoc formats
    /// (`quarto.doc.is_format("html:js")`, `is_format("revealjs")`, …), not on
    /// q2's preview pseudo-formats or extension-style format strings. This
    /// resolves a `Format` to the base format it emulates:
    /// - reveal targets (`revealjs` render **and** `q2-slides` preview) →
    ///   `"revealjs"`, so `is_format("revealjs")` fires in preview too. The
    ///   bare [`identifier`](Self::identifier) would otherwise collapse
    ///   `q2-slides` to `Html` (its *output writer* is HTML), losing
    ///   reveal-ness — the reason a user filter in a reveal preview saw
    ///   `"html"` (bd-5b21rbaq).
    /// - everything else → the identifier base (`html`, `pdf`, …), which
    ///   already canonicalizes extension formats (`acm-pdf` → `pdf`) and the
    ///   HTML preview pseudo-formats (`q2-preview`/`q2-debug`/`q2-sandboxed-preview` → `html`).
    ///
    /// This is the [`Format`]-aware companion to the string-only
    /// [`lua_format_for`] (used where only a `target_format` string is in hand,
    /// e.g. the transform-pipeline builder); the two agree on the pseudo-format
    /// and base cases.
    pub fn lua_format(&self) -> &str {
        if is_revealjs_target(&self.target_format) {
            "revealjs"
        } else {
            self.identifier.as_str()
        }
    }

    /// Parse a format string into a Format.
    ///
    /// Accepts:
    /// - Known base formats: "html", "pdf", "docx", etc.
    /// - Extension-style formats: "acm-html", "my-journal-pdf"
    /// - Builtin pseudo-formats: "q2-slides", "q2-debug", "q2-preview"
    ///   (temporary, until extensions)
    ///
    /// Returns Err for unrecognized format strings.
    pub fn from_format_string(format_str: &str) -> Result<Self, String> {
        // 1. Try as a known base format directly
        if let Ok(identifier) = FormatIdentifier::try_from(format_str) {
            return Ok(Self {
                identifier,
                target_format: format_str.to_string(),
                extension_name: None,
                display_name: identifier.as_str().to_uppercase(),
                output_extension: output_extension_for(identifier),
                native_pipeline: identifier.is_native(),
                pipeline_kind: None,
            });
        }

        // 2. Try as extension-style: "acm-html" -> base "html", extension "acm"
        // Note: For pseudo-formats like "q2-slides", parse_format_descriptor returns
        // base_format="q2-slides" (no known suffix), so the try_from below fails
        // just like step 1. This is intentional — step 3 handles pseudo-formats.
        let desc = parse_format_descriptor(format_str);
        if let Ok(identifier) = FormatIdentifier::try_from(desc.base_format.as_str()) {
            return Ok(Self {
                identifier,
                target_format: format_str.to_string(),
                extension_name: desc.extension_name,
                display_name: format_str.to_string(),
                output_extension: output_extension_for(identifier),
                native_pipeline: identifier.is_native(),
                pipeline_kind: None,
            });
        }

        // 3. Try as a builtin pseudo-format: "q2-preview" -> base "html"
        // with `pipeline_kind = Some("preview")`. The (base, kind)
        // pair is the single source of truth; pipeline-dispatching
        // stages read `pipeline_kind` instead of string-matching.
        if let Some((base, pipeline_kind)) = builtin_pseudo_format(format_str) {
            let identifier = FormatIdentifier::try_from(base).unwrap();
            return Ok(Self {
                identifier,
                target_format: format_str.to_string(),
                extension_name: None,
                display_name: format_str.to_string(),
                output_extension: output_extension_for(identifier),
                native_pipeline: identifier.is_native(),
                pipeline_kind,
            });
        }

        // 4. Unknown format
        Err(format!("Unknown format: {}", format_str))
    }

    /// Check if this format is HTML-based
    pub fn is_html(&self) -> bool {
        self.identifier.is_html_based()
    }

    /// Check if this format produces multiple files
    pub fn is_multi_file(&self) -> bool {
        self.identifier.is_multi_file()
    }

    /// Get the output file path for an input file
    pub fn output_path(&self, input: &std::path::Path) -> PathBuf {
        let mut output = input.to_path_buf();
        output.set_extension(&self.output_extension);
        output
    }
}

impl Default for Format {
    fn default() -> Self {
        Self::html()
    }
}

/// Check if minimal HTML mode should be used, based on merged metadata.
///
/// This is the `ConfigValue`-based equivalent of `Format::use_minimal_html()`.
/// It reads `minimal` and `theme` from the fully merged document metadata
/// (`doc.ast.meta`) instead of from `Format.metadata`.
///
/// Returns true when:
/// - `minimal: true` is set
/// - `theme: none` is set
/// - `theme: pandoc` is set
///
/// When `minimal: true` is set, it takes precedence regardless of theme.
pub fn is_minimal_html(meta: &ConfigValue) -> bool {
    if let Some(true) = meta.get("minimal").and_then(|v| v.as_bool()) {
        return true;
    }

    // `as_plain_text` (not `as_str`): a bare `theme: none` / `theme: pandoc`
    // front-matter string is stored as `ConfigValueKind::PandocInlines`, for
    // which `as_str` returns `None`, so minimal mode was silently not applied.
    // A theme *list* or *map* still yields `None` (correct — not "none"/"pandoc").
    // (bd-y89ihf0i)
    if let Some(theme) = meta.get("theme").and_then(|v| v.as_plain_text())
        && (theme == "none" || theme == "pandoc")
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // === FormatIdentifier tests ===

    #[test]
    fn test_format_identifier_from_string() {
        assert_eq!(
            FormatIdentifier::try_from("html").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("HTML").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("pdf").unwrap(),
            FormatIdentifier::Pdf
        );
        assert!(FormatIdentifier::try_from("unknown").is_err());
    }

    #[test]
    fn test_format_identifier_from_string_all_formats() {
        assert_eq!(
            FormatIdentifier::try_from("html").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("pdf").unwrap(),
            FormatIdentifier::Pdf
        );
        assert_eq!(
            FormatIdentifier::try_from("docx").unwrap(),
            FormatIdentifier::Docx
        );
        assert_eq!(
            FormatIdentifier::try_from("epub").unwrap(),
            FormatIdentifier::Epub
        );
        assert_eq!(
            FormatIdentifier::try_from("typst").unwrap(),
            FormatIdentifier::Typst
        );
        assert_eq!(
            FormatIdentifier::try_from("revealjs").unwrap(),
            FormatIdentifier::Revealjs
        );
        assert_eq!(
            FormatIdentifier::try_from("gfm").unwrap(),
            FormatIdentifier::Gfm
        );
        assert_eq!(
            FormatIdentifier::try_from("commonmark").unwrap(),
            FormatIdentifier::CommonMark
        );
    }

    #[test]
    fn test_format_identifier_from_string_case_insensitive() {
        assert_eq!(
            FormatIdentifier::try_from("DOCX").unwrap(),
            FormatIdentifier::Docx
        );
        assert_eq!(
            FormatIdentifier::try_from("Epub").unwrap(),
            FormatIdentifier::Epub
        );
        assert_eq!(
            FormatIdentifier::try_from("TYPST").unwrap(),
            FormatIdentifier::Typst
        );
        assert_eq!(
            FormatIdentifier::try_from("RevealJS").unwrap(),
            FormatIdentifier::Revealjs
        );
        assert_eq!(
            FormatIdentifier::try_from("GFM").unwrap(),
            FormatIdentifier::Gfm
        );
        assert_eq!(
            FormatIdentifier::try_from("CommonMark").unwrap(),
            FormatIdentifier::CommonMark
        );
    }

    #[test]
    fn test_format_identifier_from_string_error() {
        let result = FormatIdentifier::try_from("invalid_format");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown format"));
        assert!(err.contains("invalid_format"));
    }

    #[test]
    fn test_format_identifier_as_str() {
        assert_eq!(FormatIdentifier::Html.as_str(), "html");
        assert_eq!(FormatIdentifier::Pdf.as_str(), "pdf");
        assert_eq!(FormatIdentifier::Docx.as_str(), "docx");
        assert_eq!(FormatIdentifier::Epub.as_str(), "epub");
        assert_eq!(FormatIdentifier::Typst.as_str(), "typst");
        assert_eq!(FormatIdentifier::Revealjs.as_str(), "revealjs");
        assert_eq!(FormatIdentifier::Gfm.as_str(), "gfm");
        assert_eq!(FormatIdentifier::CommonMark.as_str(), "commonmark");
    }

    #[test]
    fn test_format_identifier_properties() {
        assert!(FormatIdentifier::Html.is_native());
        assert!(!FormatIdentifier::Pdf.is_native());

        assert!(FormatIdentifier::Html.is_html_based());
        assert!(FormatIdentifier::Revealjs.is_html_based());
        assert!(!FormatIdentifier::Pdf.is_html_based());
    }

    #[test]
    fn test_format_identifier_is_native_all() {
        // Native formats
        assert!(FormatIdentifier::Html.is_native());
        assert!(FormatIdentifier::Revealjs.is_native());

        // Non-native formats
        assert!(!FormatIdentifier::Pdf.is_native());
        assert!(!FormatIdentifier::Docx.is_native());
        assert!(!FormatIdentifier::Epub.is_native());
        assert!(!FormatIdentifier::Typst.is_native());
        assert!(!FormatIdentifier::Gfm.is_native());
        assert!(!FormatIdentifier::CommonMark.is_native());
    }

    #[test]
    fn test_format_identifier_is_html_based_all() {
        // HTML-based formats
        assert!(FormatIdentifier::Html.is_html_based());
        assert!(FormatIdentifier::Revealjs.is_html_based());

        // Non-HTML formats
        assert!(!FormatIdentifier::Pdf.is_html_based());
        assert!(!FormatIdentifier::Docx.is_html_based());
        assert!(!FormatIdentifier::Epub.is_html_based());
        assert!(!FormatIdentifier::Typst.is_html_based());
        assert!(!FormatIdentifier::Gfm.is_html_based());
        assert!(!FormatIdentifier::CommonMark.is_html_based());
    }

    #[test]
    fn test_format_identifier_is_multi_file() {
        // Multi-file formats
        assert!(FormatIdentifier::Html.is_multi_file());
        assert!(FormatIdentifier::Revealjs.is_multi_file());

        // Single-file formats
        assert!(!FormatIdentifier::Pdf.is_multi_file());
        assert!(!FormatIdentifier::Docx.is_multi_file());
        assert!(!FormatIdentifier::Epub.is_multi_file());
        assert!(!FormatIdentifier::Typst.is_multi_file());
        assert!(!FormatIdentifier::Gfm.is_multi_file());
        assert!(!FormatIdentifier::CommonMark.is_multi_file());
    }

    #[test]
    fn test_format_identifier_display() {
        assert_eq!(format!("{}", FormatIdentifier::Html), "html");
        assert_eq!(format!("{}", FormatIdentifier::Pdf), "pdf");
    }

    #[test]
    fn test_format_identifier_clone_copy() {
        let original = FormatIdentifier::Html;
        let cloned = original;
        let copied = original; // Copy trait

        assert_eq!(original, cloned);
        assert_eq!(original, copied);
    }

    #[test]
    fn test_format_identifier_hash() {
        let mut set = HashSet::new();
        set.insert(FormatIdentifier::Html);
        set.insert(FormatIdentifier::Pdf);
        set.insert(FormatIdentifier::Html); // Duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&FormatIdentifier::Html));
        assert!(set.contains(&FormatIdentifier::Pdf));
    }

    // === Format tests ===

    #[test]
    fn test_format_html() {
        let format = Format::html();

        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.target_format, "html");
        assert!(format.extension_name.is_none());
        assert_eq!(format.extension_name, None);
        assert_eq!(format.display_name, "HTML");
        assert_eq!(format.output_extension, "html");
        assert!(format.native_pipeline);
    }

    #[test]
    fn test_format_pdf() {
        let format = Format::pdf();

        assert_eq!(format.identifier, FormatIdentifier::Pdf);
        assert_eq!(format.target_format, "pdf");
        assert!(format.extension_name.is_none());
        assert_eq!(format.extension_name, None);
        assert_eq!(format.display_name, "PDF");
        assert_eq!(format.output_extension, "pdf");
        assert!(!format.native_pipeline);
    }

    #[test]
    fn test_format_docx() {
        let format = Format::docx();

        assert_eq!(format.identifier, FormatIdentifier::Docx);
        assert_eq!(format.target_format, "docx");
        assert!(format.extension_name.is_none());
        assert_eq!(format.extension_name, None);
        assert_eq!(format.display_name, "DOCX");
        assert_eq!(format.output_extension, "docx");
        assert!(!format.native_pipeline);
    }

    #[test]
    fn test_format_is_html() {
        assert!(Format::html().is_html());
        assert!(!Format::pdf().is_html());
        assert!(!Format::docx().is_html());
    }

    #[test]
    fn test_format_is_multi_file() {
        assert!(Format::html().is_multi_file());
        assert!(!Format::pdf().is_multi_file());
        assert!(!Format::docx().is_multi_file());
    }

    #[test]
    fn test_format_output_path() {
        let format = Format::html();
        let input = std::path::Path::new("/path/to/document.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/document.html"));
    }

    #[test]
    fn test_format_output_path_pdf() {
        let format = Format::pdf();
        let input = std::path::Path::new("/path/to/document.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/document.pdf"));
    }

    #[test]
    fn test_format_output_path_docx() {
        let format = Format::docx();
        let input = std::path::Path::new("/path/to/report.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/report.docx"));
    }

    #[test]
    fn test_format_output_path_no_extension() {
        let format = Format::html();
        let input = std::path::Path::new("/path/to/README");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/README.html"));
    }

    #[test]
    fn test_format_default() {
        let format = Format::default();

        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.target_format, "html");
        assert!(format.extension_name.is_none());
        assert_eq!(format.extension_name, None);
        assert_eq!(format.display_name, "HTML");
        assert_eq!(format.output_extension, "html");
        assert!(format.native_pipeline);
    }

    #[test]
    fn test_format_clone() {
        let original = Format::html();
        let cloned = original.clone();

        assert_eq!(original.identifier, cloned.identifier);
        assert_eq!(original.target_format, cloned.target_format);
        assert_eq!(original.extension_name, cloned.extension_name);
        assert_eq!(original.display_name, cloned.display_name);
        assert_eq!(original.output_extension, cloned.output_extension);
        assert_eq!(original.native_pipeline, cloned.native_pipeline);
    }

    // === from_format_string tests ===

    #[test]
    fn test_from_format_string_known_base() {
        let f = Format::from_format_string("html").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "html");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.display_name, "HTML");
        assert_eq!(f.output_extension, "html");
    }

    #[test]
    fn test_from_format_string_extension() {
        let f = Format::from_format_string("acm-pdf").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Pdf);
        assert_eq!(f.target_format, "acm-pdf");
        assert_eq!(f.extension_name, Some("acm".to_string()));
        assert_eq!(f.output_extension, "pdf");
    }

    #[test]
    fn test_from_format_string_pseudo_format() {
        let f = Format::from_format_string("q2-slides").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "q2-slides");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.output_extension, "html");
        assert!(f.native_pipeline);
        // revealjs epic Phase 1P: q2-slides is now the preview
        // pseudo-format for `format: revealjs` — it uses the q2-preview
        // pipeline_kind (AST path) so the SPA renders the reveal-section
        // AST with a reveal shell. The reveal slide construction fires
        // because `is_revealjs_target("q2-slides")` is true.
        assert_eq!(f.pipeline_kind, Some("preview"));
    }

    #[test]
    fn test_from_format_string_q2_preview() {
        let f = Format::from_format_string("q2-preview").unwrap();
        // q2-preview is HTML-based; the pseudo-format mapping
        // gives it `identifier: Html` while preserving the original
        // string in `target_format`.
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "q2-preview");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.output_extension, "html");
        assert!(f.native_pipeline);
        // The structured selector pipeline-dispatching code reads.
        // `AstTransformsStage::run()` will branch on this in a
        // later commit (Plan 7 cleanup retires the temporary
        // `target_format == "q2-preview"` string match).
        assert_eq!(f.pipeline_kind, Some("preview"));
    }

    #[test]
    fn test_lua_format_for_maps_preview_pseudo_formats() {
        // HTML-emulating pseudo-formats resolve to `html` so
        // `is_format("html:js")` fires in preview (bd-5b21rbaq).
        assert_eq!(lua_format_for("q2-preview"), "html");
        assert_eq!(lua_format_for("q2-debug"), "html");
        assert_eq!(lua_format_for("q2-sandboxed-preview"), "html");
        // The reveal preview pseudo-format resolves to `revealjs` so
        // `is_format("revealjs")` fires — distinct from
        // `builtin_pseudo_format`, which reports the output-writer base `html`.
        assert_eq!(lua_format_for("q2-slides"), "revealjs");
    }

    #[test]
    fn test_lua_format_for_passes_through_real_formats() {
        for f in ["html", "revealjs", "latex", "pdf", "gfm", "typst", "docx"] {
            assert_eq!(lua_format_for(f), f, "real format {f} must pass through");
        }
    }

    #[test]
    fn test_format_lua_format_canonicalizes() {
        // Real formats: identifier base.
        assert_eq!(
            Format::from_format_string("html").unwrap().lua_format(),
            "html"
        );
        assert_eq!(
            Format::from_format_string("revealjs").unwrap().lua_format(),
            "revealjs"
        );
        // Extension formats canonicalize to their base (the bug `identifier`
        // already handled; `lua_format` must preserve it).
        assert_eq!(
            Format::from_format_string("acm-pdf").unwrap().lua_format(),
            "pdf"
        );
        // HTML preview pseudo-formats → html.
        assert_eq!(
            Format::from_format_string("q2-preview")
                .unwrap()
                .lua_format(),
            "html"
        );
        assert_eq!(
            Format::from_format_string("q2-debug").unwrap().lua_format(),
            "html"
        );
        // Reveal preview pseudo-format → revealjs (NOT its html output base) —
        // the parity fix for user filters in reveal preview.
        assert_eq!(
            Format::from_format_string("q2-slides")
                .unwrap()
                .lua_format(),
            "revealjs"
        );
    }

    /// `Format::lua_format` and the string-only `lua_format_for` must agree on
    /// the pseudo-format + base cases both handle, so the shortcode pipeline
    /// (string-only) and user-filter stage (Format-aware) don't drift.
    #[test]
    fn test_lua_format_helpers_agree_on_shared_cases() {
        for fmt in ["html", "revealjs", "q2-preview", "q2-slides", "q2-debug"] {
            let f = Format::from_format_string(fmt).unwrap();
            assert_eq!(
                f.lua_format(),
                lua_format_for(&f.target_format),
                "lua_format helpers disagree for {fmt}"
            );
        }
    }

    #[test]
    fn test_from_format_string_typst_extension() {
        let f = Format::from_format_string("typst").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Typst);
        assert_eq!(f.output_extension, "pdf");
    }

    #[test]
    fn test_from_format_string_revealjs() {
        let f = Format::from_format_string("revealjs").unwrap();
        assert_eq!(f.output_extension, "html");
    }

    #[test]
    fn test_from_format_string_gfm() {
        let f = Format::from_format_string("gfm").unwrap();
        assert_eq!(f.output_extension, "md");
    }

    #[test]
    fn test_from_format_string_unknown_errors() {
        assert!(Format::from_format_string("unknown").is_err());
        assert!(Format::from_format_string("htlm").is_err());
    }

    #[test]
    fn test_from_format_string_error_message() {
        let err = Format::from_format_string("htlm").unwrap_err();
        assert!(
            err.contains("htlm"),
            "error should include the bad format name"
        );
    }

    #[test]
    fn test_from_format_string_empty_string() {
        assert!(Format::from_format_string("").is_err());
    }

    #[test]
    fn test_from_format_string_hyphen_only() {
        assert!(Format::from_format_string("-").is_err());
    }

    #[test]
    fn test_from_format_string_trailing_hyphen() {
        assert!(Format::from_format_string("html-").is_err());
    }

    #[test]
    fn test_from_format_string_leading_hyphen() {
        assert!(Format::from_format_string("-html").is_err());
    }

    // === is_minimal_html tests ===

    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::SourceInfo;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn meta_with(entries: Vec<ConfigMapEntry>) -> ConfigValue {
        ConfigValue::new_map(entries, si())
    }

    fn entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: si(),
            value,
        }
    }

    #[test]
    fn test_is_minimal_html_default() {
        let meta = meta_with(vec![]);
        assert!(!is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_minimal_true() {
        let meta = meta_with(vec![entry("minimal", ConfigValue::new_bool(true, si()))]);
        assert!(is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_minimal_false() {
        let meta = meta_with(vec![entry("minimal", ConfigValue::new_bool(false, si()))]);
        assert!(!is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_theme_none() {
        let meta = meta_with(vec![entry("theme", ConfigValue::new_string("none", si()))]);
        assert!(is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_theme_pandoc() {
        let meta = meta_with(vec![entry(
            "theme",
            ConfigValue::new_string("pandoc", si()),
        )]);
        assert!(is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_theme_bootstrap() {
        let meta = meta_with(vec![entry("theme", ConfigValue::new_string("cosmo", si()))]);
        assert!(!is_minimal_html(&meta));
    }

    /// bd-y89ihf0i: a bare `theme: none` front-matter string is stored as
    /// `PandocInlines`, not `Scalar(String)`. With `as_str()` this resolved to
    /// `None`, so minimal mode was silently NOT applied; `as_plain_text` fixes
    /// it. (Surfaced by the `metadata-as-str` lint, not the original seed.)
    #[test]
    fn test_is_minimal_html_theme_none_as_inlines() {
        use quarto_pandoc_types::inline::{Inline, Str};
        let theme = ConfigValue::new_inlines(
            vec![Inline::Str(Str {
                text: "none".to_string(),
                source_info: si(),
            })],
            si(),
        );
        let meta = meta_with(vec![entry("theme", theme)]);
        assert!(is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_minimal_overrides_theme() {
        let meta = meta_with(vec![
            entry("minimal", ConfigValue::new_bool(true, si())),
            entry("theme", ConfigValue::new_string("cosmo", si())),
        ]);
        assert!(is_minimal_html(&meta));
    }
}
