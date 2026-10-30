/*
 * components/config.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The `components:` metadata contract (bd-react-components-a8flgldm).
 */

//! Parse the `components:` metadata key into [`ComponentDecl`]s.
//!
//! The key is a list of maps. Each entry names a component (`name`), says
//! where its ES module comes from (`src`), and optionally which export to
//! use (`export`, default `default`), which element it binds to (`bind`,
//! `#id` or `.class`, default `#<name>`), and how the element's content
//! reaches the component (`content`: `dom`, `ast` or `none`, default
//! `dom`). The merge-time path marking
//! ([`crate::project::format_paths`]) has already turned a project-file
//! `src` into a document-relative [`ConfigValueKind::Path`]; an `npm:`
//! source is left as written and is not supported yet (Q-25-3).
//!
//! Every problem is a located warning (Q-25-1 for a malformed entry,
//! Q-25-6 for an option value outside its vocabulary) and the entry is
//! skipped, so one bad declaration never takes the page's other
//! components down with it.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_source_map::SourceInfo;

/// The metadata key.
pub const COMPONENTS_KEY: &str = "components";

/// The reserved name of the table-of-contents component, declared through
/// `toc-component:` and synthesized by
/// [`TocJsTransform`](crate::transforms::TocJsTransform)
/// (bd-toc-scrollspy-js-41e624d8).
pub const TOC_COMPONENT_NAME: &str = "quarto-toc";

/// The names a [`Binding::Chrome`] declaration may carry. Chrome
/// components are declared through their own option (`toc-component:`),
/// never by an author's `components:` entry, so `bind: chrome` on any
/// other name is Q-25-6.
pub const CHROME_COMPONENT_NAMES: &[&str] = &[TOC_COMPONENT_NAME];

/// Prefix of an npm package source (`npm:<pkg>@<version>`); deferred by
/// decision 5 of the plan.
const NPM_PREFIX: &str = "npm:";

/// Where a component's ES module comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentSource {
    /// A project file, kept unresolved (the merge-time marked form, or
    /// the authored string with its span) until the transform resolves
    /// it against the file that declared it (`crate::config_paths`) and
    /// copies it under the project lib dir.
    File(crate::config_paths::ConfigPath),
}

impl ComponentSource {
    /// The path as the author wrote it (or as the merge marked it).
    pub fn as_written(&self) -> &str {
        match self {
            ComponentSource::File(path) => path.as_written(),
        }
    }
}

/// Which element(s) a component binds to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    /// `#id`: the one `Div`/`Span` with that id.
    Id(String),
    /// `.class`: every `Div`/`Span` carrying the class.
    Class(String),
    /// `chrome`: the host is emitted by the template (`nav#TOC` for
    /// `toc-component:`), not found in the AST. Never matches an element
    /// and never raises Q-25-4; the emitter puts `data-quarto-component`
    /// on the host itself. Reserved for declarations q2 synthesizes
    /// ([`CHROME_COMPONENT_NAMES`]); internal, not documented for authors.
    Chrome,
}

impl Binding {
    /// Parse a `bind:` value; `None` when it is neither `#id` nor `.class`.
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        if spec == "chrome" {
            return Some(Binding::Chrome);
        }
        if let Some(id) = spec.strip_prefix('#') {
            return Self::name(id).map(Binding::Id);
        }
        if let Some(class) = spec.strip_prefix('.') {
            return Self::name(class).map(Binding::Class);
        }
        None
    }

    /// A bare `#id`/`.class` tail: non-empty and a single token.
    fn name(s: &str) -> Option<String> {
        if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c == '#' || c == '.') {
            None
        } else {
            Some(s.to_string())
        }
    }

    /// Does an element's `Attr` match this binding?
    pub fn matches(&self, id: &str, classes: &[String]) -> bool {
        match self {
            Binding::Id(want) => id == want,
            Binding::Class(want) => classes.iter().any(|c| c == want),
            Binding::Chrome => false,
        }
    }

    /// The selector as the author would write it.
    pub fn selector(&self) -> String {
        match self {
            Binding::Id(id) => format!("#{id}"),
            Binding::Class(class) => format!(".{class}"),
            Binding::Chrome => "chrome".to_string(),
        }
    }
}

/// How the bound element's content reaches the component (plan D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentMode {
    /// The runtime hands the host's no-script DOM back as `children`.
    #[default]
    Dom,
    /// The node's Pandoc AST is attached as JSON and arrives as `node`
    /// (the preview-parity option).
    Ast,
    /// The content is dropped; `children` is empty.
    None,
}

impl ContentMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "dom" => Some(ContentMode::Dom),
            "ast" => Some(ContentMode::Ast),
            "none" => Some(ContentMode::None),
            _ => Option::None,
        }
    }

    /// The manifest spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ContentMode::Dom => "dom",
            ContentMode::Ast => "ast",
            ContentMode::None => "none",
        }
    }
}

