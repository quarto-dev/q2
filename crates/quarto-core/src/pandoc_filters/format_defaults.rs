//! The per-format pandoc-defaults literal table and the pandoc-defaults
//! forwarding allow-list (P7 Task 4, `2026-09-18-pandoc-hybrid-P7-implementation.md`
//! Task 4, Finding 1/Finding 2).
//!
//! Two families of "pandoc-defaults" state live here, on purpose kept
//! distinct even though both ultimately become CLI args or filter-params
//! entries:
//!
//! - [`format_pandoc_defaults`] — literals Q2 already knows and always
//!   applies for a given base format (`page-width`, `output-divs`,
//!   `default-image-extension`), taken from Q1's `formats.ts`/
//!   `formats-shared.ts` at the pinned tag. `page-width`/`output-divs` are
//!   **not** pandoc CLI flags — Q1's own `layout.ts`/filter code reads them
//!   from `QUARTO_FILTER_PARAMS` (verified: `resources/pandoc-filters/filters/layout/wp.lua`'s
//!   `wpPageWidth()` reads `param("page-width", nil)`; `output-divs` is
//!   already a `QUARTO_FILTER_PARAMS` key in
//!   [`super::params::insert_active_filters`]). `default-image-extension`
//!   **is** a real pandoc CLI flag (`--default-image-extension`).
//! - [`PANDOC_DEFAULTS_ALLOW_LIST`] + [`build_forwarded_args`] — an
//!   **allow-list**, not a full `kPandocDefaultsKeys` pass-through
//!   (Finding 2): forward exactly the keys this plan names a v1 need for,
//!   each read from the document's already-format-resolved metadata.
//!   `number-sections`/`number-offset` are deliberately excluded — see
//!   `test_allow_list_excludes_number_sections` below and the epic's
//!   Missing-test-pass item 2.

use std::ffi::OsString;
use std::path::Path;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;

use crate::format::FormatIdentifier;
use crate::stage::PipelineError;

/// The per-format pandoc-defaults literal table (Finding 1). Fields are
/// `None` when the format has no opinion (native/HTML-family formats never
/// reach this table — [`crate::stage::stages::PandocWriteStage`] is the only
/// caller).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FormatPandocDefaults {
    /// `QUARTO_FILTER_PARAMS["page-width"]` (inches) — read by the vendored
    /// layout Lua (`wp.lua`'s `wpPageWidth()`), not a pandoc CLI flag.
    pub page_width: Option<f64>,
    /// `QUARTO_FILTER_PARAMS["output-divs"]` override. `None` means "use
    /// the builder's own base default" (`true`,
    /// [`super::params::insert_active_filters`]).
    pub output_divs: Option<bool>,
    /// `--default-image-extension` — a real pandoc CLI flag.
    pub default_image_extension: Option<&'static str>,
}

