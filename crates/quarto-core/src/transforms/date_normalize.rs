/*
 * date_normalize.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that resolves, parses, and formats date metadata.
 */

//! Date normalization transform (bd-gx9cic8z P4, bd-13f821l5).
//!
//! The Rust counterpart of Quarto 1's pre-Pandoc date rewrite
//! (`src/command/render/pandoc.ts:1186-1197`) plus
//! `documentTitleMetadata`'s forced `long`: for each of `date` and
//! `date-modified`,
//!
//! 1. resolve the `today` / `now` / `last-modified` keywords
//!    (`today`/`now` via `SystemRuntime::unix_timestamp` — UTC, a
//!    documented deviation from Q1's local time; `last-modified` via
//!    the runtime's VFS-aware file mtime);
//! 2. parse the value ([`crate::dates::parse_date`]; on failure, a
//!    render diagnostic names the accepted forms and the raw string
//!    is left untouched — Q1 can silently emit `Invalid Date`);
//! 3. write the ISO form to **`date-meta`** / **`date-modified-meta`**
//!    (unless already present — an explicit value wins, the
//!    `description-meta` precedent). The head's
//!    `<meta name="dcterms.date">` consumes `date-meta`, keeping the
//!    machine slot ISO even when `date-format` is set (deliberate
//!    deviation: Q1 leaks the human-formatted string there);
//! 4. **replace** the field in place with the formatted string
//!    (Q1-familiar: `$date$` is formatted for every consumer —
//!    built-in partials, user `template-partials`, the q2-preview
//!    React title block).
//!
//! Format precedence (Q1's): the field-local `format` of a
//! `date: { value, format }` map > document `date-format` > default.
//! The default is `long` when the styled HTML title block is active
//! (format-html and `title-block-style` ≠ none — Q1's
//! `documentTitleMetadata` rule) and `iso` otherwise (Q1 normalizes
//! every render, all formats — design question Q-b).
//!
//! Design: `claude-notes/plans/2026-07-17-date-formatting-design.md`.

use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;
use time::OffsetDateTime;

use crate::Result;
use crate::dates::{ACCEPTED_DATE_FORMS, DateStyle, ParsedDate, parse_date};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::TitleBlockStyle;

/// The two document metadata fields this transform normalizes.
const DATE_FIELDS: [&str; 2] = ["date", "date-modified"];

/// Transform that resolves date keywords and formats date metadata.
pub struct DateNormalizeTransform {
    runtime: Arc<dyn SystemRuntime>,
}

