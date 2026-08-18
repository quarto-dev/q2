//! End-to-end integration tests for navbar and page-footer.
//!
//! Each test starts from a merged `ast.meta` (simulating the state after
//! `MetadataMergeStage`) and runs the navigation Generate + Render transforms
//! in pipeline order, then asserts that the rendered HTML strings at
//! `rendered.navigation.{navbar,footer}` have the expected shape. A follow-up
//! assertion feeds those strings into the full HTML template and confirms
//! the template injects them into the right spots.

use quarto_core::format::Format;
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::template::{full_html_template, render_with_compiled_template};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::{
    FooterGenerateTransform, FooterRenderTransform, NavbarGenerateTransform, NavbarRenderTransform,
};
use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;
use std::path::PathBuf;

fn s(x: &str) -> ConfigValue {
    ConfigValue::new_string(x, SourceInfo::for_test())
}

fn b(x: bool) -> ConfigValue {
    ConfigValue::new_bool(x, SourceInfo::for_test())
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

async fn run_navigation_pipeline(meta: ConfigValue) -> ConfigValue {
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
    FooterGenerateTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("footer-generate");
    NavbarRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("navbar-render");
    FooterRenderTransform::new()
        .transform(&mut ast, &mut ctx)
        .await
        .expect("footer-render");

    ast.meta
}

#[tokio::test]
async fn navbar_from_raw_yaml_through_to_template_html() {
    // Start from merged metadata that mimics what a user wrote:
    //
    //   navbar:
    //     title: "My Site"
    //     background: primary
    //     left:
    //       - index.qmd
    //       - { text: About, href: about.qmd }
    let meta = map(vec![(
        "navbar",
        map(vec![
            ("title", s("My Site")),
            ("background", s("primary")),
            (
                "left",
                arr(vec![
                    s("index.qmd"),
                    map(vec![("text", s("About")), ("href", s("about.qmd"))]),
                ]),
            ),
        ]),
    )]);

    let out = run_navigation_pipeline(meta).await;

    // The full pipeline populates both structured and rendered slots.
    assert!(out.contains_path(&["navigation", "navbar"]));
    assert!(out.contains_path(&["rendered", "navigation", "navbar"]));

    let navbar_html = out
        .get_path(&["rendered", "navigation", "navbar"])
        .unwrap()
        .as_plain_text()
        .unwrap();
    assert!(navbar_html.contains("<nav class=\"navbar"));
    assert!(navbar_html.contains("bg-primary"));
    assert!(navbar_html.contains("My Site"));
    assert!(navbar_html.contains("href=\"index.qmd\""));
    assert!(navbar_html.contains("href=\"about.qmd\""));

    // Feed the rendered HTML through the template and confirm positioning.
    let template = full_html_template().unwrap();
    let (final_html, _diags) =
        render_with_compiled_template(&template, "<p>Body</p>", &out, &[], &[]).unwrap();

    let nav_pos = final_html.find("<nav class=\"navbar").unwrap();
    let body_pos = final_html.find("<p>Body</p>").unwrap();
    assert!(
        nav_pos < body_pos,
        "navbar should precede body in final HTML:\n{}",
        final_html
    );
}

#[tokio::test]
async fn footer_from_raw_yaml_through_to_template_html() {
    // page-footer: "Copyright 2026"
    let meta = map(vec![("page-footer", s("Copyright 2026"))]);
    let out = run_navigation_pipeline(meta).await;

    let footer_html = out
        .get_path(&["rendered", "navigation", "footer"])
        .unwrap()
        .as_plain_text()
        .unwrap();
    assert!(footer_html.contains("<footer class=\"footer\">"));
    assert!(footer_html.contains("Copyright 2026"));

    let template = full_html_template().unwrap();
    let (final_html, _diags) =
        render_with_compiled_template(&template, "<p>Body</p>", &out, &[], &[]).unwrap();
    let body_pos = final_html.find("<p>Body</p>").unwrap();
    let footer_pos = final_html.find("<footer class=\"footer\">").unwrap();
    assert!(
        footer_pos > body_pos,
        "footer should follow body in final HTML:\n{}",
        final_html
    );
}

#[tokio::test]
async fn navbar_false_suppresses_output_even_with_navbar_config_alongside() {
    // navbar: false overrides — nothing rendered.
    let meta = map(vec![("navbar", b(false)), ("page-footer", s("hi"))]);
    let out = run_navigation_pipeline(meta).await;

    assert!(!out.contains_path(&["navigation", "navbar"]));
    assert!(!out.contains_path(&["rendered", "navigation", "navbar"]));

    // Footer still renders.
    assert!(out.contains_path(&["rendered", "navigation", "footer"]));
}

#[tokio::test]
async fn both_absent_yields_no_navigation_keys() {
    let out = run_navigation_pipeline(ConfigValue::default()).await;
    assert!(!out.contains_path(&["navigation", "navbar"]));
    assert!(!out.contains_path(&["navigation", "footer"]));
    assert!(!out.contains_path(&["rendered", "navigation", "navbar"]));
    assert!(!out.contains_path(&["rendered", "navigation", "footer"]));
}

#[tokio::test]
async fn user_prerendered_navbar_is_preserved() {
    // A user filter might inject fully-rendered HTML at `rendered.navigation.navbar`.
    // The Generate stage must still fill in `navigation.navbar` for consumers that
    // want structured data, but Render must leave the HTML alone.
    let mut meta = map(vec![("navbar", map(vec![("title", s("Mine"))]))]);
    meta.insert_path(
        &["rendered", "navigation", "navbar"],
        s("<nav id=\"override\">Custom</nav>"),
    );
    let out = run_navigation_pipeline(meta).await;

    let stored = out
        .get_path(&["rendered", "navigation", "navbar"])
        .unwrap()
        .as_plain_text()
        .unwrap();
    assert_eq!(stored, "<nav id=\"override\">Custom</nav>");

    // Structured data still populated by Generate.
    assert!(out.contains_path(&["navigation", "navbar"]));
}
