/*
 * tests/integration/website_social_meta.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for `og:*` / `twitter:*` social meta tags (bd-tyvt).
 */

//! Social meta tags, end to end through `ProjectPipeline`.
//!
//! The fixtures mirror the two Quarto 1 reference sites recorded in
//! `claude-notes/plans/2026-11-20-social-meta-tags.md` § "Quarto 1
//! reference output". Expected tags are Quarto 1's output, except where
//! the plan records a deliberate divergence (each such assertion says so).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, ProjectRenderSummary, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// A PNG that is only a signature and an `IHDR` chunk: enough for a
/// header-only size reader, and for the file to be copied.
fn write_png(path: &Path, width: u32, height: u32) {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 2, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0]); // CRC (not checked by readers)
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

fn render_project(fixture: impl FnOnce(&Path)) -> (PathBuf, ProjectRenderSummary) {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    // Keep the directory for inspection; cleaned up at process exit.
    std::mem::forget(temp);
    (project_dir, summary)
}

fn output_for<'a>(
    summary: &'a ProjectRenderSummary,
    project_dir: &Path,
    rel_html: &str,
) -> &'a RenderToFileResult {
    let want = project_dir.join("_site").join(rel_html);
    summary
        .outputs
        .iter()
        .find(|out| out.output_path == want)
        .unwrap_or_else(|| {
            panic!(
                "no output {}; got: {:?}",
                want.display(),
                summary
                    .outputs
                    .iter()
                    .map(|o| o.output_path.display().to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// The page's social meta tags, one per line as written.
fn social_tags(project_dir: &Path, rel_html: &str) -> BTreeSet<String> {
    let path = project_dir.join("_site").join(rel_html);
    let html =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    html.lines()
        .map(str::trim)
        .filter(|l| l.contains(r#"property="og:"#) || l.contains(r#"name="twitter:"#))
        .map(str::to_string)
        .collect()
}

fn og(prop: &str, content: &str) -> String {
    format!(r#"<meta property="og:{prop}" content="{content}">"#)
}

fn tw(name: &str, content: &str) -> String {
    format!(r#"<meta name="twitter:{name}" content="{content}">"#)
}

fn set(tags: Vec<String>) -> BTreeSet<String> {
    tags.into_iter().collect()
}

fn codes(summary: &ProjectRenderSummary, project_dir: &Path, rel_html: &str) -> Vec<String> {
    output_for(summary, project_dir, rel_html)
        .render_output
        .diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .filter(|c| c.starts_with("Q-5-"))
        .collect()
}

// ─── Reference site 1: `open-graph: true`, twitter map, site image ─────

fn site_one(dir: &Path) {
    write(
        &dir.join("_quarto.yml"),
        r#"quarto-required: "^2"
project:
  type: website
website:
  title: Site
  site-url: https://example.com/
  description: Site description.
  image: site.png
  image-alt: Site image alt
  open-graph: true
  twitter-card:
    creator: "@me"
    site: "@site"
"#,
    );
    write_png(&dir.join("preview.png"), 4, 3);
    write_png(&dir.join("other.png"), 5, 5);
    write_png(&dir.join("figures/feature-cover.png"), 8, 6);
    write_png(&dir.join("classy.png"), 7, 2);
    write_png(&dir.join("explicit.png"), 12, 9);
    write_png(&dir.join("site.png"), 20, 10);
    write_png(&dir.join("sub/local.png"), 3, 3);
    write(
        &dir.join("index.qmd"),
        "---\ntitle: Home\ndescription: A page with a description.\n---\n\n\
         First paragraph of the page.\n\n![A preview](preview.png)\n",
    );
    write(
        &dir.join("nodesc.qmd"),
        "---\ntitle: No *Desc*\n---\n\nSome text.\n\n![Other](other.png)\n\n\
         ![Named](figures/feature-cover.png)\n",
    );
    write(
        &dir.join("classy.qmd"),
        "---\ntitle: Classy\nsubtitle: A subtitle used as description\n---\n\n\
         ![First](other.png)\n\n![Marked](classy.png){.preview-image}\n",
    );
    write(
        &dir.join("explicit.qmd"),
        "---\ntitle: Explicit\nimage: explicit.png\nimage-alt: Explicit alt\n\
         open-graph:\n  title: OG *title*\n  description: OG description.\n  \
         locale: en_US\n  image-width: 99\n  image-height: 77\n\
         twitter-card: false\n---\n\nBody paragraph.\n",
    );
    write(
        &dir.join("noimg.qmd"),
        "---\ntitle: No image\n---\n\nOnly text here.\n",
    );
    write(
        &dir.join("sub/page.qmd"),
        "---\ntitle: Sub page\nabstract: An abstract.\n---\n\n![Local](local.png)\n",
    );
}

fn twitter_site_tags() -> Vec<String> {
    vec![tw("creator", "@me"), tw("site", "@site")]
}

#[test]
fn site_one_matches_quarto_1() {
    let (dir, summary) = render_project(site_one);

    let mut index = vec![
        og("title", "Home – Site"),
        og("description", "A page with a description."),
        og("image", "https://example.com/preview.png"),
        og("site_name", "Site"),
        og("image:height", "3"),
        og("image:width", "4"),
        tw("title", "Home – Site"),
        tw("description", "A page with a description."),
        tw("image", "https://example.com/preview.png"),
        tw("card", "summary_large_image"),
    ];
    index.extend(twitter_site_tags());
    assert_eq!(social_tags(&dir, "index.html"), set(index));

    // A named file (`feature`) wins over an earlier plain image; the
    // site description beats the (here unused) first paragraph.
    let mut nodesc = vec![
        og("title", "No Desc – Site"),
        og("description", "Site description."),
        og("image", "https://example.com/figures/feature-cover.png"),
        og("site_name", "Site"),
        og("image:height", "6"),
        og("image:width", "8"),
        tw("title", "No Desc – Site"),
        tw("description", "Site description."),
        tw("image", "https://example.com/figures/feature-cover.png"),
        tw("card", "summary_large_image"),
    ];
    nodesc.extend(twitter_site_tags());
    assert_eq!(social_tags(&dir, "nodesc.html"), set(nodesc));

    let mut classy = vec![
        og("title", "Classy – Site"),
        og("description", "A subtitle used as description"),
        og("image", "https://example.com/classy.png"),
        og("site_name", "Site"),
        og("image:height", "2"),
        og("image:width", "7"),
        tw("title", "Classy – Site"),
        tw("description", "A subtitle used as description"),
        tw("image", "https://example.com/classy.png"),
        tw("card", "summary_large_image"),
    ];
    classy.extend(twitter_site_tags());
    assert_eq!(social_tags(&dir, "classy.html"), set(classy));

    // The size read from the file beats the declared 99x77.
    assert_eq!(
        social_tags(&dir, "explicit.html"),
        set(vec![
            og("title", "OG title"),
            og("description", "OG description."),
            og("image", "https://example.com/explicit.png"),
            og("site_name", "Site"),
            og("image:height", "9"),
            og("image:width", "12"),
            og("locale", "en_US"),
            og("image:alt", "Explicit alt"),
        ])
    );

    let mut noimg = vec![
        og("title", "No image – Site"),
        og("description", "Site description."),
        og("image", "https://example.com/site.png"),
        og("site_name", "Site"),
        og("image:alt", "Site image alt"),
        og("image:height", "10"),
        og("image:width", "20"),
        tw("title", "No image – Site"),
        tw("description", "Site description."),
        tw("image", "https://example.com/site.png"),
        tw("image:alt", "Site image alt"),
        tw("card", "summary_large_image"),
    ];
    noimg.extend(twitter_site_tags());
    assert_eq!(social_tags(&dir, "noimg.html"), set(noimg));

    let mut sub = vec![
        og("title", "Sub page – Site"),
        og("description", "An abstract."),
        og("image", "https://example.com/sub/local.png"),
        og("site_name", "Site"),
        og("image:height", "3"),
        og("image:width", "3"),
        tw("title", "Sub page – Site"),
        tw("description", "An abstract."),
        tw("image", "https://example.com/sub/local.png"),
        tw("card", "summary_large_image"),
    ];
    sub.extend(twitter_site_tags());
    assert_eq!(social_tags(&dir, "sub/page.html"), set(sub));

    for page in ["index.html", "explicit.html", "noimg.html", "sub/page.html"] {
        assert!(codes(&summary, &dir, page).is_empty(), "{page}");
    }
}

#[test]
fn explicit_and_site_images_are_copied_to_the_output() {
    let (dir, _summary) = render_project(site_one);
    assert!(dir.join("_site/explicit.png").is_file());
    assert!(dir.join("_site/site.png").is_file());
}

// ─── Reference site 2: site maps, no site description ──────────────────

const LONG_PARA: &str = "This is the first paragraph, and it is deliberately long so \
    that the truncation logic has to cut it. It keeps going with a second sentence \
    that pushes the total well past two hundred characters, so we can see where the \
    cut lands! And a third clause, without a final stop";

fn site_two(dir: &Path) {
    write(
        &dir.join("_quarto.yml"),
        r#"quarto-required: "^2"
project:
  type: website
website:
  title: Site
  site-url: https://example.com/base
  open-graph:
    image: site.png
    image-width: 640
    site-name: Custom Site Name
  twitter-card:
    card-style: summary
    image-width: 10
"#,
    );
    write_png(&dir.join("other.png"), 5, 5);
    write_png(&dir.join("site.png"), 20, 10);
    write_png(&dir.join("preview.png"), 4, 3);
    write(
        &dir.join("index.qmd"),
        &format!(
            "---\ntitle: Home\n---\n\n{LONG_PARA}\n\n\
             Some text with an inline ![inline alt](other.png) image.\n"
        ),
    );
    write(
        &dir.join("ogtrue.qmd"),
        "---\ntitle: OG true\nopen-graph: true\n---\n\nText paragraph.\n",
    );
    write(
        &dir.join("noimage.qmd"),
        "---\ntitle: Image false\nimage: false\n---\n\nText paragraph.\n\n![x](other.png)\n",
    );
    write(
        &dir.join("remote.qmd"),
        "---\ntitle: Remote\n---\n\n![r](https://example.org/remote-preview.png)\n\nPara.\n",
    );
}

#[test]
fn site_two_matches_quarto_1_with_recorded_divergences() {
    let (dir, summary) = render_project(site_two);

    // Divergence: Quarto 1 writes `content=""` here (its markdown
    // pipeline erases the discovered paragraph); q2 keeps the first
    // paragraph, truncated at sentence punctuation within 200 chars.
    let truncated = "This is the first paragraph, and it is deliberately long so \
                     that the truncation logic has to cut it.";
    assert_eq!(
        social_tags(&dir, "index.html"),
        set(vec![
            og("title", "Home – Site"),
            og("description", truncated),
            og("image", "https://example.com/base/site.png"),
            og("site_name", "Custom Site Name"),
            og("image:width", "20"),
            og("image:height", "10"),
            tw("title", "Home – Site"),
            tw("description", truncated),
            tw("image", "https://example.com/base/other.png"),
            tw("card", "summary"),
            tw("image:alt", "inline alt"),
        ])
    );
    // Twitter Cards have no size tags.
    assert!(codes(&summary, &dir, "index.html").contains(&"Q-5-61".to_string()));

    // Divergence: Quarto 1 drops the site options when a page sets
    // `open-graph: true`; q2 keeps them.
    assert_eq!(
        social_tags(&dir, "ogtrue.html"),
        set(vec![
            og("title", "OG true – Site"),
            og("description", "Text paragraph."),
            og("image", "https://example.com/base/site.png"),
            og("site_name", "Custom Site Name"),
            og("image:width", "20"),
            og("image:height", "10"),
            tw("title", "OG true – Site"),
            tw("description", "Text paragraph."),
            tw("card", "summary"),
        ])
    );

    // `image: false` stops discovery, not the site-level OG image.
    assert_eq!(
        social_tags(&dir, "noimage.html"),
        set(vec![
            og("title", "Image false – Site"),
            og("description", "Text paragraph."),
            og("image", "https://example.com/base/site.png"),
            og("site_name", "Custom Site Name"),
            og("image:width", "20"),
            og("image:height", "10"),
            tw("title", "Image false – Site"),
            tw("description", "Text paragraph."),
            tw("card", "summary"),
        ])
    );

    // A remote image matching the named-file pattern is discovered
    // and passed through unchanged.
    assert_eq!(
        social_tags(&dir, "remote.html"),
        set(vec![
            og("title", "Remote – Site"),
            og("description", "Para."),
            og("image", "https://example.com/base/site.png"),
            og("site_name", "Custom Site Name"),
            og("image:width", "20"),
            og("image:height", "10"),
            tw("title", "Remote – Site"),
            tw("description", "Para."),
            tw("image", "https://example.org/remote-preview.png"),
            tw("card", "summary"),
        ])
    );
}

// ─── No `site-url`: image tags are dropped, with Q-5-58 ────────────────

#[test]
fn without_site_url_image_tags_are_omitted_with_a_warning() {
    let (dir, summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "quarto-required: \"^2\"\nproject:\n  type: website\n\
             website:\n  title: Site\n  open-graph: true\n  twitter-card: true\n",
        );
        write_png(&dir.join("preview.png"), 4, 3);
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\ndescription: D.\n---\n\n![P](preview.png)\n",
        );
        write(
            &dir.join("plain.qmd"),
            "---\ntitle: Plain\ndescription: D.\n---\n\nNo images.\n",
        );
    });
    assert_eq!(
        social_tags(&dir, "index.html"),
        set(vec![
            og("title", "Home – Site"),
            og("description", "D."),
            og("site_name", "Site"),
            tw("title", "Home – Site"),
            tw("description", "D."),
            tw("card", "summary"),
        ])
    );
    assert_eq!(
        codes(&summary, &dir, "index.html"),
        vec!["Q-5-58".to_string()]
    );
    // Nothing was dropped on a page without an image: no warning.
    assert!(codes(&summary, &dir, "plain.html").is_empty());
}

// ─── Off unless enabled; attribute escaping; type checks ───────────────

#[test]
fn no_tags_unless_enabled() {
    let (dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "quarto-required: \"^2\"\nproject:\n  type: website\n\
             website:\n  title: Site\n  site-url: https://example.com\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nText.\n");
        write(
            &dir.join("on.qmd"),
            "---\ntitle: On\ntwitter-card: true\n---\n\nText.\n",
        );
    });
    assert!(social_tags(&dir, "index.html").is_empty());
    assert_eq!(
        social_tags(&dir, "on.html"),
        set(vec![
            tw("title", "On – Site"),
            tw("description", "Text."),
            tw("card", "summary"),
        ])
    );
}

#[test]
fn values_are_attribute_escaped() {
    let (dir, _summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "quarto-required: \"^2\"\nproject:\n  type: website\n\
             website:\n  site-url: https://example.com\n  open-graph: true\n",
        );
        write(
            &dir.join("index.qmd"),
            "---\ntitle: R&D\ndescription: A & B > C\n---\n\nText.\n",
        );
    });
    assert_eq!(
        social_tags(&dir, "index.html"),
        set(vec![
            og("title", "R&amp;D"),
            og("description", "A &amp; B &gt; C"),
        ])
    );
}

#[test]
fn wrong_types_warn_and_are_ignored() {
    let (dir, summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "quarto-required: \"^2\"\nproject:\n  type: website\n\
             website:\n  title: Site\n  site-url: https://example.com\n  \
             open-graph:\n    title: [not, a, string]\n    locale: en_US\n    \
             unknown-key: is fine\n  twitter-card: 3\n",
        );
        write(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nText.\n");
    });
    // `title` falls back to the page title; the unknown key is ignored
    // silently; `twitter-card: 3` disables the provider.
    assert_eq!(
        social_tags(&dir, "index.html"),
        set(vec![
            og("title", "Home – Site"),
            og("description", "Text."),
            og("site_name", "Site"),
            og("locale", "en_US"),
        ])
    );
    assert_eq!(
        codes(&summary, &dir, "index.html"),
        vec!["Q-5-59".to_string(), "Q-5-59".to_string()]
    );
}

#[test]
fn data_uri_image_is_ignored_with_a_warning() {
    let (dir, summary) = render_project(|dir| {
        write(
            &dir.join("_quarto.yml"),
            "quarto-required: \"^2\"\nproject:\n  type: website\n\
             website:\n  site-url: https://example.com\n  twitter-card: true\n",
        );
        write(
            &dir.join("index.qmd"),
            "---\ntitle: Home\nimage: \"data:image/png;base64,AAAA\"\n---\n\nText.\n",
        );
    });
    assert_eq!(
        social_tags(&dir, "index.html"),
        set(vec![
            tw("title", "Home"),
            tw("description", "Text."),
            tw("card", "summary"),
        ])
    );
    assert_eq!(
        codes(&summary, &dir, "index.html"),
        vec!["Q-5-60".to_string()]
    );
}
