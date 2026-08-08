/*
 * tests/integration/extension_metadata.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `contributes.metadata` project-level merging (bd-ad7i1pc6 Phase 5,
 * absorbing bd-zb2tod5f).
 */

//! Extension `contributes.metadata.project` → project config.
//!
//! Unlike `contributes.project` (opt-in by naming the extension in
//! `project.type`), `contributes.metadata.project` applies from
//! **every** discovered extension, unconditionally — the Q1 mechanism
//! quarto-openapi uses to inject its `pre-render` script. Precedence
//! deliberately diverges from Q1 (whose `mergeExtensionMetadata` lets
//! the extension override the user): here the **user wins**, arrays
//! concat (extension entries first), and bundled file paths rebase
//! ext-dir → project-root exactly like `contributes.project` fragments.

use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::project::{ProjectContext, ProjectKind};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Build a project: `_quarto.yml`, plus extensions given as
/// `(rel_dir, manifest, assets)` where assets are files created
/// relative to the extension dir.
fn discover(
    quarto_yml: &str,
    extensions: &[(&str, &str, &[&str])],
) -> (quarto_core::error::Result<ProjectContext>, TempDir) {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("_quarto.yml"), quarto_yml).unwrap();
    std::fs::write(tmp.path().join("index.qmd"), "# Hello\n").unwrap();
    for (rel_dir, manifest, assets) in extensions {
        let dir = tmp.path().join("_extensions").join(rel_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("_extension.yml"), manifest).unwrap();
        for asset in *assets {
            let p = dir.join(asset);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "// asset\n").unwrap();
        }
    }
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = ProjectContext::discover(tmp.path(), runtime.as_ref());
    (result, tmp)
}

/// The quarto-openapi shape: a pre-render script injected via
/// `contributes.metadata.project`.
const OPENAPI_LIKE: &str = r#"
contributes:
  metadata:
    project:
      pre-render:
        - openapi-to-markdown.ts
  format:
    html:
      css:
        - openapi-styles.css
"#;

#[test]
fn metadata_project_pre_render_lands_in_project_config() {
    let (result, _tmp) = discover(
        "project:\n  type: website\n",
        &[(
            "posit-dev/quarto-openapi",
            OPENAPI_LIKE,
            &["openapi-to-markdown.ts"],
        )],
    );
    let project = result.expect("builtin type with metadata contribution must parse");
    assert_eq!(project.project_kind(), ProjectKind::Website);
    let scripts: Vec<&str> = project
        .config
        .pre_render_scripts
        .iter()
        .map(|s| s.command.as_str())
        .collect();
    assert_eq!(
        scripts,
        vec!["_extensions/posit-dev/quarto-openapi/openapi-to-markdown.ts"],
        "bundled script path must rebase to project root"
    );
}

#[test]
fn metadata_project_concats_with_user_scripts_extension_first() {
    let (result, _tmp) = discover(
        "project:\n  type: website\n  pre-render:\n    - ./user-script.sh\n",
        &[(
            "posit-dev/quarto-openapi",
            OPENAPI_LIKE,
            &["openapi-to-markdown.ts"],
        )],
    );
    let project = result.unwrap();
    let scripts: Vec<&str> = project
        .config
        .pre_render_scripts
        .iter()
        .map(|s| s.command.as_str())
        .collect();
    assert_eq!(
        scripts,
        vec![
            "_extensions/posit-dev/quarto-openapi/openapi-to-markdown.ts",
            "./user-script.sh",
        ]
    );
}

#[test]
fn user_scalar_wins_over_metadata_contribution() {
    let ext = r#"
contributes:
  metadata:
    project:
      output-dir: ext-out
"#;
    let (result, _tmp) = discover(
        "project:\n  type: website\n  output-dir: mine\n",
        &[("meta-ext", ext, &[])],
    );
    let project = result.unwrap();
    assert!(project.output_dir.ends_with("mine"));

    // Gap-fill: without a user value the extension's applies.
    let (result, _tmp) = discover("project:\n  type: website\n", &[("meta-ext", ext, &[])]);
    let project = result.unwrap();
    assert!(
        project.output_dir.ends_with("ext-out"),
        "extension output-dir must fill the gap; got {}",
        project.output_dir.display()
    );
}

#[test]
fn metadata_project_applies_from_multiple_extensions() {
    let ext_a = r#"
contributes:
  metadata:
    project:
      resources:
        - a-resource.json
"#;
    let ext_b = r#"
contributes:
  metadata:
    project:
      resources:
        - b-resource.json
"#;
    let (result, _tmp) = discover(
        "project:\n  type: website\n",
        &[("aext", ext_a, &[]), ("bext", ext_b, &[])],
    );
    let project = result.unwrap();
    let mut patterns: Vec<&str> = project
        .config
        .resources
        .iter()
        .map(|r| r.pattern.as_str())
        .collect();
    patterns.sort();
    assert_eq!(patterns, vec!["a-resource.json", "b-resource.json"]);
}

#[test]
fn metadata_contribution_composes_with_custom_project_type() {
    let type_ext = r#"
contributes:
  project:
    project:
      type: website
"#;
    let (result, _tmp) = discover(
        "project:\n  type: fancysite\n",
        &[
            ("acme/fancysite", type_ext, &[]),
            (
                "posit-dev/quarto-openapi",
                OPENAPI_LIKE,
                &["openapi-to-markdown.ts"],
            ),
        ],
    );
    let project = result.expect("custom type + metadata contribution must compose");
    assert_eq!(project.project_kind(), ProjectKind::Website);
    assert_eq!(
        project.config.custom_project_type.as_ref().unwrap().name,
        "fancysite"
    );
    let scripts: Vec<&str> = project
        .config
        .pre_render_scripts
        .iter()
        .map(|s| s.command.as_str())
        .collect();
    assert_eq!(
        scripts,
        vec!["_extensions/posit-dev/quarto-openapi/openapi-to-markdown.ts"]
    );
}

#[test]
fn metadata_without_project_key_leaves_project_config_alone() {
    let ext = r#"
contributes:
  metadata:
    author-notice: "from extension"
"#;
    let (result, _tmp) = discover("project:\n  type: website\n", &[("meta-ext", ext, &[])]);
    let project = result.unwrap();
    // Non-`project` metadata keys are a *document-level* concern; the
    // project config must not absorb them at parse time.
    assert!(
        project
            .config
            .metadata
            .as_ref()
            .and_then(|m| m.get("author-notice"))
            .is_none(),
        "non-project metadata keys must not merge into project config"
    );
    assert!(project.config.pre_render_scripts.is_empty());
}