impl DateNormalizeTransform {
    /// Create a new date normalization transform.
    pub fn new(runtime: Arc<dyn SystemRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for DateNormalizeTransform {
    fn name(&self) -> &str {
        "date-normalize"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !ast.meta.is_map() {
            return Ok(());
        }

        let default_style = default_style(&ast.meta, ctx);
        let doc_style = ast
            .meta
            .get("date-format")
            .and_then(|v| v.as_plain_text())
            .map(|s| DateStyle::parse(&s));

        for field in DATE_FIELDS {
            let Some(raw) = read_date_field(&ast.meta, field) else {
                continue;
            };

            // Keyword resolution (today / now / last-modified).
            let resolved = match raw.value.as_str() {
                "today" => self.now_utc().map(|dt| iso_timestamp(dt.date().midnight())),
                "now" => self
                    .now_utc()
                    .map(|dt| iso_timestamp(time::PrimitiveDateTime::new(dt.date(), dt.time()))),
                "last-modified" => self.input_mtime(ctx),
                _ => Some(raw.value.clone()),
            };
            let Some(resolved) = resolved else {
                ctx.diagnostics.push(DiagnosticMessage::warning(format!(
                    "could not resolve `{field}: {}` (keyword resolution failed); \
                     leaving the value as-is",
                    raw.value
                )));
                continue;
            };

            let Some(parsed) = parse_date(&resolved) else {
                ctx.diagnostics.push(DiagnosticMessage::warning(format!(
                    "could not parse `{field}: {resolved}` as a date; accepted forms are \
                     {ACCEPTED_DATE_FORMS}. Leaving the value as-is."
                )));
                continue;
            };

            // ISO machine form (explicit value wins).
            let meta_key = format!("{field}-meta");
            if ast.meta.get(&meta_key).is_none() {
                ast.meta.insert_path(
                    &[meta_key.as_str()],
                    ConfigValue::new_string(parsed.iso_string(), gen_si()),
                );
            }

            // Human form: field format > date-format > default.
            let style = raw
                .field_format
                .as_deref()
                .map(DateStyle::parse)
                .or_else(|| doc_style.clone())
                .unwrap_or_else(|| default_style.clone());
            let (formatted, warnings) = crate::dates::format_date(&parsed, &style);
            for w in warnings {
                ctx.diagnostics.push(DiagnosticMessage::warning(w));
            }
            ast.meta
                .insert_path(&[field], ConfigValue::new_string(formatted, gen_si()));
        }

        Ok(())
    }
}

impl DateNormalizeTransform {
    /// The current instant, UTC, via the runtime clock (WASM-safe).
    fn now_utc(&self) -> Option<OffsetDateTime> {
        let ts = self.runtime.unix_timestamp().ok()?;
        OffsetDateTime::from_unix_timestamp(ts as i64).ok()
    }

    /// The input file's modification time as an ISO timestamp
    /// (VFS-aware; the `listing_item_info::mtime_iso` pattern).
    fn input_mtime(&self, ctx: &RenderContext) -> Option<String> {
        let metadata = self.runtime.path_metadata(&ctx.document.input).ok()?;
        let modified = metadata.modified?;
        let dt = OffsetDateTime::from(modified);
        Some(iso_timestamp(time::PrimitiveDateTime::new(
            dt.date(),
            dt.time(),
        )))
    }
}

/// A raw date field value: the scalar string, or Q1's
/// `{ value, format }` map form with its field-local format.
struct RawDateField {
    value: String,
    field_format: Option<String>,
}

/// Read `date` / `date-modified`, accepting both the scalar and the
/// `{ value, format }` map forms.
fn read_date_field(meta: &ConfigValue, field: &str) -> Option<RawDateField> {
    let value = meta.get(field)?;
    if value.is_map() {
        let inner = value.get("value")?.as_plain_text()?;
        let field_format = value.get("format").and_then(|v| v.as_plain_text());
        Some(RawDateField {
            value: inner,
            field_format,
        })
    } else {
        Some(RawDateField {
            value: value.as_plain_text()?,
            field_format: None,
        })
    }
}

/// Q1's default: `long` when the styled HTML title block is active
/// (`documentTitleMetadata`), `iso` otherwise (the global pre-Pandoc
/// normalization).
fn default_style(meta: &ConfigValue, ctx: &RenderContext) -> DateStyle {
    let styled_html_title_block =
        matches!(ctx.format.identifier, crate::format::FormatIdentifier::Html)
            && TitleBlockStyle::from_meta(meta) != TitleBlockStyle::None;
    if styled_html_title_block {
        DateStyle::Long
    } else {
        DateStyle::Iso
    }
}

/// Render a `PrimitiveDateTime` as the ISO timestamp shape the
/// keyword resolution materializes (Q1 materializes keywords as
/// timestamps, then formats).
fn iso_timestamp(dt: time::PrimitiveDateTime) -> String {
    let parsed = ParsedDate {
        datetime: dt,
        offset: Some(time::UtcOffset::UTC),
        has_time: true,
    };
    parsed.iso_string()
}

fn gen_si() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_system_runtime::NativeRuntime;
    use std::path::PathBuf;

    fn si() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: si(),
                    value: v,
                })
                .collect(),
            si(),
        )
    }

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, si())
    }

    fn project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
        }
    }

    /// Run the transform over `meta` for the given format string;
    /// returns (meta, diagnostics).
    fn run(meta: ConfigValue, format: &str) -> (ConfigValue, Vec<String>) {
        let project = project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::from_format_string(format).unwrap();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = Pandoc {
            meta,
            ..Default::default()
        };
        let transform = DateNormalizeTransform::new(Arc::new(NativeRuntime::new()));
        pollster::block_on(transform.transform(&mut ast, &mut ctx)).unwrap();
        let diags = ctx.diagnostics.iter().map(|d| format!("{d:?}")).collect();
        (ast.meta, diags)
    }

    fn text(meta: &ConfigValue, key: &str) -> Option<String> {
        meta.get(key).and_then(|v| v.as_plain_text())
    }

    #[test]
    fn html_default_is_long_with_iso_meta() {
        let (meta, diags) = run(map(vec![("date", s("2026-07-01"))]), "html");
        assert_eq!(text(&meta, "date").as_deref(), Some("July 1, 2026"));
        assert_eq!(text(&meta, "date-meta").as_deref(), Some("2026-07-01"));
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn non_html_default_is_iso_normalization() {
        // Q-b: every format normalizes; 03/07/2026 → ISO for PDF.
        let (meta, _) = run(map(vec![("date", s("03/07/2026"))]), "pdf");
        assert_eq!(text(&meta, "date").as_deref(), Some("2026-03-07"));
        assert_eq!(text(&meta, "date-meta").as_deref(), Some("2026-03-07"));
    }

    #[test]
    fn title_block_style_none_defaults_to_iso() {
        let (meta, _) = run(
            map(vec![
                ("date", s("2026-07-01")),
                ("title-block-style", s("none")),
            ]),
            "html",
        );
        assert_eq!(text(&meta, "date").as_deref(), Some("2026-07-01"));
    }

    #[test]
    fn date_format_option_and_field_map_form() {
        let (meta, _) = run(
            map(vec![
                ("date", s("2026-07-01")),
                (
                    "date-modified",
                    map(vec![("value", s("2026-07-10")), ("format", s("iso"))]),
                ),
                ("date-format", s("MMM D, YYYY")),
            ]),
            "html",
        );
        assert_eq!(text(&meta, "date").as_deref(), Some("Jul 1, 2026"));
        // Field-local format beats the document date-format.
        assert_eq!(text(&meta, "date-modified").as_deref(), Some("2026-07-10"));
        assert_eq!(
            text(&meta, "date-modified-meta").as_deref(),
            Some("2026-07-10")
        );
    }

    #[test]
    fn unparseable_date_warns_and_preserves_raw() {
        let (meta, diags) = run(map(vec![("date", s("the ides of march"))]), "html");
        assert_eq!(text(&meta, "date").as_deref(), Some("the ides of march"));
        assert!(meta.get("date-meta").is_none());
        assert_eq!(diags.len(), 1);
        assert!(diags[0].contains("accepted forms"), "{}", diags[0]);
    }

    #[test]
    fn explicit_date_meta_wins() {
        let (meta, _) = run(
            map(vec![("date", s("2026-07-01")), ("date-meta", s("keep-me"))]),
            "html",
        );
        assert_eq!(text(&meta, "date-meta").as_deref(), Some("keep-me"));
        assert_eq!(text(&meta, "date").as_deref(), Some("July 1, 2026"));
    }

    #[test]
    fn today_keyword_resolves_and_formats() {
        let (meta, diags) = run(map(vec![("date", s("today"))]), "html");
        assert!(diags.is_empty(), "{diags:?}");
        let formatted = text(&meta, "date").unwrap();
        // Long style: "<Month> <D>, <YYYY>".
        assert!(
            formatted.chars().next().unwrap().is_ascii_uppercase() && formatted.contains(", 2"),
            "unexpected: {formatted}"
        );
        // Machine slot is a full ISO timestamp (keywords materialize
        // with a time component, like Q1).
        assert!(text(&meta, "date-meta").unwrap().contains('T'));
    }

    #[test]
    fn last_modified_resolves_from_input_mtime() {
        // Real file so path_metadata has an mtime.
        let temp = tempfile::TempDir::new().unwrap();
        let input = temp.path().join("doc.qmd");
        std::fs::write(&input, "x").unwrap();

        let project = project();
        let doc = DocumentInfo::from_path(&input);
        let format = Format::html();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        let mut ast = Pandoc {
            meta: map(vec![("date", s("last-modified"))]),
            ..Default::default()
        };
        let transform = DateNormalizeTransform::new(Arc::new(NativeRuntime::new()));
        pollster::block_on(transform.transform(&mut ast, &mut ctx)).unwrap();
        assert!(ctx.diagnostics.is_empty());
        let formatted = ast.meta.get("date").unwrap().as_plain_text().unwrap();
        assert!(formatted.contains(", 2"), "unexpected: {formatted}");
    }
}
