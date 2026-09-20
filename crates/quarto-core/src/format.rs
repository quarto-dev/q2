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
    /// PowerPoint presentation (requires Pandoc)
    Pptx,
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
            FormatIdentifier::Pptx => "pptx",
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
            "pptx" => Ok(FormatIdentifier::Pptx),
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
        // Sandboxed-preview port (bd-jgpz4hfq): same preview pipeline as
        // q2-preview — the sandboxed renderer needs the post-pipeline AST
        // (highlight spans, chrome metadata, theme fingerprint).
        "q2-sandboxed-preview" => Some(("html", Some("preview"))),
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

/// The pipeline-composition axis: which family of transforms
/// (HTML-scaffolding, revealjs-scaffolding, or none) a render runs, and
/// whether it's the full render or the render/preview kind.
///
/// This is the single derivation point replacing two previously-independent
/// ad-hoc checks: the inline `is_revealjs` family check in
/// `build_transform_pipeline`, and the `pipeline_kind: Option<&'static str>`
/// kind check in `AstTransformsStage::run()`. `RevealjsRender` is the native
/// `revealjs` render; `RevealjsPreview` is `q2-slides` — the two must stay
/// distinct cells (not collapsed into one "reveal" variant) because their
/// surviving transform lists differ once a preview exclude-list applies.
/// `Pandoc(fmt)` carries the raw `target_format` string (e.g. `"docx"`,
/// `"pptx"`) since Pandoc has no bounded enum of destination formats here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineProfile {
    /// Native HTML render (`html`, extension-style HTML formats, `q2-debug`).
    HtmlRender,
    /// HTML preview (`q2-preview`, `q2-sandboxed-preview`).
    HtmlPreview,
    /// Native revealjs render (`revealjs`).
    RevealjsRender,
    /// Revealjs preview (`q2-slides`).
    RevealjsPreview,
    /// A Pandoc-writer output format, carrying its raw format string
    /// (e.g. `"docx"`, `"pptx"`, `"gfm"`).
    Pandoc(String),
}

