/*
 * transforms/components_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: bind declared components to document elements and ship
 * the client runtime (bd-react-components-a8flgldm).
 */

//! Bind `components:` declarations to the elements they name.
//!
//! For every [`ComponentDecl`] the transform finds the `Div`s and `Span`s
//! its binding selects (`#id`: the one element with that id; `.class`:
//! every element with the class) and sets
//! `data-quarto-component="<name>"` on them. The explicit `data-` key
//! survives both the native HTML writer and the preview renderer
//! unchanged, so the DOM contract does not depend on either writer's
//! attribute conventions. Nothing else about the element changes: its
//! markup is the no-script version the runtime mounts over.
//!
//! When at least one element is bound, the transform also:
//!
//! - copies each bound component's module under the project lib dir
//!   (`quarto-components/<project-relative path>`, so relative imports
//!   between an author's files keep working) through a
//!   [`ResourceCopyIntent`], exactly as `format-css` ships stylesheets;
//! - registers the vendored React modules and the client runtime as
//!   artifacts ([`register_component_assets`]);
//! - appends the head block (import map, manifest, runtime module tag;
//!   [`head_block`]) to the `header` include slot.
//!
//! Diagnostics: Q-25-4 for a binding that selects nothing, Q-25-5 for an
//! element two declarations both select (the first keeps it), Q-25-2 for
//! a module outside the project (it cannot be shipped), plus whatever
//! [`parse_components`] reports about the declarations themselves.
//!
//! Scope: `PipelineProfile::HtmlRender` only. The preview renders the
//! AST itself and owns its own loading story (plan D8, a sibling
//! strand); revealjs has its own asset scaffold and is not a target yet.
//! Runs in the Finalization phase after content transforms and before
//! `resource-collector`, so a bound subtree's images are still
//! collected.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::Attr;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::artifact::{Artifact, ArtifactScope};
use crate::ast_walk::{for_each_block_list_mut, for_each_inline_list_mut};
use crate::components::runtime::{
    HEAD_SENTINEL, ManifestEntry, asset_url, head_block, script_json,
};
use crate::components::{
    Binding, COMPONENT_HOST_ATTR, COMPONENTS_KEY, ComponentDecl, ComponentSource, ContentMode,
    parse_components, register_component_assets,
};
use crate::format::PipelineProfile;
use crate::render::{RenderContext, ResourceCopyIntent};
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::append_with_sentinel;

/// Lib-dir prefix under which component modules are shipped.
const COMPONENTS_LIB_PREFIX: &str = "quarto-components/";

/// The element route: `component=<name>` on a Div/Span attr (plan D1).
const ELEMENT_KEY: &str = "component";
/// Element-level props: `component-props='{"a": 1}'`, a JSON object.
const ELEMENT_PROPS_KEY: &str = "component-props";
/// The attribute the runtime reads element props from.
const HOST_PROPS_ATTR: &str = "data-quarto-component-props";
/// Class of the child `<script type="application/json">` that carries a
/// bound node's AST (`content: ast`); the runtime reads and removes it.
const AST_SCRIPT_CLASS: &str = "quarto-component-ast";
/// Class of the props script `quarto.component` emits; left out of the
/// serialized AST, since it is runtime plumbing rather than content.
const PROPS_SCRIPT_CLASS: &str = "quarto-component-props";

/// See module docs.
pub struct ComponentsResolveTransform;

impl ComponentsResolveTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ComponentsResolveTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ComponentsResolveTransform {
    fn name(&self) -> &str {
        "components-resolve"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        if ctx.pipeline_profile != PipelineProfile::HtmlRender {
            return Ok(());
        }
        let Some(value) = ast.meta.get(COMPONENTS_KEY) else {
            return Ok(());
        };
        let mut diags = Vec::new();
        let mut decls = parse_components(value, &mut diags);

        let mut bound = bind_elements(ast, &mut decls, &mut diags);

        if bound.is_empty() {
            ctx.diagnostics.extend(diags);
            return Ok(());
        }
        attach_ast_payloads(ast, &decls);

        // Ship: copy intents, assets, head block. Mirrors `format-css`:
        // VFS-root mode (the in-browser preview) has no output tree to
        // copy into, and the preview profile never reaches here anyway.
        let can_ship = ctx
            .resource_resolver
            .as_ref()
            .is_some_and(|r| !r.is_vfs_root_mode());
        let path_ctx = crate::config_paths::PathCtx::for_document(
            ctx.source_context,
            &ctx.project.dir,
            &ctx.document.input,
        );

        let mut entries = Vec::new();
        let mut copies = Vec::new();
        let mut shipped: BTreeSet<PathBuf> = BTreeSet::new();
        bound.sort_by_key(|i| *i);
        for decl in bound.iter().map(|&i| &decls[i]) {
            let ComponentSource::File(config_path) = &decl.src;
            let Some(source) = config_path.resolve(&path_ctx).file() else {
                continue;
            };
            let Some(project_relative) = project_relative_path(&source, &ctx.project.dir) else {
                diags.push(outside_project_diagnostic(decl, &source));
                continue;
            };
            let lib_path = shipped_lib_path(&project_relative);
            if can_ship {
                let mut shipper = Shipper {
                    ctx,
                    decl,
                    diags: &mut diags,
                    copies: &mut copies,
                    shipped: &mut shipped,
                    new_artifacts: Vec::new(),
                };
                let ok = shipper.ship(&source, &project_relative, &decl.src_source_info);
                let new_artifacts = std::mem::take(&mut shipper.new_artifacts);
                for (key, artifact) in new_artifacts {
                    ctx.artifacts.store(key, artifact);
                }
                if !ok {
                    continue;
                }
            }
            entries.push(ManifestEntry {
                name: decl.name.clone(),
                module_url: asset_url(ctx, &lib_path),
                export: decl.export.clone(),
                content: decl.content,
                props: decl.props.clone(),
            });
        }

        if !entries.is_empty() {
            ctx.resource_copies.extend(copies);
            register_component_assets(&mut ctx.artifacts);
            let block = head_block(ctx, &entries);
            append_with_sentinel(&mut ast.meta, "header", HEAD_SENTINEL, block);
        }
        ctx.diagnostics.extend(diags);
        Ok(())
    }
}

