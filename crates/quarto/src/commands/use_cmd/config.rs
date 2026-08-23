//! Inspecting and editing a project's `_quarto.yml` (bd-1vlw8).
//!
//! Two jobs, both in service of one rule: **never damage a file the
//! user hand-wrote.**
//!
//! 1. *Inspect* — find any existing `brand:` declaration (top level or
//!    `format.<fmt>.brand`) so `q2 use brand` can refuse rather than
//!    write a second, shadowed one; and establish that the file is a
//!    shape we can safely append to.
//! 2. *Edit* — produce the text block to append. Appending is the
//!    whole edit strategy: a key at column 0 closes every preceding
//!    nested block, so no existing byte is ever rewritten and comments,
//!    key order, and formatting survive untouched. A serde round-trip
//!    would silently discard all three.
//!
//! Parsing goes through `quarto_yaml::parse_file`, which is the same
//! source-located parser the render pipeline uses — so "what counts as
//! a brand declaration" here cannot drift from what actually takes
//! effect at render time.

use std::fmt;
use std::path::{Path, PathBuf};

use quarto_yaml::{YamlWithSourceInfo, parse_file};
use yaml_rust2::Yaml;

use crate::commands::common::plan::CommandFailure;

/// The two spellings Quarto accepts for a project config, in the order
/// [`quarto_core`]'s project loader probes them.
pub const CONFIG_FILENAMES: [&str; 2] = ["_quarto.yml", "_quarto.yaml"];

/// Where in `_quarto.yml` an existing `brand:` declaration was found.
#[derive(Debug, PartialEq, Eq)]
pub enum BrandDeclSite {
    /// Top-level `brand:`.
    TopLevel,
    /// `format.<format>.brand:`.
    Format(String),
}

impl fmt::Display for BrandDeclSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrandDeclSite::TopLevel => f.write_str("brand"),
            BrandDeclSite::Format(fmt_name) => write!(f, "format.{fmt_name}.brand"),
        }
    }
}

/// An existing brand declaration: where it is, what it says, and which
/// line it is on.
#[derive(Debug)]
pub struct BrandDeclaration {
    pub site: BrandDeclSite,
    /// A short rendering of the declared value, for the diagnostic.
    /// A path declaration renders as the path; an inline block renders
    /// as `(inline brand block)` rather than dumping the whole map.
    pub value_summary: String,
    /// 1-based line of the `brand` key.
    pub line: usize,
    /// Byte range of the declared *value*, when it is a plain scalar
    /// that could be repointed in place. `None` for inline blocks and
    /// anything else we will not rewrite.
    pub value_span: Option<(usize, usize)>,
}

/// A parsed, append-safe project config.
#[derive(Debug)]
pub struct ProjectConfigFile {
    /// The config filename as the user sees it (`_quarto.yml`).
    pub filename: String,
    /// Raw text, exactly as read.
    pub text: String,
    /// `None` when the file holds no YAML document at all (empty, or
    /// nothing but comments). That is a perfectly appendable config —
    /// appending `brand:` to it yields a valid single-key mapping — but
    /// `quarto_yaml::parse_file` reports it as a parse error rather
    /// than a null document, so it is modeled explicitly here instead
    /// of being mistaken for malformed YAML.
    parsed: Option<YamlWithSourceInfo>,
}

/// Locate the project root by walking up from `start`, looking for
/// either config spelling.
///
/// Mirrors `quarto_core::project::ProjectContext::find_project_config`'s
/// probe order. We do not reuse that function directly because it also
/// *parses* into a `ProjectConfig` (resolving brand, project type, and
/// more), and a config we are about to refuse for being unparseable
/// must not blow up in a resolution step first — the diagnostic we owe
/// the user is about editability, not about project semantics.
pub fn find_project_config(start: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut current = start.to_path_buf();
    loop {
        for name in CONFIG_FILENAMES {
            let candidate = current.join(name);
            if candidate.is_file() {
                return Some((current.clone(), candidate));
            }
        }
        if !current.pop() {
            return None;
        }
    }
}

