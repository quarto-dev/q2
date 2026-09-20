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

/// Look up [`FormatPandocDefaults`] for a pandoc base/output format
/// (`Format::output_extension`, e.g. `"docx"`, `"pptx"`).
pub fn format_pandoc_defaults(base_format: &str) -> FormatPandocDefaults {
    match base_format {
        "docx" | "odt" => FormatPandocDefaults {
            page_width: Some(6.5),
            output_divs: None,
            default_image_extension: Some("png"),
        },
        "pptx" => FormatPandocDefaults {
            page_width: None,
            output_divs: Some(false),
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
/// `slide-level` is forwarded only for `base_format == "pptx"` (T4.3) —
/// pandoc accepts `--slide-level` for every writer, but Q1 never sets it
/// for docx, and forwarding it unconditionally would silently change docx
/// output the day a document declares `slide-level:` for an unrelated
/// reason (e.g. a shared `_metadata.yml`).
pub fn build_forwarded_args(
    stage_name: &str,
    doc_dir: &Path,
    meta: &ConfigValue,
    base_format: &str,
) -> Result<Vec<OsString>, PipelineError> {
    let mut args = Vec::new();

    for key in PATH_SHAPED_ALLOW_LIST_KEYS {
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
    if base_format == "pptx"
        && let Some(n) = meta.get("slide-level").and_then(|v| v.as_int())
    {
        args.push(OsString::from("--slide-level"));
        args.push(OsString::from(n.to_string()));
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

    /// T4.1: the per-format defaults table.
    #[test]
    fn test_format_defaults_table() {
        let docx = format_pandoc_defaults("docx");
        assert_eq!(docx.page_width, Some(6.5));
        assert_eq!(docx.default_image_extension, Some("png"));

        let pptx = format_pandoc_defaults("pptx");
        assert_eq!(pptx.output_divs, Some(false));
        assert_eq!(pptx.default_image_extension, Some("png"));

        let html = format_pandoc_defaults("html");
        assert_eq!(html, FormatPandocDefaults::default());
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

        let args = build_forwarded_args("pandoc-write", Path::new("/doc/dir"), &meta, "docx")
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

        let pptx_args =
            build_forwarded_args("pandoc-write", Path::new("/doc/dir"), &meta, "pptx").unwrap();
        assert!(
            pptx_args
                .iter()
                .any(|a| a.to_string_lossy() == "--slide-level")
        );

        let docx_args =
            build_forwarded_args("pandoc-write", Path::new("/doc/dir"), &meta, "docx").unwrap();
        assert!(
            !docx_args
                .iter()
                .any(|a| a.to_string_lossy() == "--slide-level"),
            "docx must not receive --slide-level: {docx_args:?}"
        );
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
        let args =
            build_forwarded_args("pandoc-write", Path::new("/doc/dir"), &meta, "docx").unwrap();
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
        let err = build_forwarded_args("pandoc-write", Path::new("/doc/dir"), &meta, "docx")
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
}