/// The project-relative path of a module, or `None` when it escapes the
/// project root (nothing outside the project can be shipped).
fn project_relative_path(source: &Path, project_dir: &Path) -> Option<PathBuf> {
    pathdiff::diff_paths(source, project_dir).filter(|p| {
        !p.components()
            .next()
            .is_some_and(|c| c == Component::ParentDir)
    })
}

/// Where a module lands under the lib dir: its project-relative path
/// under `quarto-components/`, with a TypeScript or JSX extension
/// rewritten to `.js` (the transpiler's output), matching how the
/// transpiler rewrites relative import specifiers.
fn shipped_lib_path(project_relative: &Path) -> String {
    let rel = quarto_util::to_forward_slashes(project_relative);
    let rel = if quarto_transpile::needs_transpile(&rel) {
        match rel.rfind('.') {
            Some(i) => format!("{}.js", &rel[..i]),
            None => rel,
        }
    } else {
        rel
    };
    format!("{COMPONENTS_LIB_PREFIX}{rel}")
}

/// Ships one component's module graph: the entry module and, transitively,
/// every relative import inside the project. TypeScript and JSX files are
/// transpiled and stored as artifacts; plain JavaScript is copied. A file
/// is shipped once per page however many components reach it.
struct Shipper<'a, 'c> {
    ctx: &'a RenderContext<'c>,
    decl: &'a ComponentDecl,
    diags: &'a mut Vec<DiagnosticMessage>,
    copies: &'a mut Vec<ResourceCopyIntent>,
    shipped: &'a mut BTreeSet<PathBuf>,
    /// Artifacts to store once the borrow of `ctx` ends.
    new_artifacts: Vec<(String, Artifact)>,
}

impl Shipper<'_, '_> {
    /// Returns false when the entry module itself cannot be shipped
    /// (then the component is left out of the manifest).
    fn ship(&mut self, source: &Path, project_relative: &Path, origin: &SourceInfo) -> bool {
        if !self.shipped.insert(source.to_path_buf()) {
            return true;
        }
        let resolver = self
            .ctx
            .resource_resolver
            .as_ref()
            .expect("shipping needs a resolver");
        let lib_path = shipped_lib_path(project_relative);
        let file_name = quarto_util::to_forward_slashes(project_relative);
        let dest = resolver.on_disk_path_for(ArtifactScope::Project, Path::new(&lib_path));

        let imports: Vec<String> = if quarto_transpile::needs_transpile(&file_name) {
            let text = match std::fs::read_to_string(source) {
                Ok(t) => t,
                Err(e) => {
                    self.diags.push(transpile_diagnostic(
                        self.decl,
                        &file_name,
                        &format!("could not read the file: {e}"),
                        origin,
                    ));
                    return false;
                }
            };
            match quarto_transpile::transpile(&text, &file_name) {
                Ok(out) => {
                    self.new_artifacts.push((
                        format!("components:module:{lib_path}"),
                        Artifact::from_string(&out.code, "text/javascript")
                            .with_path(&lib_path)
                            .with_scope(ArtifactScope::Project),
                    ));
                    out.relative_imports
                }
                Err(errors) => {
                    let first = errors.first().map(ToString::to_string).unwrap_or_default();
                    self.diags
                        .push(transpile_diagnostic(self.decl, &file_name, &first, origin));
                    return false;
                }
            }
        } else {
            if dest != source {
                self.copies.push(ResourceCopyIntent {
                    src: source.to_path_buf(),
                    dest,
                    origin: origin.clone(),
                });
            }
            // Follow the module's relative imports; an unreadable file is
            // the copy drain's Q-5-6, and ships nothing further.
            std::fs::read_to_string(source)
                .map(|text| quarto_transpile::relative_imports_of(&text, &file_name))
                .unwrap_or_default()
        };

        let dir = source.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        for spec in imports {
            let Some(target) = resolve_relative_import(&dir, &spec) else {
                self.diags.push(transpile_diagnostic(
                    self.decl,
                    &file_name,
                    &format!("import `{spec}` names no file"),
                    origin,
                ));
                continue;
            };
            let Some(target_relative) = project_relative_path(&target, &self.ctx.project.dir)
            else {
                self.diags
                    .push(outside_project_diagnostic(self.decl, &target));
                continue;
            };
            // Imports are shipped on a best-effort basis: a broken helper
            // is reported, and the entry module still ships.
            self.ship(&target, &target_relative, origin);
        }
        true
    }
}

