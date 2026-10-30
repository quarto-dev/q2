/*
 * transforms/toc_js.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The client side of the HTML table of contents: ship the scroll-tracking
 * script, or hand the TOC to a bound React component.
 */

//! The client side of the table of contents (bd-toc-scrollspy-js-41e624d8).
//!
//! Quarto 1's `quarto.js` keeps the "On this page" TOC in step with the
//! reader: the current section's link is `.active`, nested lists collapse
//! and expand around it (`toc-expand`), and `quarto-sectionChanged` fires
//! on `window`. q2 renders the same markup (`nav#TOC.toc-active`,
//! `a.nav-link[data-scroll-target]`, see
//! [`TocRenderTransform`](super::TocRenderTransform)) and this transform
//! supplies the behavior, in one of two ways:
//!
//! - **Default**: the port of that script,
//!   `resources/js/quarto-toc/quarto-toc.js`, stored as the
//!   Project-scoped `js:quarto-toc` artifact (the
//!   [`QuartoNavJsTransform`](super::QuartoNavJsTransform) pattern), so
//!   `ApplyTemplateStage` emits one `<script src>` and a website writes
//!   one copy under `site_libs/quarto-toc/`.
//! - **`toc-component: <src>`**: the author's React component takes the
//!   TOC over. The option is sugar for a `components:` declaration
//!   (bd-react-components-a8flgldm) that this transform synthesizes and
//!   appends to the document's list, named [`TOC_COMPONENT_NAME`] and
//!   bound with [`Binding::Chrome`](crate::components::Binding::Chrome):
//!   the host element is emitted by the template, not found in the AST.
//!   `rendered.navigation.toc-component` tells the three `nav#TOC`
//!   emitters (`TOC_BLOCK_PARTIAL`, `toc_block_html`, the preview's
//!   `TocSlot`) to put `data-quarto-component` on the nav, and
//!   `ComponentsResolveTransform` does the rest: ships the module graph,
//!   registers React and the runtime, writes the manifest. The component
//!   receives the structured TOC as `props` ([`toc_component_props`]) and
//!   the static `<h2>`, `<ul>` and `.toc-actions` as `children`. The
//!   default script is not shipped in this case; it would fight the
//!   component over `.active`.
//!
//! ## Predicate and ordering
//!
//! Both halves gate on a rendered TOC (`rendered.navigation.toc`
//! non-empty), the same signal the `nav#TOC` emitters gate on; without
//! one both the script and `toc-component` are inert, as `toc-location`
//! is. Registered in the Navigation phase after
//! [`TocLocationTransform`](super::TocLocationTransform), whose
//! `rendered.navigation.toc-location` the props carry, and before
//! `SidebarRenderTransform`, whose `toc_block_html` reads
//! `rendered.navigation.toc-component`.
//!
//! ## WASM
//!
//! The declaration half runs on both targets: the preview's `TocSlot`
//! reads `rendered.navigation.toc-component` (mounting the component
//! there is bd-s73m7at2's). The script half is
//! `cfg(not(target_arch = "wasm32"))`: the preview excludes
//! `ApplyTemplateStage`, so a `js:*` artifact would never become a
//! `<script>` tag there, and the preview injects the same file itself at
//! `ts-packages/preview-renderer/src/q2-preview/entry.tsx` module top
//! (the Phase F.1 pattern). Native and preview must load the same bytes.

use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValue, ConfigValueKind};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::Result;
#[cfg(not(target_arch = "wasm32"))]
use crate::artifact::{Artifact, ArtifactScope, ArtifactStore};
use crate::components::{COMPONENTS_KEY, TOC_COMPONENT_NAME};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::toc_render::toc_expand_depth;

/// The metadata key naming the component module.
pub const TOC_COMPONENT_KEY: &str = "toc-component";

/// The q2 port of the TOC subset of Q1's quarto.js. Version contract:
/// `resources/js/README.md` § `quarto-toc/`.
#[cfg(not(target_arch = "wasm32"))]
const QUARTO_TOC_JS: &[u8] = include_bytes!("../../../../resources/js/quarto-toc/quarto-toc.js");

/// Artifact key. Sorts after `js:quarto-nav:*` and before `js:tabsets`;
/// the script binds at `DOMContentLoaded`, so order is not load-bearing.
pub const TOC_JS_KEY: &str = "js:quarto-toc";

/// On-disk layout: `site_libs/quarto-toc/quarto-toc.js`.
pub const TOC_JS_REL_PATH: &str = "quarto-toc/quarto-toc.js";