/// Look up [`FormatPandocDefaults`] for a pandoc-hybrid base format.
///
/// Keyed by [`FormatIdentifier`], not the output-extension string
/// (long-tail Phase 1 wrinkle 1): extension keys collide for the tail —
/// `"xml"` would be both opendocument's wordprocessor defaults and
/// docbook's plaintext defaults, and Typst's extension is `"pdf"`, not
/// `"typst"`. The old `"docx" | "odt"` arm's Odt half returns together
/// with the `Odt` variant (long-tail Phase 2).
pub fn format_pandoc_defaults(id: FormatIdentifier) -> FormatPandocDefaults {
    match id {
        FormatIdentifier::Docx => FormatPandocDefaults {
            page_width: Some(6.5),
            output_divs: None,
            default_image_extension: Some("png"),
        },
        FormatIdentifier::Pptx => FormatPandocDefaults {
            page_width: None,
            output_divs: Some(false),
            default_image_extension: Some("png"),
        },
        // Long-tail Phase 2 (Tier A): the wordprocessor trio (Q1's
        // `createWordprocessorFormat`; `rtfFormat()` shares the base) gets
        // the docx-shaped 6.5-inch page + png; fb2 (Q1's
        // `createEbookFormat`) is reflowable so no page-width, png only;
        // the 21 `plaintextFormat` variants are png-only too.
        FormatIdentifier::Odt | FormatIdentifier::Opendocument | FormatIdentifier::Rtf => {
            FormatPandocDefaults {
                page_width: Some(6.5),
                output_divs: None,
                default_image_extension: Some("png"),
            }
        }
        FormatIdentifier::Fb2
        | FormatIdentifier::Plain
        | FormatIdentifier::Rst
        | FormatIdentifier::Org
        | FormatIdentifier::Muse
        | FormatIdentifier::Ms
        | FormatIdentifier::Man
        | FormatIdentifier::Texinfo
        | FormatIdentifier::Tei
        | FormatIdentifier::Zimwiki
        | FormatIdentifier::Dokuwiki
        | FormatIdentifier::Haddock
        | FormatIdentifier::Json
        | FormatIdentifier::Native
        | FormatIdentifier::Icml
        | FormatIdentifier::Jira
        | FormatIdentifier::Mediawiki
        | FormatIdentifier::Xwiki
        | FormatIdentifier::Textile
        | FormatIdentifier::Docbook
        | FormatIdentifier::Docbook4
        | FormatIdentifier::Docbook5 => FormatPandocDefaults {
            page_width: None,
            output_divs: None,
            default_image_extension: Some("png"),
        },
        // Long-tail Phase 3 (Tier B): the markdown family. Q1's bare
        // `markdown` is `pandocMarkdownFormat()` — plaintext base, no
        // output-divs override — while every other `isMarkdownOutput`
        // flavor is `markdownFormat(displayName)`, which sets
        // `render: {output-divs: false}` (executed-cell output must not be
        // wrapped in a `::: cell` Div markdown writers can't express).
        // All nine inherit plaintextFormat's png image default; gfm and
        // commonmark join the table here after Phase 1 left them at the
        // no-op default.
        FormatIdentifier::Markdown => FormatPandocDefaults {
            page_width: None,
            output_divs: None,
            default_image_extension: Some("png"),
        },
        FormatIdentifier::MarkdownStrict
        | FormatIdentifier::MarkdownPhpExtra
        | FormatIdentifier::MarkdownGithub
        | FormatIdentifier::MarkdownMmd
        | FormatIdentifier::Markua
        | FormatIdentifier::CommonmarkX
        | FormatIdentifier::Gfm
        | FormatIdentifier::CommonMark => FormatPandocDefaults {
            page_width: None,
            output_divs: Some(false),
            default_image_extension: Some("png"),
        },
        // Long-tail Phase 5 (Tier D): the JS slide family
        // (`createHtmlPresentationFormat`) — png-only image default like
        // the plaintext tail; no page-width (decks have no pages) and no
        // output-divs override (the cell Div renders fine into HTML).
        FormatIdentifier::S5
        | FormatIdentifier::Dzslides
        | FormatIdentifier::Slidy
        | FormatIdentifier::Slideous => FormatPandocDefaults {
            page_width: None,
            output_divs: None,
            default_image_extension: Some("png"),
        },
        _ => FormatPandocDefaults::default(),
    }
}

/// The pandoc-defaults forwarding allow-list (Finding 2). Deliberately
/// **not** full `kPandocDefaultsKeys` pass-through — every key here has its
/// own bound test row in `2026-09-18-pandoc-hybrid-P7-implementation.md`
/// Task 4's Test Seam Spec. `number-sections`/`number-offset` are excluded
/// on purpose (Missing-test-pass item 2): Q1 itself deletes them for a
/// non-latex/typst/markdown target (`pandoc.ts:1044-1057` at the pinned
/// tag), so forwarding them here would produce a third, unaudited
/// behavior.
pub const PANDOC_DEFAULTS_ALLOW_LIST: &[&str] = &[
    "reference-doc",
    "template",
    "highlight-style",
    "toc",
    "toc-depth",
    "reference-location",
    "shift-heading-level-by",
    "slide-level",
];

/// The two path-shaped forwarded keys. Resolved to a document-relative
/// [`ConfigValueKind::Path`] at merge time by
/// [`crate::project::format_paths::FORMAT_PATH_KEYS`]'s
/// `MarkPolicy::ExistenceSilent` policy when the file exists; left as
/// `Scalar` (unresolved) when it does not — see [`build_forwarded_args`]'s
/// handling of that case.
const PATH_SHAPED_ALLOW_LIST_KEYS: &[&str] = &["reference-doc", "template"];