/// Resolve `./x.js`-style specifier against a directory, accepting the
/// TypeScript convention of importing `./x.js` when the file on disk is
/// `x.ts` or `x.tsx`.
fn resolve_relative_import(dir: &Path, spec: &str) -> Option<PathBuf> {
    // lint:allow(config-path-base) — an ES-module import specifier inside a component's source file, relative to that file's directory by the module system's rule; not a YAML path
    let direct = lexically_normalize(&dir.join(spec));
    if direct.is_file() {
        return Some(direct);
    }
    if let Some(stem) = spec.strip_suffix(".js") {
        for ext in ["ts", "tsx"] {
            let candidate = lexically_normalize(&dir.join(format!("{stem}.{ext}")));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn transpile_diagnostic(
    decl: &ComponentDecl,
    file_name: &str,
    detail: &str,
    origin: &SourceInfo,
) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!(
        "component `{}`: `{file_name}` could not be prepared for the browser",
        decl.name
    ))
    .with_code("Q-25-9")
    .with_location(origin.clone())
    .problem(format!(
        "TypeScript and JSX modules are transpiled at render time, and this one failed: \
         {detail}. The component was not shipped; its element keeps the static content."
    ))
    .add_hint("Fix the error in the module (the position is in the module, not the document).")
    .build()
}

/// Bind elements to declarations, by both routes, and set the host
/// attribute. Returns the indices of the declarations that bound at least
/// one element, in first-match order; diagnoses the rest (Q-25-4) and any
/// element two declarations compete for (Q-25-5).
///
/// Per element, the element route goes first: a `component=<name>` key
/// (or a `data-quarto-component` already set by `quarto.component` in
/// Lua) names a declaration, which must exist (Q-25-7); a path-like value
/// (`component=./x.js`) declares the component on the spot. Then every
/// declaration's `bind:` selector is tried. `component-props` must be a
/// JSON object (Q-25-8) and is renamed to the runtime's attribute.
fn bind_elements(
    ast: &mut Pandoc,
    decls: &mut Vec<ComponentDecl>,
    diags: &mut Vec<DiagnosticMessage>,
) -> Vec<usize> {
    let mut matched = vec![0usize; decls.len()];
    let mut order: Vec<usize> = Vec::new();

    let mut visit = |attr: &mut Attr, location: Option<SourceInfo>| {
        // Element route.
        if let Some(value) = attr.2.remove(ELEMENT_KEY) {
            let value = value.trim().to_string();
            match decls.iter().position(|d| d.name == value) {
                Some(i) => {
                    attr.2.insert(COMPONENT_HOST_ATTR.to_string(), value);
                    matched[i] += 1;
                    if !order.contains(&i) {
                        order.push(i);
                    }
                }
                None if looks_like_module_path(&value) => {
                    let i = decls.len();
                    decls.push(ComponentDecl {
                        name: value.clone(),
                        src: ComponentSource::File(crate::config_paths::ConfigPath::Raw {
                            raw: value.clone(),
                            source: quarto_source_map::SourceInfo::for_test(),
                        }),
                        export: None,
                        bind: crate::components::Binding::Id(value.clone()),
                        content: Default::default(),
                        props: None,
                        source_info: location.clone().unwrap_or_default(),
                        src_source_info: location.clone().unwrap_or_default(),
                    });
                    matched.push(1);
                    order.push(i);
                    attr.2.insert(COMPONENT_HOST_ATTR.to_string(), value);
                }
                None => {
                    diags.push(unknown_component_diagnostic(&value, location.clone()));
                }
            }
        } else if let Some(existing) = attr.2.get(COMPONENT_HOST_ATTR).cloned() {
            // Pre-tagged by `quarto.component(...)`: counts as bound.
            match decls.iter().position(|d| d.name == existing) {
                Some(i) => {
                    matched[i] += 1;
                    if !order.contains(&i) {
                        order.push(i);
                    }
                }
                None => diags.push(unknown_component_diagnostic(&existing, location.clone())),
            }
        }
        if let Some(raw) = attr.2.remove(ELEMENT_PROPS_KEY) {
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(v) if v.is_object() => {
                    attr.2.insert(HOST_PROPS_ATTR.to_string(), raw);
                }
                _ => diags.push(bad_props_diagnostic(&raw, location.clone())),
            }
        }

        // Selector route.
        for (i, decl) in decls.iter().enumerate() {
            if !decl.bind.matches(&attr.0, &attr.1) {
                continue;
            }
            match attr.2.get(COMPONENT_HOST_ATTR) {
                Some(existing) if existing != &decl.name => {
                    diags.push(competing_binding_diagnostic(
                        decl,
                        existing,
                        location.clone(),
                    ));
                    continue;
                }
                _ => {}
            }
            attr.2
                .insert(COMPONENT_HOST_ATTR.to_string(), decl.name.clone());
            matched[i] += 1;
            if !order.contains(&i) {
                order.push(i);
            }
        }
    };

    for_each_block_list_mut(&mut ast.blocks, &mut |list| {
        for block in list.iter_mut() {
            if let Block::Div(div) = block {
                let location = div
                    .attr_source
                    .combine_all()
                    .or_else(|| Some(div.source_info.clone()));
                visit(&mut div.attr, location);
            }
        }
    });
    for_each_inline_list_mut(&mut ast.blocks, &mut |list| {
        for inline in list.iter_mut() {
            if let Inline::Span(span) = inline {
                let location = span
                    .attr_source
                    .combine_all()
                    .or_else(|| Some(span.source_info.clone()));
                visit(&mut span.attr, location);
            }
        }
    });

    for (i, decl) in decls.iter().enumerate() {
        if decl.bind == Binding::Chrome {
            // The host is template chrome (`nav#TOC` for `toc-component:`,
            // bd-toc-scrollspy-js-41e624d8), not an AST element: always
            // bound, and never Q-25-4.
            if !order.contains(&i) {
                order.push(i);
            }
        } else if matched[i] == 0 {
            diags.push(unbound_diagnostic(decl));
        }
    }
    order
}

/// For every element bound to a `content: ast` declaration, prepend a
/// `<script type="application/json" class="quarto-component-ast">` child
/// holding the node's Pandoc JSON (source information stripped, the
/// wire shape the preview uses), so the runtime can hand the component
/// `node`. The element's own props script is left out of the payload.
/// Idempotent: an element that already carries the payload is skipped.
fn attach_ast_payloads(ast: &mut Pandoc, decls: &[ComponentDecl]) {
    let wants_ast = |attr: &Attr| {
        attr.2
            .get(COMPONENT_HOST_ATTR)
            .and_then(|name| decls.iter().find(|d| &d.name == name))
            .is_some_and(|d| d.content == ContentMode::Ast)
    };
    let context = pampa::pandoc::ASTContext::default();

    for_each_block_list_mut(&mut ast.blocks, &mut |list| {
        for block in list.iter_mut() {
            let Block::Div(div) = block else { continue };
            if !wants_ast(&div.attr) || has_payload_block(&div.content) {
                continue;
            }
            let mut node = Block::Div(div.clone());
            if let Block::Div(d) = &mut node {
                d.content.retain(|b| !is_props_script_block(b));
            }
            let json = pampa::writers::json::blocks_to_source_free_json(&[node], &context);
            let text = ast_script(&json[0]);
            div.content.insert(
                0,
                Block::RawBlock(quarto_pandoc_types::block::RawBlock {
                    format: "html".to_string(),
                    text,
                    source_info: div.source_info.clone(),
                }),
            );
        }
    });
    for_each_inline_list_mut(&mut ast.blocks, &mut |list| {
        for inline in list.iter_mut() {
            let Inline::Span(span) = inline else { continue };
            if !wants_ast(&span.attr) || has_payload_inline(&span.content) {
                continue;
            }
            let mut node = Inline::Span(span.clone());
            if let Inline::Span(s) = &mut node {
                s.content.retain(|i| !is_props_script_inline(i));
            }
            let json = pampa::writers::json::inlines_to_source_free_json(&vec![node], &context);
            let text = ast_script(&json[0]);
            span.content.insert(
                0,
                Inline::RawInline(quarto_pandoc_types::inline::RawInline {
                    format: "html".to_string(),
                    text,
                    source_info: span.source_info.clone(),
                }),
            );
        }
    });
}

