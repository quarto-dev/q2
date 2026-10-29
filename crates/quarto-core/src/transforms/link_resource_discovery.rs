/*
 * link_resource_discovery.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Auto-discovery of linked local files (bd-twfep08m).
//!
//! A Markdown link to a local file that is not a document — a CSV, a
//! JSON dashboard, a notebook offered as a download — must exist next
//! to the rendered page for the link to work, exactly like an image.
//! Quarto 1 finds these with an HTML postprocessor over `a[href]`;
//! Quarto 2 finds them on the AST, and only on `Link` nodes (raw HTML,
//! attribute values and code are never inspected).
//!
//! Each discovered file is appended to `meta.resources` as a
//! doc-relative, `/`-separated path. That makes the result visible to
//! (and prunable by) post filters: `ResourceReportStage` publishes only
//! what is still in `meta.resources` after `UserFiltersStage::post`,
//! attributing the survivors listed in
//! [`RenderContext::discovered_resources`] to
//! [`ResourceOrigin::AutoDiscovery`](crate::project_resources::ResourceOrigin).
//!
//! `resource-discovery: false` in merged metadata turns the walk off;
//! any non-boolean value is `Q-5-52`, an error, so a typo cannot publish
//! files the author meant to keep out.
//!
//! ## What is a candidate
//!
//! The target of a `Link`, minus any `?query` / `#fragment`, percent-
//! decoded, resolved against the document's directory (a leading `/`
//! against the project root), that is an existing regular file inside
//! the project and is none of:
//!
//! - a `.qmd` file, or any project input document;
//! - a hidden path (any component starting with `.`), or under `_freeze/`;
//! - under the output directory (when it differs from the project dir);
//! - a likely render output of a project document — `report.pdf` next
//!   to `report.qmd` — which a stale in-place render must not overwrite.
//!
//! Images are not handled here: [`super::ResourceCollectorTransform`]
//! copies them through `resource_copies`, which Typst needs flushed
//! before `typst compile` runs.
//!
//! ## Pipeline placement
//!
//! Finalization, immediately before [`super::LinkRewriteTransform`]: by
//! then shortcodes, includes and pre filters have produced their links,
//! and targets are still source-relative (the rewrite page-relativizes
//! them, and clamps `..` at the project root).

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::project_resources::extract_resource_patterns;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// The metadata key that turns link discovery on (`true`, the default)
/// or off (`false`).
pub const RESOURCE_DISCOVERY_KEY: &str = "resource-discovery";

/// Extensions treated as render outputs when a project document with
/// the same path stem exists (see the module docs).
const RENDER_OUTPUT_EXTENSIONS: &[&str] = &[
    "html", "htm", "pdf", "docx", "pptx", "odt", "epub", "typ", "tex",
];

/// Discover local files that Markdown links point at and append them to
/// `meta.resources`. See the module docs.
///
/// Holds the runtime so existence checks see the hub-client VFS as
/// well as a real filesystem (the `TitleBannerTransform` pattern).
pub struct LinkResourceDiscoveryTransform {
    runtime: Arc<dyn SystemRuntime>,
}

impl LinkResourceDiscoveryTransform {
    pub fn new(runtime: Arc<dyn SystemRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for LinkResourceDiscoveryTransform {
    fn name(&self) -> &str {
        "link-resource-discovery"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if !discovery_enabled(&ast.meta, ctx)? {
            return Ok(());
        }
        // Same gates as `ResourceCollectorTransform`: without a resolver
        // (unit-test scaffolding) there is no output to publish to, and
        // in VFS-root mode (hub-client preview) nothing is copied.
        match ctx.resource_resolver.as_ref() {
            Some(resolver) if !resolver.is_vfs_root_mode() => {}
            _ => return Ok(()),
        }

        let mut targets: Vec<String> = Vec::new();
        crate::ast_walk::for_each_inline_mut(&mut ast.blocks, &mut |inline| {
            if let Inline::Link(link) = inline {
                targets.push(link.target.0.clone());
            }
        });
        if targets.is_empty() {
            return Ok(());
        }

        let finder = CandidateFinder::new(ctx, self.runtime.as_ref());
        let existing: HashSet<String> = extract_resource_patterns(&ast.meta, &["resources"])
            .into_iter()
            .map(|p| p.pattern)
            .collect();
        let mut seen = HashSet::new();
        let mut discovered = Vec::new();
        for target in &targets {
            let Some(rel) = finder.candidate(target) else {
                continue;
            };
            if existing.contains(&rel) || !seen.insert(rel.clone()) {
                continue;
            }
            discovered.push(rel);
        }
        if discovered.is_empty() {
            return Ok(());
        }

        append_resources(&mut ast.meta, &discovered);
        tracing::debug!(
            "link-resource-discovery: {} linked file(s) added to meta.resources",
            discovered.len()
        );
        ctx.discovered_resources.extend(discovered);
        Ok(())
    }
}

/// Read `resource-discovery` from merged metadata. Absent means `true`;
/// a non-boolean value is the `Q-5-52` error.
fn discovery_enabled(meta: &ConfigValue, ctx: &RenderContext) -> Result<bool> {
    let Some(value) = meta.get(RESOURCE_DISCOVERY_KEY) else {
        return Ok(true);
    };
    if let Some(enabled) = value.as_bool() {
        return Ok(enabled);
    }
    let shown = value.as_plain_text().map_or_else(
        || "a value that is not a boolean".to_string(),
        |s| format!("`{s}`"),
    );
    let diagnostic = DiagnosticMessageBuilder::error("Invalid `resource-discovery` Value")
        .with_code("Q-5-52")
        .with_location(value.source_info.clone())
        .problem(format!(
            "`{RESOURCE_DISCOVERY_KEY}` must be `true` or `false`, but it is {shown}."
        ))
        .add_detail(
            "The document was not rendered, so none of the files it links to were \
                 published.",
        )
        .add_hint(format!(
            "Write `{RESOURCE_DISCOVERY_KEY}: false` to stop copying linked files, or \
                 remove the key to keep the default (`true`)."
        ))
        .build();
    let source_context = ctx.source_context.cloned().unwrap_or_default();
    Err(crate::error::QuartoError::Parse(
        crate::error::ParseError::new(vec![diagnostic], source_context),
    ))
}

/// Append `paths` to `meta.resources`, keeping whatever is there (a
/// list, or the single-scalar shorthand).
fn append_resources(meta: &mut ConfigValue, paths: &[String]) {
    let generated = || SourceInfo::generated(By::programmatic_config());
    let mut items: Vec<ConfigValue> = match meta.get("resources") {
        Some(v) => match v.as_array() {
            Some(arr) => arr.to_vec(),
            None if v.is_null() => Vec::new(),
            None => vec![v.clone()],
        },
        None => Vec::new(),
    };
    items.extend(
        paths
            .iter()
            .map(|p| ConfigValue::new_string(p.as_str(), generated())),
    );
    let source = meta
        .get("resources")
        .map_or_else(generated, |v| v.source_info.clone());
    meta.insert_path(&["resources"], ConfigValue::new_array(items, source));
}

/// Decides whether a link target is a publishable local file.
struct CandidateFinder<'a> {
    ctx: &'a RenderContext<'a>,
    runtime: &'a dyn SystemRuntime,
    /// Canonical project root.
    project_dir: PathBuf,
    /// Canonical output dir, when it differs from the project root.
    output_dir: Option<PathBuf>,
    /// Canonical directory of the document being rendered.
    doc_dir: PathBuf,
    /// Project-relative input paths with their extension removed, for
    /// the render-output sibling check.
    input_stems: HashSet<PathBuf>,
}

impl<'a> CandidateFinder<'a> {
    fn new(ctx: &'a RenderContext<'a>, runtime: &'a dyn SystemRuntime) -> Self {
        let canon = |p: &Path| runtime.canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let project_dir = canon(&ctx.project.dir);
        let output_dir = Some(canon(&ctx.project.output_dir)).filter(|o| *o != project_dir);
        let doc_dir = canon(ctx.document.input.parent().unwrap_or(Path::new(".")));
        let mut input_stems: HashSet<PathBuf> = ctx
            .project
            .files
            .iter()
            .filter_map(|f| {
                canon(&f.input)
                    .strip_prefix(&project_dir)
                    .ok()
                    .map(Path::to_path_buf)
            })
            .map(|p| p.with_extension(""))
            .collect();
        if let Some(index) = ctx.project_index.as_ref() {
            input_stems.extend(
                index
                    .profiles()
                    .iter()
                    .map(|p| p.source_path.with_extension("")),
            );
        }
        Self {
            ctx,
            runtime,
            project_dir,
            output_dir,
            doc_dir,
            input_stems,
        }
    }

    /// The doc-relative, `/`-separated path to publish for `target`, or
    /// `None` when it is not a candidate.
    fn candidate(&self, target: &str) -> Option<String> {
        let path_part = link_target_path(target)?;
        let decoded = percent_encoding::percent_decode_str(path_part)
            .decode_utf8_lossy()
            .into_owned();
        let joined = match decoded.strip_prefix('/') {
            Some(rest) => self.project_dir.join(rest),
            None => self.doc_dir.join(&decoded),
        };

        let runtime = self.runtime;
        // Canonicalizing resolves `..` and symlinks, so the containment
        // check below catches escapes either way. A file that doesn't
        // exist fails here.
        let canonical = runtime.canonicalize(&joined).ok()?;
        if !runtime.is_file(&canonical).unwrap_or(false) {
            return None;
        }
        let project_rel = canonical.strip_prefix(&self.project_dir).ok()?;
        if let Some(output_dir) = &self.output_dir
            && canonical.starts_with(output_dir)
        {
            return None;
        }
        if is_hidden_or_frozen(project_rel) || self.is_document_or_render_output(project_rel) {
            return None;
        }

        let doc_rel = pathdiff::diff_paths(&canonical, &self.doc_dir)?;
        Some(quarto_util::to_forward_slashes(&doc_rel))
    }

    fn is_document_or_render_output(&self, project_rel: &Path) -> bool {
        let ext = project_rel
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        if ext.as_deref() == Some("qmd") {
            return true;
        }
        let in_index = self
            .ctx
            .project_index
            .as_ref()
            .is_some_and(|index| index.lookup_by_source(project_rel).is_some());
        let in_files = self.ctx.project.files.iter().any(|f| {
            self.runtime
                .canonicalize(&f.input)
                .is_ok_and(|c| c == self.project_dir.join(project_rel))
        });
        if in_index || in_files {
            return true;
        }
        ext.is_some_and(|e| RENDER_OUTPUT_EXTENSIONS.contains(&e.as_str()))
            && self.input_stems.contains(&project_rel.with_extension(""))
    }
}

/// The path part of a link target that may name a local file: `None`
/// for external URLs (any `scheme:`), protocol-relative and
/// fragment-only targets, and empty paths.
fn link_target_path(target: &str) -> Option<&str> {
    if target.is_empty() || target.starts_with('#') || quarto_util::is_external_url(target) {
        return None;
    }
    let path_part = match target.find(['#', '?']) {
        Some(i) => &target[..i],
        None => target,
    };
    if path_part.is_empty() || path_part == "/" || path_part.ends_with('/') {
        return None;
    }
    Some(path_part)
}

/// A dot-prefixed component anywhere, or a top-level `_freeze/`.
fn is_hidden_or_frozen(project_rel: &Path) -> bool {
    let mut components = project_rel.components().peekable();
    if components
        .peek()
        .is_some_and(|c| c.as_os_str() == "_freeze")
    {
        return true;
    }
    components.any(|c| matches!(c, Component::Normal(s) if s.to_string_lossy().starts_with('.')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_target_path_strips_tails_and_skips_non_files() {
        assert_eq!(link_target_path("data.csv"), Some("data.csv"));
        assert_eq!(link_target_path("data.csv#row=3"), Some("data.csv"));
        assert_eq!(link_target_path("data.csv?x=1#y"), Some("data.csv"));
        assert_eq!(link_target_path("/guide/x.csv"), Some("/guide/x.csv"));
        assert_eq!(link_target_path("../x.csv"), Some("../x.csv"));
        for skipped in [
            "",
            "#top",
            "?q=1",
            "/",
            "dir/",
            "https://example.com/x.csv",
            "//cdn.example.com/x.csv",
            "mailto:a@b.c",
            "data:text/plain,hi",
            "tel:+1",
        ] {
            assert_eq!(link_target_path(skipped), None, "{skipped:?}");
        }
    }

    #[test]
    fn hidden_and_frozen_paths() {
        assert!(is_hidden_or_frozen(Path::new(".hidden/x.csv")));
        assert!(is_hidden_or_frozen(Path::new("a/.git/config")));
        assert!(is_hidden_or_frozen(Path::new(".env")));
        assert!(is_hidden_or_frozen(Path::new(
            "_freeze/page/execute-results/html.json"
        )));
        assert!(!is_hidden_or_frozen(Path::new("a/_freeze/x.csv")));
        assert!(!is_hidden_or_frozen(Path::new("a/b.c.csv")));
        assert!(!is_hidden_or_frozen(Path::new("_underscore.csv")));
    }

    fn meta_with(key: &str, value: ConfigValue) -> ConfigValue {
        let mut meta = ConfigValue::new_map(vec![], SourceInfo::for_test());
        meta.insert_path(&[key], value);
        meta
    }

    fn strings(meta: &ConfigValue) -> Vec<String> {
        extract_resource_patterns(meta, &["resources"])
            .into_iter()
            .map(|p| p.pattern)
            .collect()
    }

    #[test]
    fn append_resources_extends_a_list_or_scalar() {
        let list = ConfigValue::new_array(
            vec![ConfigValue::new_string("a.txt", SourceInfo::for_test())],
            SourceInfo::for_test(),
        );
        let mut meta = meta_with("resources", list);
        append_resources(&mut meta, &["b.csv".into()]);
        assert_eq!(strings(&meta), vec!["a.txt", "b.csv"]);

        let mut meta = meta_with(
            "resources",
            ConfigValue::new_string("only.txt", SourceInfo::for_test()),
        );
        append_resources(&mut meta, &["b.csv".into()]);
        assert_eq!(strings(&meta), vec!["only.txt", "b.csv"]);

        let mut meta = ConfigValue::new_map(vec![], SourceInfo::for_test());
        append_resources(&mut meta, &["b.csv".into(), "c/d.json".into()]);
        assert_eq!(strings(&meta), vec!["b.csv", "c/d.json"]);
    }
}
