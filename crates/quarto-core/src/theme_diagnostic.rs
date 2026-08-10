//! Convert [`quarto_sass::SassError`] into the project's structured
//! [`ParseError`](crate::error::ParseError) so theme-config failures
//! can be rendered as ariadne reports with a source span pointing at
//! the offending YAML.
//!
//! Mirrors the pattern established by
//! [`crate::project_resources::resource_error_to_parse_error`]
//! (bd-c1et2 / Q-5-1..Q-5-3): a domain error carrying a
//! [`SourceInfo`] is lifted into a `ParseError` that owns the
//! diagnostic message + the file content the renderer needs.
//!
//! The "Parse" in `ParseError` is historical — the type is just a
//! `Vec<DiagnosticMessage>` + `SourceContext` envelope.

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_sass::SassError;
use quarto_source_map::{FileId, SourceContext};

use crate::error::ParseError;

/// Build a [`ParseError`] from a [`SassError`], loading the source
/// file matching the diagnostic's [`FileId`] from
/// `candidate_sources` so the resulting diagnostic can render an
/// ariadne snippet pointing at the offending YAML value.
///
/// `candidate_sources` is a slice of `(FileId, &Path)` pairs — the
/// caller declares the FileId binding for each plausible source
/// file. This matters because different parsers use different
/// FileId schemes:
///
/// - `quarto_yaml::parse_file` hashes the filename string to derive
///   a `FileId`.
/// - Pampa's [`ASTContext`] uses sequential `FileId(0)` for the
///   document's primary file.
///
/// The caller knows which scheme applies to which path, so it
/// computes the FileId explicitly. The converter looks for the
/// candidate whose FileId equals the one on the diagnostic and
/// loads its file content into the [`SourceContext`].
///
/// Handles [`SassError::InvalidThemeConfig`] (Q-14-1) and
/// [`SassError::UnknownTheme`] (Q-14-2) specifically; other
/// variants fall back to a span-less diagnostic carrying the raw
/// error message.
///
/// If no candidate matches the diagnostic's FileId, or the error
/// has no `location`, the diagnostic still renders — just without
/// the source snippet.
pub fn sass_error_to_parse_error(
    err: &SassError,
    candidate_sources: &[(FileId, PathBuf)],
) -> ParseError {
    let location = sass_error_location(err);

    // Candidate-matched binding via the shared helper (this function
    // was the original precedent for it; bd-m6wmztln → bd-r64mj1aa):
    // registers only the candidate whose id equals the diagnostic's
    // resolved id, only with readable content. No match ⇒ span-less
    // render — never a wrong span.
    let mut source_context = SourceContext::new();
    if let Some(loc) = &location {
        crate::config_sources::bind_source_candidates(
            &mut source_context,
            loc,
            candidate_sources.iter().map(|(fid, p)| (*fid, p.as_path())),
        );
    }

    let diagnostic = match err {
        SassError::InvalidThemeConfig { message, location } => {
            let mut b = DiagnosticMessageBuilder::error("Invalid theme configuration")
                .with_code("Q-14-1")
                .problem(message.clone());
            if let Some(loc) = location {
                b = b.with_location(loc.clone());
            }
            b.build()
        }
        SassError::UnknownTheme { name, location } => {
            let mut b = DiagnosticMessageBuilder::error("Unknown theme name")
                .with_code("Q-14-2")
                .problem(format!(
                    "`{}` is not a recognized built-in theme and is not a path to a \
                     `.scss`/`.css` file.",
                    name
                ))
                .add_hint(
                    "Use one of the built-in Bootswatch names (e.g. `cosmo`, `darkly`), \
                     a path to a `.scss`/`.css` file, or `theme: none` to suppress \
                     Bootstrap?",
                );
            if let Some(loc) = location {
                b = b.with_location(loc.clone());
            }
            b.build()
        }
        SassError::CustomThemeNotFound { path, location } => {
            let mut b = DiagnosticMessageBuilder::error("Theme file not found")
                .with_code("Q-14-4")
                .problem(format!(
                    "the `theme:` entry resolves to `{}`, which does not exist.",
                    path.display()
                ))
                .add_hint(
                    "Check the spelling and location of the file. Relative theme paths \
                     resolve against the document's directory; extension-bundled themes \
                     must sit next to the extension's `_extension.yml`?",
                );
            if let Some(loc) = location {
                b = b.with_location(loc.clone());
            }
            b.build()
        }
        // Fallback for SassError variants we haven't migrated yet.
        // Returning *something* structured is better than the legacy
        // plain `e.to_string()` form — no code is assigned because
        // the catalog only covers migrated variants.
        other => DiagnosticMessageBuilder::error("SASS error")
            .problem(other.to_string())
            .build(),
    };

    ParseError::new(vec![diagnostic], source_context)
}