fn ast_script(node: &serde_json::Value) -> String {
    format!(
        "<script type=\"application/json\" class=\"{AST_SCRIPT_CLASS}\">{}</script>",
        script_json(node)
    )
}

fn is_script_with_class(text: &str, class: &str) -> bool {
    text.starts_with("<script type=\"application/json\" class=\"") && text.contains(class)
}

fn has_payload_block(blocks: &[Block]) -> bool {
    blocks.first().is_some_and(
        |b| matches!(b, Block::RawBlock(r) if is_script_with_class(&r.text, AST_SCRIPT_CLASS)),
    )
}

fn has_payload_inline(inlines: &[Inline]) -> bool {
    inlines.first().is_some_and(
        |i| matches!(i, Inline::RawInline(r) if is_script_with_class(&r.text, AST_SCRIPT_CLASS)),
    )
}

fn is_props_script_block(b: &Block) -> bool {
    matches!(b, Block::RawBlock(r) if is_script_with_class(&r.text, PROPS_SCRIPT_CLASS))
}

fn is_props_script_inline(i: &Inline) -> bool {
    matches!(i, Inline::RawInline(r) if is_script_with_class(&r.text, PROPS_SCRIPT_CLASS))
}

/// `component=./x.js` declares on the spot; `component=name` refers to a
/// declaration. A value with a path separator or a module extension is
/// the former.
fn looks_like_module_path(value: &str) -> bool {
    value.contains('/') || value.ends_with(".js") || value.ends_with(".mjs")
}

fn unknown_component_diagnostic(name: &str, location: Option<SourceInfo>) -> DiagnosticMessage {
    let mut b =
        DiagnosticMessageBuilder::warning(format!("no component named `{name}` is declared"))
            .with_code("Q-25-7")
            .problem(
                "The element asks for a component that `components:` does not declare, so nothing \
         will mount on it; it keeps its static content.",
            )
            .add_hint(format!(
                "Add `- name: {name}` with a `src:` under `components:`, or write the module path \
         directly (`component=./{name}.js`)."
            ));
    if let Some(loc) = location {
        b = b.with_location(loc);
    }
    b.build()
}

fn bad_props_diagnostic(raw: &str, location: Option<SourceInfo>) -> DiagnosticMessage {
    let shown: String = raw.chars().take(60).collect();
    let mut b = DiagnosticMessageBuilder::warning(format!(
        "`component-props` is not a JSON object: {shown}"
    ))
    .with_code("Q-25-8")
    .problem(
        "Element props must be a JSON object literal; this value was dropped, so the \
         component gets only the props declared in metadata.",
    )
    .add_hint("Write `component-props='{\"key\": value}'`, with the JSON in single quotes.");
    if let Some(loc) = location {
        b = b.with_location(loc);
    }
    b.build()
}

fn unbound_diagnostic(decl: &ComponentDecl) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!(
        "component `{}` binds to `{}`, which matches no element",
        decl.name,
        decl.bind.selector()
    ))
    .with_code("Q-25-4")
    .with_location(decl.source_info.clone())
    .problem(
        "No Div or Span on this page has that id or class, so the component was not \
         shipped with the page.",
    )
    .add_hint(
        "Give the element the id or class (`::: {#name}` or `[text]{.name}`), or set \
         `bind:` to the one it has.",
    )
    .build()
}

fn competing_binding_diagnostic(
    decl: &ComponentDecl,
    winner: &str,
    location: Option<SourceInfo>,
) -> DiagnosticMessage {
    let mut b = DiagnosticMessageBuilder::warning(format!(
        "component `{}` also selects an element already bound to `{winner}`",
        decl.name
    ))
    .with_code("Q-25-5")
    .problem(format!(
        "An element can host one component. `{winner}` was declared first and keeps it; \
         `{}` (`{}`) is ignored for this element.",
        decl.name,
        decl.bind.selector()
    ))
    .add_hint("Narrow one of the two `bind:` selectors so they select different elements.");
    if let Some(loc) = location {
        b = b.with_location(loc);
    }
    b.build()
}

fn outside_project_diagnostic(decl: &ComponentDecl, source: &Path) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!(
        "component `{}`: `{}` is outside the project and cannot be shipped",
        decl.name,
        source.display()
    ))
    .with_code("Q-25-2")
    .with_location(decl.src_source_info.clone())
    .problem(
        "Only files inside the project are copied into the output, so the page would link \
         a module that is not there. The element keeps its static content.",
    )
    .add_hint("Move the module into the project and point `src:` at it.")
    .build()
}