/// What the page gets for its TOC.
#[derive(Debug, PartialEq, Eq)]
enum TocClient {
    /// No rendered TOC: nothing to track.
    Nothing,
    /// The default script.
    Script,
    /// `toc-component:` names a module.
    Component,
}

/// Decide from document metadata. Pure so the predicate is unit-testable
/// without a `RenderContext`.
fn decide(meta: &ConfigValue) -> TocClient {
    let has_toc = meta
        .get_path(&["rendered", "navigation", "toc"])
        .and_then(|v| v.as_plain_text())
        .is_some_and(|s| !s.is_empty());
    if !has_toc {
        return TocClient::Nothing;
    }
    if meta.get(TOC_COMPONENT_KEY).is_some() {
        TocClient::Component
    } else {
        TocClient::Script
    }
}

/// Idempotently store the script artifact. `ArtifactStore::store`
/// overwrites by key, so re-running is fine.
#[cfg(not(target_arch = "wasm32"))]
fn store_script(artifacts: &mut ArtifactStore) {
    artifacts.store(
        TOC_JS_KEY,
        Artifact::from_bytes(QUARTO_TOC_JS.to_vec(), "text/javascript")
            .with_path(TOC_JS_REL_PATH)
            .with_scope(ArtifactScope::Project),
    );
}

fn generated() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

fn entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
    ConfigMapEntry {
        key: key.to_string(),
        key_source: generated(),
        value,
    }
}

fn string(s: &str) -> ConfigValue {
    ConfigValue::new_string(s, generated())
}

/// The `props` the TOC component receives, as metadata (the manifest
/// turns it into plain JSON through `ConfigValue::to_plain_json`, so
/// entry titles arrive as their plain text):
///
/// ```json
/// { "title": "On this page",
///   "entries": [{ "id": "intro", "title": "Introduction", "level": 2,
///                 "number": "1", "children": [] }],
///   "expanded": 1,          // toc-expand as the script reads it; null when unset
///   "location": "right" }   // the normalized toc-location
/// ```
///
/// The TypeScript shape is `QuartoTocProps` in `@quarto/components`.
pub fn toc_component_props(meta: &ConfigValue) -> ConfigValue {
    let mut entries = Vec::new();
    if let Some(title) = meta.get_path(&["navigation", "toc", "title"]) {
        entries.push(entry("title", title.clone()));
    }
    let toc_entries = meta
        .get_path(&["navigation", "toc", "entries"])
        .map_or_else(
            || ConfigValue::new_array(Vec::new(), generated()),
            with_children,
        );
    entries.push(entry("entries", toc_entries));
    let expanded = match toc_expand_depth(meta) {
        Some(n) => ConfigValue::new_scalar(yaml_rust2::Yaml::Integer(n), generated()),
        None => ConfigValue::null(generated()),
    };
    entries.push(entry("expanded", expanded));
    let location = meta
        .get_path(&["rendered", "navigation", "toc-location"])
        .and_then(|v| v.as_plain_text())
        .unwrap_or_else(|| "right".to_string());
    entries.push(entry("location", string(&location)));
    ConfigValue::new_map(entries, generated())
}

/// `TocEntry::to_config_value` omits `children` when the list is empty;
/// the component contract has the key on every entry
/// (`QuartoTocEntry.children`), so fill it in, recursively. `value` is
/// the `entries` array or one entry's `children` array.
fn with_children(value: &ConfigValue) -> ConfigValue {
    let ConfigValueKind::Array(items) = &value.value else {
        return value.clone();
    };
    let items = items
        .iter()
        .map(|item| {
            let mut item = item.clone();
            let children = match item.get("children") {
                Some(children) => with_children(children),
                None => ConfigValue::new_array(Vec::new(), generated()),
            };
            item.insert_path(&["children"], children);
            item
        })
        .collect();
    ConfigValue::new_array(items, value.source_info.clone())
}

/// The synthesized `components:` entry for `toc-component: <src>`. The
/// `src` value is the author's, span and merge-time path marking
/// included, so `parse_components` diagnoses a bad one at the right
/// place.
fn toc_component_decl(meta: &ConfigValue, src: &ConfigValue) -> ConfigValue {
    ConfigValue::new_map(
        vec![
            entry("name", string(TOC_COMPONENT_NAME)),
            entry("src", src.clone()),
            entry("bind", string("chrome")),
            entry("content", string("dom")),
            entry("props", toc_component_props(meta)),
        ],
        generated(),
    )
}