/// Extract the source location carried by a [`SassError`], if any.
/// Centralized so the variant-to-location mapping lives in one
/// place — both the SourceContext loader and the diagnostic
/// constructor use it.
fn sass_error_location(err: &SassError) -> Option<quarto_source_map::SourceInfo> {
    match err {
        SassError::InvalidThemeConfig { location, .. } => location.clone(),
        SassError::UnknownTheme { location, .. } => location.clone(),
        SassError::CustomThemeNotFound { location, .. } => location.clone(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::SourceInfo;
    use tempfile::TempDir;

    /// Look up the [`FileId`] the YAML parser would assign to this
    /// path, via the canonical helper in `quarto_yaml`. Tests use
    /// this to mint SourceInfo whose FileId matches what the
    /// converter will find in `candidate_sources`.
    fn file_id_for(path: &Path) -> FileId {
        quarto_yaml::file_id_for_filename(&path.to_string_lossy())
    }

    /// Strip ANSI SGR / hyperlink escapes so substring assertions
    /// against rendered diagnostics don't break on the interleaved
    /// color codes that ariadne emits.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // CSI: ESC '[' ... letter
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for nc in chars.by_ref() {
                        if nc.is_ascii_alphabetic() {
                            break;
                        }
                    }
                    continue;
                }
                // OSC 8 hyperlink: ESC ']' ... BEL (\x07) or ESC '\\'
                if chars.peek() == Some(&']') {
                    chars.next();
                    while let Some(&nc) = chars.peek() {
                        chars.next();
                        if nc == '\x07' {
                            break;
                        }
                        if nc == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                    continue;
                }
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn invalid_theme_config_renders_with_code_and_span() {
        // End-to-end: a SassError with a SourceInfo pointing into a
        // real on-disk _quarto.yml is turned into a ParseError whose
        // diagnostic carries the Q-14-1 code, the offending message,
        // and renders an ariadne snippet of the right line.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let yaml_path = root.join("_quarto.yml");
        // Hand-crafted contents so we know the byte offsets. The
        // `theme:` value spans the mapping starting after `theme: `
        // on line 2 — though for the diagnostic we point at the
        // whole `theme:` key+value region.
        let contents = "project:\n  type: website\ntheme:\n  light: [cosmo]\n";
        std::fs::write(&yaml_path, contents).unwrap();

        let theme_start = contents.find("theme:").unwrap();
        let theme_end = contents.len(); // through end-of-file for simplicity
        let location = SourceInfo::Original {
            file_id: file_id_for(&yaml_path),
            start_offset: theme_start,
            end_offset: theme_end,
        };

        let err = SassError::InvalidThemeConfig {
            message: "theme must be a string or array of strings".to_string(),
            location: Some(location.clone()),
        };

        let parse_err =
            sass_error_to_parse_error(&err, &[(file_id_for(&yaml_path), yaml_path.clone())]);
        assert_eq!(parse_err.diagnostics.len(), 1);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-1"));
        assert!(
            d.title.contains("Invalid theme configuration"),
            "title was: {}",
            d.title
        );
        assert_eq!(d.location.as_ref(), Some(&location));

        // Render with hyperlinks disabled so the assertion is
        // path-independent. The ariadne snippet should mention the
        // file and an excerpt of the contents.
        let opts = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered = d.to_text_with_options(Some(&parse_err.source_context), &opts);
        assert!(
            rendered.contains("Q-14-1"),
            "rendered output missing code Q-14-1:\n{}",
            rendered
        );
        assert!(
            rendered.contains("string or array"),
            "rendered output missing problem text:\n{}",
            rendered
        );
        // ariadne includes the source line numbers in the snippet
        // header when the location resolves successfully. The
        // mapping is independently exercised by SourceContext tests;
        // the value here is just "did we get *some* source snippet
        // back, not the plain text fallback?". `3 │` is the line-3
        // marker for the `theme:` line in the fixture. We strip ANSI
        // because the renderer interleaves escape codes per glyph,
        // which would otherwise foil a literal substring match.
        let stripped = strip_ansi(&rendered);
        assert!(
            stripped.contains("3 │"),
            "rendered output missing line marker for the `theme:` line:\n{}",
            stripped,
        );
    }

    #[test]
    fn invalid_theme_config_without_location_renders_span_less() {
        // When the SassError has no location (internal variants like
        // brand_err), the helper still produces a structured
        // diagnostic — just without an ariadne snippet.
        let err = SassError::InvalidThemeConfig {
            message: "no source info available".to_string(),
            location: None,
        };
        let parse_err =
            sass_error_to_parse_error(&err, &[(FileId(0), PathBuf::from("/nonexistent"))]);
        assert_eq!(parse_err.diagnostics.len(), 1);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-1"));
        assert_eq!(d.location, None);
    }

    #[test]
    fn theme_diagnostic_code_is_registered_in_catalog() {
        // Belt-and-braces: every code emitted by
        // sass_error_to_parse_error must exist in the shared
        // catalog, under the 'theme' subsystem.
        // Query the catalog data directly (the codes live in
        // `quarto-error-catalog` now, not in `quarto-error-reporting`).
        // Q-14-3 (dark-theme-variant-ignored warning, bd-o76p01wb) is
        // emitted by CompileThemeCssStage rather than this converter,
        // but it lives in the same subsystem and must be registered.
        for code in ["Q-14-1", "Q-14-2", "Q-14-3", "Q-14-4"] {
            let info = quarto_error_catalog::ERROR_CATALOG.get(code);
            assert!(
                info.is_some(),
                "{} is not registered in error_catalog.json",
                code,
            );
            assert_eq!(
                info.unwrap().subsystem,
                "theme",
                "{} should live under the 'theme' subsystem",
                code,
            );
        }
    }

    #[test]
    fn unknown_theme_renders_with_q142_code_and_span() {
        // Parallel to invalid_theme_config_renders_with_code_and_span,
        // but for the UnknownTheme variant. A document with
        // `theme: default` in its frontmatter triggers
        // ThemeSpec::parse("default") → UnknownTheme; the helper
        // must lift it into a Q-14-2 ariadne diagnostic whose
        // location points back at the document.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        // Imagine this is the document frontmatter (or a
        // _metadata.yml referenced by the page). The byte offset of
        // the `default` token is what we point at.
        let yaml_path = root.join("doc.qmd");
        let contents = "---\nformat:\n  html:\n    theme: default\n---\n";
        std::fs::write(&yaml_path, contents).unwrap();

        let scalar_start = contents.find("default").unwrap();
        let scalar_end = scalar_start + "default".len();
        let location = SourceInfo::Original {
            file_id: file_id_for(&yaml_path),
            start_offset: scalar_start,
            end_offset: scalar_end,
        };

        let err = SassError::UnknownTheme {
            name: "default".to_string(),
            location: Some(location.clone()),
        };

        let parse_err =
            sass_error_to_parse_error(&err, &[(file_id_for(&yaml_path), yaml_path.clone())]);
        assert_eq!(parse_err.diagnostics.len(), 1);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-2"));
        assert!(
            d.title.contains("Unknown theme name"),
            "title was: {}",
            d.title,
        );
        assert_eq!(d.location.as_ref(), Some(&location));

        let opts = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered = d.to_text_with_options(Some(&parse_err.source_context), &opts);
        assert!(
            rendered.contains("Q-14-2"),
            "rendered output missing code Q-14-2:\n{}",
            rendered,
        );
        assert!(
            rendered.contains("not a recognized"),
            "rendered output missing problem text:\n{}",
            rendered,
        );
        let stripped = strip_ansi(&rendered);
        assert!(
            stripped.contains("4 │"),
            "rendered output missing line marker for the `theme:` line:\n{}",
            stripped,
        );
    }

    #[test]
    fn unknown_theme_without_location_renders_span_less() {
        let err = SassError::UnknownTheme {
            name: "whatever".to_string(),
            location: None,
        };
        let parse_err =
            sass_error_to_parse_error(&err, &[(FileId(0), PathBuf::from("/nonexistent"))]);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-2"));
        assert_eq!(d.location, None);
    }

    #[test]
    fn custom_theme_not_found_renders_with_q144_code_and_span() {
        // Parallel to unknown_theme_renders_with_q142_code_and_span,
        // but for the CustomThemeNotFound variant (bd-of20unsb): a
        // `theme:` entry naming a `.scss` file that resolves to no
        // file must lift into a Q-14-4 ariadne diagnostic pointing at
        // the offending entry, and its problem text must name the
        // resolved path.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let yaml_path = root.join("doc.qmd");
        let contents = "---\nformat:\n  html:\n    theme: [cosmo, nope.scss]\n---\n";
        std::fs::write(&yaml_path, contents).unwrap();

        let entry_start = contents.find("nope.scss").unwrap();
        let entry_end = entry_start + "nope.scss".len();
        let location = SourceInfo::Original {
            file_id: file_id_for(&yaml_path),
            start_offset: entry_start,
            end_offset: entry_end,
        };

        let err = SassError::CustomThemeNotFound {
            path: root.join("nope.scss"),
            location: Some(location.clone()),
        };

        let parse_err =
            sass_error_to_parse_error(&err, &[(file_id_for(&yaml_path), yaml_path.clone())]);
        assert_eq!(parse_err.diagnostics.len(), 1);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-4"));
        assert!(
            d.title.contains("Theme file not found"),
            "title was: {}",
            d.title,
        );
        assert_eq!(d.location.as_ref(), Some(&location));

        let opts = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered = d.to_text_with_options(Some(&parse_err.source_context), &opts);
        assert!(
            rendered.contains("Q-14-4"),
            "rendered output missing code Q-14-4:\n{}",
            rendered,
        );
        assert!(
            rendered.contains("nope.scss"),
            "rendered output missing resolved path:\n{}",
            rendered,
        );
        let stripped = strip_ansi(&rendered);
        assert!(
            stripped.contains("4 \u{2502}"),
            "rendered output missing line marker for the `theme:` line:\n{}",
            stripped,
        );
    }

    #[test]
    fn custom_theme_not_found_without_location_renders_span_less() {
        let err = SassError::CustomThemeNotFound {
            path: std::path::PathBuf::from("/somewhere/nope.scss"),
            location: None,
        };
        let parse_err =
            sass_error_to_parse_error(&err, &[(FileId(0), PathBuf::from("/nonexistent"))]);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-4"));
        assert_eq!(d.location, None);
    }
}