impl PipelineProfile {
    /// Derive the pipeline profile from a `target_format` string.
    ///
    /// Reproduces [`is_revealjs_target`] and [`builtin_pseudo_format`]
    /// exactly, deriving family from the raw string (never from a resolved
    /// [`Format::identifier`]) — `q2-slides`'s *output writer* base is
    /// `"html"`, so deriving family from the identifier would misclassify it
    /// as `HtmlPreview`, losing its reveal-ness.
    pub fn from_format(target_format: &str) -> PipelineProfile {
        // Reveal family is resolved by the string itself, not by any
        // downstream identifier: covers "revealjs" (Render) and
        // "q2-slides" (Preview).
        if is_revealjs_target(target_format) {
            return if target_format == "q2-slides" {
                PipelineProfile::RevealjsPreview
            } else {
                PipelineProfile::RevealjsRender
            };
        }

        // Builtin pseudo-formats resolve to an HTML base with an optional
        // "preview" pipeline_kind (q2-preview, q2-debug, q2-sandboxed-preview).
        if let Some((_, pipeline_kind)) = builtin_pseudo_format(target_format) {
            return if pipeline_kind == Some("preview") {
                PipelineProfile::HtmlPreview
            } else {
                PipelineProfile::HtmlRender
            };
        }

        // Known base format, e.g. "html", "docx", "pptx", "gfm".
        if let Ok(identifier) = FormatIdentifier::try_from(target_format) {
            return match identifier {
                FormatIdentifier::Html => PipelineProfile::HtmlRender,
                FormatIdentifier::Revealjs => PipelineProfile::RevealjsRender,
                _ => PipelineProfile::Pandoc(target_format.to_string()),
            };
        }

        // Extension-style formats, e.g. "acm-html" -> base "html".
        let desc = parse_format_descriptor(target_format);
        if let Ok(identifier) = FormatIdentifier::try_from(desc.base_format.as_str()) {
            return match identifier {
                FormatIdentifier::Html => PipelineProfile::HtmlRender,
                FormatIdentifier::Revealjs => PipelineProfile::RevealjsRender,
                _ => PipelineProfile::Pandoc(desc.base_format),
            };
        }

        // Unknown format string: treat as a Pandoc writer name verbatim.
        // `Format::from_format_string` is the authority on whether the
        // string actually resolves; this function is total.
        PipelineProfile::Pandoc(target_format.to_string())
    }
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

/// Extract every key a document's leading YAML front-matter `format:`
/// declares, in declaration order: the scalar wrapped as a single-element
/// vec, or every key when it is a map (e.g. `format: {docx: default, html:
/// default}` → `["docx", "html"]`). Returns an empty vec when there is no
/// front matter or no `format:` key.
///
/// The counterpart to [`format_key_from_frontmatter`], which returns only
/// the single key that reduction picks; this returns the full declaration so
/// [`multi_format_diagnostics`] can name what else was skipped.
pub fn format_keys_from_frontmatter(content: &str) -> Vec<String> {
    let Some(yaml) = extract_yaml_frontmatter(content) else {
        return Vec::new();
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&yaml) else {
        return Vec::new();
    };
    match value.get("format") {
        Some(serde_yaml::Value::String(s)) => vec![s.clone()],
        Some(serde_yaml::Value::Mapping(m)) => m
            .keys()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Warn when a document's `format:` declares more than one key but only one
/// is actually rendered — a signal `render.rs`'s blanket non-native-format
/// refusal used to provide as a side effect before P7-foundation relaxed it
/// to admit `Docx`/`Pptx` (design doc §14). Returns an empty vec when
/// `all_keys` has zero or one entries, or when every declared key besides
/// `used_key` has already been accounted for.
///
/// Modeled on [`crate::project::project_kind_diagnostics`]'s shape: a pure
/// function over already-resolved data, returning `Vec<DiagnosticMessage>`
/// for the caller to print.
pub fn multi_format_diagnostics(
    all_keys: &[String],
    used_key: &str,
) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    use quarto_error_reporting::DiagnosticMessageBuilder;

    if all_keys.len() <= 1 {
        return Vec::new();
    }
    let skipped: Vec<&str> = all_keys
        .iter()
        .map(String::as_str)
        .filter(|k| *k != used_key)
        .collect();
    if skipped.is_empty() {
        return Vec::new();
    }
    vec![
        DiagnosticMessageBuilder::warning(
            "`format:` declares more than one format; only one is rendered",
        )
        .with_code("Q-20-8")
        .problem(format!(
            "rendered `{used_key}`; skipped `{}`.",
            skipped.join("`, `")
        ))
        .build(),
    ]
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
        FormatIdentifier::Pptx => "pptx",
        FormatIdentifier::Epub => "epub",
        FormatIdentifier::Typst => "pdf",
        FormatIdentifier::Revealjs => "html",
        FormatIdentifier::Gfm => "md",
        FormatIdentifier::CommonMark => "md",
    }
    .to_string()
}

/// The pandoc **writer** name to pass as `-t` for a `PipelineProfile::Pandoc`
/// render — distinct from [`output_extension_for`], which names the file
/// extension of the *final* user-facing artifact.
///
/// For every format currently routed through `PandocWriteStage` except
/// typst, the two coincide (`docx` writes `.docx`, `pptx` writes `.pptx`,
/// …), which is why nothing needed this distinction before. Typst breaks
/// that: pandoc's typst *writer* is invoked with `-t typst`, but the
/// user-facing output is a compiled PDF (`output_extension_for` correctly
/// says `"pdf"`) — there is no direct `-t pdf` path through pandoc's typst
/// writer, and passing the final extension here would skip the writer, the
/// vendored Lua filters, and the template entirely. Compiling the `.typ`
/// pandoc produces into that PDF is `TypstCompileStage`'s job (pandoc-hybrid
/// Phase 2), not this stage's.
fn pandoc_writer_name_for(id: FormatIdentifier) -> String {
    match id {
        FormatIdentifier::Typst => "typst".to_string(),
        other => output_extension_for(other),
    }
}

/// Extra pandoc CLI flags `PandocWriteStage` should append for a
/// `PipelineProfile::Pandoc` render, beyond the shared `-f json -t
/// <writer> -o <output>` invocation every format gets.
///
/// Only typst needs any (`format-typst.ts`'s `pandoc.standalone = true`,
/// `wrap: none`, `default-image-extension: svg`) — every other
/// Pandoc-hybrid format keeps the pre-existing bare invocation unchanged.
/// `citeproc: false` from the same registration needs no entry here: Q2
/// never passes `--citeproc` to pandoc for *any* Pandoc-hybrid format
/// (citations are resolved upstream of this stage), so "false" is the
/// already-existing default, not a flag to add. The opt-in `-citations`
/// variant (Q1 auto-adds a target-format variant when the user asks for
/// pandoc's native citeproc) is deferred — there is no existing
/// pseudo-format-variant seam to model it on (see
/// `builtin_pseudo_format` above, which only maps whole-format aliases,
/// not opt-in suffixes on a base format), so it needs its own design
/// decision rather than a guess here.
fn pandoc_invocation_args_for(id: FormatIdentifier) -> Vec<String> {
    match id {
        FormatIdentifier::Typst => vec![
            "--standalone".to_string(),
            "--wrap".to_string(),
            "none".to_string(),
            "--default-image-extension".to_string(),
            "svg".to_string(),
        ],
        _ => Vec::new(),
    }
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

    /// The pandoc writer name `PandocWriteStage` should pass as `-t` for a
    /// `PipelineProfile::Pandoc` render. See [`pandoc_writer_name_for`] for
    /// why this differs from [`Self::output_extension`] for typst.
    pub fn pandoc_writer_name(&self) -> String {
        pandoc_writer_name_for(self.identifier)
    }

    /// Extra pandoc CLI flags this format needs beyond the shared
    /// invocation. See [`pandoc_invocation_args_for`] for why only typst
    /// needs any today.
    pub fn pandoc_invocation_args(&self) -> Vec<String> {
        pandoc_invocation_args_for(self.identifier)
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
        // T2.6 (P7-foundation Task 2): `Pptx` must read as non-HTML too.
        // `is_html_based()` and `is_native()` return the same value for
        // every variant that existed before P1 added `Pptx`, so a future
        // regression that routes this gate through `is_native()` instead
        // would be invisible to any test that only checks outcomes —
        // this pins the predicate's own table.
        assert!(!FormatIdentifier::Pptx.is_html_based());
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
    fn test_from_format_string_q2_sandboxed_preview() {
        let f = Format::from_format_string("q2-sandboxed-preview").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "q2-sandboxed-preview");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.output_extension, "html");
        assert!(f.native_pipeline);
        // Sandboxed-preview port (bd-jgpz4hfq): the sandboxed renderer
        // consumes the same post-pipeline AST as q2-preview (highlight
        // spans, chrome metadata, theme fingerprint), so it uses the
        // preview pipeline_kind rather than the raw parse-only path.
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
        for f in [
            "html", "revealjs", "latex", "pdf", "gfm", "typst", "docx", "pptx", "beamer",
        ] {
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

    /// The pandoc writer name (`-t` argument) must stay `"typst"` even
    /// though the final user-facing `output_extension` is `"pdf"` — see
    /// `pandoc_writer_name_for`'s doc comment.
    #[test]
    fn test_typst_pandoc_writer_name_differs_from_output_extension() {
        let f = Format::from_format_string("typst").unwrap();
        assert_eq!(f.output_extension, "pdf");
        assert_eq!(f.pandoc_writer_name(), "typst");
    }

    /// For every other Pandoc-routed format, the writer name and the
    /// output extension still coincide (no behavior change for docx/pptx).
    #[test]
    fn test_pandoc_writer_name_matches_output_extension_for_non_typst() {
        for fmt in ["docx", "pptx", "epub", "gfm", "commonmark"] {
            let f = Format::from_format_string(fmt).unwrap();
            assert_eq!(
                f.pandoc_writer_name(),
                f.output_extension,
                "writer name should match output_extension for {fmt}"
            );
        }
    }

    /// pandoc-hybrid-typst Phase 1 invocation builder: typst needs
    /// `--standalone`, `--wrap none`, and `--default-image-extension svg`
    /// (`format-typst.ts`'s `pandoc.standalone = true` / `wrap: none` /
    /// `default-image-extension: svg`) — none of which any other
    /// Pandoc-hybrid format passes today.
    ///
    /// Revert hunk: emptying `pandoc_invocation_args_for`'s `Typst` arm
    /// makes this RED (the expected flags go missing).
    #[test]
    fn test_typst_invocation_args_include_standalone_wrap_and_image_extension() {
        let f = Format::from_format_string("typst").unwrap();
        let args = f.pandoc_invocation_args();
        assert!(
            args.iter().any(|a| a == "--standalone"),
            "expected --standalone, got {args:?}"
        );
        assert_eq!(
            windowed_pair(&args, "--wrap"),
            Some("none".to_string()),
            "expected --wrap none, got {args:?}"
        );
        assert_eq!(
            windowed_pair(&args, "--default-image-extension"),
            Some("svg".to_string()),
            "expected --default-image-extension svg, got {args:?}"
        );
    }

    /// `citeproc: false` in Q1's typst registration means "don't ask
    /// pandoc's own citeproc to run" — Q2 never asks pandoc for citeproc on
    /// any Pandoc-hybrid format (citations are resolved upstream), so this
    /// is a non-emission, not a flag. Confirms the invocation builder
    /// doesn't regress that default by accidentally emitting `--citeproc`.
    #[test]
    fn test_typst_invocation_args_omit_citeproc_by_default() {
        let f = Format::from_format_string("typst").unwrap();
        let args = f.pandoc_invocation_args();
        assert!(
            !args.iter().any(|a| a == "--citeproc"),
            "typst must not pass --citeproc by default, got {args:?}"
        );
    }

    /// Every other Pandoc-hybrid format gets no extra invocation args —
    /// this bullet is typst-only (no behavior change for docx/pptx/epub).
    #[test]
    fn test_non_typst_formats_get_no_extra_invocation_args() {
        for fmt in ["docx", "pptx", "epub", "gfm", "commonmark"] {
            let f = Format::from_format_string(fmt).unwrap();
            assert!(
                f.pandoc_invocation_args().is_empty(),
                "expected no extra invocation args for {fmt}, got {:?}",
                f.pandoc_invocation_args()
            );
        }
    }

    /// Test helper: returns the value immediately following `flag` in
    /// `args`, if `flag` appears. Used to assert `--wrap none`-shaped
    /// two-token pairs without depending on exact adjacent indices.
    fn windowed_pair(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
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

    // === pptx resolvability (Task 1 seam, T1.2) ===

    #[test]
    fn test_from_format_string_pptx() {
        let f = Format::from_format_string("pptx").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Pptx);
        assert_eq!(f.output_extension, "pptx");
        assert!(!f.native_pipeline);
    }

    // === PipelineProfile tests (Task 1 seam, T1.1) ===

    #[test]
    fn test_pipeline_profile_from_format() {
        assert_eq!(
            PipelineProfile::from_format("html"),
            PipelineProfile::HtmlRender
        );
        assert_eq!(
            PipelineProfile::from_format("q2-debug"),
            PipelineProfile::HtmlRender
        );
        assert_eq!(
            PipelineProfile::from_format("acm-html"),
            PipelineProfile::HtmlRender
        );
        assert_eq!(
            PipelineProfile::from_format("q2-preview"),
            PipelineProfile::HtmlPreview
        );
        assert_eq!(
            PipelineProfile::from_format("q2-sandboxed-preview"),
            PipelineProfile::HtmlPreview
        );
        assert_eq!(
            PipelineProfile::from_format("revealjs"),
            PipelineProfile::RevealjsRender
        );
        assert_eq!(
            PipelineProfile::from_format("q2-slides"),
            PipelineProfile::RevealjsPreview
        );
        assert_eq!(
            PipelineProfile::from_format("docx"),
            PipelineProfile::Pandoc("docx".to_string())
        );
        assert_eq!(
            PipelineProfile::from_format("pptx"),
            PipelineProfile::Pandoc("pptx".to_string())
        );
        assert_eq!(
            PipelineProfile::from_format("gfm"),
            PipelineProfile::Pandoc("gfm".to_string())
        );
    }

    /// Refactor-induced-vacuity guard: a four-variant `PipelineProfile` (no
    /// slot for reveal+preview) would map `q2-slides` to `RevealjsRender`,
    /// silently running the full reveal-render pipeline for the preview leg.
    /// This asserts the `RevealjsPreview` cell by name AND by distinctness
    /// from `RevealjsRender` — the only surface the two differ on.
    #[test]
    fn test_pipeline_profile_q2_slides_is_revealjs_preview_not_render() {
        assert_ne!(
            PipelineProfile::from_format("q2-slides"),
            PipelineProfile::RevealjsRender
        );
        assert_eq!(
            PipelineProfile::from_format("q2-slides"),
            PipelineProfile::RevealjsPreview
        );
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

    // === multi_format_diagnostics tests (P7-foundation Task 1, T1.1-T1.3) ===

    fn keys(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    /// T1.1: a 2-key `format:` map names the used key and the skipped key in
    /// their own clauses — not a whole-message equality assertion, which
    /// would be brittle against wording edits and non-discriminating about
    /// which key landed in which clause.
    #[test]
    fn test_multi_format_names_skipped_key() {
        let diags = multi_format_diagnostics(&keys(&["docx", "html"]), "docx");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-20-8"));
        let text = diags[0].to_text(None);
        assert!(
            text.contains("docx") && !text.split("skipped").next().unwrap().contains("html"),
            "used clause must name docx, not html: {text}"
        );
        assert!(
            text.contains("html"),
            "skipped clause must name html: {text}"
        );
    }

    /// T1.2: the skipped list preserves declaration order. Pinned to a
    /// non-alphabetical declaration (`docx, pptx, html`) so a regression to a
    /// sorted/`BTreeSet` collection reddens this test instead of passing by
    /// accident (an alphabetical fixture would make sorted and
    /// declaration-order output indistinguishable).
    #[test]
    fn test_multi_format_skipped_order() {
        let diags = multi_format_diagnostics(&keys(&["docx", "pptx", "html"]), "docx");
        assert_eq!(diags.len(), 1);
        let text = diags[0].to_text(None);
        let pptx_pos = text.find("pptx").expect("pptx named");
        let html_pos = text.find("html").expect("html named");
        assert!(
            pptx_pos < html_pos,
            "skipped keys must appear in declaration order (pptx before html): {text}"
        );
    }

    /// T1.3: a single-key map, a scalar `format:`, and no declared keys at
    /// all all produce no warning.
    #[test]
    fn test_single_format_no_warning() {
        assert!(multi_format_diagnostics(&keys(&["html"]), "html").is_empty());
        assert!(multi_format_diagnostics(&[], "html").is_empty());
    }

    #[test]
    fn test_format_keys_from_frontmatter_map_preserves_order() {
        let content =
            "---\nformat:\n  docx: default\n  pptx: default\n  html: default\n---\nbody\n";
        assert_eq!(
            format_keys_from_frontmatter(content),
            vec!["docx", "pptx", "html"]
        );
    }

    #[test]
    fn test_format_keys_from_frontmatter_scalar() {
        let content = "---\nformat: html\n---\nbody\n";
        assert_eq!(format_keys_from_frontmatter(content), vec!["html"]);
    }

    #[test]
    fn test_format_keys_from_frontmatter_absent() {
        assert!(format_keys_from_frontmatter("no front matter here").is_empty());
    }
}
