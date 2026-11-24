/*
 * transforms/code_tools.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! `code-tools`: the "Code" button in the title block.
//!
//! Port of Quarto 1's `command/render/codetools.ts` (a DOM
//! post-processor) and the `codeTools` part of
//! `quarto-html-after-body.ejs`. With `code-tools: true` a page gets a
//! button next to its title that opens a menu:
//!
//! - **Show All Code** / **Hide All Code** open and close every folded
//!   cell (`details.code-fold`). Offered only when the page has one.
//! - **View Source** opens the document's own source in a modal, or
//!   goes to a URL when `code-tools: { source: <url> }`.
//!
//! With `toggle: false` (or nothing to toggle) the button itself is the
//! View Source control and there is no menu.
//!
//! q2 has no DOM pass, so the pieces reach the page through metadata:
//!
//! - the button and menu as `rendered.code-tools`, which the title-block
//!   partial places beside the `<h1>` (`template.rs`);
//! - the modal and the script as one `rendered.includes.after-body`
//!   entry.
//!
//! Called from
//! [`CodeBlockGenerateTransform`](super::CodeBlockGenerateTransform),
//! which knows whether any cell folds.
//!
//! ## Scope
//!
//! `format: html` in the render pipeline only. Quarto 1 requires
//! Bootstrap (`formatHasBootstrap`): the menu is a Bootstrap dropdown
//! and the source view a Bootstrap modal. The q2-preview React renderer
//! is skipped.
//!
//! ## Differences from Quarto 1
//!
//! - The source in the modal is plain text. Q1 highlights it as
//!   markdown.
//! - The toggle is offered when a cell folds. Q1 also offers it for
//!   cells whose code is hidden with the `hidden` class.
//! - Labels are English; Q1 reads them from the language table.

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use crate::format::{FormatIdentifier, PipelineProfile};
use crate::render::RenderContext;

/// Dedup key for the after-body include.
const CODE_TOOLS_SENTINEL: &str = "<!-- quarto-code-tools -->";

/// Where View Source goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceTarget {
    /// No View Source control.
    None,
    /// Show the embedded source in a modal.
    Embedded,
    /// Navigate to this URL.
    Url(String),
}

/// The `code-tools` option after defaults are applied
/// (`resolveCodeTools` in Q1's `codetools.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeTools {
    pub source: SourceTarget,
    pub toggle: bool,
    /// Button label; `"none"` means icon only.
    pub caption: String,
}

impl CodeTools {
    /// Resolve the option from document metadata. `None` when
    /// `code-tools` is unset or `false`.
    ///
    /// A map enables both tools unless a key turns one off, so
    /// `code-tools: { caption: "Menu" }` still has source and toggle.
    pub fn from_meta(meta: &ConfigValue) -> Option<Self> {
        let value = meta.get("code-tools")?;
        let default_caption = || "Code".to_string();
        if let Some(b) = value.as_bool() {
            return b.then(|| CodeTools {
                source: SourceTarget::Embedded,
                toggle: true,
                caption: default_caption(),
            });
        }
        if !value.is_map() {
            return None;
        }
        let source = match value.get("source") {
            None => SourceTarget::Embedded,
            Some(v) => match v.as_bool() {
                Some(true) => SourceTarget::Embedded,
                Some(false) => SourceTarget::None,
                None => match v.as_plain_text() {
                    Some(url) if !url.trim().is_empty() => {
                        SourceTarget::Url(url.trim().to_string())
                    }
                    _ => SourceTarget::Embedded,
                },
            },
        };
        let toggle = value
            .get("toggle")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let caption = value
            .get("caption")
            .and_then(|v| v.as_plain_text())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(default_caption);
        Some(CodeTools {
            source,
            toggle,
            caption,
        })
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The button and, with a toggle, its dropdown menu. Markup and ids
/// match Q1's so the ported SCSS (`.code-tools-button`) and any user
/// CSS keyed on them apply.
pub fn button_html(tools: &CodeTools) -> String {
    let caption = if tools.caption == "none" {
        String::new()
    } else {
        format!(" {}", escape(&tools.caption))
    };
    let source_url = match &tools.source {
        SourceTarget::Url(url) => format!(" data-quarto-source-url=\"{}\"", escape(url)),
        _ => String::new(),
    };

    if !tools.toggle {
        return format!(
            "<button type=\"button\" class=\"btn code-tools-button\" id=\"quarto-code-tools-source\"{source_url}>\
             <i class=\"bi\"></i>{caption}</button>"
        );
    }

    let item = |id: &str, text: &str, extra: &str| {
        format!(
            "<li><a id=\"{id}\" class=\"dropdown-item\" href=\"javascript:void(0)\" role=\"button\"{extra}>{text}</a></li>"
        )
    };
    let mut html = format!(
        "<button type=\"button\" class=\"btn code-tools-button dropdown-toggle\" id=\"quarto-code-tools-menu\" \
         data-bs-toggle=\"dropdown\" aria-expanded=\"false\"><i class=\"bi\"></i>{caption}</button>\
         <ul class=\"dropdown-menu dropdown-menu-end\" aria-labelledby=\"quarto-code-tools-menu\">"
    );
    html.push_str(&item("quarto-show-all-code", "Show All Code", ""));
    html.push_str(&item("quarto-hide-all-code", "Hide All Code", ""));
    if tools.source != SourceTarget::None {
        html.push_str("<li><hr class=\"dropdown-divider\"></li>");
        html.push_str(&item("quarto-view-source", "View Source", &source_url));
    }
    html.push_str("</ul>");
    html
}

/// The Bootstrap modal that shows the document source.
fn modal_html(source: &str) -> String {
    format!(
        "<div class=\"modal fade\" id=\"quarto-embedded-source-code-modal\" tabindex=\"-1\" \
         aria-labelledby=\"quarto-embedded-source-code-modal-label\" aria-hidden=\"true\">\
         <div class=\"modal-dialog modal-dialog-scrollable\"><div class=\"modal-content\">\
         <div class=\"modal-header\">\
         <h5 class=\"modal-title\" id=\"quarto-embedded-source-code-modal-label\">Source Code</h5>\
         <button class=\"btn-close\" data-bs-dismiss=\"modal\" aria-label=\"Close\"></button>\
         </div><div class=\"modal-body\">\
         <div class=\"sourceCode\"><pre class=\"sourceCode markdown\"><code class=\"sourceCode markdown\">{}</code></pre></div>\
         </div></div></div></div>",
        escape(source)
    )
}

/// Port of the `codeTools` block of Q1's `quarto-html-after-body.ejs`.
const SCRIPT: &str = r#"<script>
(function () {
  function init() {
    var viewSource = document.getElementById("quarto-view-source") ||
      document.getElementById("quarto-code-tools-source");
    if (viewSource) {
      var sourceUrl = viewSource.getAttribute("data-quarto-source-url");
      viewSource.addEventListener("click", function (e) {
        e.preventDefault();
        if (sourceUrl) {
          window.location.href = sourceUrl;
        } else {
          var el = document.getElementById("quarto-embedded-source-code-modal");
          if (el && window.bootstrap) new window.bootstrap.Modal(el).show();
        }
      });
    }
    function toggleAll(show) {
      return function (e) {
        e.preventDefault();
        document.querySelectorAll(".cell details.code-fold").forEach(function (details) {
          details.open = show;
        });
      };
    }
    var hideAll = document.getElementById("quarto-hide-all-code");
    if (hideAll) hideAll.addEventListener("click", toggleAll(false));
    var showAll = document.getElementById("quarto-show-all-code");
    if (showAll) showAll.addEventListener("click", toggleAll(true));
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
</script>"#;

/// Read the document's source for the View Source modal. `None` when it
/// cannot be read (and always on WASM, which has no file system here).
fn read_source(ctx: &RenderContext) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read_to_string(&ctx.document.input).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        None
    }
}

/// Emit the code-tools button, modal and script for this document.
/// `has_folded_cell` says whether any cell renders as a fold.
pub(crate) fn apply_code_tools(ast: &mut Pandoc, ctx: &RenderContext, has_folded_cell: bool) {
    if ctx.format.identifier != FormatIdentifier::Html
        || ctx.pipeline_profile != PipelineProfile::HtmlRender
    {
        return;
    }
    let Some(mut tools) = CodeTools::from_meta(&ast.meta) else {
        return;
    };

    // Only offer what the page can do (Q1 `resolveCodeTools`).
    tools.toggle = tools.toggle && has_folded_cell;
    let source_text = match tools.source {
        SourceTarget::Embedded => {
            let text = read_source(ctx);
            if text.is_none() {
                tools.source = SourceTarget::None;
            }
            text
        }
        _ => None,
    };
    if !tools.toggle && tools.source == SourceTarget::None {
        return;
    }

    let generated = || SourceInfo::generated(By::programmatic_config());
    ast.meta.insert_path(
        &["rendered", "code-tools"],
        ConfigValue::new_string(button_html(&tools), generated()),
    );

    let mut after_body = String::from(CODE_TOOLS_SENTINEL);
    if let Some(text) = source_text {
        after_body.push('\n');
        after_body.push_str(&modal_html(&text));
    }
    after_body.push('\n');
    after_body.push_str(SCRIPT);
    super::append_with_sentinel(&mut ast.meta, "after-body", CODE_TOOLS_SENTINEL, after_body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind, MergeOp};

    fn value(kind: ConfigValueKind) -> ConfigValue {
        ConfigValue {
            value: kind,
            source_info: SourceInfo::for_test(),
            merge_op: MergeOp::Concat,
        }
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        value(ConfigValueKind::Map(
            entries
                .into_iter()
                .map(|(key, value)| ConfigMapEntry {
                    key: key.to_string(),
                    key_source: SourceInfo::for_test(),
                    value,
                })
                .collect(),
        ))
    }

    fn boolean(b: bool) -> ConfigValue {
        value(ConfigValueKind::scalar(yaml_rust2::Yaml::Boolean(b)))
    }

    fn string(s: &str) -> ConfigValue {
        value(ConfigValueKind::scalar(yaml_rust2::Yaml::String(
            s.to_string(),
        )))
    }

    #[test]
    fn unset_and_false_resolve_to_none() {
        assert_eq!(CodeTools::from_meta(&ConfigValue::default()), None);
        assert_eq!(
            CodeTools::from_meta(&map(vec![("code-tools", boolean(false))])),
            None
        );
    }

    #[test]
    fn true_enables_source_and_toggle_with_the_default_caption() {
        assert_eq!(
            CodeTools::from_meta(&map(vec![("code-tools", boolean(true))])),
            Some(CodeTools {
                source: SourceTarget::Embedded,
                toggle: true,
                caption: "Code".to_string(),
            })
        );
    }

    #[test]
    fn a_map_overrides_each_part() {
        let meta = map(vec![(
            "code-tools",
            map(vec![
                ("source", string("https://example.com/src.qmd")),
                ("toggle", boolean(false)),
                ("caption", string("Menu")),
            ]),
        )]);
        assert_eq!(
            CodeTools::from_meta(&meta),
            Some(CodeTools {
                source: SourceTarget::Url("https://example.com/src.qmd".to_string()),
                toggle: false,
                caption: "Menu".to_string(),
            })
        );
        // Keys left out keep their defaults.
        let meta = map(vec![("code-tools", map(vec![("source", boolean(false))]))]);
        let tools = CodeTools::from_meta(&meta).unwrap();
        assert_eq!(tools.source, SourceTarget::None);
        assert!(tools.toggle);
    }

    #[test]
    fn menu_button_lists_toggle_items_then_view_source() {
        let html = button_html(&CodeTools {
            source: SourceTarget::Embedded,
            toggle: true,
            caption: "Code".to_string(),
        });
        assert!(html.starts_with(
            "<button type=\"button\" class=\"btn code-tools-button dropdown-toggle\" id=\"quarto-code-tools-menu\""
        ));
        assert!(html.contains("<i class=\"bi\"></i> Code</button>"));
        let show = html.find("quarto-show-all-code").unwrap();
        let hide = html.find("quarto-hide-all-code").unwrap();
        let view = html.find("quarto-view-source").unwrap();
        assert!(show < hide && hide < view);
        assert!(html.contains("<li><hr class=\"dropdown-divider\"></li>"));
    }

    #[test]
    fn without_a_toggle_the_button_is_the_source_control() {
        let html = button_html(&CodeTools {
            source: SourceTarget::Url("https://example.com/a?b=1&c=2".to_string()),
            toggle: false,
            caption: "Menu".to_string(),
        });
        assert_eq!(
            html,
            "<button type=\"button\" class=\"btn code-tools-button\" id=\"quarto-code-tools-source\" \
             data-quarto-source-url=\"https://example.com/a?b=1&amp;c=2\"><i class=\"bi\"></i> Menu</button>"
        );
    }

    #[test]
    fn caption_none_leaves_only_the_icon() {
        let html = button_html(&CodeTools {
            source: SourceTarget::Embedded,
            toggle: false,
            caption: "none".to_string(),
        });
        assert!(html.ends_with("<i class=\"bi\"></i></button>"));
    }

    #[test]
    fn modal_escapes_the_source() {
        let html = modal_html("a < b\n```{r}\n1 & 2\n```");
        assert!(html.contains("a &lt; b\n```{r}\n1 &amp; 2\n```"));
        assert!(html.contains("id=\"quarto-embedded-source-code-modal\""));
    }
}
