//! Metadata-merge interaction tests for navbar and page-footer.
//!
//! These tests document how `!prefer` and `!concat` merge-tag semantics
//! compose with navbar/page-footer config across layers. The behavior comes
//! from `MergedConfig::materialize()` in `quarto-config`; this file pins
//! down the expected outcomes so navbar/footer stay in lock-step with the
//! general merge rules.

use quarto_config::MergedConfig;
use quarto_core::format::Format;
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::{FooterGenerateTransform, NavbarGenerateTransform};
use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::{ConfigValue, MergeOp};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;
use std::path::PathBuf;

fn s(x: &str) -> ConfigValue {
    ConfigValue::new_string(x, SourceInfo::for_test())
}

fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
    let info = SourceInfo::for_test();
    let map_entries: Vec<ConfigMapEntry> = entries
        .into_iter()
        .map(|(k, v)| ConfigMapEntry {
            key: k.to_string(),
            key_source: info.clone(),
            value: v,
        })
        .collect();
    ConfigValue::new_map(map_entries, info)
}

fn arr(items: Vec<ConfigValue>) -> ConfigValue {
    ConfigValue::new_array(items, SourceInfo::for_test())
}

fn make_test_project() -> ProjectContext {
    ProjectContext {
        dir: PathBuf::from("/project"),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/doc.qmd")],
        output_dir: PathBuf::from("/project"),

        ..Default::default()
    }
}

/// Merge two raw `ConfigValue` maps (project lower, document higher) via the
/// production merge machinery, then return the flat `ConfigValue` ready to be
/// assigned to `ast.meta`.
fn merge(project: &ConfigValue, document: &ConfigValue) -> ConfigValue {
    MergedConfig::new(vec![project, document])
        .materialize()
        .expect("materialize")
}

async fn run_navbar(meta: ConfigValue) -> ConfigValue {
    let mut ast = Pandoc {
        meta,
        blocks: vec![],
    };
    let project = make_test_project();
    let doc = DocumentInfo::from_path("/project/doc.qmd");
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    NavbarGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("navbar-generate");
    ast.meta
}

async fn run_footer(meta: ConfigValue) -> ConfigValue {
    let mut ast = Pandoc {
        meta,
        blocks: vec![],
    };
    let project = make_test_project();
    let doc = DocumentInfo::from_path("/project/doc.qmd");
    let format = Format::html();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
    FooterGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("footer-generate");
    ast.meta
}

fn left_hrefs(meta: &ConfigValue) -> Vec<String> {
    meta.get_path(&["navigation", "navbar"])
        .and_then(|n| n.get("left"))
        .and_then(|l| l.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("href").and_then(|v| v.as_plain_text()))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn default_array_merge_concatenates_left_items() {
    // Default merge op for arrays is `!concat`: project items + doc items.
    let project = map(vec![(
        "navbar",
        map(vec![("left", arr(vec![s("index.qmd"), s("about.qmd")]))]),
    )]);
    let document = map(vec![(
        "navbar",
        map(vec![("left", arr(vec![s("extra.qmd")]))]),
    )]);
    let merged = merge(&project, &document);
    let out = run_navbar(merged).await;
    assert_eq!(
        left_hrefs(&out),
        vec!["index.qmd", "about.qmd", "extra.qmd"]
    );
}

#[tokio::test]
async fn prefer_tag_replaces_left_entirely() {
    // `!prefer` on the document's `left` array discards the project items.
    let project = map(vec![(
        "navbar",
        map(vec![("left", arr(vec![s("index.qmd"), s("about.qmd")]))]),
    )]);
    let document = map(vec![(
        "navbar",
        map(vec![(
            "left",
            arr(vec![s("replacement.qmd")]).with_merge_op(MergeOp::Prefer),
        )]),
    )]);
    let merged = merge(&project, &document);
    let out = run_navbar(merged).await;
    assert_eq!(left_hrefs(&out), vec!["replacement.qmd"]);
}

#[tokio::test]
async fn prefer_on_scalar_overrides_background() {
    // Scalars already default to last-wins, but `!prefer` makes the intent
    // explicit. Either way, the doc's value must win.
    let project = map(vec![("navbar", map(vec![("background", s("primary"))]))]);
    let document = map(vec![(
        "navbar",
        map(vec![(
            "background",
            s("secondary").with_merge_op(MergeOp::Prefer),
        )]),
    )]);
    let merged = merge(&project, &document);
    let out = run_navbar(merged).await;
    let bg = out
        .get_path(&["navigation", "navbar", "background"])
        .and_then(|v| v.as_plain_text());
    assert_eq!(bg.as_deref(), Some("secondary"));
}

#[tokio::test]
async fn prefer_on_whole_page_footer_replaces_object() {
    // With `!prefer` on the entire `page-footer` map at the document layer,
    // project `left` / `background` are discarded.
    let project = map(vec![(
        "page-footer",
        map(vec![
            ("left", s("Project Left")),
            ("background", s("light")),
        ]),
    )]);
    let document = map(vec![(
        "page-footer",
        map(vec![("center", s("Only Center"))]).with_merge_op(MergeOp::Prefer),
    )]);
    let merged = merge(&project, &document);
    let out = run_footer(merged).await;

    let footer = out.get_path(&["navigation", "footer"]).unwrap();
    assert_eq!(
        footer
            .get("center")
            .and_then(|v| v.as_plain_text())
            .as_deref(),
        Some("Only Center")
    );
    assert!(
        footer.get("left").is_none(),
        "left should be discarded under !prefer"
    );
    assert!(
        footer.get("background").is_none(),
        "background should be discarded under !prefer"
    );
}

#[tokio::test]
async fn map_merge_preserves_sibling_keys_but_scalar_children_are_overwritten() {
    // Without any tag, map-vs-map merges deeply: document's `background`
    // replaces project's (scalars win last), but project's `left` array is
    // preserved (and concatenated if both layers have one).
    let project = map(vec![(
        "navbar",
        map(vec![
            ("background", s("primary")),
            ("left", arr(vec![s("index.qmd")])),
        ]),
    )]);
    let document = map(vec![("navbar", map(vec![("background", s("dark"))]))]);
    let merged = merge(&project, &document);
    let out = run_navbar(merged).await;

    let stored = out.get_path(&["navigation", "navbar"]).unwrap();
    assert_eq!(
        stored
            .get("background")
            .and_then(|v| v.as_plain_text())
            .as_deref(),
        Some("dark")
    );
    // Project's `left` survives because the document didn't touch it.
    assert_eq!(left_hrefs(&out), vec!["index.qmd"]);
}

#[tokio::test]
async fn document_false_beats_project_full_config() {
    // If project config sets up a full navbar and the document writes
    // `navbar: false`, the affirmative-disable scalar wins (default scalar
    // merge is last-wins).
    let project = map(vec![(
        "navbar",
        map(vec![
            ("title", s("Project Site")),
            ("left", arr(vec![s("index.qmd")])),
        ]),
    )]);
    let document = map(vec![(
        "navbar",
        ConfigValue::new_bool(false, SourceInfo::for_test()),
    )]);
    let merged = merge(&project, &document);
    let out = run_navbar(merged).await;
    assert!(
        !out.contains_path(&["navigation", "navbar"]),
        "navbar: false at document layer must suppress generation"
    );
}