/// Build the `Q-5-30` hard-error diagnostic for a `reference-doc`/`template`
/// entry whose declared file does not exist. Unlike `css`'s `Q-5-29` (a
/// warning — a missing stylesheet degrades gracefully), a missing
/// `reference-doc`/`template` cannot be handed to pandoc at all: **(measured)**
/// pandoc 3.11 exits 99 with a bare, span-free `File X not found in resource
/// path` if this diagnostic did not fire first.
fn missing_pandoc_path_error(key: &str, declared: &str, entry: &ConfigValue) -> DiagnosticMessage {
    DiagnosticMessageBuilder::error(format!("`{key}: {declared}` does not exist"))
        .with_code("Q-5-30")
        .with_location(entry.source_info.clone())
        .problem(format!(
            "The `{key}` entry names a file that does not exist, so pandoc \
             cannot use it. Resolved relative to the file that declared it \
             (or the project root, for a leading `/`)."
        ))
        .add_hint(format!(
            "Check the path in the `{key}` entry, or add the missing file."
        ))
        .build()
}

/// Build the pandoc CLI args for the allow-listed keys present in `meta`
/// (already the render's format-resolved metadata — `format:` scoping has
/// been flattened by the time [`crate::stage::stages::PandocWriteStage`]
/// runs), plus the per-format literal defaults
/// ([`format_pandoc_defaults`]'s `default_image_extension`).
///
/// `slide-level` is forwarded only for [`FormatIdentifier::Pptx`] (T4.3) —
/// pandoc accepts `--slide-level` for every writer, but Q1 never sets it
/// for docx, and forwarding it unconditionally would silently change docx
/// output the day a document declares `slide-level:` for an unrelated
/// reason (e.g. a shared `_metadata.yml`).
pub fn build_forwarded_args(
    stage_name: &str,
    doc_dir: &Path,
    meta: &ConfigValue,
    base_format: FormatIdentifier,
) -> Result<Vec<OsString>, PipelineError> {
    let mut args = Vec::new();

    for key in PATH_SHAPED_ALLOW_LIST_KEYS {
        // Typst has its own dedicated `--template` mechanism
        // (`pandoc_write::resolve_user_template_path` + copying the user's
        // file over the vendored 8-partial template directory, since
        // Pandoc's `$partial.typ()$` inclusion requires every partial to be
        // a sibling of the file passed via `--template`). Forwarding
        // `template` generically here as well would emit a second
        // `--template` pointing directly at the user's single file, which
        // — placed later in the pandoc invocation than typst's own flag —
        // would silently win and drop the vendored partials.
        if *key == "template" && base_format == FormatIdentifier::Typst {
            continue;
        }
        let Some(value) = meta.get(key) else {
            continue;
        };
        match &value.value {
            // `FORMAT_PATH_KEYS`' `MarkPolicy::ExistenceSilent` resolves
            // this to a **document-relative** path at merge time. Pandoc
            // is spawned without a `current_dir` override
            // (`PandocWriteStage::run` inherits the process cwd), so this
            // must be rebased to an absolute path here, not passed
            // verbatim.
            ConfigValueKind::Path(p) => {
                args.push(OsString::from(format!("--{key}")));
                args.push(OsString::from(doc_dir.join(p)));
            }
            _ => {
                let declared = value.as_plain_text().unwrap_or_default();
                return Err(PipelineError::stage_error_with_diagnostics(
                    stage_name,
                    vec![missing_pandoc_path_error(key, &declared, value)],
                ));
            }
        }
    }

    if let Some(v) = meta.get("highlight-style").and_then(|v| v.as_plain_text()) {
        args.push(OsString::from("--highlight-style"));
        args.push(OsString::from(v));
    }
    if meta.get("toc").and_then(|v| v.as_bool()) == Some(true) {
        args.push(OsString::from("--toc"));
    }
    if let Some(n) = meta.get("toc-depth").and_then(|v| v.as_int()) {
        args.push(OsString::from("--toc-depth"));
        args.push(OsString::from(n.to_string()));
    }
    if let Some(v) = meta
        .get("reference-location")
        .and_then(|v| v.as_plain_text())
    {
        args.push(OsString::from("--reference-location"));
        args.push(OsString::from(v));
    }
    if let Some(n) = meta.get("shift-heading-level-by").and_then(|v| v.as_int()) {
        args.push(OsString::from("--shift-heading-level-by"));
        args.push(OsString::from(n.to_string()));
    }
    if base_format == FormatIdentifier::Pptx
        && let Some(n) = meta.get("slide-level").and_then(|v| v.as_int())
    {
        args.push(OsString::from("--slide-level"));
        args.push(OsString::from(n.to_string()));
    }
    // `top-level-division` is the chapter-boundary lever Typst and
    // LaTeX share (book-projects P2: a single-file book merge sets
    // `top-level-division: chapter`). Gated to those two writers for
    // the same reason `slide-level` is pptx-only.
    if matches!(base_format, FormatIdentifier::Typst | FormatIdentifier::Pdf)
        && let Some(v) = meta
            .get("top-level-division")
            .and_then(|v| v.as_plain_text())
    {
        args.push(OsString::from("--top-level-division"));
        args.push(OsString::from(v));
    }

    if let Some(ext) = format_pandoc_defaults(base_format).default_image_extension {
        args.push(OsString::from("--default-image-extension"));
        args.push(OsString::from(ext));
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use quarto_source_map::SourceInfo;

    fn scalar_meta(entries: &[(&str, &str)]) -> ConfigValue {
        use quarto_pandoc_types::ConfigMapEntry;
        ConfigValue::new_map(
            entries
                .iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_string(*v, SourceInfo::for_test()),
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }

    fn bool_meta(key: &str, value: bool) -> ConfigValue {
        use quarto_pandoc_types::ConfigMapEntry;
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: key.to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_bool(value, SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        )
    }

    fn int_value(n: i64) -> ConfigValue {
        ConfigValue::new_scalar(yaml_rust2::Yaml::Integer(n), SourceInfo::for_test())
    }

    /// T4.1: the per-format defaults table, keyed by
    /// [`FormatIdentifier`] (Phase 1 long-tail wrinkle 1: extension-string
    /// keys collide for the tail — `xml` would be both opendocument's
    /// wordprocessor defaults and docbook's plaintext defaults).
    #[test]
    fn test_format_defaults_table() {
        let docx = format_pandoc_defaults(FormatIdentifier::Docx);
        assert_eq!(docx.page_width, Some(6.5));
        assert_eq!(docx.default_image_extension, Some("png"));

        let pptx = format_pandoc_defaults(FormatIdentifier::Pptx);
        assert_eq!(pptx.output_divs, Some(false));
        assert_eq!(pptx.default_image_extension, Some("png"));

        for id in [
            FormatIdentifier::Html,
            FormatIdentifier::Pdf,
            FormatIdentifier::Epub,
            FormatIdentifier::Typst,
            FormatIdentifier::Revealjs,
        ] {
            assert_eq!(
                format_pandoc_defaults(id),
                FormatPandocDefaults::default(),
                "{id} must have no pandoc-defaults opinion"
            );
        }

        // Long-tail Phase 3 (Tier B): Q1's bare `markdown` is
        // `pandocMarkdownFormat()` — plaintextFormat with **no**
        // output-divs override, so the builder default (`true`) stays;
        // every other `isMarkdownOutput` flavor goes through
        // `markdownFormat(displayName)`, which sets
        // `render: {output-divs: false}`. All nine are png-image.
        let markdown = format_pandoc_defaults(FormatIdentifier::Markdown);
        assert_eq!(markdown.page_width, None);
        assert_eq!(markdown.output_divs, None);
        assert_eq!(markdown.default_image_extension, Some("png"));

        for id in [
            FormatIdentifier::MarkdownStrict,
            FormatIdentifier::MarkdownPhpExtra,
            FormatIdentifier::MarkdownGithub,
            FormatIdentifier::MarkdownMmd,
            FormatIdentifier::Markua,
            FormatIdentifier::CommonmarkX,
            FormatIdentifier::Gfm,
            FormatIdentifier::CommonMark,
        ] {
            let defaults = format_pandoc_defaults(id);
            assert_eq!(
                defaults.output_divs,
                Some(false),
                "output-divs for {id} (Q1 markdownFormat override)"
            );
            assert_eq!(defaults.page_width, None, "page-width for {id}");
            assert_eq!(
                defaults.default_image_extension,
                Some("png"),
                "default image extension for {id}"
            );
        }
    }

    /// Phase 1 wrinkle 2: typst's `template` is owned by its dedicated
    /// vendored-partials mechanism, so `build_forwarded_args` must skip the
    /// generic `template` forwarding for Typst while still forwarding it
    /// for Docx — both polarities, or the skip is vacuous.
    #[test]
    fn test_template_forwarding_is_typst_skipped() {
        use quarto_pandoc_types::ConfigMapEntry;
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "template".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_path(
                    "custom-template.docx".to_string(),
                    SourceInfo::for_test(),
                ),
            }],
            SourceInfo::for_test(),
        );

        let docx_args = build_forwarded_args(
            "pandoc-write",
            Path::new("/doc/dir"),
            &meta,
            FormatIdentifier::Docx,
        )
        .unwrap();
        assert!(
            docx_args
                .iter()
                .any(|a| a.to_string_lossy() == "--template"),
            "docx must forward --template: {docx_args:?}"
        );

        let typst_args = build_forwarded_args(
            "pandoc-write",
            Path::new("/doc/dir"),
            &meta,
            FormatIdentifier::Typst,
        )
        .unwrap();
        assert!(
            !typst_args
                .iter()
                .any(|a| a.to_string_lossy() == "--template"),
            "typst's template is owned by the vendored-partials mechanism: {typst_args:?}"
        );
    }

    /// T4.2: the forwarding allow-list — the discriminator is the
    /// *excluded* keys (`citeproc`, `wrap`, `columns`), not just presence
    /// of the included ones (see the module's Refactor-induced-vacuity
    /// note in the plan: a test asserting only inclusion survives a full
    /// pass-through refactor).
    #[test]
    fn test_forwarding_is_allow_listed() {
        use quarto_pandoc_types::ConfigMapEntry;
        let meta = ConfigValue::new_map(
            vec![
                ConfigMapEntry {
                    key: "highlight-style".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_string("pygments", SourceInfo::for_test()),
                },
                ConfigMapEntry {
                    key: "toc".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_bool(true, SourceInfo::for_test()),
                },
                ConfigMapEntry {
                    key: "toc-depth".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: int_value(2),
                },
                ConfigMapEntry {
                    key: "reference-location".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_string("section", SourceInfo::for_test()),
                },
                ConfigMapEntry {
                    key: "shift-heading-level-by".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: int_value(1),
                },
                // Non-allow-listed keys — must NOT be forwarded.
                ConfigMapEntry {
                    key: "citeproc".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_bool(true, SourceInfo::for_test()),
                },
                ConfigMapEntry {
                    key: "wrap".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_string("auto", SourceInfo::for_test()),
                },
                ConfigMapEntry {
                    key: "columns".to_string(),
                    key_source: SourceInfo::for_test(),
                    value: int_value(72),
                },
            ],
            SourceInfo::for_test(),
        );

        let args = build_forwarded_args(
            "pandoc-write",
            Path::new("/doc/dir"),
            &meta,
            FormatIdentifier::Docx,
        )
        .expect("no path-shaped keys present, must not error");
        let joined: Vec<String> = args
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();

        assert!(joined.contains(&"--highlight-style".to_string()));
        assert!(joined.contains(&"pygments".to_string()));
        assert!(joined.contains(&"--toc".to_string()));
        assert!(joined.contains(&"--toc-depth".to_string()));
        assert!(joined.contains(&"--reference-location".to_string()));
        assert!(joined.contains(&"--shift-heading-level-by".to_string()));

        assert!(
            !joined.iter().any(|a| a.contains("citeproc")),
            "citeproc must not be forwarded: {joined:?}"
        );
        assert!(
            !joined.iter().any(|a| a == "wrap" || a == "auto"),
            "wrap must not be forwarded: {joined:?}"
        );
        assert!(
            !joined.iter().any(|a| a == "columns" || a == "72"),
            "columns must not be forwarded: {joined:?}"
        );
    }

    /// T4.3: `slide-level` is forwarded for pptx only.
    #[test]
    fn test_slide_level_is_pptx_only() {
        let meta = scalar_meta(&[]);
        // Use an int entry directly since scalar_meta only builds strings.
        use quarto_pandoc_types::ConfigMapEntry;
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "slide-level".to_string(),
                key_source: SourceInfo::for_test(),
                value: int_value(3),
            }],
            meta.source_info.clone(),
        );

        let pptx_args = build_forwarded_args(
            "pandoc-write",
            Path::new("/doc/dir"),
            &meta,
            FormatIdentifier::Pptx,
        )
        .unwrap();
        assert!(
            pptx_args
                .iter()
                .any(|a| a.to_string_lossy() == "--slide-level")
        );

        let docx_args = build_forwarded_args(
            "pandoc-write",
            Path::new("/doc/dir"),
            &meta,
            FormatIdentifier::Docx,
        )
        .unwrap();
        assert!(
            !docx_args
                .iter()
                .any(|a| a.to_string_lossy() == "--slide-level"),
            "docx must not receive --slide-level: {docx_args:?}"
        );
    }

    /// `top-level-division` is forwarded for typst and pdf only
    /// (book-projects P2: single-file book merges set
    /// `top-level-division: chapter` so the compiler treats each
    /// chapter's H1 as a true chapter boundary — the lever Typst and
    /// LaTeX share). Gated like `slide-level`: forwarding it
    /// unconditionally would hand an unaudited new flag to writers
    /// (docx, pptx, …) Q1 never sets it for.
    #[test]
    fn test_top_level_division_is_typst_and_pdf_only() {
        let meta = scalar_meta(&[("top-level-division", "chapter")]);

        for base in [FormatIdentifier::Typst, FormatIdentifier::Pdf] {
            let args =
                build_forwarded_args("pandoc-write", Path::new("/doc/dir"), &meta, base).unwrap();
            let joined: Vec<String> = args
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            let pos = joined
                .iter()
                .position(|a| a == "--top-level-division")
                .unwrap_or_else(|| panic!("{base} must receive --top-level-division: {joined:?}"));
            assert_eq!(joined[pos + 1], "chapter");
        }

        for base in [FormatIdentifier::Docx, FormatIdentifier::Epub] {
            let args =
                build_forwarded_args("pandoc-write", Path::new("/doc/dir"), &meta, base).unwrap();
            assert!(
                !args
                    .iter()
                    .any(|a| a.to_string_lossy().contains("top-level-division")),
                "{base} must not receive --top-level-division: {args:?}"
            );
        }
    }

    /// T4.11 (Missing-test-pass item 2): `number-sections`/`number-offset`
    /// are never in the allow-list — Q1 deletes them itself for docx
    /// (neither latex, typst, nor markdown), so forwarding them would be a
    /// third, unaudited behavior.
    #[test]
    fn test_allow_list_excludes_number_sections() {
        assert!(!PANDOC_DEFAULTS_ALLOW_LIST.contains(&"number-sections"));
        assert!(!PANDOC_DEFAULTS_ALLOW_LIST.contains(&"number-offset"));
    }

    /// A `reference-doc` that resolved to a real `Path` is forwarded
    /// verbatim.
    #[test]
    fn test_reference_doc_path_forwarded() {
        use quarto_pandoc_types::ConfigMapEntry;
        let meta = ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "reference-doc".to_string(),
                key_source: SourceInfo::for_test(),
                value: ConfigValue::new_path("custom.docx".to_string(), SourceInfo::for_test()),
            }],
            SourceInfo::for_test(),
        );
        let args = build_forwarded_args(
            "pandoc-write",
            Path::new("/doc/dir"),
            &meta,
            FormatIdentifier::Docx,
        )
        .unwrap();
        let joined: Vec<String> = args
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            joined,
            vec![
                "--reference-doc",
                "/doc/dir/custom.docx",
                "--default-image-extension",
                "png"
            ],
            "the document-relative Path value must be rebased against doc_dir"
        );
    }

    /// The hard-error branch: a `reference-doc` that stayed `Scalar`
    /// (`FORMAT_PATH_KEYS`'s `ExistenceSilent` policy could not find the
    /// file) is a fatal `Q-5-30`, not a silent pass-through to pandoc.
    #[test]
    fn test_missing_reference_doc_is_fatal_q_5_30() {
        let meta = scalar_meta(&[("reference-doc", "missing.docx")]);
        let err = build_forwarded_args(
            "pandoc-write",
            Path::new("/doc/dir"),
            &meta,
            FormatIdentifier::Docx,
        )
        .expect_err("a missing reference-doc must be a hard error");
        let msg = err.to_string();
        assert!(
            msg.contains("missing.docx"),
            "error must name the missing file: {msg}"
        );
    }

    #[test]
    fn test_bool_meta_helper_smoke() {
        // Exercises the helper so `cargo clippy` doesn't flag it as unused
        // if a future edit trims the other callers.
        let meta = bool_meta("toc", true);
        assert_eq!(meta.get("toc").and_then(|v| v.as_bool()), Some(true));
    }

    // === long-tail Phase 2: Tier A bulk tail ===

    /// Deferred from Phase 1 wrinkle 1 as an explicit discrimination test:
    /// `docbook` and `opendocument` share the `xml` output extension but
    /// must get **different** defaults (plaintext `png`-only vs
    /// wordprocessor 6.5-inch + `png`). Keyed by `FormatIdentifier`, the
    /// table cannot collide; the old extension-keyed shape would.
    #[test]
    fn test_docbook_and_opendocument_share_extension_not_defaults() {
        let docbook = Format::from_format_string("docbook").unwrap();
        let opendocument = Format::from_format_string("opendocument").unwrap();
        assert_eq!(docbook.output_extension, "xml");
        assert_eq!(opendocument.output_extension, "xml");

        let docbook_defaults = format_pandoc_defaults(docbook.identifier);
        let opendocument_defaults = format_pandoc_defaults(opendocument.identifier);
        assert_eq!(
            opendocument_defaults.page_width,
            Some(6.5),
            "opendocument is a wordprocessor format"
        );
        assert_ne!(
            docbook_defaults, opendocument_defaults,
            "the shared .xml extension must not mean shared defaults"
        );
    }

    /// The wordprocessor trio (odt/opendocument/rtf — Q1's
    /// `createWordprocessorFormat` plus `rtfFormat()`'s base) gets
    /// `page-width: 6.5` + `default-image-extension: png`.
    #[test]
    fn test_tier_a_wordprocessor_defaults() {
        for name in ["odt", "opendocument", "rtf"] {
            let f = Format::from_format_string(name).unwrap();
            let defaults = format_pandoc_defaults(f.identifier);
            assert_eq!(defaults.page_width, Some(6.5), "page-width for {name}");
            assert_eq!(
                defaults.default_image_extension,
                Some("png"),
                "image extension for {name}"
            );
            assert_eq!(
                defaults.output_divs, None,
                "{name} must not override output-divs"
            );
        }
    }

    /// fb2 (Q1's `createEbookFormat`) gets `png` but **no** page-width —
    /// ebooks are reflowable, wordprocessors are not.
    #[test]
    fn test_tier_a_ebook_defaults() {
        let fb2 = format_pandoc_defaults(FormatIdentifier::try_from("fb2").unwrap());
        assert_eq!(fb2.default_image_extension, Some("png"));
        assert_eq!(fb2.page_width, None);
    }

    /// The 21 plaintext variants get `png` and nothing else — Q1's
    /// `plaintextFormat` sets `default-image-extension: png` and no
    /// page-width/output-divs opinion.
    #[test]
    fn test_tier_a_plaintext_defaults() {
        const PLAINTEXT: &[&str] = &[
            "plain",
            "rst",
            "org",
            "muse",
            "ms",
            "man",
            "texinfo",
            "tei",
            "zimwiki",
            "dokuwiki",
            "haddock",
            "json",
            "native",
            "icml",
            "jira",
            "mediawiki",
            "xwiki",
            "textile",
            "docbook",
            "docbook4",
            "docbook5",
        ];
        for name in PLAINTEXT {
            let f = Format::from_format_string(name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            let defaults = format_pandoc_defaults(f.identifier);
            assert_eq!(
                defaults,
                FormatPandocDefaults {
                    page_width: None,
                    output_divs: None,
                    default_image_extension: Some("png"),
                },
                "defaults for {name}"
            );
        }
    }

    /// The single `--default-image-extension` sink (D2): one per-family row
    /// each of wordprocessor (odt), ebook (fb2), and plaintext (plain) gets
    /// `--default-image-extension png` from `build_forwarded_args` with an
    /// empty meta — and, for rtf, that `--standalone` does **not** come
    /// from here (it is `pandoc_invocation_args_for`'s job).
    #[test]
    fn test_tier_a_forwarded_args_emit_image_extension() {
        for name in ["odt", "fb2", "plain", "rtf"] {
            let f = Format::from_format_string(name).unwrap();
            let args = build_forwarded_args(
                "pandoc-write",
                Path::new("/doc/dir"),
                &scalar_meta(&[]),
                f.identifier,
            )
            .unwrap();
            let joined: Vec<String> = args
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                joined,
                vec!["--default-image-extension".to_string(), "png".to_string()],
                "forwarded args for {name} with empty meta"
            );
        }
    }
}
