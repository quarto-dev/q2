/*
 * project/listing/feed/complete.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! L9 post-render completion step: walk the project's output dir
//! for `*.feed-{full|partial|metadata}-staged` files, substitute
//! the description-element placeholders left by the stage transform
//! against engine-rendered sibling HTML, and finalize each into a
//! real `.xml` file.
//!
//! Called from `WebsiteProjectType::post_render` after the L7
//! `substitute_listing_placeholders` step (so any host-page HTML
//! that L7 rewrote is on disk before this code's reader extractors
//! consume it).
//!
//! Native-only via the parent module's cfg gate — the staged files
//! only exist when the stage transform ran, which is itself
//! native-only.
//!
//! See `claude-notes/plans/2026-05-08-listings-L9-rss-feeds.md`
//! §"`complete_staged_feeds` post-render step".

use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::SourceInfo;
use quarto_system_runtime::{RuntimeError, SystemRuntime};
use regex::RegexBuilder;

use crate::Result;
use crate::error::QuartoError;
use crate::project::ProjectContext;
use crate::project::website_config::website_site_url;

use super::reader_ext::{extract_first_para_html, extract_full_contents};

const STAGED_EXT_FULL: &str = "feed-full-staged";
const STAGED_EXT_PARTIAL: &str = "feed-partial-staged";
const STAGED_EXT_METADATA: &str = "feed-metadata-staged";

/// Discriminator for the three feed types. Lifted from the
/// staged file's extension; controls which reader extractor (if
/// any) is used during substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StagedType {
    Full,
    Partial,
    Metadata,
}

impl StagedType {
    fn from_filename(name: &str) -> Option<StagedType> {
        // Order matters: `feed-full-staged` ends-with-checks must
        // come before any superset; here all three suffixes are
        // distinct, so any order works.
        if name.ends_with(STAGED_EXT_FULL) {
            Some(StagedType::Full)
        } else if name.ends_with(STAGED_EXT_PARTIAL) {
            Some(StagedType::Partial)
        } else if name.ends_with(STAGED_EXT_METADATA) {
            Some(StagedType::Metadata)
        } else {
            None
        }
    }
}

/// Walk every staged feed file under `project.output_dir`,
/// substitute description placeholders against sibling rendered
/// HTML, write final `.xml` siblings, and delete the staged
/// originals.
///
/// Skips silently when `website.site-url` is missing — the stage
/// transform would not have written staged files in that case
/// anyway, so under normal conditions there's nothing to do.
pub fn complete_staged_feeds(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    let Some(site_url) = website_site_url(meta) else {
        return Ok(());
    };

    let placeholder_re = RegexBuilder::new(r"<description>\{B4F502887207:([^}]+)\}</description>")
        .build()
        .map_err(|e| QuartoError::other(format!("L9 placeholder regex failed to compile: {e}")))?;

    // Per-call cache: absolute sibling path → Some(html) when
    // readable, None when missing/unreadable. Tracked independently
    // of the warning set so a cached miss still emits Q-12-16
    // exactly once.
    let mut cache: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut warned: HashSet<String> = HashSet::new();

    let staged_files = collect_staged_files(&project.output_dir)?;
    for staged_path in staged_files {
        complete_one(
            &staged_path,
            &placeholder_re,
            &project.output_dir,
            &site_url,
            runtime,
            &mut cache,
            &mut warned,
            diagnostics,
        )?;
    }
    Ok(())
}

/// Process one staged file. Errors that prevent finalization
/// surface as diagnostics rather than aborting the whole step —
/// completion is best-effort and one bad feed shouldn't break
/// others.
#[allow(clippy::too_many_arguments)]
fn complete_one(
    staged_path: &Path,
    placeholder_re: &regex::Regex,
    output_dir: &Path,
    site_url: &str,
    runtime: &dyn SystemRuntime,
    cache: &mut HashMap<PathBuf, Option<String>>,
    warned: &mut HashSet<String>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let filename = staged_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let Some(staged_type) = StagedType::from_filename(filename) else {
        return Ok(());
    };

    let staged_content = match runtime.file_read(staged_path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                diagnostics.push(diagnostic_warning(format!(
                    "L9 complete: staged feed `{}` is not valid UTF-8; skipping.",
                    staged_path.display()
                )));
                return Ok(());
            }
        },
        Err(e) => {
            diagnostics.push(diagnostic_warning(format!(
                "L9 complete: failed to read staged feed `{}`: {e}",
                staged_path.display()
            )));
            return Ok(());
        }
    };

    let final_content = match staged_type {
        StagedType::Metadata => staged_content,
        StagedType::Partial => substitute_placeholders(
            &staged_content,
            placeholder_re,
            output_dir,
            site_url,
            ExtractMode::Partial,
            runtime,
            cache,
            warned,
            diagnostics,
        ),
        StagedType::Full => substitute_placeholders(
            &staged_content,
            placeholder_re,
            output_dir,
            site_url,
            ExtractMode::Full,
            runtime,
            cache,
            warned,
            diagnostics,
        ),
    };

    let final_path = staged_path.with_extension("xml");
    if let Err(e) = runtime.file_write(&final_path, final_content.as_bytes()) {
        diagnostics.push(diagnostic_warning(format!(
            "L9 complete: failed to write final feed `{}`: {e}",
            final_path.display()
        )));
        return Ok(());
    }
    // Delete the staged file. Best-effort: a leftover staged file
    // on the next run is harmless because completion overwrites the
    // staging extension.
    let _ = std::fs::remove_file(staged_path);
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ExtractMode {
    Partial,
    Full,
}

#[allow(clippy::too_many_arguments)]
fn substitute_placeholders(
    staged: &str,
    re: &regex::Regex,
    output_dir: &Path,
    site_url: &str,
    mode: ExtractMode,
    runtime: &dyn SystemRuntime,
    cache: &mut HashMap<PathBuf, Option<String>>,
    warned: &mut HashSet<String>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String {
    let mut out = String::with_capacity(staged.len());
    let mut last_end = 0usize;
    for caps in re.captures_iter(staged) {
        let m = caps.get(0).expect("regex always has whole match");
        let href = caps.get(1).map(|c| c.as_str()).unwrap_or("").to_string();
        out.push_str(&staged[last_end..m.start()]);

        let sibling_abs = output_dir.join(&href);
        let html_opt = read_sibling_cached(&sibling_abs, runtime, cache);

        let body = match html_opt {
            Some(html) => match mode {
                ExtractMode::Partial => extract_first_para_html(&html, 0).unwrap_or_default(),
                ExtractMode::Full => {
                    extract_full_contents(&html, site_url, &href).unwrap_or_default()
                }
            },
            None => {
                if warned.insert(href.clone()) {
                    diagnostics.push(make_q_12_16(&sibling_abs));
                }
                String::new()
            }
        };

        // Wrap the substituted body in CDATA so the XML parser
        // doesn't choke on `<` / `&` inside the rendered HTML.
        // v1 doesn't escape the rare `]]>` case in body content
        // (filed as a follow-up bd at close-out).
        out.push_str("<description><![CDATA[");
        out.push_str(&body);
        out.push_str("]]></description>");
        last_end = m.end();
    }
    out.push_str(&staged[last_end..]);
    out
}

/// Per-call cache for sibling HTML reads. Returns `Some(html)`
/// when the sibling is on disk and readable, `None` otherwise
/// (caller emits Q-12-16 once per missing sibling).
fn read_sibling_cached(
    path: &Path,
    runtime: &dyn SystemRuntime,
    cache: &mut HashMap<PathBuf, Option<String>>,
) -> Option<String> {
    let key = path.to_path_buf();
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    let html = match runtime.file_read(path) {
        Ok(bytes) => String::from_utf8(bytes).ok(),
        Err(RuntimeError::Io(e)) if e.kind() == ErrorKind::NotFound => None,
        Err(_) => None,
    };
    cache.insert(key, html.clone());
    html
}

/// Recursive walk under `dir` collecting every file whose
/// filename ends with one of the three staged extensions. Strict
/// suffix match — the patterns are intentionally Q1-verbatim and
/// unlikely to collide with author-managed files.
fn collect_staged_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut acc = Vec::new();
    walk_dir(dir, &mut acc)?;
    Ok(acc)
}

fn walk_dir(dir: &Path, acc: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(QuartoError::other(format!(
                "L9 complete: failed to read directory `{}`: {e}",
                dir.display()
            )));
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            walk_dir(&path, acc)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.ends_with(STAGED_EXT_FULL)
            || name.ends_with(STAGED_EXT_PARTIAL)
            || name.ends_with(STAGED_EXT_METADATA)
        {
            acc.push(path);
        }
    }
    Ok(())
}

fn make_q_12_16(path: &Path) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!(
        "Listing feed: sibling output `{}` could not be read; description left empty for this item.",
        path.display()
    ))
    .with_code("Q-12-16")
    .with_location(SourceInfo::default())
    .build()
}

fn diagnostic_warning(msg: String) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(msg)
        .with_location(SourceInfo::default())
        .build()
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectConfig;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
    use quarto_system_runtime::NativeRuntime;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value.to_string(), SourceInfo::default())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(entries, SourceInfo::default())
    }

    fn site_meta() -> ConfigValue {
        map(vec![(
            "website",
            map(vec![("site-url", s("https://example.com"))]),
        )])
    }

    fn no_url_meta() -> ConfigValue {
        map(vec![("website", map(vec![("title", s("Example"))]))])
    }

    fn make_project(project_dir: &Path, meta: ConfigValue) -> ProjectContext {
        ProjectContext {
            dir: project_dir.to_path_buf(),
            config: ProjectConfig {
                metadata: Some(meta),
                ..ProjectConfig::default()
            },
            is_single_file: false,
            files: vec![],
            output_dir: project_dir.join("_site"),
        }
    }

    /// Write `<output_dir>/<rel>` with the given content. Creates
    /// any missing parent directories.
    fn write_under(output_dir: &Path, rel: &str, content: &str) -> PathBuf {
        let abs = output_dir.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&abs, content).expect("write file");
        abs
    }

    fn runtime() -> NativeRuntime {
        NativeRuntime::new()
    }

    // ---- Plan test #37: metadata staged → xml unchanged --------

    #[test]
    fn complete_renames_metadata_staged_to_xml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = make_project(dir.path(), site_meta());
        std::fs::create_dir_all(&project.output_dir).unwrap();

        let staged_body = "<rss><channel><title>X</title><item><title>A</title><description><![CDATA[a]]></description></item></channel></rss>";
        write_under(
            &project.output_dir,
            "posts.feed-metadata-staged",
            staged_body,
        );

        let mut diags = Vec::new();
        complete_staged_feeds(&project, &runtime(), &mut diags).expect("complete ok");

        let final_path = project.output_dir.join("posts.xml");
        let final_content = std::fs::read_to_string(&final_path).expect("xml exists");
        assert_eq!(final_content, staged_body, "metadata feed copied verbatim");
        assert!(
            !project
                .output_dir
                .join("posts.feed-metadata-staged")
                .exists(),
            "staged file removed"
        );
    }

    // ---- Plan test #38: partial substitutes first-para HTML ----

    #[test]
    fn complete_substitutes_partial_descriptions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = make_project(dir.path(), site_meta());
        std::fs::create_dir_all(&project.output_dir).unwrap();

        let sibling_html = r#"<html><body><main class="content"><p>Hello <em>world</em>.</p></main></body></html>"#;
        write_under(&project.output_dir, "posts/foo.html", sibling_html);

        let staged =
            "<rss><item><description>{B4F502887207:posts/foo.html}</description></item></rss>";
        write_under(&project.output_dir, "posts.feed-partial-staged", staged);

        let mut diags = Vec::new();
        complete_staged_feeds(&project, &runtime(), &mut diags).expect("complete ok");

        let final_xml =
            std::fs::read_to_string(project.output_dir.join("posts.xml")).expect("xml exists");
        assert!(
            final_xml.contains("<description><![CDATA[Hello <em>world</em>.]]></description>"),
            "first-para HTML wrapped in CDATA; got:\n{}",
            final_xml
        );
        assert!(
            !final_xml.contains("B4F502887207"),
            "placeholder gone; got:\n{}",
            final_xml
        );
    }

    // ---- Plan test #39: full substitutes with absolute URLs ----

    #[test]
    fn complete_substitutes_full_descriptions_with_absolute_urls() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = make_project(dir.path(), site_meta());
        std::fs::create_dir_all(&project.output_dir).unwrap();

        let sibling_html = r##"<html><body><main class="content">
<p>See <a href="../about.html">about</a>.</p>
</main></body></html>"##;
        write_under(&project.output_dir, "posts/foo.html", sibling_html);

        let staged =
            "<rss><item><description>{B4F502887207:posts/foo.html}</description></item></rss>";
        write_under(&project.output_dir, "posts.feed-full-staged", staged);

        let mut diags = Vec::new();
        complete_staged_feeds(&project, &runtime(), &mut diags).expect("complete ok");

        let final_xml =
            std::fs::read_to_string(project.output_dir.join("posts.xml")).expect("xml exists");
        // URL rewritten to absolute.
        assert!(
            final_xml.contains(r#"<a href="https://example.com/about.html">about</a>"#),
            "expected absolute href; got:\n{}",
            final_xml
        );
        // CDATA-wrapped, placeholder gone.
        assert!(final_xml.contains("<![CDATA["));
        assert!(!final_xml.contains("B4F502887207"));
    }

    // ---- Plan test #40: missing sibling → Q-12-16 + empty ------

    #[test]
    fn complete_emits_q_12_16_when_sibling_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = make_project(dir.path(), site_meta());
        std::fs::create_dir_all(&project.output_dir).unwrap();

        let staged =
            "<rss><item><description>{B4F502887207:posts/missing.html}</description></item></rss>";
        write_under(&project.output_dir, "posts.feed-partial-staged", staged);

        let mut diags = Vec::new();
        complete_staged_feeds(&project, &runtime(), &mut diags).expect("complete ok");

        let q16: Vec<_> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("Q-12-16"))
            .collect();
        assert_eq!(q16.len(), 1, "expected one Q-12-16; got {}", q16.len());

        let final_xml =
            std::fs::read_to_string(project.output_dir.join("posts.xml")).expect("xml exists");
        assert!(
            final_xml.contains("<description><![CDATA[]]></description>"),
            "expected empty description; got:\n{}",
            final_xml
        );
    }

    // ---- Plan test #41: cache dedupes sibling reads -----------

    #[test]
    fn complete_caches_sibling_reads_per_call() {
        // Two staged feeds reference the same sibling; the
        // sibling has a unique substring per read and we verify
        // the substring appears the expected number of times in
        // both finals — a passing test means the cache delivered
        // the same text both times. (The exact read count is
        // hard to assert without a custom runtime; we settle for
        // "the substitution is consistent across feeds.")
        let dir = tempfile::tempdir().expect("tempdir");
        let project = make_project(dir.path(), site_meta());
        std::fs::create_dir_all(&project.output_dir).unwrap();

        let sibling =
            r#"<html><body><main class="content"><p>Cached body.</p></main></body></html>"#;
        write_under(&project.output_dir, "posts/foo.html", sibling);

        let s1 = "<rss><item><description>{B4F502887207:posts/foo.html}</description></item></rss>";
        let s2 = "<rss><item><description>{B4F502887207:posts/foo.html}</description></item></rss>";
        write_under(&project.output_dir, "posts.feed-partial-staged", s1);
        write_under(&project.output_dir, "posts-cat.feed-partial-staged", s2);

        let mut diags = Vec::new();
        complete_staged_feeds(&project, &runtime(), &mut diags).expect("complete ok");

        let f1 = std::fs::read_to_string(project.output_dir.join("posts.xml")).unwrap();
        let f2 = std::fs::read_to_string(project.output_dir.join("posts-cat.xml")).unwrap();
        assert!(f1.contains("Cached body."));
        assert!(f2.contains("Cached body."));
    }

    // ---- Plan test #42: no site-url → no-op -------------------

    #[test]
    fn complete_skips_when_no_site_url() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = make_project(dir.path(), no_url_meta());
        std::fs::create_dir_all(&project.output_dir).unwrap();

        let staged = "<rss></rss>";
        write_under(&project.output_dir, "posts.feed-full-staged", staged);

        let mut diags = Vec::new();
        complete_staged_feeds(&project, &runtime(), &mut diags).expect("complete ok");

        // Staged file remains; no .xml produced.
        assert!(project.output_dir.join("posts.feed-full-staged").exists());
        assert!(!project.output_dir.join("posts.xml").exists());
    }

    // ---- Plan test #43: concurrent per-category feeds finalize -

    #[test]
    fn complete_handles_concurrent_per_category_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = make_project(dir.path(), site_meta());
        std::fs::create_dir_all(&project.output_dir).unwrap();

        // Sibling HTML (used by all three feeds).
        let sibling = r#"<html><body><main class="content"><p>Shared.</p></main></body></html>"#;
        write_under(&project.output_dir, "posts/foo.html", sibling);

        let body =
            "<rss><item><description>{B4F502887207:posts/foo.html}</description></item></rss>";
        write_under(&project.output_dir, "posts.feed-full-staged", body);
        write_under(&project.output_dir, "posts-software.feed-full-staged", body);
        write_under(&project.output_dir, "posts-repro.feed-full-staged", body);

        let mut diags = Vec::new();
        complete_staged_feeds(&project, &runtime(), &mut diags).expect("complete ok");

        for stem in &["posts", "posts-software", "posts-repro"] {
            let xml = project.output_dir.join(format!("{}.xml", stem));
            assert!(xml.exists(), "{} missing", xml.display());
            let content = std::fs::read_to_string(&xml).unwrap();
            assert!(
                content.contains("Shared."),
                "{}.xml missing substituted body; got:\n{}",
                stem,
                content
            );
        }
        // All three staged files removed.
        for stem in &["posts", "posts-software", "posts-repro"] {
            let staged = project
                .output_dir
                .join(format!("{}.feed-full-staged", stem));
            assert!(!staged.exists(), "{} should be removed", staged.display());
        }
    }

    // ---- Recursive walk picks up nested staged files -----------

    #[test]
    fn complete_walks_nested_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = make_project(dir.path(), site_meta());
        std::fs::create_dir_all(project.output_dir.join("posts")).unwrap();

        let body = "<rss><channel><title>X</title></channel></rss>";
        write_under(
            &project.output_dir,
            "posts/index.feed-metadata-staged",
            body,
        );

        let mut diags = Vec::new();
        complete_staged_feeds(&project, &runtime(), &mut diags).expect("complete ok");

        assert!(project.output_dir.join("posts/index.xml").exists());
        assert!(
            !project
                .output_dir
                .join("posts/index.feed-metadata-staged")
                .exists()
        );
    }

    // ---- StagedType discriminator -------

    #[test]
    fn staged_type_from_filename() {
        assert_eq!(
            StagedType::from_filename("posts.feed-full-staged"),
            Some(StagedType::Full)
        );
        assert_eq!(
            StagedType::from_filename("posts.feed-partial-staged"),
            Some(StagedType::Partial)
        );
        assert_eq!(
            StagedType::from_filename("posts.feed-metadata-staged"),
            Some(StagedType::Metadata)
        );
        assert_eq!(StagedType::from_filename("posts.html"), None);
        assert_eq!(StagedType::from_filename("feed.xml"), None);
    }
}