/// Append the synthesized declaration to `components:` (creating the
/// list when absent), replacing an earlier `quarto-toc` entry so a
/// re-run leaves one. An author's own `quarto-toc` entry is replaced
/// too: the name is reserved (`Binding::Chrome` says so in the parser).
fn declare_toc_component(meta: &mut ConfigValue) {
    let Some(src) = meta.get(TOC_COMPONENT_KEY).cloned() else {
        return;
    };
    let decl = toc_component_decl(meta, &src);
    let is_toc = |v: &ConfigValue| {
        v.get("name")
            .and_then(|n| n.as_plain_text())
            .is_some_and(|n| n.trim() == TOC_COMPONENT_NAME)
    };
    match meta.get_path_mut(&[COMPONENTS_KEY]) {
        Some(ConfigValue {
            value: ConfigValueKind::Array(items),
            ..
        }) => {
            items.retain(|v| !is_toc(v));
            items.push(decl);
        }
        Some(_) => {
            // Not a list: leave it for `parse_components` to diagnose
            // (Q-25-1); the TOC component cannot be declared alongside.
        }
        None => {
            meta.insert_path(
                &[COMPONENTS_KEY],
                ConfigValue::new_array(vec![decl], generated()),
            );
        }
    }
    meta.insert_path(
        &["rendered", "navigation", "toc-component"],
        string(TOC_COMPONENT_NAME),
    );
}

pub struct TocJsTransform;