/// Fold `.` and `..` lexically (no filesystem access), as the output sink
/// does for destinations, so src/dest comparisons agree.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(Component::ParentDir.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use crate::resource_resolver::ResourceResolverContext;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{Div, Paragraph};
    use quarto_pandoc_types::inline::{Span, Str};

    fn attr(id: &str, classes: &[&str]) -> Attr {
        (
            id.to_string(),
            classes.iter().map(|c| c.to_string()).collect(),
            hashlink::LinkedHashMap::new(),
        )
    }

    fn div(id: &str, classes: &[&str], content: Vec<Block>) -> Block {
        Block::Div(Div {
            attr: attr(id, classes),
            content,
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::for_test(),
            })],
            source_info: SourceInfo::for_test(),
        })
    }

    fn span_para(id: &str, classes: &[&str]) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Span(Span {
                attr: attr(id, classes),
                content: vec![],
                source_info: SourceInfo::for_test(),
                attr_source: AttrSourceInfo::empty(),
            })],
            source_info: SourceInfo::for_test(),
        })
    }

    /// Front matter → materialized metadata, the pipeline's shape.
    fn meta(yaml: &str) -> ConfigValue {
        use pampa::pandoc::yaml_to_config_value;
        use pampa::utils::diagnostic_collector::DiagnosticCollector;
        use quarto_config::{InterpretationContext, MergedConfig};
        let parsed = quarto_yaml::parse_file(yaml, "index.qmd").expect("valid yaml");
        let mut collector = DiagnosticCollector::new();
        let doc = yaml_to_config_value(
            parsed,
            InterpretationContext::DocumentMetadata,
            &mut collector,
        );
        MergedConfig::new(vec![&doc])
            .materialize()
            .expect("materialize")
    }

    fn project() -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![DocumentInfo::from_path("/project/docs/page.qmd")],
            output_dir: PathBuf::from("/site"),
            ..Default::default()
        }
    }

    struct Run {
        ast: Pandoc,
        diags: Vec<DiagnosticMessage>,
        copies: Vec<ResourceCopyIntent>,
        artifact_keys: Vec<String>,
        artifact_text: std::collections::BTreeMap<String, String>,
    }

    async fn run_with(
        ast: Pandoc,
        format: Format,
        resolver: Option<ResourceResolverContext>,
    ) -> Run {
        let project = project();
        let doc = DocumentInfo::from_path("/project/docs/page.qmd");
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        if let Some(r) = resolver {
            ctx = ctx.with_resource_resolver(r);
        }
        let mut ast = ast;
        ComponentsResolveTransform::new()
            .transform(&mut ast, &mut ctx)
            .await
            .unwrap();
        let mut artifact_keys: Vec<String> = ctx.artifacts.keys().map(|k| k.to_string()).collect();
        artifact_keys.sort();
        Run {
            ast,
            diags: ctx.diagnostics,
            copies: ctx.resource_copies,
            artifact_keys,
            artifact_text: Default::default(),
        }
    }

    async fn run(ast: Pandoc) -> Run {
        run_with(
            ast,
            Format::html(),
            Some(ResourceResolverContext::website(
                "/site",
                "/site/docs/page.html",
                "site_libs",
                "page",
            )),
        )
        .await
    }

    fn header_includes(meta: &ConfigValue) -> Vec<String> {
        meta.get_path(&["rendered", "includes", "header"])
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn host_attr(block: &Block) -> Option<&str> {
        match block {
            Block::Div(d) => d.attr.2.get(COMPONENT_HOST_ATTR).map(String::as_str),
            _ => None,
        }
    }

    fn codes(diags: &[DiagnosticMessage]) -> Vec<&str> {
        diags.iter().filter_map(|d| d.code.as_deref()).collect()
    }

    const ONE: &str = "components:\n  - name: counter\n    src: counter.js\n";

    #[tokio::test]
    async fn binds_by_id_marks_the_element_and_ships_the_runtime() {
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![
                para("before"),
                div("counter", &[], vec![para("inside")]),
                para("after"),
            ],
        };
        let r = run(ast).await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        assert_eq!(host_attr(&r.ast.blocks[1]), Some("counter"));
        assert_eq!(host_attr(&r.ast.blocks[0]), None);
        // The element's own content is untouched: it is the no-script version.
        match &r.ast.blocks[1] {
            Block::Div(d) => assert_eq!(d.content, vec![para("inside")]),
            _ => unreachable!(),
        }
        let includes = header_includes(&r.ast.meta);
        assert_eq!(includes.len(), 1);
        let block = &includes[0];
        assert!(block.starts_with(HEAD_SENTINEL));
        assert!(
            block.contains("\"module\":\"../site_libs/quarto-components/docs/counter.js\""),
            "{block}"
        );
        assert!(block.contains("src=\"../site_libs/quarto-components/quarto-components.js\""));
        assert_eq!(
            r.artifact_keys,
            vec![
                "components:react:jsx-runtime",
                "components:react:react",
                "components:react:react-dom",
                "components:react:react-dom-client",
                "components:runtime",
            ]
        );
        assert_eq!(r.copies.len(), 1);
        assert_eq!(r.copies[0].src, PathBuf::from("/project/docs/counter.js"));
        assert_eq!(
            r.copies[0].dest,
            PathBuf::from("/site/site_libs/quarto-components/docs/counter.js")
        );
    }

    #[tokio::test]
    async fn binds_by_class_marks_every_element_including_spans() {
        let yaml = "components:\n  - name: card\n    src: card.js\n    bind: .card\n";
        let ast = Pandoc {
            meta: meta(yaml),
            blocks: vec![
                div("", &["card"], vec![]),
                div("", &["other"], vec![div("", &["card", "x"], vec![])]),
                span_para("", &["card"]),
            ],
        };
        let r = run(ast).await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        assert_eq!(host_attr(&r.ast.blocks[0]), Some("card"));
        assert_eq!(host_attr(&r.ast.blocks[1]), None);
        match &r.ast.blocks[1] {
            Block::Div(d) => assert_eq!(host_attr(&d.content[0]), Some("card")),
            _ => unreachable!(),
        }
        match &r.ast.blocks[2] {
            Block::Paragraph(p) => match &p.content[0] {
                Inline::Span(s) => assert_eq!(
                    s.attr.2.get(COMPONENT_HOST_ATTR).map(String::as_str),
                    Some("card")
                ),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
        // One manifest entry and one copy for the three hosts.
        assert_eq!(r.copies.len(), 1);
        assert_eq!(
            header_includes(&r.ast.meta)[0]
                .matches("\"card\":{")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn no_match_is_q_25_4_and_ships_nothing() {
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![div("other", &[], vec![])],
        };
        let r = run(ast).await;
        assert_eq!(codes(&r.diags), vec!["Q-25-4"]);
        assert!(
            r.diags[0].title.contains("`#counter`"),
            "{}",
            r.diags[0].title
        );
        assert!(header_includes(&r.ast.meta).is_empty());
        assert!(r.artifact_keys.is_empty());
        assert!(r.copies.is_empty());
    }

    #[tokio::test]
    async fn competing_bindings_keep_the_first_and_report_q_25_5() {
        let yaml = "components:\n  - name: a\n    src: a.js\n    bind: .x\n  - name: b\n    src: b.js\n    bind: '#one'\n";
        let ast = Pandoc {
            meta: meta(yaml),
            blocks: vec![div("one", &["x"], vec![]), div("two", &["x"], vec![])],
        };
        let r = run(ast).await;
        assert_eq!(codes(&r.diags), vec!["Q-25-5", "Q-25-4"]);
        assert_eq!(host_attr(&r.ast.blocks[0]), Some("a"));
        assert_eq!(host_attr(&r.ast.blocks[1]), Some("a"));
        // `b` bound nothing, so only `a` ships.
        assert_eq!(r.copies.len(), 1);
        assert!(!header_includes(&r.ast.meta)[0].contains("\"b\":{"));
    }

    #[tokio::test]
    async fn module_outside_the_project_is_q_25_2() {
        let yaml = "components:\n  - name: counter\n    src: ../../elsewhere/counter.js\n";
        let ast = Pandoc {
            meta: meta(yaml),
            blocks: vec![div("counter", &[], vec![])],
        };
        let r = run(ast).await;
        assert_eq!(codes(&r.diags), vec!["Q-25-2"]);
        // The element is marked (the contract) but nothing ships.
        assert_eq!(host_attr(&r.ast.blocks[0]), Some("counter"));
        assert!(header_includes(&r.ast.meta).is_empty());
        assert!(r.artifact_keys.is_empty());
    }

    #[tokio::test]
    async fn leading_slash_src_is_project_root_relative_after_marking() {
        // The merge-time marking turns `/lib/x.js` into a project-root-relative
        // Path; here that is `../lib/x.js` from docs/page.qmd.
        let mut m = meta("components:\n  - name: counter\n    src: /lib/counter.js\n");
        let items = m.get_mut(COMPONENTS_KEY).unwrap();
        if let quarto_pandoc_types::config_value::ConfigValueKind::Array(items) = &mut items.value {
            items[0].get_mut("src").unwrap().value =
                quarto_pandoc_types::config_value::ConfigValueKind::Path("lib/counter.js".into());
        }
        let ast = Pandoc {
            meta: m,
            blocks: vec![div("counter", &[], vec![])],
        };
        let r = run(ast).await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        assert_eq!(r.copies[0].src, PathBuf::from("/project/lib/counter.js"));
        assert!(
            header_includes(&r.ast.meta)[0]
                .contains("../site_libs/quarto-components/lib/counter.js"),
            "{}",
            header_includes(&r.ast.meta)[0]
        );
    }

    fn div_with(id: &str, kvs: &[(&str, &str)], content: Vec<Block>) -> Block {
        let mut b = div(id, &[], content);
        if let Block::Div(d) = &mut b {
            for (k, v) in kvs {
                d.attr.2.insert(k.to_string(), v.to_string());
            }
        }
        b
    }

    #[tokio::test]
    async fn element_route_binds_a_declared_name_and_keeps_props() {
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![
                div_with(
                    "",
                    &[
                        ("component", "counter"),
                        ("component-props", "{\"start\": 5}"),
                    ],
                    vec![],
                ),
                div_with("", &[("component", "counter")], vec![]),
            ],
        };
        let r = run(ast).await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        for b in &r.ast.blocks {
            assert_eq!(host_attr(b), Some("counter"));
            let Block::Div(d) = b else { unreachable!() };
            assert!(
                !d.attr.2.contains_key("component"),
                "the author key is consumed"
            );
        }
        let Block::Div(d) = &r.ast.blocks[0] else {
            unreachable!()
        };
        assert_eq!(
            d.attr
                .2
                .get("data-quarto-component-props")
                .map(String::as_str),
            Some("{\"start\": 5}")
        );
        assert!(!d.attr.2.contains_key("component-props"));
        assert_eq!(r.copies.len(), 1);
    }

    #[tokio::test]
    async fn element_route_unknown_name_is_q_25_7_and_bad_props_is_q_25_8() {
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![
                div_with("", &[("component", "nope")], vec![]),
                div_with("counter", &[("component-props", "[1, 2]")], vec![]),
            ],
        };
        let r = run(ast).await;
        assert_eq!(codes(&r.diags), vec!["Q-25-7", "Q-25-8"]);
        assert_eq!(host_attr(&r.ast.blocks[0]), None);
        assert_eq!(host_attr(&r.ast.blocks[1]), Some("counter"));
        let Block::Div(d) = &r.ast.blocks[1] else {
            unreachable!()
        };
        assert!(!d.attr.2.contains_key("component-props"));
        assert!(!d.attr.2.contains_key("data-quarto-component-props"));
    }

    #[tokio::test]
    async fn element_route_path_value_declares_on_the_spot() {
        let ast = Pandoc {
            meta: meta("components: []\n"),
            blocks: vec![div_with("", &[("component", "./widgets/w.js")], vec![])],
        };
        let r = run(ast).await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        assert_eq!(host_attr(&r.ast.blocks[0]), Some("./widgets/w.js"));
        assert_eq!(r.copies.len(), 1);
        assert_eq!(r.copies[0].src, PathBuf::from("/project/docs/widgets/w.js"));
        assert!(
            header_includes(&r.ast.meta)[0]
                .contains("\"./widgets/w.js\":{\"content\":\"dom\",\"export\":\"default\",\"module\":\"../site_libs/quarto-components/docs/widgets/w.js\"}"),
            "{}",
            header_includes(&r.ast.meta)[0]
        );
    }

    #[tokio::test]
    async fn pre_tagged_element_from_lua_counts_as_bound() {
        // `quarto.component("counter", …)` sets the host attribute itself.
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![div_with(
                "",
                &[("data-quarto-component", "counter")],
                vec![],
            )],
        };
        let r = run(ast).await;
        assert!(
            r.diags.is_empty(),
            "no Q-25-4: the element route satisfied the declaration: {:?}",
            r.diags
        );
        assert_eq!(r.copies.len(), 1);

        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![div_with("", &[("data-quarto-component", "ghost")], vec![])],
        };
        let r = run(ast).await;
        assert_eq!(codes(&r.diags), vec!["Q-25-7", "Q-25-4"]);
    }

    #[tokio::test]
    async fn declared_props_reach_the_manifest() {
        let yaml =
            "components:\n  - name: counter\n    src: counter.js\n    props:\n      start: 2\n";
        let ast = Pandoc {
            meta: meta(yaml),
            blocks: vec![div("counter", &[], vec![])],
        };
        let r = run(ast).await;
        assert!(header_includes(&r.ast.meta)[0].contains("\"props\":{\"start\":2}"));
    }

    #[tokio::test]
    async fn content_ast_prepends_the_node_json_without_the_props_script() {
        let yaml = "components:\n  - name: gallery\n    src: gallery.js\n    content: ast\n";
        let props = Block::RawBlock(quarto_pandoc_types::block::RawBlock {
            format: "html".into(),
            text: "<script type=\"application/json\" class=\"quarto-component-props\">{}</script>"
                .into(),
            source_info: SourceInfo::for_test(),
        });
        let ast = Pandoc {
            meta: meta(yaml),
            blocks: vec![
                div("gallery", &["demo"], vec![props, para("hi")]),
                span_para("", &["x"]),
            ],
        };
        let r = run(ast).await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        let Block::Div(d) = &r.ast.blocks[0] else {
            unreachable!()
        };
        assert_eq!(d.content.len(), 3, "payload + props script + paragraph");
        let Block::RawBlock(raw) = &d.content[0] else {
            panic!("payload first")
        };
        let json_text = raw
            .text
            .strip_prefix("<script type=\"application/json\" class=\"quarto-component-ast\">")
            .and_then(|t| t.strip_suffix("</script>"))
            .expect("payload script shape");
        let node: serde_json::Value = serde_json::from_str(json_text).unwrap();
        assert_eq!(node["t"], "Div");
        assert_eq!(node["c"][0][0], "gallery");
        assert_eq!(
            node["c"][0][2],
            serde_json::json!([["data-quarto-component", "gallery"]])
        );
        let blocks = node["c"][1].as_array().unwrap();
        assert_eq!(
            blocks.len(),
            1,
            "the props script is not content: {blocks:?}"
        );
        assert_eq!(blocks[0]["t"], "Para");
        assert!(
            json_text.contains("\\u003c") || !json_text.contains('<'),
            "escaped for <script>"
        );
        // Re-running does not add a second payload.
        let again = run(r.ast.clone()).await;
        let Block::Div(d) = &again.ast.blocks[0] else {
            unreachable!()
        };
        assert_eq!(d.content.len(), 3);
        // The manifest says `ast` so the runtime knows to look.
        assert!(header_includes(&r.ast.meta)[0].contains("\"content\":\"ast\""));
    }

    #[tokio::test]
    async fn content_ast_on_a_span_prepends_an_inline_payload() {
        let yaml =
            "components:\n  - name: spark\n    src: spark.js\n    bind: .spark\n    content: ast\n";
        let ast = Pandoc {
            meta: meta(yaml),
            blocks: vec![span_para("", &["spark"])],
        };
        let r = run(ast).await;
        let Block::Paragraph(p) = &r.ast.blocks[0] else {
            unreachable!()
        };
        let Inline::Span(s) = &p.content[0] else {
            unreachable!()
        };
        let Inline::RawInline(raw) = &s.content[0] else {
            panic!("payload first")
        };
        assert!(raw.text.contains("\"t\":\"Span\""), "{}", raw.text);
    }

    #[tokio::test]
    async fn content_dom_attaches_no_payload() {
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![div("counter", &[], vec![para("hi")])],
        };
        let r = run(ast).await;
        let Block::Div(d) = &r.ast.blocks[0] else {
            unreachable!()
        };
        assert_eq!(d.content, vec![para("hi")]);
    }

    /// A real project on disk, for the shipping tests: the transform
    /// reads TypeScript sources and follows imports there.
    struct DiskProject {
        temp: tempfile::TempDir,
    }

    impl DiskProject {
        fn new(files: &[(&str, &str)]) -> Self {
            let temp = tempfile::TempDir::new().unwrap();
            for (rel, text) in files {
                let p = temp.path().join(rel);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, text).unwrap();
            }
            Self { temp }
        }

        fn root(&self) -> PathBuf {
            self.temp.path().canonicalize().unwrap()
        }

        async fn run(&self, yaml: &str, blocks: Vec<Block>) -> Run {
            let root = self.root();
            let project = ProjectContext {
                dir: root.clone(),
                config: ProjectConfig::default(),
                is_single_file: false,
                files: vec![DocumentInfo::from_path(root.join("index.qmd"))],
                output_dir: root.join("_site"),
                ..Default::default()
            };
            let doc = DocumentInfo::from_path(root.join("index.qmd"));
            let format = Format::html();
            let binaries = BinaryDependencies::new();
            let resolver = ResourceResolverContext::website(
                root.join("_site"),
                root.join("_site/index.html"),
                "site_libs",
                "index",
            );
            let mut ctx = RenderContext::new(&project, &doc, &format, &binaries)
                .with_resource_resolver(resolver);
            let mut ast = Pandoc {
                meta: meta(yaml),
                blocks,
            };
            ComponentsResolveTransform::new()
                .transform(&mut ast, &mut ctx)
                .await
                .unwrap();
            let mut artifact_keys: Vec<String> =
                ctx.artifacts.keys().map(|k| k.to_string()).collect();
            artifact_keys.sort();
            Run {
                ast,
                diags: ctx.diagnostics,
                copies: ctx.resource_copies,
                artifact_keys,
                artifact_text: ctx
                    .artifacts
                    .keys()
                    .map(|k| {
                        (
                            k.to_string(),
                            String::from_utf8_lossy(&ctx.artifacts.get(k).unwrap().content)
                                .to_string(),
                        )
                    })
                    .collect(),
            }
        }
    }

    const TSX: &str = "components:\n  - name: counter\n    src: counter.tsx\n";

    #[tokio::test]
    async fn tsx_entry_is_transpiled_and_its_ts_imports_follow() {
        let p = DiskProject::new(&[
            (
                "counter.tsx",
                "import { fmt } from \"./lib/format.ts\";\nexport default function C({ children }: { children: unknown }) { return <b>{fmt(1)}{children}</b>; }\n",
            ),
            (
                "lib/format.ts",
                "import { pad } from \"./pad.js\";\nexport const fmt = (n: number): string => pad(String(n));\n",
            ),
            (
                "lib/pad.js",
                "export const pad = (s) => s.padStart(2, \"0\");\n",
            ),
        ]);
        let r = p.run(TSX, vec![div("counter", &[], vec![])]).await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        assert!(
            r.artifact_keys
                .contains(&"components:module:quarto-components/counter.js".to_string()),
            "{:?}",
            r.artifact_keys
        );
        assert!(
            r.artifact_keys
                .contains(&"components:module:quarto-components/lib/format.js".to_string()),
            "{:?}",
            r.artifact_keys
        );
        let counter = &r.artifact_text["components:module:quarto-components/counter.js"];
        assert!(counter.contains("from \"./lib/format.js\""), "{counter}");
        assert!(counter.contains("react/jsx-runtime"), "{counter}");
        assert!(!counter.contains(": unknown"), "types stripped: {counter}");
        let format = &r.artifact_text["components:module:quarto-components/lib/format.js"];
        assert!(!format.contains(": number"), "{format}");
        // The plain JS helper is copied, not transpiled.
        assert_eq!(r.copies.len(), 1);
        assert!(
            r.copies[0]
                .dest
                .ends_with("site_libs/quarto-components/lib/pad.js")
        );
        assert!(
            header_includes(&r.ast.meta)[0]
                .contains("\"module\":\"site_libs/quarto-components/counter.js\"")
        );
    }

    #[tokio::test]
    async fn js_entry_imports_are_copied_and_shared_files_ship_once() {
        let p = DiskProject::new(&[
            (
                "a.js",
                "import { u } from \"./util.js\";\nexport default () => u;\n",
            ),
            (
                "b.js",
                "import { u } from \"./util.js\";\nexport default () => u + 1;\n",
            ),
            ("util.js", "export const u = 1;\n"),
        ]);
        let yaml = "components:\n  - name: a\n    src: a.js\n  - name: b\n    src: b.js\n";
        let r = p
            .run(yaml, vec![div("a", &[], vec![]), div("b", &[], vec![])])
            .await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        let mut dests: Vec<String> = r
            .copies
            .iter()
            .map(|c| c.dest.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        dests.sort();
        assert_eq!(dests, vec!["a.js", "b.js", "util.js"]);
        assert!(
            r.artifact_keys
                .iter()
                .all(|k| !k.starts_with("components:module:"))
        );
    }

    #[tokio::test]
    async fn transpile_error_is_q_25_9_and_leaves_the_component_out() {
        let p = DiskProject::new(&[(
            "counter.tsx",
            "export default function C() { return <b>; }\n",
        )]);
        let r = p.run(TSX, vec![div("counter", &[], vec![])]).await;
        assert_eq!(codes(&r.diags), vec!["Q-25-9"]);
        assert!(
            r.diags[0].title.contains("counter.tsx"),
            "{}",
            r.diags[0].title
        );
        assert!(
            r.artifact_keys.is_empty(),
            "nothing ships when the entry fails: {:?}",
            r.artifact_keys
        );
        assert!(header_includes(&r.ast.meta).is_empty());
    }

    #[tokio::test]
    async fn missing_import_is_reported_and_the_entry_still_ships() {
        let p = DiskProject::new(&[(
            "counter.tsx",
            "import { x } from \"./nope.ts\";\nexport default () => x;\n",
        )]);
        let r = p.run(TSX, vec![div("counter", &[], vec![])]).await;
        assert_eq!(codes(&r.diags), vec!["Q-25-9"]);
        assert!(
            format!("{:?}", r.diags[0]).contains("names no file"),
            "{:?}",
            r.diags[0]
        );
        assert!(
            r.artifact_keys
                .contains(&"components:module:quarto-components/counter.js".to_string())
        );
        assert_eq!(header_includes(&r.ast.meta).len(), 1);
    }

    #[tokio::test]
    async fn import_outside_the_project_is_q_25_2() {
        let outside = tempfile::TempDir::new().unwrap();
        std::fs::write(outside.path().join("x.js"), "export const x = 1;\n").unwrap();
        let p = DiskProject::new(&[]);
        let rel = pathdiff::diff_paths(
            outside.path().canonicalize().unwrap().join("x.js"),
            p.root(),
        )
        .unwrap();
        let spec = quarto_util::to_forward_slashes(&rel);
        std::fs::write(
            p.root().join("counter.js"),
            format!("import {{ x }} from \"{spec}\";\nexport default () => x;\n"),
        )
        .unwrap();
        let r = p.run(ONE, vec![div("counter", &[], vec![])]).await;
        assert_eq!(codes(&r.diags), vec!["Q-25-2"]);
    }

    #[tokio::test]
    async fn no_components_key_is_a_no_op() {
        let ast = Pandoc {
            meta: meta("title: x\n"),
            blocks: vec![div("counter", &[], vec![])],
        };
        let before = ast.clone();
        let r = run(ast).await;
        assert_eq!(r.ast, before);
        assert!(r.diags.is_empty());
        assert!(r.artifact_keys.is_empty());
    }

    #[tokio::test]
    async fn non_html_profile_is_a_no_op() {
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![div("counter", &[], vec![])],
        };
        let before = ast.clone();
        let r = run_with(ast, Format::docx(), None).await;
        assert_eq!(r.ast, before);
        assert!(r.diags.is_empty());
        assert!(r.artifact_keys.is_empty());
    }

    #[tokio::test]
    async fn without_resolver_marks_and_registers_but_copies_nothing() {
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![div("counter", &[], vec![])],
        };
        let r = run_with(ast, Format::html(), None).await;
        assert!(r.diags.is_empty(), "{:?}", r.diags);
        assert_eq!(host_attr(&r.ast.blocks[0]), Some("counter"));
        assert!(r.copies.is_empty());
        assert_eq!(r.artifact_keys.len(), 5);
        assert!(
            header_includes(&r.ast.meta)[0]
                .contains("\"module\":\"quarto-components/docs/counter.js\"")
        );
    }

    #[tokio::test]
    async fn rerun_is_idempotent() {
        let ast = Pandoc {
            meta: meta(ONE),
            blocks: vec![div("counter", &[], vec![])],
        };
        let first = run(ast).await;
        let second = run(first.ast.clone()).await;
        assert_eq!(second.ast, first.ast);
        assert!(second.diags.is_empty(), "{:?}", second.diags);
        assert_eq!(header_includes(&second.ast.meta).len(), 1);
    }
}