impl ProjectConfigFile {
    /// Read and parse `path`, rejecting any shape we cannot safely
    /// append a top-level key to.
    ///
    /// Rejected shapes and why:
    ///
    /// - **Multi-document stream** (`---` / `...` separators). Appending
    ///   would add the key to the *last* document, which is not the one
    ///   Quarto reads. Silently editing the wrong document is worse
    ///   than refusing.
    /// - **Top-level sequence or scalar.** A top-level `brand:` key
    ///   cannot coexist with a sequence root; the result would not
    ///   parse.
    ///
    /// An empty config is fine — appending to it yields a valid
    /// single-key mapping.
    pub fn load(path: &Path) -> Result<Self, CommandFailure> {
        let filename = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );

        let text = std::fs::read_to_string(path).map_err(|e| {
            CommandFailure::new(
                format!("Failed to read {filename}"),
                format!("{}: {e}", path.display()),
            )
        })?;

        if let Some(marker) = multi_document_marker(&text) {
            return Err(CommandFailure::new(
                format!("Cannot add a brand to {filename}"),
                format!(
                    "{} contains more than one YAML document (found a `{marker}` \
                     document marker). Quarto reads only the first, so appending \
                     `brand:` could not take effect. Add `brand: _brand.yml` to the \
                     first document by hand instead.",
                    path.display()
                ),
            ));
        }

        let parsed = if has_no_document(&text) {
            None
        } else {
            let parsed = parse_file(&text, &path.to_string_lossy()).map_err(|e| {
                CommandFailure::new(
                    format!("Failed to parse {filename}"),
                    format!("{}: {e}", path.display()),
                )
            })?;

            if !parsed.is_hash() {
                return Err(CommandFailure::new(
                    format!("Cannot add a brand to {filename}"),
                    format!(
                        "{} does not contain a top-level YAML mapping, so a `brand:` \
                         key cannot be added to it. A Quarto project config should look \
                         like `project:` / `format:` / … at the top level.",
                        path.display()
                    ),
                ));
            }
            Some(parsed)
        };

        Ok(Self {
            filename,
            text,
            parsed,
        })
    }

    /// Find an existing brand declaration, if any.
    ///
    /// Checks the two places a project config can put one: top-level
    /// `brand:` and `format.<fmt>.brand:`. Deliberately does *not*
    /// consult `_metadata.yml` layers or document front matter — those
    /// can legitimately carry a per-directory or per-document brand,
    /// and refusing on them would block a supported pattern.
    pub fn brand_declaration(&self) -> Option<BrandDeclaration> {
        let parsed = self.parsed.as_ref()?;
        if let Some(entry) = hash_entry(parsed, "brand") {
            return Some(BrandDeclaration {
                site: BrandDeclSite::TopLevel,
                value_summary: summarize_value(&entry.value),
                line: self.line_of(entry.key_span.start_offset()),
                value_span: self.scalar_value_span(entry),
            });
        }

        let formats = parsed.get_hash_value("format")?;
        let entries = formats.as_hash()?;
        for fmt_entry in entries {
            let Yaml::String(fmt_name) = &fmt_entry.key.yaml else {
                continue;
            };
            if let Some(brand_entry) = hash_entry(&fmt_entry.value, "brand") {
                return Some(BrandDeclaration {
                    site: BrandDeclSite::Format(fmt_name.clone()),
                    value_summary: summarize_value(&brand_entry.value),
                    line: self.line_of(brand_entry.key_span.start_offset()),
                    value_span: self.scalar_value_span(brand_entry),
                });
            }
        }
        None
    }

    /// Byte range of a hash entry's value, but only when the value is a
    /// plain string scalar *and* the bytes at that range read back
    /// exactly as the parsed string.
    ///
    /// The read-back check is the point. A quoted, escaped, or folded
    /// scalar has a source range whose bytes differ from its parsed
    /// value (`"a.yml"` vs `a.yml`), and replacing such a range with a
    /// bare replacement would drop the quoting. Rather than model every
    /// YAML scalar style, we simply decline to rewrite anything whose
    /// source text is not literally its value — the caller then reports
    /// that it cannot repoint the declaration, which is honest and
    /// leaves the user's file alone.
    fn scalar_value_span(&self, entry: &quarto_yaml::YamlHashEntry) -> Option<(usize, usize)> {
        let Yaml::String(parsed) = &entry.value.yaml else {
            return None;
        };
        // The two raw accessors below are safe *only* because of the
        // byte-equality check on the next line. The accessor rule
        // (audit findings section 1) otherwise forbids
        // `start_offset()`/`end_offset()` on a span that might be a
        // `Concat`, or a `Substring` over one: those report *content*
        // coordinates, not file offsets — measured, findings section 8,
        // fixture `C`: `start_offset() == 0` and `end_offset() ==`
        // content length. On this path the shape is not that today —
        // `parse_file` (`ProjectConfigFile::load`) parses the whole
        // `_quarto.yml`, so `value_span` is an `Original` over its own
        // file — but the check does not depend on that holding. Handed a
        // `Concat`-derived span, `self.text[start..end]` would be an
        // unrelated prefix of the file rather than the value's text, the
        // comparison would fail, and the function would return `None`.
        // So it refuses rather than mis-points. (The one coincidence the
        // check cannot catch is a file whose first `end` bytes literally
        // spell `parsed`.)
        //
        // That refusal is why the `map_offset`-hull simplification —
        // declined by Plan 2 (R-8, hand-off item 1) — is declined
        // **permanently** as of 2026-08-23 (Plan 3 Phase 8), with no
        // strand: the function is limited, not wrong.
        let start = entry.value_span.start_offset();
        let end = entry.value_span.end_offset();
        if self.text.get(start..end)? == parsed.as_str() {
            Some((start, end))
        } else {
            None
        }
    }

    /// 1-based line number containing byte `offset`.
    fn line_of(&self, offset: usize) -> usize {
        self.text
            .get(..offset)
            .map_or(1, |prefix| prefix.matches('\n').count() + 1)
    }
}

