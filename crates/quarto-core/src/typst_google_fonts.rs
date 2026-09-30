/*
 * typst_google_fonts.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Download `source: google` brand fonts so Typst can use them.
//!
//! Typst is a native compiler: it only sees fonts that exist as files on
//! a `--font-path` directory. A `brand.typography.fonts` entry with
//! `source: google` therefore has to be fetched, which HTML output never
//! needs (the browser follows the `@import`). Mirrors Q1's
//! `command/render/pandoc.ts` typst branch:
//!
//! 1. request the Google Fonts CSS for the family,
//! 2. read the font file URLs out of its `@font-face` rules,
//! 3. download each file into `<project>/.quarto/typst/fonts`, skipping
//!    files already there.
//!
//! **Only TrueType/OpenType files are usable.** Typst cannot load
//! `woff`/`woff2`. Google serves TTF to clients that send no browser
//! `User-Agent` (which is what [`SystemRuntime::fetch_url`] does) and
//! woff2 to browsers, so a runtime that started sending a browser UA
//! would land in the "no usable format" diagnostic below rather than
//! break silently.
//!
//! The CSS is read with `cssparser` (the tokenizer browsers' CSS stack
//! is built on) rather than Q1's line-oriented `^ *src: (.*); *$`
//! regex, which silently misses a declaration that is wrapped, minified
//! onto one line, or has a quoted URL containing a comma or paren.
//!
//! [`SystemRuntime::fetch_url`]: quarto_system_runtime::SystemRuntime::fetch_url

use std::path::{Path, PathBuf};

use cssparser::{Delimiter, ParseError, Parser, ParserInput, Token};
use quarto_brand::{BrandFont, BrandFontGoogle, ResolvedBrand};
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

/// Where downloaded fonts live, relative to the project directory.
/// Matches Q1's `typst-font-cache` scratch path (`.quarto/typst/fonts`).
pub fn font_cache_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".quarto").join("typst").join("fonts")
}

/// One `src:` entry of an `@font-face` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSource {
    pub url: String,
    /// The `format('...')` hint, lowercased, when present.
    pub format: Option<String>,
}

/// One `@font-face` rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFace {
    pub family: Option<String>,
    pub style: Option<String>,
    pub weight: Option<String>,
    pub sources: Vec<FontSource>,
}

/// Every `@font-face` rule in `css`, in order. Malformed declarations
/// are skipped, not fatal: a face we can read nothing from simply has no
/// sources.
pub fn parse_font_faces(css: &str) -> Vec<FontFace> {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut faces = Vec::new();
    while let Ok(token) = parser.next().cloned() {
        let Token::AtKeyword(name) = &token else {
            continue;
        };
        if !name.eq_ignore_ascii_case("font-face") {
            continue;
        }
        // Prelude is empty; the next token is the `{}` block.
        if matches!(parser.next(), Ok(Token::CurlyBracketBlock))
            && let Ok(face) = parser
                .parse_nested_block(|block| Ok::<_, ParseError<'_, ()>>(parse_face_block(block)))
        {
            faces.push(face);
        }
    }
    faces
}

fn parse_face_block(block: &mut Parser<'_, '_>) -> FontFace {
    let mut face = FontFace::default();
    while let Ok(token) = block.next().cloned() {
        let Token::Ident(property) = token else {
            continue;
        };
        let property = property.to_ascii_lowercase();
        if block.expect_colon().is_err() {
            continue;
        }
        // One declaration's value: everything up to `;`, which is consumed.
        let _ = block.parse_until_after(Delimiter::Semicolon, |value| {
            match property.as_str() {
                "font-family" => face.family = Some(words(value)),
                "font-style" => face.style = Some(words(value)),
                "font-weight" => face.weight = Some(words(value)),
                "src" => face.sources.extend(parse_src(value)),
                _ => {}
            }
            Ok::<_, ParseError<'_, ()>>(())
        });
    }
    face
}

/// A value made of strings, identifiers and numbers, space-joined
/// (`"Fira Code"`, `Fira Code`, `100 900`).
fn words(value: &mut Parser<'_, '_>) -> String {
    let mut parts: Vec<String> = Vec::new();
    while let Ok(token) = value.next() {
        match token {
            Token::QuotedString(s) | Token::Ident(s) => parts.push(s.to_string()),
            Token::Number { value, .. } => parts.push(value.to_string()),
            _ => {}
        }
    }
    parts.join(" ")
}

/// The comma-separated `src:` list; `local(...)` entries and anything
/// without a URL are dropped.
fn parse_src(value: &mut Parser<'_, '_>) -> Vec<FontSource> {
    let entries = value.parse_comma_separated(|entry| {
        let mut url: Option<String> = None;
        let mut format: Option<String> = None;
        while let Ok(token) = entry.next().cloned() {
            match token {
                Token::UnquotedUrl(u) => url = Some(u.to_string()),
                Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                    url = entry
                        .parse_nested_block(|b| {
                            Ok::<_, ParseError<'_, ()>>(b.expect_string()?.to_string())
                        })
                        .ok();
                }
                Token::Function(name) if name.eq_ignore_ascii_case("format") => {
                    format = entry
                        .parse_nested_block(|b| {
                            let token = b.next()?.clone();
                            match token {
                                Token::QuotedString(s) | Token::Ident(s) => {
                                    Ok(s.to_ascii_lowercase())
                                }
                                t => Err(b.new_unexpected_token_error::<()>(t)),
                            }
                        })
                        .ok();
                }
                _ => {}
            }
        }
        Ok::<_, ParseError<'_, ()>>(url.map(|url| FontSource { url, format }))
    });
    entries.unwrap_or_default().into_iter().flatten().collect()
}

/// The font files Typst can use, plus what was offered but unusable.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UsableFiles {
    /// One URL per face, in order, deduplicated.
    pub urls: Vec<String>,
    /// Formats of faces for which no TTF/OTF was offered, e.g. `woff2`.
    pub unusable_formats: Vec<String>,
}

/// Pick, for each face, the first source Typst can load.
pub fn usable_files(faces: &[FontFace]) -> UsableFiles {
    let mut out = UsableFiles::default();
    for face in faces {
        let usable = face.sources.iter().find(|s| is_typst_loadable(s));
        match usable {
            Some(source) => {
                if !out.urls.contains(&source.url) {
                    out.urls.push(source.url.clone());
                }
            }
            None => {
                for source in &face.sources {
                    let format = source.format.clone().unwrap_or_else(|| "unknown".into());
                    if !out.unusable_formats.contains(&format) {
                        out.unusable_formats.push(format);
                    }
                }
            }
        }
    }
    out
}

fn is_typst_loadable(source: &FontSource) -> bool {
    match source.format.as_deref() {
        Some(f) => matches!(f, "truetype" | "opentype" | "collection"),
        None => {
            let path = source.url.split(['?', '#']).next().unwrap_or("");
            let lower = path.to_ascii_lowercase();
            [".ttf", ".otf", ".ttc"].iter().any(|e| lower.ends_with(e))
        }
    }
}

/// Cache location for a font URL: `<host>/<path...>`, as in Q1. `None`
/// when the URL has no usable path or tries to escape the cache.
fn cache_relative_path(url: &str) -> Option<PathBuf> {
    let rest = url.split_once("://")?.1;
    let rest = rest.split(['?', '#']).next()?;
    let mut path = PathBuf::new();
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        path.push(part);
    }
    (path.components().count() >= 2).then_some(path)
}

/// The `source: google` and `source: bunny` fonts of a brand with the
/// `typography.fonts[i]` path used in diagnostics.
fn remote_fonts(brand: &ResolvedBrand) -> Vec<(String, &BrandFont)> {
    brand
        .brand
        .fonts()
        .iter()
        .enumerate()
        .filter(|(_, f)| matches!(f, BrandFont::Google(_) | BrandFont::Bunny(_)))
        .map(|(i, f)| (format!("typography.fonts[{i}]"), f))
        .collect()
}

fn unavailable(title: &str, problem: String) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(title)
        .with_code("Q-21-4")
        .problem(problem)
        .add_hint(
            "Typst will use a fallback font. Use `source: file` to supply the font files yourself",
        )
        .build()
}

/// Download the brand's `source: google` fonts into `cache_dir`.
///
/// `fetch` returns a URL's body. Never fails the render: each problem
/// becomes a `Q-21-4` warning and that font is skipped (Typst then falls
/// back, as it would have before this existed).
pub fn stage_brand_fonts(
    brand: &ResolvedBrand,
    cache_dir: &Path,
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Vec<DiagnosticMessage> {
    let mut diagnostics = Vec::new();
    for (font_path, font) in remote_fonts(brand) {
        match font {
            BrandFont::Bunny(b) => diagnostics.push(unavailable(
                "Bunny font not supported for Typst",
                format!(
                    "Font '{}' uses `source: bunny`, which Typst output does not support.",
                    b.family
                ),
            )),
            BrandFont::Google(g) => {
                stage_google_font(g, &font_path, cache_dir, fetch, &mut diagnostics)
            }
            _ => {}
        }
    }
    diagnostics
}

fn stage_google_font(
    font: &BrandFontGoogle,
    font_path: &str,
    cache_dir: &Path,
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let title = "Google font not available to Typst";
    let family = &font.family;

    let css_url = match quarto_sass::brand_layer::google_font_css_url(font, font_path) {
        Ok(url) => url,
        // The same error surfaces, with a source span, from the theme
        // pipeline; don't report it twice.
        Err(_) => return,
    };
    let css = match fetch(&css_url) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => {
            diagnostics.push(unavailable(
                title,
                format!("Could not fetch the Google Fonts description for '{family}': {e}"),
            ));
            return;
        }
    };

    let files = usable_files(&parse_font_faces(&css));
    if files.urls.is_empty() {
        let detail = if files.unusable_formats.is_empty() {
            "Google Fonts returned no font files for it (is the family name spelled correctly?)"
                .to_string()
        } else {
            format!(
                "Google Fonts only offered {}, and Typst needs TrueType or OpenType",
                files.unusable_formats.join(", ")
            )
        };
        diagnostics.push(unavailable(title, format!("Font '{family}': {detail}.")));
        return;
    }

    for url in &files.urls {
        let Some(relative) = cache_relative_path(url) else {
            diagnostics.push(unavailable(
                title,
                format!("Font '{family}' lists a file URL that cannot be cached: {url}"),
            ));
            continue;
        };
        let dest = cache_dir.join(relative);
        if dest.exists() {
            continue;
        }
        if let Err(e) = download(fetch, url, &dest) {
            diagnostics.push(unavailable(
                title,
                format!("Could not download a file for '{family}' from {url}: {e}"),
            ));
        }
    }
}

/// Fetch `url` into `dest` via a temp file, so an interrupted write
/// never leaves a truncated font that later renders would trust.
fn download(
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    url: &str,
    dest: &Path,
) -> Result<(), String> {
    let bytes = fetch(url)?;
    if bytes.is_empty() {
        return Err("empty response".into());
    }
    let parent = dest.parent().ok_or("no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let partial = dest.with_extension("part");
    std::fs::write(&partial, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, dest).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape of a real `fonts.googleapis.com/css2` response for a client
    /// without a browser User-Agent.
    const GOOGLE_TTF_CSS: &str = "\
/* latin */
@font-face {
  font-family: 'Fira Code';
  font-style: normal;
  font-weight: 400;
  src: url(https://fonts.gstatic.com/s/firacode/v27/regular.ttf) format('truetype');
}
/* latin */
@font-face {
  font-family: 'Fira Code';
  font-style: normal;
  font-weight: 700;
  src: url(https://fonts.gstatic.com/s/firacode/v27/bold.ttf) format('truetype');
}
";

    #[test]
    fn parses_google_ttf_response() {
        let faces = parse_font_faces(GOOGLE_TTF_CSS);
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0].family.as_deref(), Some("Fira Code"));
        assert_eq!(faces[0].style.as_deref(), Some("normal"));
        assert_eq!(faces[0].weight.as_deref(), Some("400"));
        assert_eq!(
            faces[1].sources,
            vec![FontSource {
                url: "https://fonts.gstatic.com/s/firacode/v27/bold.ttf".into(),
                format: Some("truetype".into()),
            }]
        );
    }

    /// The whole stylesheet on one line, quoted URLs, extra whitespace,
    /// comments inside the rule, and a `local()` entry with a comma in
    /// its name — every one of which a line-oriented `src:` regex gets
    /// wrong.
    #[test]
    fn parses_minified_and_quirky_css() {
        let css = "@font-face{font-family:\"Fira Code\";/* c */src:local('Fira, Code'),url( \"https://h/a,b(1).woff2\" )format(\"woff2\"),url(https://h/a.ttf) format('truetype');font-weight:300 700}@font-face{font-family:X;src:url(https://h/x.otf)}";
        let faces = parse_font_faces(css);
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0].weight.as_deref(), Some("300 700"));
        assert_eq!(faces[0].sources.len(), 2);
        assert_eq!(faces[0].sources[0].url, "https://h/a,b(1).woff2");
        assert_eq!(faces[0].sources[0].format.as_deref(), Some("woff2"));
        assert_eq!(faces[0].sources[1].url, "https://h/a.ttf");
        assert_eq!(faces[1].family.as_deref(), Some("X"));
        assert_eq!(faces[1].sources[0].format, None);
    }

    #[test]
    fn unusable_declarations_do_not_abort_the_parse() {
        let css = "@font-face { font-family: ; src: ; garbage } @font-face { src: url(https://h/ok.ttf) format('truetype'); }";
        let faces = parse_font_faces(css);
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[1].sources[0].url, "https://h/ok.ttf");
    }

    #[test]
    fn non_font_face_rules_are_ignored() {
        let css = "@import url(x.css); body { src: url(no.ttf) } @media print { @font-face { src: url(https://h/n.ttf) } }";
        // The nested @font-face inside @media is skipped with its block;
        // Google never emits one.
        assert!(parse_font_faces(css).is_empty());
    }

    #[test]
    fn picks_first_loadable_source_and_dedupes() {
        let css = "@font-face{src:url(https://h/a.woff2) format('woff2'),url(https://h/a.ttf) format('truetype')}\
                   @font-face{src:url(https://h/a.ttf) format('truetype')}\
                   @font-face{src:url(https://h/b.otf)}";
        let files = usable_files(&parse_font_faces(css));
        assert_eq!(
            files.urls,
            vec!["https://h/a.ttf".to_string(), "https://h/b.otf".to_string()]
        );
        assert!(files.unusable_formats.is_empty());
    }

    #[test]
    fn woff2_only_reports_formats() {
        let css = "@font-face{src:url(https://h/a.woff2) format('woff2')}";
        let files = usable_files(&parse_font_faces(css));
        assert!(files.urls.is_empty());
        assert_eq!(files.unusable_formats, vec!["woff2".to_string()]);
    }

    #[test]
    fn cache_path_keeps_host_and_path_and_rejects_escapes() {
        assert_eq!(
            cache_relative_path("https://fonts.gstatic.com/s/firacode/v27/a.ttf?x=1"),
            Some(PathBuf::from("fonts.gstatic.com/s/firacode/v27/a.ttf"))
        );
        assert_eq!(cache_relative_path("https://h/../../etc/passwd"), None);
        assert_eq!(cache_relative_path("https://h"), None);
        assert_eq!(cache_relative_path("not a url"), None);
    }

    fn brand_with_fonts(yaml: &str) -> ResolvedBrand {
        let unified = quarto_brand::UnifiedBrand::from_yaml_str(yaml).expect("brand parses");
        ResolvedBrand::new(unified.split().light, None)
    }

    #[test]
    fn stage_downloads_once_and_reuses_cache() {
        let brand = brand_with_fonts(
            "typography:\n  fonts:\n    - family: Fira Code\n      source: google\n",
        );
        let dir = tempfile::tempdir().unwrap();
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let fetch = |url: &str| -> Result<Vec<u8>, String> {
            calls.borrow_mut().push(url.to_string());
            if url.contains("fonts.googleapis.com") {
                Ok(GOOGLE_TTF_CSS.as_bytes().to_vec())
            } else {
                Ok(b"FONTBYTES".to_vec())
            }
        };

        let diags = stage_brand_fonts(&brand, dir.path(), &fetch);
        assert!(diags.is_empty(), "{diags:?}");
        assert!(
            dir.path()
                .join("fonts.gstatic.com/s/firacode/v27/regular.ttf")
                .is_file()
        );
        assert!(
            dir.path()
                .join("fonts.gstatic.com/s/firacode/v27/bold.ttf")
                .is_file()
        );
        assert_eq!(calls.borrow().len(), 3, "1 css + 2 files");

        calls.borrow_mut().clear();
        assert!(stage_brand_fonts(&brand, dir.path(), &fetch).is_empty());
        assert_eq!(
            calls.borrow().len(),
            1,
            "second render refetches only the css, not the files"
        );
    }

    #[test]
    fn stage_warns_instead_of_failing_when_offline() {
        let brand = brand_with_fonts(
            "typography:\n  fonts:\n    - family: Fira Code\n      source: google\n",
        );
        let dir = tempfile::tempdir().unwrap();
        let diags = stage_brand_fonts(&brand, dir.path(), &|_| Err("no route to host".into()));
        assert_eq!(diags.len(), 1);
        let text = format!("{:?}", diags[0]);
        assert!(text.contains("Q-21-4") && text.contains("Fira Code") && text.contains("no route"));
    }

    #[test]
    fn stage_explains_woff2_only_response() {
        let brand = brand_with_fonts(
            "typography:\n  fonts:\n    - family: Fira Code\n      source: google\n",
        );
        let dir = tempfile::tempdir().unwrap();
        let css = "@font-face{font-family:'Fira Code';src:url(https://h/a.woff2) format('woff2')}";
        let diags = stage_brand_fonts(&brand, dir.path(), &|_| Ok(css.as_bytes().to_vec()));
        assert_eq!(diags.len(), 1);
        assert!(format!("{:?}", diags[0]).contains("woff2"));
    }

    #[test]
    fn bunny_fonts_warn() {
        let brand =
            brand_with_fonts("typography:\n  fonts:\n    - family: Inter\n      source: bunny\n");
        let dir = tempfile::tempdir().unwrap();
        let diags = stage_brand_fonts(&brand, dir.path(), &|_| panic!("no fetch for bunny"));
        assert_eq!(diags.len(), 1);
    }
}