/// One `components:` entry, validated.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentDecl {
    /// The registry key; what `data-quarto-component` carries.
    pub name: String,
    pub src: ComponentSource,
    /// The module export to mount; `None` means `default`.
    pub export: Option<String>,
    pub bind: Binding,
    pub content: ContentMode,
    /// Static props from `props:`, as plain JSON; overlaid by the
    /// element's own props in the browser.
    pub props: Option<serde_json::Value>,
    /// The whole entry, for diagnostics about the binding.
    pub source_info: SourceInfo,
    /// The `src` value, for diagnostics about the file (the copy
    /// intent's origin).
    pub src_source_info: SourceInfo,
}

/// Parse `components:`. Malformed entries are diagnosed and skipped; the
/// returned list is in declaration order with unique names.
pub fn parse_components(
    value: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<ComponentDecl> {
    let Some(items) = value.as_array() else {
        push_diag(
            diagnostics,
            "Q-25-1",
            "`components` must be a list of component entries",
            value,
            "Each entry is a map with at least `name:` and `src:`.",
        );
        return Vec::new();
    };
    let mut decls: Vec<ComponentDecl> = Vec::new();
    for item in items {
        if let Some(decl) = parse_entry(item, diagnostics) {
            if decls.iter().any(|d| d.name == decl.name) {
                push_diag(
                    diagnostics,
                    "Q-25-1",
                    format!("component `{}` is declared more than once", decl.name),
                    item,
                    "Component names are the keys the page binds by; give each entry its own.",
                );
                continue;
            }
            decls.push(decl);
        }
    }
    decls
}

fn parse_entry(
    item: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Option<ComponentDecl> {
    if !item.is_map() {
        push_diag(
            diagnostics,
            "Q-25-1",
            "a `components` entry must be a map",
            item,
            "Write `- name: <name>` and `src: <file>` under it.",
        );
        return None;
    }

    let name = match item.get("name").and_then(ConfigValue::as_plain_text) {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => {
            push_diag(
                diagnostics,
                "Q-25-1",
                "a `components` entry has no `name`",
                item,
                "The name is what the element binds by; add `name: <name>`.",
            );
            return None;
        }
    };

    let Some(src_value) = item.get("src") else {
        push_diag(
            diagnostics,
            "Q-25-1",
            format!("component `{name}` has no `src`"),
            item,
            "Point `src:` at the component's ES module, relative to this file.",
        );
        return None;
    };
    let src = match &src_value.value {
        ConfigValueKind::Path(p) => {
            ComponentSource::File(crate::config_paths::ConfigPath::Marked(p.clone()))
        }
        _ => {
            let Some(text) = src_value.as_plain_text() else {
                push_diag(
                    diagnostics,
                    "Q-25-1",
                    format!("component `{name}`: `src` must be a file path"),
                    src_value,
                    "Write the path of the component's ES module as a string.",
                );
                return None;
            };
            let text = text.trim().to_string();
            if text.starts_with(NPM_PREFIX) {
                push_diag(
                    diagnostics,
                    "Q-25-3",
                    format!("component `{name}`: npm sources are not supported yet"),
                    src_value,
                    "Point `src:` at a file in the project for now; `npm:` sources are planned.",
                );
                return None;
            }
            if text.is_empty() || quarto_util::is_external_url(&text) {
                push_diag(
                    diagnostics,
                    "Q-25-1",
                    format!("component `{name}`: `src` must be a project file"),
                    src_value,
                    "Only files inside the project can be shipped with the page.",
                );
                return None;
            }
            // Not marked at merge time (an entry a filter added, or a
            // layer the marking did not see): keep the string and its
            // span; the resolver reads it against the declaring file.
            ComponentSource::File(crate::config_paths::ConfigPath::Raw {
                raw: quarto_util::to_forward_slashes(std::path::Path::new(&text)),
                source: src_value.source_info.clone(),
            })
        }
    };

    let export = item
        .get("export")
        .and_then(ConfigValue::as_plain_text)
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty() && e != "default");

    let bind = match item.get("bind") {
        None => Binding::Id(name.clone()),
        Some(v) => {
            let text = v.as_plain_text().unwrap_or_default();
            match Binding::parse(&text) {
                Some(b) => b,
                None => {
                    push_diag(
                        diagnostics,
                        "Q-25-6",
                        format!("component `{name}`: `bind: {text}` is not `#id` or `.class`"),
                        v,
                        "Write `bind: \"#some-id\"` or `bind: .some-class`.",
                    );
                    return None;
                }
            }
        }
    };

    if bind == Binding::Chrome && !CHROME_COMPONENT_NAMES.contains(&name.as_str()) {
        push_diag(
            diagnostics,
            "Q-25-6",
            format!(
                "component `{name}`: `bind: chrome` is reserved for Quarto's own chrome components"
            ),
            item.get("bind").unwrap_or(item),
            "Chrome components are declared through their own option (`toc-component:`); bind yours with `bind: \"#some-id\"` or `bind: .some-class`.",
        );
        return None;
    }

    let content = match item.get("content") {
        None => ContentMode::Dom,
        Some(v) => {
            let text = v.as_plain_text().unwrap_or_default();
            match ContentMode::parse(&text) {
                Some(c) => c,
                None => {
                    push_diag(
                        diagnostics,
                        "Q-25-6",
                        format!("component `{name}`: `content: {text}` is not dom, ast or none"),
                        v,
                        "Choose `dom` (default), `ast` or `none`.",
                    );
                    return None;
                }
            }
        }
    };

    let props = match item.get("props") {
        None => None,
        Some(v) if v.is_map() => Some(v.to_plain_json()),
        Some(v) => {
            push_diag(
                diagnostics,
                "Q-25-6",
                format!("component `{name}`: `props` must be a map"),
                v,
                "Write `props:` as key/value pairs; they reach the component as a JSON object.",
            );
            return None;
        }
    };

    Some(ComponentDecl {
        name,
        src,
        export,
        bind,
        content,
        props,
        source_info: item.source_info.clone(),
        src_source_info: src_value.source_info.clone(),
    })
}

fn push_diag(
    diagnostics: &mut Vec<DiagnosticMessage>,
    code: &str,
    message: impl Into<String>,
    value: &ConfigValue,
    hint: &str,
) {
    diagnostics.push(
        DiagnosticMessageBuilder::warning(message)
            .with_code(code)
            .with_location(value.source_info.clone())
            .add_hint(hint)
            .build(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse front matter the way the render pipeline does (YAML →
    /// `ConfigValue` → merge → materialize) and hand back the
    /// `components:` value.
    fn components_value(yaml: &str) -> ConfigValue {
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
        let merged = MergedConfig::new(vec![&doc])
            .materialize()
            .expect("materialize");
        merged.get(COMPONENTS_KEY).expect("components key").clone()
    }

    fn parse(yaml: &str) -> (Vec<ComponentDecl>, Vec<DiagnosticMessage>) {
        let mut diags = Vec::new();
        let decls = parse_components(&components_value(yaml), &mut diags);
        (decls, diags)
    }

    fn codes(diags: &[DiagnosticMessage]) -> Vec<&str> {
        diags.iter().filter_map(|d| d.code.as_deref()).collect()
    }

    #[test]
    fn minimal_entry_gets_the_defaults() {
        let (decls, diags) = parse("components:\n  - name: counter\n    src: counter.js\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(decls.len(), 1);
        let d = &decls[0];
        assert_eq!(d.name, "counter");
        assert_eq!(d.src.as_written(), "counter.js");
        assert_eq!(d.export, None);
        assert_eq!(d.bind, Binding::Id("counter".into()));
        assert_eq!(d.content, ContentMode::Dom);
    }

    #[test]
    fn every_option_is_read() {
        let (decls, diags) = parse(
            "components:\n  - name: cards\n    src: /lib/cards.js\n    export: Cards\n    bind: .card\n    content: ast\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        let d = &decls[0];
        assert_eq!(d.export.as_deref(), Some("Cards"));
        assert_eq!(d.bind, Binding::Class("card".into()));
        assert_eq!(d.content, ContentMode::Ast);
        // Unmarked (no merge-time marking in this harness): kept as
        // written, forward slashes.
        assert_eq!(d.src.as_written(), "/lib/cards.js");
    }

    #[test]
    fn props_map_becomes_plain_json_and_non_map_is_q_25_6() {
        let (decls, diags) = parse(
            "components:\n  - name: a\n    src: a.js\n    props:\n      interval: 3000\n      label: \"Hi *there*\"\n      tags: [x, y]\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            decls[0].props,
            Some(serde_json::json!({"interval": 3000, "label": "Hi there", "tags": ["x", "y"]}))
        );
        let (decls, diags) = parse("components:\n  - name: a\n    src: a.js\n    props: 3\n");
        assert!(decls.is_empty());
        assert_eq!(codes(&diags), vec!["Q-25-6"]);
        let (decls, _) = parse("components:\n  - name: a\n    src: a.js\n");
        assert_eq!(decls[0].props, None);
    }

    #[test]
    fn export_default_is_the_absence_of_an_export() {
        let (decls, _) = parse("components:\n  - name: a\n    src: a.js\n    export: default\n");
        assert_eq!(decls[0].export, None);
    }

    #[test]
    fn marked_path_is_taken_verbatim() {
        let mut value = components_value("components:\n  - name: a\n    src: a.js\n");
        let items = match &mut value.value {
            ConfigValueKind::Array(items) => items,
            _ => unreachable!(),
        };
        items[0].get_mut("src").unwrap().value = ConfigValueKind::Path("../lib/a.js".into());
        let mut diags = Vec::new();
        let decls = parse_components(&value, &mut diags);
        assert_eq!(decls[0].src.as_written(), "../lib/a.js");
    }

    #[test]
    fn missing_name_and_missing_src_are_q_25_1_and_skipped() {
        let (decls, diags) =
            parse("components:\n  - src: a.js\n  - name: b\n  - name: c\n    src: c.js\n");
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "c");
        assert_eq!(codes(&diags), vec!["Q-25-1", "Q-25-1"]);
        assert!(diags[0].title.contains("no `name`"), "{}", diags[0].title);
        assert!(
            diags[1].title.contains("has no `src`"),
            "{}",
            diags[1].title
        );
        assert!(diags[0].location.is_some(), "entry diagnostics are located");
    }

    #[test]
    fn non_list_and_non_map_entries_are_q_25_1() {
        let (decls, diags) = parse("components: counter\n");
        assert!(decls.is_empty());
        assert_eq!(codes(&diags), vec!["Q-25-1"]);

        let (decls, diags) = parse("components:\n  - counter\n  - name: ok\n    src: ok.js\n");
        assert_eq!(decls.len(), 1);
        assert_eq!(codes(&diags), vec!["Q-25-1"]);
        assert!(diags[0].title.contains("must be a map"));
    }

    #[test]
    fn duplicate_names_keep_the_first() {
        let (decls, diags) =
            parse("components:\n  - name: a\n    src: a.js\n  - name: a\n    src: b.js\n");
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].src.as_written(), "a.js");
        assert_eq!(codes(&diags), vec!["Q-25-1"]);
        assert!(diags[0].title.contains("more than once"));
    }

    #[test]
    fn npm_source_is_q_25_3_and_skipped() {
        let (decls, diags) = parse("components:\n  - name: t\n    src: npm:@acme/table@2\n");
        assert!(decls.is_empty());
        assert_eq!(codes(&diags), vec!["Q-25-3"]);
    }

    #[test]
    fn url_source_is_q_25_1() {
        let (decls, diags) = parse("components:\n  - name: t\n    src: https://example.com/t.js\n");
        assert!(decls.is_empty());
        assert_eq!(codes(&diags), vec!["Q-25-1"]);
    }

    #[test]
    fn bad_bind_and_bad_content_are_q_25_6_and_skipped() {
        let (decls, diags) = parse(
            "components:\n  - name: a\n    src: a.js\n    bind: div.a\n  - name: b\n    src: b.js\n    content: html\n  - name: c\n    src: c.js\n    bind: '#x y'\n",
        );
        assert!(decls.is_empty());
        assert_eq!(codes(&diags), vec!["Q-25-6", "Q-25-6", "Q-25-6"]);
        assert!(
            diags[0].title.contains("`bind: div.a`"),
            "{}",
            diags[0].title
        );
        assert!(
            diags[1].title.contains("`content: html`"),
            "{}",
            diags[1].title
        );
    }

    #[test]
    fn binding_parse_and_match() {
        assert_eq!(
            Binding::parse("#carousel"),
            Some(Binding::Id("carousel".into()))
        );
        assert_eq!(
            Binding::parse(" .card "),
            Some(Binding::Class("card".into()))
        );
        assert_eq!(Binding::parse("carousel"), None);
        assert_eq!(Binding::parse("#"), None);
        assert_eq!(Binding::parse("#a.b"), None);
        let classes = vec!["x".to_string(), "card".to_string()];
        assert_eq!(Binding::parse("chrome"), Some(Binding::Chrome));
        assert!(!Binding::Chrome.matches("TOC", &classes));
        assert_eq!(Binding::Chrome.selector(), "chrome");
        assert!(Binding::Id("c".into()).matches("c", &classes));
        assert!(!Binding::Id("c".into()).matches("d", &classes));
        assert!(Binding::Class("card".into()).matches("", &classes));
        assert!(!Binding::Class("cards".into()).matches("", &classes));
        assert_eq!(Binding::Class("card".into()).selector(), ".card");
    }

    #[test]
    fn content_mode_vocabulary() {
        assert_eq!(ContentMode::parse("dom"), Some(ContentMode::Dom));
        assert_eq!(ContentMode::parse("ast"), Some(ContentMode::Ast));
        assert_eq!(ContentMode::parse("none"), Some(ContentMode::None));
        assert_eq!(ContentMode::parse("DOM"), None);
        assert_eq!(ContentMode::Ast.as_str(), "ast");
    }
}