/// The text block appended to `_quarto.yml` to declare `brand_path`.
///
/// The leading blank line separates the key from whatever precedes it;
/// the comment records where the key came from, so a reader who did not
/// run the command is not left wondering.
pub fn brand_declaration_block(brand_path: &str) -> String {
    format!("\n# Added by `q2 use brand`\nbrand: {brand_path}\n")
}

/// Detect a second YAML document.
///
/// Only markers at column 0 count: an indented `---` is block-scalar
/// content, not a separator. A `---` on the very first line *opens* the
/// first document rather than starting a second, so it is not itself a
/// multi-document signal — any later one is. `...` closes a document
/// explicitly and always counts.
///
/// `str::lines()` strips a trailing `\r`, so CRLF files are handled
/// without a separate branch.
fn multi_document_marker(text: &str) -> Option<&'static str> {
    for (idx, line) in text.lines().enumerate() {
        if line == "..." || line.starts_with("... ") {
            return Some("...");
        }
        if idx > 0 && (line == "---" || line.starts_with("--- ")) {
            return Some("---");
        }
    }
    None
}

/// True when the text carries no YAML document: empty, whitespace, or
/// nothing but comments and document markers.
///
/// `quarto_yaml::parse_file` returns "No YAML document found" for these,
/// which is *correct* for a parser but wrong as a user-facing verdict
/// here — an empty `_quarto.yml` is a config we can append to, not a
/// broken one.
fn has_no_document(text: &str) -> bool {
    text.lines().all(|line| {
        let t = line.trim();
        t.is_empty() || t.starts_with('#') || t == "---"
    })
}

/// Look up `key` in a hash node, returning the whole entry (so callers
/// get the key span for diagnostics, not just the value).
fn hash_entry<'a>(
    node: &'a YamlWithSourceInfo,
    key: &str,
) -> Option<&'a quarto_yaml::YamlHashEntry> {
    node.as_hash()?
        .iter()
        .find(|e| matches!(&e.key.yaml, Yaml::String(k) if k == key))
}