impl TocJsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TocJsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for TocJsTransform {
    fn name(&self) -> &str {
        "toc-js"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Navigation
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        match decide(&ast.meta) {
            TocClient::Nothing => {}
            TocClient::Component => declare_toc_component(&mut ast.meta),
            TocClient::Script => {
                #[cfg(not(target_arch = "wasm32"))]
                store_script(&mut ctx.artifacts);
                #[cfg(target_arch = "wasm32")]
                let _ = ctx;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pampa::toc::{NavigationToc, TocEntry};
    use quarto_pandoc_types::inline::{Inline, Inlines, Str};

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::for_test())
    }

    fn plain(text: &str) -> Inlines {
        vec![Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })]
    }

    fn toc_entry(id: &str, title: &str, level: i32, children: Vec<TocEntry>) -> TocEntry {
        TocEntry {
            id: id.into(),
            title: plain(title),
            level,
            number: None,
            children,
        }
    }

    /// Metadata as it stands after `toc-generate`, `toc-render` and
    /// `toc-location` on a page with two sections.
    fn meta_with_toc() -> ConfigValue {
        let mut meta = ConfigValue::null(SourceInfo::for_test());
        let toc = NavigationToc {
            title: Some(plain("On this page")),
            entries: vec![
                toc_entry(
                    "intro",
                    "Introduction",
                    2,
                    vec![toc_entry("why", "Why", 3, vec![])],
                ),
                toc_entry("methods", "Methods", 2, vec![]),
            ],
        };
        meta.insert_path(&["navigation", "toc"], toc.to_config_value());
        meta.insert_path(
            &["rendered", "navigation", "toc"],
            s("<ul><li><a href=\"#intro\">Introduction</a></li></ul>"),
        );
        meta.insert_path(&["rendered", "navigation", "toc-location"], s("right"));
        meta
    }

    #[test]
    fn no_rendered_toc_ships_nothing() {
        let meta = ConfigValue::null(SourceInfo::for_test());
        assert_eq!(decide(&meta), TocClient::Nothing);

        // An empty string is "nothing rendered" (the template's `$if$`
        // treats it as falsy and emits no nav).
        let mut meta = ConfigValue::null(SourceInfo::for_test());
        meta.insert_path(&["rendered", "navigation", "toc"], s(""));
        assert_eq!(decide(&meta), TocClient::Nothing);

        // `toc-component` without a TOC is inert too, like toc-location.
        meta.insert_path(&[TOC_COMPONENT_KEY], s("toc.tsx"));
        assert_eq!(decide(&meta), TocClient::Nothing);
    }

    #[test]
    fn rendered_toc_ships_the_script() {
        assert_eq!(decide(&meta_with_toc()), TocClient::Script);
    }

    #[test]
    fn toc_component_replaces_the_script() {
        let mut meta = meta_with_toc();
        meta.insert_path(&[TOC_COMPONENT_KEY], s("toc.tsx"));
        assert_eq!(decide(&meta), TocClient::Component);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn script_artifact_is_project_scoped_at_the_q1_like_path() {
        let mut store = ArtifactStore::new();
        store_script(&mut store);
        let a = store.get(TOC_JS_KEY).expect("quarto-toc.js stored");
        assert_eq!(a.scope, ArtifactScope::Project);
        assert_eq!(
            a.path.as_deref(),
            Some(std::path::Path::new(TOC_JS_REL_PATH))
        );
        assert_eq!(a.content_type, "text/javascript");
        let text = String::from_utf8_lossy(&a.content);
        assert!(text.contains("quarto-sectionChanged"));
        assert!(text.contains("data-toc-expanded"));
        assert!(text.contains("data-quarto-component"));
        // Idempotent.
        store_script(&mut store);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn key_sorts_between_quarto_nav_and_tabsets() {
        // ApplyTemplateStage emits scripts in sorted-key order.
        let mut keys = vec![
            "js:tabsets",
            TOC_JS_KEY,
            "js:quarto-nav:nav",
            "js:bootstrap",
        ];
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "js:bootstrap",
                "js:quarto-nav:nav",
                TOC_JS_KEY,
                "js:tabsets"
            ]
        );
    }

    #[test]
    fn props_carry_the_structured_toc_as_plain_json() {
        let mut meta = meta_with_toc();
        meta.insert_path(
            &["toc-expand"],
            ConfigValue::new_bool(true, SourceInfo::for_test()),
        );
        let props = toc_component_props(&meta).to_plain_json();
        assert_eq!(props["title"], "On this page");
        assert_eq!(props["location"], "right");
        assert_eq!(props["expanded"], 99);
        let entries = props["entries"].as_array().expect("entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["id"], "intro");
        assert_eq!(entries[0]["title"], "Introduction");
        assert_eq!(entries[0]["level"], 2);
        assert_eq!(entries[0]["children"][0]["id"], "why");
        assert_eq!(entries[1]["children"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn props_expanded_is_null_when_toc_expand_is_unset() {
        let props = toc_component_props(&meta_with_toc()).to_plain_json();
        assert!(props["expanded"].is_null(), "{props}");
        assert_eq!(props["location"], "right");
    }

    #[test]
    fn props_location_defaults_to_right_without_toc_location() {
        let mut meta = meta_with_toc();
        meta.insert_path(&["rendered", "navigation", "toc-location"], s("left"));
        assert_eq!(
            toc_component_props(&meta).to_plain_json()["location"],
            "left"
        );
    }

    #[test]
    fn declare_creates_the_components_list_and_the_rendered_flag() {
        let mut meta = meta_with_toc();
        meta.insert_path(&[TOC_COMPONENT_KEY], s("toc.tsx"));
        declare_toc_component(&mut meta);

        let list = meta
            .get(COMPONENTS_KEY)
            .and_then(|v| v.as_array())
            .expect("list");
        assert_eq!(list.len(), 1);
        let decl = &list[0];
        assert_eq!(decl.get("name").unwrap().as_str(), Some(TOC_COMPONENT_NAME));
        assert_eq!(decl.get("src").unwrap().as_str(), Some("toc.tsx"));
        assert_eq!(decl.get("bind").unwrap().as_str(), Some("chrome"));
        assert_eq!(decl.get("content").unwrap().as_str(), Some("dom"));
        assert!(decl.get("props").unwrap().is_map());
        assert_eq!(
            meta.get_path(&["rendered", "navigation", "toc-component"])
                .unwrap()
                .as_str(),
            Some(TOC_COMPONENT_NAME)
        );
    }

    #[test]
    fn declare_appends_to_an_existing_list_and_is_idempotent() {
        let mut meta = meta_with_toc();
        meta.insert_path(&[TOC_COMPONENT_KEY], s("toc.tsx"));
        let other = ConfigValue::new_map(
            vec![entry("name", s("carousel")), entry("src", s("carousel.js"))],
            SourceInfo::for_test(),
        );
        meta.insert_path(
            &[COMPONENTS_KEY],
            ConfigValue::new_array(vec![other], SourceInfo::for_test()),
        );

        declare_toc_component(&mut meta);
        declare_toc_component(&mut meta);

        let list = meta
            .get(COMPONENTS_KEY)
            .and_then(|v| v.as_array())
            .expect("list");
        let names: Vec<_> = list
            .iter()
            .map(|d| d.get("name").unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["carousel", TOC_COMPONENT_NAME]);
    }

    #[test]
    fn declare_parses_as_a_chrome_bound_component() {
        use crate::components::{Binding, ContentMode, parse_components};
        let mut meta = meta_with_toc();
        meta.insert_path(&[TOC_COMPONENT_KEY], s("toc.tsx"));
        declare_toc_component(&mut meta);
        let mut diags = Vec::new();
        let decls = parse_components(meta.get(COMPONENTS_KEY).unwrap(), &mut diags);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, TOC_COMPONENT_NAME);
        assert_eq!(decls[0].bind, Binding::Chrome);
        assert_eq!(decls[0].content, ContentMode::Dom);
        let props = decls[0].props.as_ref().expect("props");
        assert_eq!(props["entries"][1]["id"], "methods");
    }
}
