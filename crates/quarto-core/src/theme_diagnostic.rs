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
/// Handles [`SassError::InvalidThemeConfig`] (Q-14-1),
/// [`SassError::UnknownTheme`] (Q-14-2),
/// [`SassError::CustomThemeNotFound`] (Q-14-4), and
/// [`SassError::InvalidScssFile`] (Q-14-7), and
/// [`SassError::InvalidBrandFontWeight`] (Q-14-8) specifically; every
/// other variant is a compile failure and renders as Q-14-6 carrying
/// the compiler's text (see [`sass_error_to_parse_error_at`] for how
/// it gets a location).
///
/// If no candidate matches the diagnostic's FileId, or the error
/// has no `location`, the diagnostic still renders — just without
/// the source snippet.
pub fn sass_error_to_parse_error(
    err: &SassError,
    candidate_sources: &[(FileId, PathBuf)],
) -> ParseError {
    sass_error_to_parse_error_at(err, None, candidate_sources)
}

/// [`sass_error_to_parse_error`] with a fallback location for
/// variants that carry none of their own.
///
/// A compile failure ([`SassError::CompilationFailed`]) is the
/// crate-boundary stringification of a grass / dart-sass error: it
/// points into the *assembled* SCSS bundle, never into user YAML, so
/// the variant has no `location` field. The stage that knows which
/// `theme:` value triggered the compile passes that value's span as
/// `fallback_location`, and the diagnostic anchors there
/// (bd-jsvetdea). A variant that already carries a span keeps it —
/// the fallback only fills in `None`.
pub fn sass_error_to_parse_error_at(
    err: &SassError,
    fallback_location: Option<quarto_source_map::SourceInfo>,
    candidate_sources: &[(FileId, PathBuf)],
) -> ParseError {
    let location = sass_error_location(err).or(fallback_location);

    // Candidate-matched binding via the shared helper (this function
    // was the original precedent for it; bd-m6wmztln → bd-r64mj1aa):
    // registers only the candidate whose id equals the diagnostic's
    // resolved id, only with readable content. No match ⇒ span-less
    // render — never a wrong span.
    //
    // A brand error knows which `_brand.yml` it was read from; that
    // file is not in any caller's candidate list (those enumerate
    // config files), so it is added here from the error itself. Its
    // id is the same filename hash quarto-yaml assigned when the
    // brand was re-parsed for the span, so the match is exact.
    let mut source_context = SourceContext::new();
    if let Some(loc) = &location {
        let brand_candidate = match err {
            SassError::InvalidBrandFontWeight {
                brand_file: Some(p),
                ..
            } => Some((
                quarto_yaml::file_id_for_filename(&p.to_string_lossy()),
                p.as_path(),
            )),
            _ => None,
        };
        crate::config_sources::bind_source_candidates(
            &mut source_context,
            loc,
            candidate_sources
                .iter()
                .map(|(fid, p)| (*fid, p.as_path()))
                .chain(brand_candidate),
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
        SassError::InvalidBrandFontWeight {
            path,
            value,
            reason,
            location,
            ..
        } => {
            let mut b = DiagnosticMessageBuilder::error("Invalid brand font weight")
                .with_code("Q-14-8")
                .problem(format!(
                    "`{value}` at `{path}` is not a font weight Quarto understands: {reason}."
                ))
                .add_hint(
                    "Use a number from 100 to 900, a keyword such as `bold` or `semi-bold`, \
                     a list of those, or — on `typography.fonts` entries — a numeric range \
                     such as `400..700`?",
                );
            if let Some(loc) = location {
                b = b.with_location(loc.clone());
            }
            b.build()
        }
        SassError::InvalidScssFile { path, .. } => {
            let mut b = DiagnosticMessageBuilder::error("Theme file has no layer boundary markers")
                .with_code("Q-14-7")
                .problem(format!(
                    "the `theme:` entry resolves to `{}`, which contains none of the \
                     `/*-- scss:... --*/` layer boundary markers Quarto needs to merge it \
                     into the Bootstrap bundle.",
                    path.display()
                ))
                .add_hint(
                    "Should the file start with a layer marker such as \
                     `/*-- scss:defaults --*/` (variables) or `/*-- scss:rules --*/` (CSS \
                     rules)? The other markers are `scss:uses`, `scss:functions`, and \
                     `scss:mixins`. A plain stylesheet with no Sass in it can be listed \
                     under `css:` instead of `theme:`.",
                );
            if let Some(loc) = &location {
                b = b.with_location(loc.clone());
            }
            b.build()
        }
        // Everything else reaches us from the compile call itself:
        // `CompilationFailed` (the grass / dart-sass error, pointing
        // into the assembled bundle), `NoBoundaryMarkers` from a
        // built-in layer, `ThemeNotFound` for a missing embedded
        // resource, `Io`. None of these carry a span of their own, so
        // the caller's fallback location (the `theme:` value) is the
        // anchor (bd-jsvetdea).
        other => {
            let mut b = DiagnosticMessageBuilder::error("Theme SCSS compilation failed")
                .with_code("Q-14-6")
                .problem(format!(
                    "compiling the theme SCSS bundle failed:\n{}",
                    compile_failure_text(other)
                ))
                .add_hint(
                    "Does a variable in a theme file set a value Bootstrap's arithmetic \
                     cannot use (a `rem` layout width, a non-numeric font weight)? The \
                     reported line counts lines of the SCSS bundle Quarto assembles from \
                     Bootstrap, its own layers, and the files under `theme:`, so it often \
                     points into Quarto's own code reacting to a theme variable rather than \
                     at your file. If no custom theme or brand is configured, this is likely \
                     a Quarto bug; please report it.",
                );
            if let Some(loc) = &location {
                b = b.with_location(loc.clone());
            }
            b.build()
        }
    };

    ParseError::new(vec![diagnostic], source_context)
}

/// The text a compile failure shows the user: the grass / dart-sass
/// output verbatim (message, excerpt, and its `./stdin:LINE:COL`
/// pointer into the assembled bundle), minus the runtime's own
/// `SASS compilation error:` prefix, which would otherwise repeat
/// the diagnostic title. Other variants render their `Display` form.
fn compile_failure_text(err: &SassError) -> String {
    match err {
        SassError::CompilationFailed { message } => message
            .strip_prefix("SASS compilation error: ")
            .unwrap_or(message)
            .trim_end()
            .to_string(),
        other => other.to_string(),
    }
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
        SassError::InvalidScssFile { location, .. } => location.clone(),
        SassError::InvalidBrandFontWeight { location, .. } => location.clone(),
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
    fn invalid_brand_font_weight_renders_q_14_8_with_span_into_brand_file() {
        // bd-5fseopxy: the error carries the brand file it was read
        // from, and the converter must register *that* file as a
        // candidate on its own — the stage's candidate list only
        // knows about `_quarto.yml`, the document, and friends.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let brand_path = root.join("_brand.yml");
        let contents = "typography:\n  fonts:\n    - family: EB Garamond\n      source: google\n      weight: 700..400\n";
        std::fs::write(&brand_path, contents).unwrap();
        let start = contents.find("700..400").unwrap();
        let location =
            SourceInfo::original(file_id_for(&brand_path), start, start + "700..400".len());

        let err = SassError::InvalidBrandFontWeight {
            path: "typography.fonts[0].weight".to_string(),
            value: "700..400".to_string(),
            reason: "the range minimum 700 is greater than its maximum 400".to_string(),
            location: Some(location.clone()),
            brand_file: Some(brand_path.clone()),
        };

        // Deliberately no candidates: the brand file must come from
        // the error itself.
        let parse_err = sass_error_to_parse_error(&err, &[]);
        assert_eq!(parse_err.diagnostics.len(), 1);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-8"));
        assert_eq!(d.location.as_ref(), Some(&location));

        let opts = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered = strip_ansi(&d.to_text_with_options(Some(&parse_err.source_context), &opts));
        assert!(rendered.contains("Q-14-8"), "{rendered}");
        assert!(
            rendered.contains("typography.fonts[0].weight"),
            "must name the YAML path:\n{rendered}"
        );
        assert!(
            rendered.contains("700..400"),
            "must quote the value:\n{rendered}"
        );
        assert!(
            rendered.contains("greater than"),
            "must carry the reason:\n{rendered}"
        );
        // Line 5 of the fixture holds `weight: 700..400`; the gutter
        // marker proves the snippet was rendered against _brand.yml.
        assert!(
            rendered.contains("5 │"),
            "expected a snippet of the weight line:\n{rendered}"
        );
    }

    #[test]
    fn invalid_brand_font_weight_without_location_renders_span_less() {
        // Inline brand blocks synthesized without source info, or the
        // defensive emission-time path, have no span; the diagnostic
        // still carries code, path, value, and reason in prose.
        let err = SassError::InvalidBrandFontWeight {
            path: "typography.headings.weight".to_string(),
            value: "500..700".to_string(),
            reason: "a typography slot takes a single weight".to_string(),
            location: None,
            brand_file: None,
        };
        let parse_err = sass_error_to_parse_error(&err, &[]);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-8"));
        assert_eq!(d.location, None);
        let opts = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered = strip_ansi(&d.to_text_with_options(None, &opts));
        assert!(
            rendered.contains("typography.headings.weight"),
            "{rendered}"
        );
        assert!(rendered.contains("500..700"), "{rendered}");
        assert!(rendered.contains("single weight"), "{rendered}");
    }

    #[test]
    fn theme_diagnostic_code_is_registered_in_catalog() {
        // Belt-and-braces: every code emitted by
        // sass_error_to_parse_error must exist in the shared
        // catalog, under the 'theme' subsystem.
        // Query the catalog data directly (the codes live in
        // `quarto-error-catalog` now, not in `quarto-error-reporting`).
        // (Q-14-3, the interim dark-theme-ignored warning from
        // bd-o76p01wb, was retired when dual light/dark compilation
        // landed — bd-0pic6 phase A2.)
        for code in [
            "Q-14-1", "Q-14-2", "Q-14-4", "Q-14-5", "Q-14-6", "Q-14-7", "Q-14-8",
        ] {
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

    /// The grass text the stage sees for the bd-jsvetdea repro
    /// (`$grid-body-width: 52rem` in a user theme). Note the location
    /// is a line of the *assembled* bundle (`./stdin`), at Quarto's own
    /// `$grid-body-column-min` default — the user's file is never named.
    const GRASS_UNITS_ERROR: &str = "SASS compilation error: Error: Incompatible units px and rem.\n     \u{2577}\n3269 \u{2502} $grid-body-column-min: quarto-math.min(500px, $grid-body-column-max) !default;\n     \u{2502}                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^\n     \u{2575}\n./stdin:3269:24\n";

    #[test]
    fn compilation_failed_renders_with_q146_code_and_span() {
        // bd-jsvetdea: a grass failure while compiling the theme
        // bundle carries no location of its own (`CompilationFailed`
        // is the crate-boundary stringification of the grass error),
        // so the stage anchors it at the whole `theme:` value via the
        // fallback location. The diagnostic must carry Q-14-6, the
        // grass text verbatim, and a span on the `theme:` line.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let yaml_path = root.join("_quarto.yml");
        let contents =
            "project:\n  type: website\nformat:\n  html:\n    theme: [cosmo, bad.scss]\n";
        std::fs::write(&yaml_path, contents).unwrap();

        let value_start = contents.find("[cosmo").unwrap();
        let value_end = contents.find(']').unwrap() + 1;
        let location = SourceInfo::Original {
            file_id: file_id_for(&yaml_path),
            start_offset: value_start,
            end_offset: value_end,
        };

        let err = SassError::CompilationFailed {
            message: GRASS_UNITS_ERROR.to_string(),
        };
        let parse_err = sass_error_to_parse_error_at(
            &err,
            Some(location.clone()),
            &[(file_id_for(&yaml_path), yaml_path.clone())],
        );
        assert_eq!(parse_err.diagnostics.len(), 1);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-6"));
        assert!(
            d.title.contains("Theme SCSS compilation failed"),
            "title was: {}",
            d.title,
        );
        assert_eq!(d.location.as_ref(), Some(&location));

        let opts = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered = d.to_text_with_options(Some(&parse_err.source_context), &opts);
        assert!(
            rendered.contains("Q-14-6"),
            "rendered output missing code Q-14-6:\n{}",
            rendered,
        );
        assert!(
            rendered.contains("Incompatible units px and rem"),
            "rendered output must carry the grass message verbatim:\n{}",
            rendered,
        );
        assert!(
            rendered.contains("$grid-body-column-min"),
            "rendered output must carry the grass excerpt verbatim:\n{}",
            rendered,
        );
        let stripped = strip_ansi(&rendered);
        assert!(
            stripped.contains("5 \u{2502}"),
            "rendered output missing line marker for the `theme:` line:\n{}",
            stripped,
        );
    }

    #[test]
    fn compilation_failed_without_location_renders_span_less() {
        // No user theme configured (the default-bundle path) → no
        // `theme:` value to anchor at; the diagnostic still carries the
        // code and the grass text.
        let err = SassError::CompilationFailed {
            message: GRASS_UNITS_ERROR.to_string(),
        };
        let parse_err =
            sass_error_to_parse_error(&err, &[(FileId(0), PathBuf::from("/nonexistent"))]);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-6"));
        assert_eq!(d.location, None);
        let rendered = d.to_text(None);
        assert!(
            rendered.contains("Incompatible units px and rem"),
            "rendered output must carry the grass message:\n{}",
            rendered,
        );
    }

    #[test]
    fn invalid_scss_file_renders_with_q147_code_and_span() {
        // bd-qmpygp02: a `theme:` entry naming a file that exists but
        // has no `/*-- scss:... --*/` layer markers must lift into a
        // Q-14-7 diagnostic pointing at that entry and naming the
        // resolved path.
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let yaml_path = root.join("doc.qmd");
        let contents = "---\nformat:\n  html:\n    theme: [cosmo, nomarkers.scss]\n---\n";
        std::fs::write(&yaml_path, contents).unwrap();

        let entry_start = contents.find("nomarkers.scss").unwrap();
        let entry_end = entry_start + "nomarkers.scss".len();
        let location = SourceInfo::Original {
            file_id: file_id_for(&yaml_path),
            start_offset: entry_start,
            end_offset: entry_end,
        };

        let err = SassError::InvalidScssFile {
            path: root.join("nomarkers.scss"),
            location: Some(location.clone()),
        };
        let parse_err =
            sass_error_to_parse_error(&err, &[(file_id_for(&yaml_path), yaml_path.clone())]);
        assert_eq!(parse_err.diagnostics.len(), 1);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-7"));
        assert!(
            d.title.contains("no layer boundary markers"),
            "title was: {}",
            d.title,
        );
        assert_eq!(d.location.as_ref(), Some(&location));

        let opts = quarto_error_reporting::TextRenderOptions {
            enable_hyperlinks: false,
        };
        let rendered = d.to_text_with_options(Some(&parse_err.source_context), &opts);
        assert!(
            rendered.contains("Q-14-7"),
            "rendered output missing code Q-14-7:\n{}",
            rendered,
        );
        assert!(
            rendered.contains("nomarkers.scss"),
            "rendered output missing resolved path:\n{}",
            rendered,
        );
        assert!(
            rendered.contains("scss:defaults"),
            "hint must name at least one layer marker:\n{}",
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
    fn invalid_scss_file_falls_back_to_theme_value_location() {
        // The loader constructs `InvalidScssFile` without a location;
        // when the stage cannot match the path to a `theme:` entry it
        // anchors at the whole `theme:` value instead.
        let fallback = SourceInfo::Original {
            file_id: FileId(7),
            start_offset: 10,
            end_offset: 30,
        };
        let err = SassError::InvalidScssFile {
            path: PathBuf::from("/somewhere/nomarkers.scss"),
            location: None,
        };
        let parse_err = sass_error_to_parse_error_at(&err, Some(fallback.clone()), &[]);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-7"));
        assert_eq!(d.location.as_ref(), Some(&fallback));
    }

    #[test]
    fn invalid_scss_file_without_location_renders_span_less() {
        let err = SassError::InvalidScssFile {
            path: PathBuf::from("/somewhere/nomarkers.scss"),
            location: None,
        };
        let parse_err = sass_error_to_parse_error(&err, &[]);
        let d = &parse_err.diagnostics[0];
        assert_eq!(d.code.as_deref(), Some("Q-14-7"));
        assert_eq!(d.location, None);
    }

    #[test]
    fn explicit_location_wins_over_fallback() {
        // A variant that already carries its own span keeps it; the
        // fallback only fills in a `None`.
        let own = SourceInfo::Original {
            file_id: FileId(1),
            start_offset: 0,
            end_offset: 4,
        };
        let fallback = SourceInfo::Original {
            file_id: FileId(2),
            start_offset: 0,
            end_offset: 4,
        };
        let err = SassError::CustomThemeNotFound {
            path: PathBuf::from("/x/nope.scss"),
            location: Some(own.clone()),
        };
        let parse_err = sass_error_to_parse_error_at(&err, Some(fallback), &[]);
        assert_eq!(parse_err.diagnostics[0].location.as_ref(), Some(&own));
    }
}