/// A short, safe rendering of a declared brand value for an error
/// message. Inline blocks are summarized rather than dumped so the
/// diagnostic stays one line.
fn summarize_value(value: &YamlWithSourceInfo) -> String {
    match &value.yaml {
        Yaml::String(s) => s.clone(),
        Yaml::Hash(_) => "(inline brand block)".to_string(),
        Yaml::Array(_) => "(list)".to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => "null".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn load_from(text: &str) -> Result<ProjectConfigFile, CommandFailure> {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("_quarto.yml");
        std::fs::write(&path, text).unwrap();
        ProjectConfigFile::load(&path)
    }

    #[test]
    fn plain_mapping_loads() {
        assert!(load_from("project:\n  type: website\n").is_ok());
    }

    #[test]
    fn leading_document_marker_is_not_multi_document() {
        // A single document that happens to open with `---`.
        assert!(load_from("---\nproject:\n  type: website\n").is_ok());
    }

    #[test]
    fn empty_config_is_appendable() {
        assert!(load_from("").is_ok());
        assert!(load_from("# just a comment\n").is_ok());
    }

    #[test]
    fn second_document_is_rejected() {
        let err = load_from("project:\n  type: website\n---\nproject:\n  type: book\n")
            .expect_err("a multi-doc stream must be refused");
        assert!(err.0.to_text(None).contains("more than one YAML document"));
    }

    #[test]
    fn crlf_document_markers_are_detected() {
        // `str::lines()` strips the `\r`, so no separate branch is
        // needed — but a regression here would silently let a
        // multi-document CRLF config through to be edited wrongly.
        let err = load_from("project:\r\n  type: website\r\n---\r\nproject:\r\n")
            .expect_err("a CRLF multi-doc stream must be refused");
        assert!(err.0.to_text(None).contains("more than one YAML document"));
    }

    #[test]
    fn explicit_document_end_is_rejected() {
        let err = load_from("project:\n  type: website\n...\n").expect_err("`...` must be refused");
        assert!(err.0.to_text(None).contains("more than one YAML document"));
    }

    #[test]
    fn document_marker_inside_a_block_scalar_is_not_a_separator() {
        // The `---` is indented, so it is content, not a separator.
        let text = "project:\n  title: |\n    ---\n    still the same doc\n";
        assert!(
            load_from(text).is_ok(),
            "an indented `---` is block-scalar content, not a document marker"
        );
    }

    #[test]
    fn top_level_sequence_is_rejected() {
        let err = load_from("- one\n- two\n").expect_err("a sequence root must be refused");
        assert!(err.0.to_text(None).contains("top-level YAML mapping"));
    }

    #[test]
    fn no_declaration_when_absent() {
        let cfg = load_from("project:\n  type: website\n").unwrap();
        assert!(cfg.brand_declaration().is_none());
    }

    #[test]
    fn top_level_declaration_is_found_with_value_and_line() {
        let cfg = load_from("project:\n  type: website\nbrand: other.yml\n").unwrap();
        let decl = cfg.brand_declaration().expect("declaration");
        assert_eq!(decl.site, BrandDeclSite::TopLevel);
        assert_eq!(decl.value_summary, "other.yml");
        assert_eq!(decl.line, 3);
    }

    #[test]
    fn format_scoped_declaration_is_found() {
        let cfg =
            load_from("project:\n  type: website\nformat:\n  html:\n    brand: b.yml\n").unwrap();
        let decl = cfg.brand_declaration().expect("declaration");
        assert_eq!(decl.site, BrandDeclSite::Format("html".to_string()));
        assert_eq!(decl.site.to_string(), "format.html.brand");
        assert_eq!(decl.value_summary, "b.yml");
    }

    #[test]
    fn inline_block_declaration_is_summarized_not_dumped() {
        let cfg = load_from("brand:\n  color:\n    primary: red\n").unwrap();
        let decl = cfg.brand_declaration().expect("declaration");
        assert_eq!(decl.value_summary, "(inline brand block)");
    }

    #[test]
    fn a_brand_key_nested_under_something_else_is_not_a_declaration() {
        // `website.brand` is not a place Quarto reads a brand from;
        // treating it as one would refuse for no reason.
        let cfg = load_from("website:\n  brand: nope.yml\n").unwrap();
        assert!(cfg.brand_declaration().is_none());
    }

    #[test]
    fn declaration_block_is_separated_and_self_documenting() {
        let block = brand_declaration_block("_brand.yml");
        assert!(block.starts_with('\n'), "must not abut the previous key");
        assert!(block.contains("q2 use brand"));
        assert!(block.ends_with("brand: _brand.yml\n"));
    }

    #[test]
    fn find_project_config_walks_up() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("_quarto.yml"), "project:\n").unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();

        let (root, config) = find_project_config(&nested).expect("found");
        assert_eq!(root, dir.path());
        assert_eq!(config.file_name().unwrap(), "_quarto.yml");
    }

    #[test]
    fn find_project_config_accepts_the_yaml_spelling() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("_quarto.yaml"), "project:\n").unwrap();
        let (_, config) = find_project_config(dir.path()).expect("found");
        assert_eq!(config.file_name().unwrap(), "_quarto.yaml");
    }
}
